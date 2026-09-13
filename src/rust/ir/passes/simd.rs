//! Bit-exact vector identities and bounded byte-shuffle composition.
//! No floating-point arithmetic, memory access, CPU observation or guard moves.
use super::{rewrite_values, PassStats};
use crate::ir::{analysis::cfg::Cfg, hir::*, ids::*, simd::PackedOp};

fn plain(inst: &Instruction) -> bool {
    inst.state.is_none()
        && inst.commit.is_none()
        && !inst.trap_after_fault
        && !inst.unmasked_word_store
        && inst.results.len() == 1
}

fn selected_byte(region: &Region, value: ValueId, byte: u8) -> (ValueId, u8) {
    if let Definition::Instruction(id, _) = region.values[value.index()].definition {
        let inst = &region.instructions[id.index()];
        if plain(inst) {
            if let Op::VectorShuffle(lanes) = &inst.op {
                let selected = lanes[byte as usize];
                return (inst.args[(selected / 16) as usize], selected % 16);
            }
        }
    }
    (value, byte)
}

pub(super) fn run(region: &mut Region, stats: &mut PassStats) -> Result<(), String> {
    if region.blocks.len() > 64 || region.instructions.len() > 8192 || region.values.len() > 16384 {
        return Err("SIMD simplification region budget exceeded".into());
    }
    if !region
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .any(|id| {
            matches!(
                region.instructions[id.index()].op,
                Op::VectorShuffle(_) | Op::VectorBinary(PackedOp::And | PackedOp::Or)
            )
        })
    {
        return Ok(());
    }
    let cfg = Cfg::compute(region)?;
    let mut order: Vec<_> = (0..region.blocks.len()).collect();
    order.sort_by_key(|&b| (cfg.dominates[b].iter().filter(|&&v| v).count(), b));
    let mut aliases = vec![None; region.values.len()];
    let mut removed = vec![false; region.instructions.len()];
    for b in order {
        for &id in &region.blocks[b].instructions {
            let inst = &mut region.instructions[id.index()];
            for arg in &mut inst.args {
                if let Some(previous) = aliases[arg.index()] {
                    *arg = previous;
                }
            }
            if !plain(inst) {
                continue;
            }
            let result = inst.results[0];
            let mut replacement = None;
            let mut rewrite = None;
            match &inst.op {
                Op::VectorBinary(PackedOp::And | PackedOp::Or) if inst.args[0] == inst.args[1] => {
                    replacement = Some(inst.args[0]);
                },
                Op::VectorShuffle(lanes) => {
                    let lanes = *lanes;
                    let inputs = [inst.args[0], inst.args[1]];
                    // Expand at most one level for each byte, not a recursive
                    // tree walk. At most 32 source comparisons per output byte.
                    let bytes: Vec<_> = lanes
                        .iter()
                        .map(|&lane| selected_byte(region, inputs[(lane / 16) as usize], lane % 16))
                        .collect();
                    let mut sources = Vec::new();
                    let mut composed = [0u8; 16];
                    for (i, &(source, byte)) in bytes.iter().enumerate() {
                        let position = match sources.iter().position(|&v| v == source) {
                            Some(position) => position,
                            None => {
                                sources.push(source);
                                sources.len() - 1
                            },
                        };
                        composed[i] = (position * 16) as u8 + byte;
                    }
                    // i8x16.shuffle has exactly two inputs. Never merge four
                    // independently contributing vectors into a two-input op.
                    if sources.len() <= 2 {
                        let left = sources[0];
                        let right = *sources.get(1).unwrap_or(&left);
                        if composed
                            .iter()
                            .enumerate()
                            .all(|(i, &v)| usize::from(v) == i)
                        {
                            replacement = Some(left);
                        } else if composed != lanes || inputs != [left, right] {
                            rewrite = Some((composed, vec![left, right]));
                        }
                    } else if lanes.iter().enumerate().all(|(i, &v)| usize::from(v) == i) {
                        replacement = Some(inputs[0]);
                    } else if lanes
                        .iter()
                        .enumerate()
                        .all(|(i, &v)| usize::from(v) == i + 16)
                    {
                        replacement = Some(inputs[1]);
                    }
                },
                _ => (),
            }
            if let Some(value) = replacement {
                aliases[result.index()] = Some(value);
                removed[id.index()] = true;
                stats.simd_simplified += 1;
            } else if let Some((lanes, args)) = rewrite {
                let inst = &mut region.instructions[id.index()];
                inst.op = Op::VectorShuffle(lanes);
                inst.args = args;
                stats.simd_simplified += 1;
            }
        }
    }
    rewrite_values(region, |value| {
        if let Some(previous) = aliases[value.index()] {
            *value = previous;
        }
    });
    for block in &mut region.blocks {
        block.instructions.retain(|id| !removed[id.index()]);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
