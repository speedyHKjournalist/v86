//! Exact integer-vector rewrites. No FP relaxation or guest-memory forwarding.
use crate::ir::{analysis::cfg::Cfg, hir::*, ids::*, simd::PackedOp, verify::verify};

pub const DEFAULT_WORK_LIMIT: usize = 262_144;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Stats {
    pub rewritten: usize,
    pub removed: usize,
    pub work: usize,
}
fn pure(inst: &Instruction) -> bool {
    !inst.op.ordered()
        && inst.results.len() == 1
        && inst.state.is_none()
        && inst.commit.is_none()
        && !inst.trap_after_fault
        && !inst.unmasked_word_store
}
fn definition(r: &Region, v: ValueId) -> Option<&Instruction> {
    match r.values[v.index()].definition {
        Definition::Instruction(id, _) => Some(&r.instructions[id.index()]),
        _ => None,
    }
}
enum Change {
    Alias(ValueId),
    Rewrite(Op, Vec<ValueId>),
}
fn simplify(r: &Region, inst: &Instruction) -> Option<Change> {
    use Change::{Alias, Rewrite};
    match inst.op {
        Op::VectorBinary(op) if inst.args[0] == inst.args[1] => matches!(
            op,
            PackedOp::And
                | PackedOp::Or
                | PackedOp::MinU8
                | PackedOp::MaxU8
                | PackedOp::MinS16
                | PackedOp::MaxS16
                | PackedOp::AvgU8
                | PackedOp::AvgU16
        )
        .then_some(Alias(inst.args[0])),
        Op::VectorShuffle(mask) => {
            // Expand one shuffle layer per operand, and only when the result can
            // still be expressed using at most two input vectors. SSA dominance
            // of these inputs follows transitively from the original operands.
            let mut sources = Vec::new();
            let mut lanes = [0u8; 16];
            for (out, lane) in mask.iter().copied().enumerate() {
                let mut source = inst.args[(lane / 16) as usize];
                let mut index = lane % 16;
                if let Some(inner) = definition(r, source).filter(|i| pure(i)) {
                    if let Op::VectorShuffle(inner_mask) = inner.op {
                        let selected = inner_mask[index as usize];
                        source = inner.args[(selected / 16) as usize];
                        index = selected % 16;
                    }
                }
                let slot = if let Some(slot) = sources.iter().position(|&v| v == source) {
                    slot
                } else {
                    if sources.len() == 2 {
                        return None;
                    }
                    sources.push(source);
                    sources.len() - 1
                };
                lanes[out] = index + 16 * slot as u8;
            }
            if sources.len() == 1 && lanes.iter().enumerate().all(|(i, &n)| n as usize == i) {
                return Some(Alias(sources[0]));
            }
            if sources.len() == 1 {
                sources.push(sources[0]);
            }
            (lanes != mask || sources != inst.args)
                .then_some(Rewrite(Op::VectorShuffle(lanes), sources))
        },
        Op::VectorExtract { bits, lane } => {
            let inner = definition(r, inst.args[0]).filter(|i| pure(i))?;
            if let Op::VectorReplace {
                bits: inserted_bits,
                lane: inserted_lane,
            } = inner.op
            {
                if bits == inserted_bits {
                    if lane != inserted_lane {
                        return Some(Rewrite(inst.op.clone(), vec![inner.args[0]]));
                    }
                    // A 16-bit extract zero-extends; the inserted i32 need not
                    // have zero high bits. Do NOT replace this with an i32 copy.
                    if bits == 32 || bits == 64 {
                        return Some(Alias(inner.args[1]));
                    }
                }
            }
            None
        },
        Op::VectorReplace { bits, lane } => {
            if let Some(inner) = definition(r, inst.args[1]).filter(|i| pure(i)) {
                if matches!(inner.op, Op::VectorExtract { bits: b, lane: l } if b == bits && l == lane)
                    && inner.args[0] == inst.args[0]
                {
                    return Some(Alias(inst.args[0]));
                }
            }
            if let Some(inner) = definition(r, inst.args[0]).filter(|i| pure(i)) {
                if matches!(inner.op, Op::VectorReplace { bits: b, lane: l } if b == bits && l == lane)
                {
                    return Some(Rewrite(inst.op.clone(), vec![inner.args[0], inst.args[1]]));
                }
            }
            None
        },
        _ => None,
    }
}

/// Transactional, bounded and StateMap-aware. Alias sources have already been
/// visited in dominance order, so the alias map stays transitively canonical.
pub fn run(region: &mut Region, work_limit: usize) -> Result<Stats, String> {
    if region.blocks.len() > 64
        || region.instructions.len() > 8192
        || region.values.len() > 16384
        || region.states.len() > 8192
    {
        return Err("SIMD rewrite region budget exceeded".into());
    }
    verify(region).map_err(|e| e.0)?;
    let cfg = Cfg::compute(region)?;
    let mut candidate = region.clone();
    let mut aliases = vec![None; region.values.len()];
    let mut remove = vec![false; region.instructions.len()];
    let mut order: Vec<_> = (0..region.blocks.len()).collect();
    order.sort_by_key(|&b| (cfg.dominates[b].iter().filter(|&&d| d).count(), b));
    let mut stats = Stats::default();
    for block in order {
        for &id in &region.blocks[block].instructions {
            let mut inst = candidate.instructions[id.index()].clone();
            let cost =
                1 + inst.args.len() + if matches!(inst.op, Op::VectorShuffle(_)) { 16 } else { 0 };
            stats.work = stats
                .work
                .checked_add(cost)
                .ok_or("SIMD rewrite work overflow")?;
            if stats.work > work_limit {
                return Err("SIMD rewrite work budget exceeded".into());
            }
            for arg in &mut inst.args {
                *arg = aliases[arg.index()].unwrap_or(*arg);
            }
            candidate.instructions[id.index()].args = inst.args.clone();
            if !pure(&inst) {
                continue;
            }
            let Some(change) = simplify(&candidate, &inst) else {
                continue;
            };
            match change {
                Change::Alias(value) => {
                    let value = aliases[value.index()].unwrap_or(value);
                    if candidate.values[value.index()].ty
                        != candidate.values[inst.results[0].index()].ty
                    {
                        return Err("SIMD rewrite alias type mismatch".into());
                    }
                    aliases[inst.results[0].index()] = Some(value);
                    remove[id.index()] = true;
                    stats.removed += 1;
                },
                Change::Rewrite(op, args) => {
                    candidate.instructions[id.index()].op = op;
                    candidate.instructions[id.index()].args = args;
                },
            }
            stats.rewritten += 1;
        }
    }
    super::rewrite_values(&mut candidate, |v| {
        *v = aliases[v.index()].unwrap_or(*v);
    });
    for block in &mut candidate.blocks {
        block.instructions.retain(|id| !remove[id.index()]);
    }
    verify(&candidate).map_err(|e| e.0)?;
    *region = candidate;
    Ok(stats)
}

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/simd_rewrite.rs"]
mod tests;
