//! Exact vector bit/lane identities. No floating-point arithmetic is reassociated.
use super::rewrite_values;
use crate::ir::{analysis::cfg::Cfg, hir::*, ids::*, simd::PackedOp, verify::verify};

#[derive(Default, Debug)]
pub struct Stats {
    pub aliases: usize,
    pub rewritten: usize,
}

type Expression = (Op, Vec<ValueId>);

fn expression(
    region: &Region,
    planned: &[Option<Expression>],
    value: ValueId,
) -> Option<Expression> {
    let Definition::Instruction(id, _) = region.values[value.index()].definition else {
        return None;
    };
    let inst = &region.instructions[id.index()];
    if inst.state.is_some() || inst.commit.is_some() {
        return None;
    }
    Some(
        planned[id.index()]
            .clone()
            .unwrap_or_else(|| (inst.op.clone(), inst.args.clone())),
    )
}

/// Budget exhaustion and failed verification leave the input region unchanged.
pub fn run(region: &mut Region, max_work: usize) -> Result<Stats, String> {
    if region.blocks.len() > 64 || region.instructions.len() > 8192 || region.values.len() > 16384 {
        return Err("SIMD simplification region budget exceeded".into());
    }
    let mut work = crate::ir::analysis::loops::WorkBudget::new(max_work.min(1_000_000));
    work.spend(1)?;
    verify(region).map_err(|e| e.0)?;
    let cfg = Cfg::compute(region)?;
    let mut order: Vec<_> = (0..region.blocks.len()).collect();
    order.sort_by_key(|&b| cfg.dominates[b].iter().filter(|&&d| d).count());
    let mut aliases: Vec<Option<ValueId>> = vec![None; region.values.len()];
    let mut planned: Vec<Option<Expression>> = vec![None; region.instructions.len()];
    let mut removed = vec![false; region.instructions.len()];
    let mut stats = Stats::default();
    for b in order {
        for &id in &region.blocks[b].instructions {
            let inst = &region.instructions[id.index()];
            work.spend(1 + inst.args.len())?;
            if inst.results.len() != 1
                || inst.state.is_some()
                || inst.commit.is_some()
                || inst.trap_after_fault
                || inst.unmasked_word_store
            {
                continue;
            }
            let args: Vec<_> = inst
                .args
                .iter()
                .map(|&v| aliases[v.index()].unwrap_or(v))
                .collect();
            let mut op = inst.op.clone();
            let mut args = args;
            let mut alias = None;
            match &mut op {
                Op::VectorBinary(PackedOp::And | PackedOp::Or) if args[0] == args[1] => {
                    alias = Some(args[0]);
                },
                Op::VectorShuffle(lanes) => {
                    if args[0] == args[1] {
                        for lane in &mut *lanes {
                            *lane %= 16;
                        }
                    }
                    if lanes
                        .iter()
                        .enumerate()
                        .all(|(i, &lane)| lane as usize == i)
                    {
                        alias = Some(args[0]);
                    } else if lanes
                        .iter()
                        .enumerate()
                        .all(|(i, &lane)| lane as usize == i + 16)
                    {
                        alias = Some(args[1]);
                    } else {
                        // A permutation that selects one input can compose with
                        // that input's permutation, without adding vector sources.
                        let side = if lanes.iter().all(|&lane| lane < 16) {
                            Some(0)
                        } else if lanes.iter().all(|&lane| lane >= 16) {
                            Some(1)
                        } else {
                            None
                        };
                        if let Some(side) = side {
                            if let Some((Op::VectorShuffle(inner), sources)) =
                                expression(region, &planned, args[side])
                            {
                                work.spend(16)?;
                                for lane in &mut *lanes {
                                    *lane = inner[*lane as usize % 16];
                                }
                                args = sources
                                    .iter()
                                    .map(|&v| aliases[v.index()].unwrap_or(v))
                                    .collect();
                                if args[0] == args[1] {
                                    for lane in &mut *lanes {
                                        *lane %= 16;
                                    }
                                }
                                if lanes
                                    .iter()
                                    .enumerate()
                                    .all(|(i, &lane)| lane as usize == i)
                                {
                                    alias = Some(args[0]);
                                } else if lanes
                                    .iter()
                                    .enumerate()
                                    .all(|(i, &lane)| lane as usize == i + 16)
                                {
                                    alias = Some(args[1]);
                                }
                            }
                        }
                    }
                },
                Op::VectorExtract { bits, lane } => {
                    if let Some((
                        Op::VectorReplace {
                            bits: inner_bits,
                            lane: inner_lane,
                        },
                        inner,
                    )) = expression(region, &planned, args[0])
                    {
                        let start = u16::from(*bits) * u16::from(*lane);
                        let inner_start = u16::from(inner_bits) * u16::from(inner_lane);
                        if *bits == inner_bits && *lane == inner_lane && matches!(*bits, 32 | 64) {
                            alias = Some(aliases[inner[1].index()].unwrap_or(inner[1]));
                        } else if start + u16::from(*bits) <= inner_start
                            || inner_start + u16::from(inner_bits) <= start
                        {
                            args[0] = aliases[inner[0].index()].unwrap_or(inner[0]);
                        }
                        // Sub-word extraction zero-extends the inserted low bits:
                        // returning the original i32 would incorrectly retain high bits.
                    }
                },
                Op::VectorReplace { bits, lane } => {
                    if let Some((
                        Op::VectorExtract {
                            bits: extracted_bits,
                            lane: extracted_lane,
                        },
                        from,
                    )) = expression(region, &planned, args[1])
                    {
                        let base = aliases[from[0].index()].unwrap_or(from[0]);
                        if *bits == extracted_bits && *lane == extracted_lane && args[0] == base {
                            alias = Some(base);
                        }
                    }
                    if alias.is_none() {
                        if let Some((
                            Op::VectorReplace {
                                bits: old_bits,
                                lane: old_lane,
                            },
                            inner,
                        )) = expression(region, &planned, args[0])
                        {
                            if *bits == old_bits && *lane == old_lane {
                                args[0] = aliases[inner[0].index()].unwrap_or(inner[0]);
                            }
                        }
                    }
                },
                _ => {},
            }
            if let Some(value) = alias {
                aliases[inst.results[0].index()] = Some(value);
                removed[id.index()] = true;
                stats.aliases += 1;
            } else {
                if op != inst.op || args != inst.args {
                    stats.rewritten += 1;
                }
                planned[id.index()] = Some((op, args));
            }
        }
    }
    if stats.aliases == 0 && stats.rewritten == 0 {
        return Ok(stats);
    }
    // Commit onto a private candidate. Recovery-only uses are rewritten along
    // with instructions and edge arguments before the final dominance/type check.
    let mut candidate = region.clone();
    for (inst, plan) in candidate.instructions.iter_mut().zip(planned) {
        if let Some((op, args)) = plan {
            inst.op = op;
            inst.args = args;
        }
    }
    rewrite_values(&mut candidate, |value| {
        if let Some(new) = aliases[value.index()] {
            *value = new;
        }
    });
    for block in &mut candidate.blocks {
        block.instructions.retain(|id| !removed[id.index()]);
    }
    verify(&candidate).map_err(|e| e.0)?;
    *region = candidate;
    Ok(stats)
}

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/simd_opt.rs"]
mod tests;
