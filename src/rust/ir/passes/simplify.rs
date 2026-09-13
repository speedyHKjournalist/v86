//! Bit-exact scalar identities and SIMD byte/lane dataflow. Never remove effects.
use super::rewrite_values;
use crate::ir::{analysis::cfg::Cfg, hir::*, ids::*, simd::PackedOp, types::Type, verify::verify};

pub const DEFAULT_WORK_LIMIT: usize = 1_000_000;
#[derive(Default, Debug)]
pub struct Stats {
    pub scalar: usize,
    pub vector: usize,
    pub work: usize,
}
struct Work {
    remaining: usize,
    used: usize,
}
impl Work {
    fn visit(&mut self) -> Result<(), String> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or("simplify work budget exceeded")?;
        self.used += 1;
        Ok(())
    }
}
enum Change {
    None,
    Alias(ValueId),
    Replace(Op, Vec<ValueId>),
    MaskWord(ValueId),
}
fn instruction(r: &Region, v: ValueId) -> Option<&Instruction> {
    let Definition::Instruction(id, _) = r.values[v.index()].definition else {
        return None;
    };
    let inst = &r.instructions[id.index()];
    (inst.state.is_none() && inst.commit.is_none() && !inst.op.ordered()).then_some(inst)
}
fn scalar(r: &Region, v: ValueId) -> Option<u64> {
    match instruction(r, v)?.op {
        Op::Const(n) => Some(n),
        _ => None,
    }
}
fn vector(r: &Region, v: ValueId) -> Option<[u8; 16]> {
    match instruction(r, v)?.op {
        Op::VectorConst(n) => Some(n),
        _ => None,
    }
}
fn literal(n: u64) -> Change {
    Change::Replace(Op::Const(n), vec![])
}
fn vector_literal(n: [u8; 16]) -> Change {
    Change::Replace(Op::VectorConst(n), vec![])
}
fn identity(r: &Region, inst: &Instruction) -> Change {
    use Binary::*;
    let a = &inst.args;
    match inst.op {
        Op::Binary(op) => {
            let (left, right) = (scalar(r, a[0]), scalar(r, a[1]));
            let bits = r.values[a[0].index()].ty.bits().unwrap();
            let mask = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
            if a[0] == a[1] {
                match op {
                    Sub | Xor | Ult | Slt => return literal(0),
                    Eq => return literal(1),
                    And | Or => return Change::Alias(a[0]),
                    _ => {},
                }
            }
            match op {
                Add | Or | Xor if left == Some(0) => Change::Alias(a[1]),
                Add | Sub | Or | Xor if right == Some(0) => Change::Alias(a[0]),
                Mul | And if left == Some(0) || right == Some(0) => literal(0),
                Mul if left == Some(1) => Change::Alias(a[1]),
                Mul if right == Some(1) => Change::Alias(a[0]),
                And if left == Some(mask) => Change::Alias(a[1]),
                And if right == Some(mask) => Change::Alias(a[0]),
                Or if left == Some(mask) || right == Some(mask) => literal(mask),
                Shl | Shr | Sar if left == Some(0) => literal(0),
                // HIR shifts use the Wasm machine count mask, not narrow lane width.
                Shl | Shr | Sar
                    if right.is_some_and(|v| v & if bits == 64 { 63 } else { 31 } == 0) =>
                {
                    Change::Alias(a[0])
                },
                _ => Change::None,
            }
        },
        Op::Select if a[1] == a[2] => Change::Alias(a[1]),
        Op::Select => match scalar(r, a[0]) {
            Some(0) => Change::Alias(a[2]),
            Some(_) => Change::Alias(a[1]),
            None => Change::None,
        },
        Op::Truncate => {
            if let Some(inner) = instruction(r, a[0]) {
                if matches!(inner.op, Op::Extend { .. })
                    && r.values[inner.args[0].index()].ty == r.values[inst.results[0].index()].ty
                {
                    return Change::Alias(inner.args[0]);
                }
            }
            Change::None
        },
        _ => Change::None,
    }
}
/// Follow shuffle definitions without recursion. Budget failure rejects the whole
/// staged pass, rather than silently accepting only part of a long chain.
fn byte_source(
    r: &Region,
    mut value: ValueId,
    mut byte: u8,
    work: &mut Work,
) -> Result<(ValueId, u8), String> {
    loop {
        work.visit()?;
        let Some(inst) = instruction(r, value) else {
            return Ok((value, byte));
        };
        let Op::VectorShuffle(lanes) = inst.op else {
            return Ok((value, byte));
        };
        let source = lanes[byte as usize];
        value = inst.args[(source / 16) as usize];
        byte = source % 16;
    }
}
fn shuffle(
    r: &Region,
    inst: &Instruction,
    lanes: [u8; 16],
    work: &mut Work,
) -> Result<Change, String> {
    let mut sources = Vec::new();
    let mut rewritten = [0u8; 16];
    let mut bytes = [0u8; 16];
    let mut all_constant = true;
    for (i, lane) in lanes.iter().enumerate() {
        let (source, byte) = byte_source(r, inst.args[(lane / 16) as usize], lane % 16, work)?;
        if let Some(data) = vector(r, source) {
            bytes[i] = data[byte as usize];
        } else {
            all_constant = false;
        }
        let index = if let Some(index) = sources.iter().position(|&v| v == source) {
            index
        } else {
            sources.push(source);
            sources.len() - 1
        };
        rewritten[i] = (index * 16) as u8 + byte;
    }
    if all_constant {
        return Ok(vector_literal(bytes));
    }
    // One Wasm shuffle has two operands. Do not widen or duplicate computation
    // when flattening would require a third source vector.
    if sources.len() > 2 {
        return Ok(Change::None);
    }
    if sources.len() == 1 && rewritten.iter().enumerate().all(|(i, &n)| n as usize == i) {
        return Ok(Change::Alias(sources[0]));
    }
    if sources.len() == 1 {
        sources.push(sources[0]);
    }
    Ok(Change::Replace(Op::VectorShuffle(rewritten), sources))
}
fn simd(r: &Region, inst: &Instruction, work: &mut Work) -> Result<Change, String> {
    use PackedOp::*;
    let a = &inst.args;
    Ok(match inst.op {
        Op::VectorBinary(op) => {
            if a[0] == a[1] {
                match op {
                    And | Or | MinU8 | MaxU8 | MinS16 | MaxS16 | AvgU8 | AvgU16 => {
                        return Ok(Change::Alias(a[0]))
                    },
                    Xor | AndNot | Sub8 | Sub16 | Sub32 | Sub64 | SubSatS8 | SubSatS16
                    | SubSatU8 | SubSatU16 | GtS8 | GtS16 | GtS32 => {
                        return Ok(vector_literal([0; 16]))
                    },
                    Eq8 | Eq16 | Eq32 => return Ok(vector_literal([255; 16])),
                    _ => {},
                }
            }
            let left = vector(r, a[0]);
            let right = vector(r, a[1]);
            if let (Some(left), Some(right)) = (left, right) {
                if matches!(op, And | AndNot | Or | Xor) {
                    return Ok(vector_literal(std::array::from_fn(|i| match op {
                        And => left[i] & right[i],
                        AndNot => !left[i] & right[i],
                        Or => left[i] | right[i],
                        Xor => left[i] ^ right[i],
                        _ => unreachable!(),
                    })));
                }
            }
            let zero = Some([0; 16]);
            match op {
                And if left == zero || right == zero => vector_literal([0; 16]),
                Or | Xor | Add8 | Add16 | Add32 | Add64 | AddSatS8 | AddSatS16 | AddSatU8
                | AddSatU16
                    if left == zero =>
                {
                    Change::Alias(a[1])
                },
                Or | Xor | Add8 | Add16 | Add32 | Add64 | AddSatS8 | AddSatS16 | AddSatU8
                | AddSatU16 | Sub8 | Sub16 | Sub32 | Sub64 | SubSatS8 | SubSatS16 | SubSatU8
                | SubSatU16 | Shl16 | Shl32 | Shl64 | Shr16 | Shr32 | Shr64 | Sar16 | Sar32
                    if right == zero =>
                {
                    Change::Alias(a[0])
                },
                _ => Change::None,
            }
        },
        Op::VectorShuffle(lanes) => return shuffle(r, inst, lanes, work),
        Op::VectorExtract { bits, lane } => {
            let start = (bits / 8) * lane;
            if let Some(bytes) = vector(r, a[0]) {
                let mut scalar = [0u8; 8];
                scalar[..(bits / 8) as usize]
                    .copy_from_slice(&bytes[start as usize..(start + bits / 8) as usize]);
                return Ok(literal(u64::from_le_bytes(scalar)));
            }
            if let Some(inner) = instruction(r, a[0]) {
                if let Op::VectorReplace {
                    bits: old_bits,
                    lane: old_lane,
                } = inner.op
                {
                    if bits == old_bits && lane == old_lane {
                        return Ok(if bits == 16 {
                            Change::MaskWord(inner.args[1])
                        } else {
                            Change::Alias(inner.args[1])
                        });
                    }
                    let old_start = old_bits / 8 * old_lane;
                    if start + bits / 8 <= old_start || old_start + old_bits / 8 <= start {
                        return Ok(Change::Replace(inst.op.clone(), vec![inner.args[0]]));
                    }
                }
            }
            let first = byte_source(r, a[0], start, work)?;
            let mut contiguous = first.1 % (bits / 8) == 0;
            for offset in 1..bits / 8 {
                contiguous &=
                    byte_source(r, a[0], start + offset, work)? == (first.0, first.1 + offset);
            }
            if contiguous && first.0 != a[0] {
                Change::Replace(
                    Op::VectorExtract {
                        bits,
                        lane: first.1 / (bits / 8),
                    },
                    vec![first.0],
                )
            } else {
                Change::None
            }
        },
        Op::VectorReplace { bits, lane } => {
            if let (Some(mut bytes), Some(value)) = (vector(r, a[0]), scalar(r, a[1])) {
                let start = (bits / 8 * lane) as usize;
                bytes[start..start + (bits / 8) as usize]
                    .copy_from_slice(&value.to_le_bytes()[..(bits / 8) as usize]);
                return Ok(vector_literal(bytes));
            }
            if let Some(inner) = instruction(r, a[0]) {
                if inner.op == inst.op {
                    return Ok(Change::Replace(inst.op.clone(), vec![inner.args[0], a[1]]));
                }
            }
            Change::None
        },
        _ => Change::None,
    })
}
/// Stage all substitutions and schedule changes, verify, then publish atomically.
/// Only CPU SSA values change; effects, helpers, polls and fault observers remain.
pub fn run(region: &mut Region, work_limit: usize) -> Result<Stats, String> {
    if region.blocks.len() > 64
        || region.instructions.len() > 8192
        || region.values.len() > 16384
        || region.states.len() > 8192
        || region.helpers.len() > 1024
    {
        return Err("simplify region budget exceeded".into());
    }
    let mut work = Work {
        remaining: work_limit,
        used: 0,
    };
    work.visit()?;
    verify(region).map_err(|e| e.0)?;
    let cfg = Cfg::compute(region)?;
    let mut order: Vec<_> = (0..region.blocks.len()).collect();
    order.sort_by_key(|&b| (cfg.dominates[b].iter().filter(|&&d| d).count(), b));
    let mut staged = region.clone();
    let mut aliases = vec![None; staged.values.len()];
    let mut stats = Stats::default();
    for b in order {
        let ids = std::mem::take(&mut staged.blocks[b].instructions);
        let terminator = staged.blocks[b].terminator.take();
        for id in ids {
            work.visit()?;
            let inst = &mut staged.instructions[id.index()];
            for arg in &mut inst.args {
                work.visit()?;
                if let Some(new) = aliases[arg.index()] {
                    *arg = new;
                }
            }
            let inst = &staged.instructions[id.index()];
            let change = if inst.results.len() != 1
                || inst.state.is_some()
                || inst.commit.is_some()
                || inst.op.ordered()
            {
                Change::None
            } else {
                let basic = identity(&staged, inst);
                if matches!(basic, Change::None) {
                    simd(&staged, inst, &mut work)?
                } else {
                    basic
                }
            };
            let is_vector = matches!(
                inst.op,
                Op::VectorBinary(_)
                    | Op::VectorShuffle(_)
                    | Op::VectorExtract { .. }
                    | Op::VectorReplace { .. }
            );
            let mut keep = true;
            let changed = match change {
                Change::None => false,
                Change::Alias(value) => {
                    aliases[inst.results[0].index()] = Some(value);
                    keep = false;
                    true
                },
                Change::Replace(op, args) => {
                    let changed = inst.op != op || inst.args != args;
                    staged.instructions[id.index()].op = op;
                    staged.instructions[id.index()].args = args;
                    changed
                },
                Change::MaskWord(value) => {
                    if staged.instructions.len() == 8192 || staged.values.len() == 16384 {
                        return Err("simplify region budget exceeded".into());
                    }
                    let mask = staged.append(
                        BlockId(b as u32),
                        Op::Const(65535),
                        vec![],
                        &[Type::I32],
                        None,
                    )[0];
                    aliases.push(None);
                    staged.instructions[id.index()].op = Op::Binary(Binary::And);
                    staged.instructions[id.index()].args = vec![value, mask];
                    true
                },
            };
            if changed {
                if is_vector {
                    stats.vector += 1;
                } else {
                    stats.scalar += 1;
                }
            }
            if keep {
                staged.blocks[b].instructions.push(id);
            }
        }
        staged.blocks[b].terminator = terminator;
    }
    // Targets are already canonical and dominate the removed definitions. This
    // also rewrites recovery-only FLAGS/XMM, dynamic PCs and dynamic count bases.
    rewrite_values(&mut staged, |v| {
        if let Some(new) = aliases[v.index()] {
            *v = new;
        }
    });
    verify(&staged).map_err(|e| e.0)?;
    stats.work = work.used;
    *region = staged;
    Ok(stats)
}

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/simplify.rs"]
mod tests;
