//! Budgeted, transactional simplification of pure packed-value expressions.
//!
//! This does not combine guest instructions: guards, memory operations and all
//! recovery points remain in place. Only already-verified SSA values are used.
use super::rewrite_values;
use crate::ir::{analysis::cfg::Cfg, hir::*, ids::*, simd::PackedOp, verify::verify};

pub const DEFAULT_WORK_LIMIT: usize = 262_144;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Stats {
    pub rewritten: usize,
    pub eliminated: usize,
    pub work: usize,
}

struct Work {
    remaining: usize,
    used: usize,
}
impl Work {
    fn spend(&mut self, amount: usize) -> Result<(), String> {
        self.remaining = self
            .remaining
            .checked_sub(amount)
            .ok_or("SIMD work budget exceeded")?;
        self.used += amount;
        Ok(())
    }
}

fn pure(inst: &Instruction) -> bool {
    inst.results.len() == 1
        && inst.state.is_none()
        && inst.commit.is_none()
        && !inst.trap_after_fault
        && !inst.unmasked_word_store
        && matches!(
            inst.op,
            Op::VectorShuffle(_)
                | Op::VectorBinary(_)
                | Op::VectorExtract { .. }
                | Op::VectorReplace { .. }
        )
}

fn definition(region: &Region, value: ValueId) -> Option<&Instruction> {
    match region.values[value.index()].definition {
        Definition::Instruction(id, _) => {
            let inst = &region.instructions[id.index()];
            pure(inst).then_some(inst)
        },
        Definition::Parameter(..) => None,
    }
}

enum Change {
    Alias(ValueId),
    Rewrite(Op, Vec<ValueId>),
}

fn shuffle(
    region: &Region,
    inst: &Instruction,
    lanes: &[u8; 16],
    work: &mut Work,
) -> Result<Option<Change>, String> {
    // Resolve one level per operand. A composition requiring three or four
    // independent vectors cannot be represented by a two-input Wasm shuffle.
    let mut sources = Vec::with_capacity(2);
    let mut mask = [0; 16];
    for (i, &lane) in lanes.iter().enumerate() {
        work.spend(1)?;
        let mut source = inst.args[(lane / 16) as usize];
        let mut offset = lane % 16;
        if let Some(parent) = definition(region, source) {
            if let Op::VectorShuffle(parent_lanes) = &parent.op {
                let selected = parent_lanes[offset as usize];
                source = parent.args[(selected / 16) as usize];
                offset = selected % 16;
            }
        }
        let slot = if let Some(slot) = sources.iter().position(|&v| v == source) {
            slot
        } else {
            if sources.len() == 2 {
                return Ok(None);
            }
            sources.push(source);
            sources.len() - 1
        };
        mask[i] = offset + slot as u8 * 16;
    }
    if mask.iter().enumerate().all(|(i, &lane)| lane as usize == i) {
        return Ok(Some(Change::Alias(sources[0])));
    }
    if sources.len() == 1 {
        sources.push(sources[0]);
    }
    if sources == inst.args && mask == *lanes {
        Ok(None)
    } else {
        Ok(Some(Change::Rewrite(Op::VectorShuffle(mask), sources)))
    }
}

fn simplify(
    region: &Region,
    inst: &Instruction,
    work: &mut Work,
) -> Result<Option<Change>, String> {
    work.spend(1)?;
    Ok(match inst.op {
        Op::VectorShuffle(ref lanes) => return shuffle(region, inst, lanes, work),
        Op::VectorBinary(operation) if inst.args[0] == inst.args[1] => {
            if matches!(
                operation,
                PackedOp::And
                    | PackedOp::Or
                    | PackedOp::MinU8
                    | PackedOp::MaxU8
                    | PackedOp::MinS16
                    | PackedOp::MaxS16
                    | PackedOp::AvgU8
                    | PackedOp::AvgU16
            ) {
                Some(Change::Alias(inst.args[0]))
            } else {
                None
            }
        },
        Op::VectorExtract { bits, lane } => {
            let Some(parent) = definition(region, inst.args[0]) else {
                return Ok(None);
            };
            match parent.op {
                Op::VectorReplace {
                    bits: width,
                    lane: written,
                } if width == bits => {
                    if written != lane {
                        Some(Change::Rewrite(inst.op.clone(), vec![parent.args[0]]))
                    } else if bits == 32 || bits == 64 {
                        Some(Change::Alias(parent.args[1]))
                    } else {
                        // A 16-bit insert truncates its i32 operand. Returning
                        // the original operand here would leak its high bits.
                        None
                    }
                },
                _ => None,
            }
        },
        Op::VectorReplace { bits, lane } => {
            if let Some(extracted) = definition(region, inst.args[1]) {
                if extracted.op == (Op::VectorExtract { bits, lane })
                    && extracted.args[0] == inst.args[0]
                {
                    return Ok(Some(Change::Alias(inst.args[0])));
                }
            }
            let Some(parent) = definition(region, inst.args[0]) else {
                return Ok(None);
            };
            if parent.op == (Op::VectorReplace { bits, lane }) {
                Some(Change::Rewrite(
                    inst.op.clone(),
                    vec![parent.args[0], inst.args[1]],
                ))
            } else {
                None
            }
        },
        _ => None,
    })
}

/// Simplify pure packed values without changing instruction observation points.
/// All errors leave `region` unchanged, including late work-budget exhaustion.
pub fn run(region: &mut Region, work_limit: usize) -> Result<Stats, String> {
    if region.blocks.len() > 64
        || region.instructions.len() > 8192
        || region.values.len() > 16384
        || region.states.len() > 8192
        || region.helpers.len() > 1024
    {
        return Err("SIMD region budget exceeded".into());
    }
    let mut work = Work {
        remaining: work_limit,
        used: 0,
    };
    work.spend(1)?;
    verify(region).map_err(|e| e.0)?;
    let cfg = Cfg::compute(region)?;
    let mut staged = region.clone();
    let mut order: Vec<_> = (0..staged.blocks.len()).collect();
    order.sort_by_key(|&b| (cfg.dominates[b].iter().filter(|&&d| d).count(), b));
    let mut aliases = vec![None; staged.values.len()];
    let mut removed = vec![false; staged.instructions.len()];
    let mut stats = Stats::default();
    for block in order {
        for id in staged.blocks[block].instructions.clone() {
            work.spend(1)?;
            for arg in &mut staged.instructions[id.index()].args {
                work.spend(1)?;
                if let Some(value) = aliases[arg.index()] {
                    *arg = value;
                }
            }
            let inst = &staged.instructions[id.index()];
            if !pure(inst) {
                continue;
            }
            match simplify(&staged, inst, &mut work)? {
                Some(Change::Alias(value)) => {
                    aliases[inst.results[0].index()] = Some(value);
                    removed[id.index()] = true;
                    stats.eliminated += 1;
                },
                Some(Change::Rewrite(op, args)) => {
                    staged.instructions[id.index()].op = op;
                    staged.instructions[id.index()].args = args;
                    stats.rewritten += 1;
                },
                None => {},
            }
        }
    }
    // Dominance-order traversal makes each alias canonical before it is used.
    // Rewriting includes recovery-only XMM values, edge arguments and counters.
    rewrite_values(&mut staged, |v| {
        if let Some(value) = aliases[v.index()] {
            *v = value;
        }
    });
    for block in &mut staged.blocks {
        block.instructions.retain(|id| !removed[id.index()]);
    }
    verify(&staged).map_err(|e| e.0)?;
    stats.work = work.used;
    *region = staged;
    Ok(stats)
}

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/simd_simplify.rs"]
mod tests;
