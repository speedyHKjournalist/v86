//! Width-aware scalar identities and exact SIMD lane/shuffle simplification.
//! Never removes an ordered operation, CPU observation, or recovery point.
use super::{constant, rewrite_values, PassStats};
use crate::ir::{analysis::cfg::Cfg, hir::*, ids::*, simd::PackedOp};
use std::collections::HashSet;

enum Change {
    Alias(ValueId),
    Rewrite(Op, Vec<ValueId>),
}
fn definition(region: &Region, value: ValueId) -> Option<&Instruction> {
    match region.values[value.index()].definition {
        Definition::Instruction(id, _) => Some(&region.instructions[id.index()]),
        Definition::Parameter(_, _) => None,
    }
}
fn scalar(region: &Region, inst: &Instruction) -> Option<Change> {
    use Change::{Alias, Rewrite};
    let a = &inst.args;
    match inst.op {
        Op::Binary(op) => {
            let x = constant(region, a[0]);
            let y = constant(region, a[1]);
            let bits = region.values[a[0].index()].ty.bits()?;
            let mask = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
            let zero = || Some(Rewrite(Op::Const(0), vec![]));
            if a[0] == a[1] {
                match op {
                    Binary::Sub | Binary::Xor | Binary::Ult | Binary::Slt => return zero(),
                    Binary::Eq => return Some(Rewrite(Op::Const(1), vec![])),
                    Binary::And | Binary::Or => return Some(Alias(a[0])),
                    _ => {},
                }
            }
            match op {
                Binary::Add | Binary::Or | Binary::Xor => {
                    if x == Some(0) {
                        return Some(Alias(a[1]));
                    }
                    if y == Some(0) {
                        return Some(Alias(a[0]));
                    }
                },
                Binary::Sub => {
                    if y == Some(0) {
                        return Some(Alias(a[0]));
                    }
                },
                Binary::Mul => {
                    if x == Some(0) || y == Some(0) {
                        return zero();
                    }
                    if x == Some(1) {
                        return Some(Alias(a[1]));
                    }
                    if y == Some(1) {
                        return Some(Alias(a[0]));
                    }
                },
                Binary::And => {
                    if x == Some(0) || y == Some(0) {
                        return zero();
                    }
                    if x == Some(mask) {
                        return Some(Alias(a[1]));
                    }
                    if y == Some(mask) {
                        return Some(Alias(a[0]));
                    }
                },
                Binary::Shl | Binary::Shr | Binary::Sar => {
                    if y.is_some_and(|n| n & if bits == 64 { 63 } else { 31 } == 0) {
                        return Some(Alias(a[0]));
                    }
                    if x == Some(0) {
                        return zero();
                    }
                },
                _ => {},
            }
            // Only audited integer commutative operations. In particular, no
            // floating-point/NaN/signed-zero assumptions are introduced here.
            if a[0].0 > a[1].0
                && matches!(
                    op,
                    Binary::Add | Binary::Mul | Binary::And | Binary::Or | Binary::Xor | Binary::Eq
                )
            {
                return Some(Rewrite(inst.op.clone(), vec![a[1], a[0]]));
            }
        },
        Op::Select => {
            if a[1] == a[2] {
                return Some(Alias(a[1]));
            }
            if let Some(c) = constant(region, a[0]) {
                return Some(Alias(a[if c == 0 { 2 } else { 1 }]));
            }
        },
        Op::Truncate => {
            let inner = definition(region, a[0])?;
            if matches!(inner.op, Op::Extend { .. })
                && region.values[inner.args[0].index()].ty
                    == region.values[inst.results[0].index()].ty
            {
                return Some(Alias(inner.args[0]));
            }
        },
        Op::Insert { lsb } => {
            let inner = definition(region, a[1])?;
            if inner.op == (Op::Extract { lsb }) && inner.args[0] == a[0] {
                return Some(Alias(a[0]));
            }
        },
        Op::Extract { lsb } => {
            let inner = definition(region, a[0])?;
            if let Op::Insert { lsb: inserted } = inner.op {
                let width = region.values[inst.results[0].index()].ty.bits()? as u16;
                let part = region.values[inner.args[1].index()].ty.bits()? as u16;
                let start = lsb as u16;
                let at = inserted as u16;
                if start >= at && start + width <= at + part {
                    return Some(if start == at && width == part {
                        Alias(inner.args[1])
                    } else {
                        Rewrite(
                            Op::Extract {
                                lsb: lsb - inserted,
                            },
                            vec![inner.args[1]],
                        )
                    });
                }
                if start + width <= at || start >= at + part {
                    return Some(Rewrite(inst.op.clone(), vec![inner.args[0]]));
                }
            }
        },
        _ => {},
    }
    None
}
fn vector(region: &Region, inst: &Instruction) -> Option<Change> {
    use Change::{Alias, Rewrite};
    let a = &inst.args;
    match &inst.op {
        Op::VectorBinary(PackedOp::And | PackedOp::Or) if a[0] == a[1] => Some(Alias(a[0])),
        Op::VectorExtract { bits, lane } => {
            let inner = definition(region, a[0])?;
            match &inner.op {
                Op::VectorReplace { bits: b, lane: l } if b == bits => {
                    if l != lane {
                        Some(Rewrite(inst.op.clone(), vec![inner.args[0]]))
                    } else if matches!(bits, 32 | 64) {
                        // Narrow extracts zero-extend; their I32 insertion
                        // operands may contain high bits, so cannot be aliased.
                        Some(Alias(inner.args[1]))
                    } else {
                        None
                    }
                },
                Op::VectorShuffle(lanes) => {
                    let bytes = *bits as usize / 8;
                    let start = *lane as usize * bytes;
                    let source = lanes[start] as usize;
                    if source % bytes == 0
                        && (0..bytes).all(|i| lanes[start + i] as usize == source + i)
                    {
                        Some(Rewrite(
                            Op::VectorExtract {
                                bits: *bits,
                                lane: ((source % 16) / bytes) as u8,
                            },
                            vec![inner.args[source / 16]],
                        ))
                    } else {
                        None
                    }
                },
                _ => None,
            }
        },
        Op::VectorReplace { bits, lane } => {
            if let Some(extracted) = definition(region, a[1]) {
                if extracted.op
                    == (Op::VectorExtract {
                        bits: *bits,
                        lane: *lane,
                    })
                    && extracted.args[0] == a[0]
                {
                    return Some(Alias(a[0]));
                }
            }
            let inner = definition(region, a[0])?;
            if inner.op == inst.op {
                Some(Rewrite(inst.op.clone(), vec![inner.args[0], a[1]]))
            } else {
                None
            }
        },
        Op::VectorShuffle(lanes) => {
            if lanes
                .iter()
                .enumerate()
                .all(|(i, &lane)| lane as usize == i)
            {
                return Some(Alias(a[0]));
            }
            if lanes
                .iter()
                .enumerate()
                .all(|(i, &lane)| lane as usize == i + 16)
            {
                return Some(Alias(a[1]));
            }
            if a[0] == a[1]
                && lanes
                    .iter()
                    .enumerate()
                    .all(|(i, &lane)| lane as usize % 16 == i)
            {
                return Some(Alias(a[0]));
            }
            // Expand one level only, and only when the result still has at
            // most two vector sources. More rounds have their own pass budget.
            let mut sources = Vec::new();
            let mut composed = [0; 16];
            let mut expanded = false;
            for (i, &lane) in lanes.iter().enumerate() {
                let mut source = a[lane as usize / 16];
                let mut byte = lane % 16;
                if let Some(inner) = definition(region, source) {
                    if let Op::VectorShuffle(selection) = &inner.op {
                        let selected = selection[byte as usize];
                        source = inner.args[selected as usize / 16];
                        byte = selected % 16;
                        expanded = true;
                    }
                }
                let slot = match sources.iter().position(|v| *v == source) {
                    Some(slot) => slot,
                    None if sources.len() < 2 => {
                        sources.push(source);
                        sources.len() - 1
                    },
                    None => return None,
                };
                composed[i] = byte + 16 * slot as u8;
            }
            if !expanded {
                return None;
            }
            if sources.len() == 1 {
                sources.push(sources[0]);
            }
            Some(Rewrite(Op::VectorShuffle(composed), sources))
        },
        _ => None,
    }
}
pub(super) fn run(region: &mut Region, stats: &mut PassStats) -> Result<(), String> {
    if region.blocks.len() > 64
        || region.instructions.len() > 8192
        || region.values.len() > 16384
        || region
            .instructions
            .iter()
            .map(|i| i.args.len())
            .sum::<usize>()
            > 65536
    {
        return Err("canonicalization region budget exceeded".into());
    }
    let cfg = Cfg::compute(region)?;
    let mut order: Vec<_> = (0..region.blocks.len()).collect();
    order.sort_by_key(|&b| cfg.dominates[b].iter().filter(|&&v| v).count());
    let mut aliases = vec![None; region.values.len()];
    let mut removed = HashSet::new();
    for b in order {
        for id in region.blocks[b].instructions.clone() {
            let mut inst = region.instructions[id.index()].clone();
            for arg in &mut inst.args {
                if let Some(value) = aliases[arg.index()] {
                    *arg = value;
                }
            }
            if !inst.op.ordered()
                && inst.results.len() == 1
                && inst.state.is_none()
                && inst.commit.is_none()
                && !inst.trap_after_fault
                && !inst.unmasked_word_store
            {
                let simd = vector(region, &inst);
                let is_simd = simd.is_some();
                if let Some(change) = simd.or_else(|| scalar(region, &inst)) {
                    match change {
                        Change::Alias(value) => {
                            aliases[inst.results[0].index()] = Some(value);
                            removed.insert(id);
                        },
                        Change::Rewrite(op, args) => {
                            inst.op = op;
                            inst.args = args;
                        },
                    }
                    stats.canonicalized += 1;
                    stats.simd_simplified += usize::from(is_simd);
                }
            }
            region.instructions[id.index()] = inst;
        }
    }
    rewrite_values(region, |value| {
        if let Some(new) = aliases[value.index()] {
            *value = new;
        }
    });
    for block in &mut region.blocks {
        block.instructions.retain(|id| !removed.contains(id));
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/canonicalize.rs"]
mod tests;
