//! Bit-exact SIMD value simplification. Never moves guards, memory or CPU reads.
use super::rewrite_values;
use crate::ir::{analysis::cfg::Cfg, hir::*, ids::*, simd::PackedOp, verify::verify};

pub const DEFAULT_WORK_BUDGET: usize = 262_144;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SimdStats {
    pub eliminated: usize,
    pub composed: usize,
    pub overwritten_lanes: usize,
}

fn plain(inst: &Instruction) -> bool {
    inst.results.len() == 1
        && inst.state.is_none()
        && inst.commit.is_none()
        && !inst.trap_after_fault
        && !inst.unmasked_word_store
}
fn definition(region: &Region, value: ValueId) -> Option<&Instruction> {
    match region.values[value.index()].definition {
        Definition::Instruction(id, _) if plain(&region.instructions[id.index()]) => {
            Some(&region.instructions[id.index()])
        },
        _ => None,
    }
}
fn projection(lanes: &[u8; 16], args: &[ValueId]) -> Option<ValueId> {
    if lanes
        .iter()
        .enumerate()
        .all(|(i, &lane)| lane as usize == i)
    {
        Some(args[0])
    } else if lanes
        .iter()
        .enumerate()
        .all(|(i, &lane)| lane as usize == i + 16)
    {
        Some(args[1])
    } else {
        None
    }
}

/// A transaction including aliases in instruction arguments, phi edges and all
/// recovery-only values. The 16-bit extract-after-insert case deliberately does
/// NOT forward an i32: the extract must discard the input's high sixteen bits.
pub fn run(region: &mut Region, mut work: usize) -> Result<SimdStats, String> {
    if region.blocks.len() > 64
        || region.instructions.len() > 8192
        || region.values.len() > 16384
        || region.states.len() > 8192
        || region.helpers.len() > 1024
    {
        return Err("SIMD simplification region budget exceeded".into());
    }
    let mut spend = |amount: usize| -> Result<(), String> {
        work = work
            .checked_sub(amount)
            .ok_or("SIMD simplification work budget exceeded")?;
        Ok(())
    };
    spend(region.blocks.len() + region.instructions.len() + region.values.len())?;
    verify(region).map_err(|e| e.0)?;
    let cfg = Cfg::compute(region)?;
    let mut staged = region.clone();
    let mut aliases = vec![None; staged.values.len()];
    let mut removed = vec![false; staged.instructions.len()];
    let mut stats = SimdStats::default();
    let mut order: Vec<_> = (0..staged.blocks.len()).collect();
    order.sort_by_key(|&b| cfg.dominates[b].iter().filter(|&&v| v).count());
    for block in order {
        for id in staged.blocks[block].instructions.clone() {
            spend(1 + staged.instructions[id.index()].args.len())?;
            for arg in &mut staged.instructions[id.index()].args {
                if let Some(value) = aliases[arg.index()] {
                    *arg = value;
                }
            }
            let inst = &staged.instructions[id.index()];
            if !plain(inst) {
                continue;
            }
            let mut op = inst.op.clone();
            let mut args = inst.args.clone();
            let mut alias = None;
            match &mut op {
                Op::VectorShuffle(lanes) => {
                    spend(32)?;
                    if args[0] == args[1] {
                        for lane in lanes.iter_mut() {
                            *lane &= 15;
                        }
                    }
                    alias = projection(lanes, &args);
                    if alias.is_none() {
                        // A shuffle whose outputs all originate in one operand
                        // can compose with that operand's shuffle. Mixed sources
                        // are left alone unless the operands are identical.
                        let source = if lanes.iter().all(|&lane| lane < 16) {
                            Some(args[0])
                        } else if lanes.iter().all(|&lane| lane >= 16) {
                            Some(args[1])
                        } else {
                            None
                        };
                        if let Some(inner) = source.and_then(|value| definition(&staged, value)) {
                            if let Op::VectorShuffle(inner_lanes) = &inner.op {
                                *lanes =
                                    std::array::from_fn(|i| inner_lanes[(lanes[i] & 15) as usize]);
                                args.clone_from(&inner.args);
                                if args[0] == args[1] {
                                    for lane in lanes.iter_mut() {
                                        *lane &= 15;
                                    }
                                }
                                alias = projection(lanes, &args);
                                stats.composed += 1;
                            }
                        }
                    }
                },
                Op::VectorBinary(PackedOp::And | PackedOp::Or) if args[0] == args[1] => {
                    alias = Some(args[0]);
                },
                Op::VectorExtract { bits, lane } if matches!(*bits, 32 | 64) => {
                    if let Some(inner) = definition(&staged, args[0]) {
                        if matches!(&inner.op, Op::VectorReplace { bits: b, lane: l } if *b == *bits && *l == *lane)
                        {
                            alias = Some(inner.args[1]);
                        }
                    }
                },
                Op::VectorReplace { bits, lane } => {
                    if let Some(inner) = definition(&staged, args[1]) {
                        if matches!(&inner.op, Op::VectorExtract { bits: b, lane: l } if *b == *bits && *l == *lane)
                            && inner.args[0] == args[0]
                        {
                            alias = Some(args[0]);
                        }
                    }
                    if alias.is_none() {
                        if let Some(inner) = definition(&staged, args[0]) {
                            if matches!(&inner.op, Op::VectorReplace { bits: b, lane: l } if *b == *bits && *l == *lane)
                            {
                                args[0] = inner.args[0];
                                stats.overwritten_lanes += 1;
                            }
                        }
                    }
                },
                _ => {},
            }
            if let Some(value) = alias {
                aliases[inst.results[0].index()] = Some(value);
                removed[id.index()] = true;
                stats.eliminated += 1;
            } else {
                staged.instructions[id.index()].op = op;
                staged.instructions[id.index()].args = args;
            }
        }
    }
    // Charge the potentially large recovery/edge rewrite before committing it.
    for inst in &staged.instructions {
        spend(inst.args.len())?;
    }
    for state in &staged.states {
        spend(state.values().len())?;
    }
    for block in &staged.blocks {
        spend(1 + block.instructions.len())?;
        for edge in block.terminator.as_ref().unwrap().edges() {
            spend(edge.args.len())?;
        }
    }
    rewrite_values(&mut staged, |value| {
        if let Some(new) = aliases[value.index()] {
            *value = new;
        }
    });
    for block in &mut staged.blocks {
        block.instructions.retain(|id| !removed[id.index()]);
    }
    verify(&staged).map_err(|e| e.0)?;
    *region = staged;
    Ok(stats)
}

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/simd_simplify.rs"]
mod tests;
