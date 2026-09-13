//! Exact integer and bit-vector identities. No floating-point reassociation,
//! memory forwarding, CPU-state reuse or exception speculation is permitted.
use super::{constant, rewrite_values};
use crate::ir::{analysis::cfg::Cfg, hir::*, ids::*, simd::PackedOp, verify::verify};

pub const DEFAULT_WORK_LIMIT: usize = 1_000_000;
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub aliases: usize,
    pub constants: usize,
    pub vectors: usize,
    pub work: usize,
}

enum Rewrite {
    Alias(ValueId),
    Constant(u64),
    Expression(Op, Vec<ValueId>),
}

fn definition(r: &Region, value: ValueId) -> Option<&Instruction> {
    let Definition::Instruction(id, _) = r.values[value.index()].definition else {
        return None;
    };
    Some(&r.instructions[id.index()])
}

fn scalar(r: &Region, inst: &Instruction) -> Option<Rewrite> {
    use Binary::*;
    use Rewrite::*;
    let args = &inst.args;
    match inst.op {
        Op::Select if args[1] == args[2] => Some(Alias(args[1])),
        Op::Select => constant(r, args[0]).map(|v| Alias(args[if v == 0 { 2 } else { 1 }])),
        Op::Extract { lsb: 0 }
            if r.values[args[0].index()].ty == r.values[inst.results[0].index()].ty =>
        {
            Some(Alias(args[0]))
        },
        Op::Truncate => {
            let input = definition(r, args[0])?;
            if matches!(input.op, Op::Extend { .. })
                && r.values[input.args[0].index()].ty == r.values[inst.results[0].index()].ty
            {
                Some(Alias(input.args[0]))
            } else {
                None
            }
        },
        Op::Binary(op) => {
            let a = args[0];
            let b = args[1];
            // Keep literal folding in the existing pass, including its statistics.
            if constant(r, a).is_some() && constant(r, b).is_some() {
                return None;
            }
            let bits = r.values[a.index()].ty.bits()?;
            let mask = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
            if a == b {
                match op {
                    And | Or => return Some(Alias(a)),
                    Sub | Xor | Ult | Slt => return Some(Constant(0)),
                    Eq => return Some(Constant(1)),
                    _ => {},
                }
            }
            let ca = constant(r, a);
            let cb = constant(r, b);
            match op {
                Add | Or | Xor if ca == Some(0) => Some(Alias(b)),
                Add | Sub | Or | Xor | Shl | Shr | Sar if cb == Some(0) => Some(Alias(a)),
                Mul if ca == Some(1) => Some(Alias(b)),
                Mul if cb == Some(1) => Some(Alias(a)),
                Mul | And if ca == Some(0) || cb == Some(0) => Some(Constant(0)),
                And if ca == Some(mask) => Some(Alias(b)),
                And if cb == Some(mask) => Some(Alias(a)),
                Or if ca == Some(mask) || cb == Some(mask) => Some(Constant(mask)),
                // Deterministic operand order also exposes commuted expressions to GVN.
                Add | Mul | And | Or | Xor | Eq if a.index() > b.index() => {
                    Some(Expression(inst.op.clone(), vec![b, a]))
                },
                _ => None,
            }
        },
        _ => None,
    }
}

/// Compose one shuffle level. Keeping no more than two distinct vector sources
/// preserves the existing Wasm shuffle signature; otherwise leave it unchanged.
fn shuffle(r: &Region, inst: &Instruction, lanes: &[u8; 16]) -> Option<Rewrite> {
    let mut sources = Vec::new();
    let mut composed = [0u8; 16];
    for (output, &lane) in lanes.iter().enumerate() {
        let mut value = inst.args[usize::from(lane / 16)];
        let mut byte = lane % 16;
        if let Some(inner) = definition(r, value) {
            if let Op::VectorShuffle(inner_lanes) = &inner.op {
                let selected = inner_lanes[usize::from(byte)];
                value = inner.args[usize::from(selected / 16)];
                byte = selected % 16;
            }
        }
        let index = if let Some(index) = sources.iter().position(|v| *v == value) {
            index
        } else {
            if sources.len() == 2 {
                return None;
            }
            sources.push(value);
            sources.len() - 1
        };
        composed[output] = index as u8 * 16 + byte;
    }
    if sources.len() == 1
        && composed
            .iter()
            .enumerate()
            .all(|(i, &v)| i == usize::from(v))
    {
        return Some(Rewrite::Alias(sources[0]));
    }
    if sources.len() == 1 {
        sources.push(sources[0]);
    }
    if composed == *lanes && sources == inst.args {
        return None;
    }
    Some(Rewrite::Expression(Op::VectorShuffle(composed), sources))
}

fn vector(r: &Region, inst: &Instruction) -> Option<Rewrite> {
    use Rewrite::*;
    match inst.op {
        Op::VectorShuffle(ref lanes) => shuffle(r, inst, lanes),
        Op::VectorBinary(op)
            if inst.args[0] == inst.args[1]
                && matches!(
                    op,
                    PackedOp::And
                        | PackedOp::Or
                        | PackedOp::MinU8
                        | PackedOp::MaxU8
                        | PackedOp::MinS16
                        | PackedOp::MaxS16
                        | PackedOp::AvgU8
                        | PackedOp::AvgU16
                ) =>
        {
            Some(Alias(inst.args[0]))
        },
        Op::VectorExtract { bits, lane } => {
            let inner = definition(r, inst.args[0])?;
            let Op::VectorReplace {
                bits: inner_bits,
                lane: inner_lane,
            } = inner.op
            else {
                return None;
            };
            if bits != inner_bits {
                return None;
            }
            if lane != inner_lane {
                Some(Expression(inst.op.clone(), vec![inner.args[0]]))
            } else if bits == 32 || bits == 64 {
                Some(Alias(inner.args[1]))
            } else {
                // PINSRW drops the high sixteen bits, PEXTRW zero-extends.
                // Aliasing the original i32 here would be a miscompile.
                None
            }
        },
        Op::VectorReplace { bits, lane } => {
            let inner = definition(r, inst.args[0])?;
            if inner.op == (Op::VectorReplace { bits, lane }) {
                Some(Expression(
                    inst.op.clone(),
                    vec![inner.args[0], inst.args[1]],
                ))
            } else {
                None
            }
        },
        _ => None,
    }
}

/// All rewrites are staged. Work exhaustion or failed verification leaves the
/// original region, including StateMap-only uses, byte-for-byte unchanged.
pub fn run(region: &mut Region, work_limit: usize) -> Result<Stats, String> {
    if region.blocks.len() > 64
        || region.instructions.len() > 8192
        || region.values.len() > 16384
        || region.states.len() > 8192
        || region.helpers.len() > 1024
    {
        return Err("canonicalization region budget exceeded".into());
    }
    if work_limit == 0 {
        return Err("canonicalization work budget exceeded".into());
    }
    verify(region).map_err(|e| e.0)?;
    let cfg = Cfg::compute(region)?;
    let mut r = region.clone();
    let mut order: Vec<_> = (0..r.blocks.len()).collect();
    order.sort_by_key(|&b| (cfg.dominates[b].iter().filter(|&&v| v).count(), b));
    let mut aliases = vec![None; r.values.len()];
    let mut removed = vec![false; r.instructions.len()];
    let mut stats = Stats {
        work: 1,
        ..Stats::default()
    };
    for block in order {
        for id in r.blocks[block].instructions.clone() {
            let cost = 1 + r.instructions[id.index()].args.len() + 16;
            stats.work = stats
                .work
                .checked_add(cost)
                .ok_or("canonicalization work overflow")?;
            if stats.work > work_limit {
                return Err("canonicalization work budget exceeded".into());
            }
            let inst = &mut r.instructions[id.index()];
            for value in &mut inst.args {
                if let Some(new) = aliases[value.index()] {
                    *value = new;
                }
            }
            if inst.results.len() != 1
                || inst.op.ordered()
                || inst.state.is_some()
                || inst.commit.is_some()
                || inst.trap_after_fault
                || inst.unmasked_word_store
            {
                continue;
            }
            let inst = &r.instructions[id.index()];
            let is_vector = matches!(
                inst.op,
                Op::VectorShuffle(_)
                    | Op::VectorBinary(_)
                    | Op::VectorExtract { .. }
                    | Op::VectorReplace { .. }
            );
            let rewrite = if is_vector { vector(&r, inst) } else { scalar(&r, inst) };
            let Some(rewrite) = rewrite else { continue };
            let inst = &mut r.instructions[id.index()];
            match rewrite {
                Rewrite::Alias(value) => {
                    if r.values[value.index()].ty != r.values[inst.results[0].index()].ty {
                        return Err("canonicalization alias type mismatch".into());
                    }
                    aliases[inst.results[0].index()] = Some(value);
                    removed[id.index()] = true;
                    stats.aliases += 1;
                },
                Rewrite::Constant(value) => {
                    inst.op = Op::Const(value);
                    inst.args.clear();
                    stats.constants += 1;
                },
                Rewrite::Expression(op, args) => {
                    inst.op = op;
                    inst.args = args;
                },
            }
            stats.vectors += usize::from(is_vector);
        }
    }
    rewrite_values(&mut r, |value| {
        if let Some(new) = aliases[value.index()] {
            *value = new;
        }
    });
    for block in &mut r.blocks {
        block.instructions.retain(|id| !removed[id.index()]);
    }
    verify(&r).map_err(|e| e.0)?;
    *region = r;
    Ok(stats)
}

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/canonicalize.rs"]
mod tests;
