//! Bounded, transactional simplification of pure vector SSA expressions.
//!
//! These rewrites neither speculate nor move instructions. In particular, CPU
//! reads, SSE checks, guest memory and recovery points are not candidates. Byte
//! provenance is used only for a single shuffle level at a time; it is not a
//! guest-memory alias or permission proof.
use super::rewrite_values;
use crate::ir::{analysis::cfg::Cfg, hir::*, ids::*, simd::PackedOp, types::Type, verify::verify};

pub const DEFAULT_WORK_LIMIT: usize = 1_000_000;
const MAX_INSTRUCTIONS: usize = 8192;
const MAX_VALUES: usize = 16384;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Stats {
    pub aliases: usize,
    pub rewritten: usize,
    pub shuffles: usize,
    pub lanes: usize,
    pub bitwise: usize,
    pub work: usize,
}
impl Stats {
    pub fn simplified(&self) -> usize { self.aliases + self.rewritten }
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
                | Op::VectorExtract { .. }
                | Op::VectorReplace { .. }
                | Op::VectorBinary(_)
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

/// Cheap pipeline prefilter. The caller has already verified this region.
/// Integer-only blocks and unrelated packed arithmetic do not pay for cloning,
/// dominance analysis or an additional verifier round.
pub(super) fn may_simplify(region: &Region) -> bool {
    region
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .any(|id| {
            let inst = &region.instructions[id.index()];
            if !pure(inst) {
                return false;
            }
            match inst.op {
                Op::VectorShuffle(_) => true,
                Op::VectorBinary(PackedOp::And | PackedOp::Or) => inst.args[0] == inst.args[1],
                Op::VectorExtract { .. } => definition(region, inst.args[0]).is_some_and(|i| {
                    matches!(i.op, Op::VectorReplace { .. } | Op::VectorShuffle(_))
                }),
                Op::VectorReplace { .. } => {
                    definition(region, inst.args[0])
                        .is_some_and(|i| matches!(i.op, Op::VectorReplace { .. }))
                        || definition(region, inst.args[1])
                            .is_some_and(|i| matches!(i.op, Op::VectorExtract { .. }))
                },
                _ => false,
            }
        })
}

enum Action {
    Keep,
    Alias(ValueId),
    Rewrite(Op, Vec<ValueId>),
    Low16(ValueId),
}

fn shuffle(
    region: &Region,
    inst: &Instruction,
    mask: [u8; 16],
    work: &mut Work,
) -> Result<Action, String> {
    work.spend(16)?;
    if mask.iter().enumerate().all(|(i, &b)| b as usize == i) {
        return Ok(Action::Alias(inst.args[0]));
    }
    if mask.iter().enumerate().all(|(i, &b)| b as usize == i + 16) {
        return Ok(Action::Alias(inst.args[1]));
    }
    // Resolve each selected byte through at most one inner shuffle. Fusion is
    // representable precisely when no more than two distinct inputs remain.
    // Looking only at selected bytes also permits unused third/fourth inputs.
    let mut inputs = Vec::new();
    let mut composed = [0; 16];
    for (i, byte) in mask.into_iter().enumerate() {
        work.spend(1)?;
        let mut source = inst.args[(byte / 16) as usize];
        let mut lane = byte % 16;
        if let Some(inner) = definition(region, source) {
            if let Op::VectorShuffle(map) = inner.op {
                let selected = map[lane as usize];
                source = inner.args[(selected / 16) as usize];
                lane = selected % 16;
            }
        }
        let slot = if let Some(slot) = inputs.iter().position(|&v| v == source) {
            slot
        }
        else {
            if inputs.len() == 2 {
                return Ok(Action::Keep);
            }
            inputs.push(source);
            inputs.len() - 1
        };
        composed[i] = lane + slot as u8 * 16;
    }
    if inputs.len() == 1 {
        inputs.push(inputs[0]);
    }
    if composed == mask && inputs == inst.args {
        Ok(Action::Keep)
    }
    else {
        Ok(Action::Rewrite(Op::VectorShuffle(composed), inputs))
    }
}

fn simplify(region: &Region, inst: &Instruction, work: &mut Work) -> Result<Action, String> {
    match inst.op {
        Op::VectorShuffle(mask) => shuffle(region, inst, mask, work),
        Op::VectorBinary(PackedOp::And | PackedOp::Or) if inst.args[0] == inst.args[1] => {
            Ok(Action::Alias(inst.args[0]))
        },
        Op::VectorExtract { bits, lane } => {
            let Some(inner) = definition(region, inst.args[0])
            else {
                return Ok(Action::Keep);
            };
            match inner.op {
                Op::VectorReplace {
                    bits: written_bits,
                    lane: written_lane,
                } => {
                    if bits == written_bits && lane == written_lane {
                        // PINSRW consumes low16 of an i32; PEXTRW zero-extends it.
                        return Ok(if bits == 16 {
                            Action::Low16(inner.args[1])
                        }
                        else {
                            Action::Alias(inner.args[1])
                        });
                    }
                    let start = bits as u16 * lane as u16;
                    let written = written_bits as u16 * written_lane as u16;
                    if start + bits as u16 <= written || written + written_bits as u16 <= start {
                        Ok(Action::Rewrite(inst.op.clone(), vec![inner.args[0]]))
                    }
                    else {
                        // Mixed-width or partially overlapping lanes cannot forward.
                        Ok(Action::Keep)
                    }
                },
                Op::VectorShuffle(mask) => {
                    let bytes = bits / 8;
                    let start = bytes as usize * lane as usize;
                    let first = mask[start];
                    work.spend(bytes as usize)?;
                    if first % bytes == 0
                        && mask[start..start + bytes as usize]
                            .iter()
                            .enumerate()
                            .all(|(i, &b)| b == first + i as u8)
                    {
                        Ok(Action::Rewrite(
                            Op::VectorExtract {
                                bits,
                                lane: first % 16 / bytes,
                            },
                            vec![inner.args[(first / 16) as usize]],
                        ))
                    }
                    else {
                        Ok(Action::Keep)
                    }
                },
                _ => Ok(Action::Keep),
            }
        },
        Op::VectorReplace { bits, lane } => {
            if let Some(source) = definition(region, inst.args[1]) {
                if source.op == (Op::VectorExtract { bits, lane }) && source.args[0] == inst.args[0]
                {
                    return Ok(Action::Alias(inst.args[0]));
                }
            }
            if let Some(inner) = definition(region, inst.args[0]) {
                if inner.op == (Op::VectorReplace { bits, lane }) {
                    return Ok(Action::Rewrite(
                        inst.op.clone(),
                        vec![inner.args[0], inst.args[1]],
                    ));
                }
            }
            Ok(Action::Keep)
        },
        _ => Ok(Action::Keep),
    }
}

fn resolve(
    aliases: &[Option<ValueId>],
    mut value: ValueId,
    work: &mut Work,
) -> Result<ValueId, String> {
    while let Some(next) = aliases[value.index()] {
        work.spend(1)?;
        value = next;
    }
    Ok(value)
}

// A fresh constant immediately precedes the rewritten extraction. Keeping its
// original i32 result ID permits all recovery maps to remain well-typed. GVN and
// DCE in the ordinary pipeline can common masks and remove obsolete vector nodes.
fn low16_mask(
    region: &mut Region,
    block: BlockId,
    work: &mut Work,
) -> Result<(InstId, ValueId), String> {
    work.spend(1)?;
    if region.instructions.len() == MAX_INSTRUCTIONS || region.values.len() == MAX_VALUES {
        return Err("SIMD arena growth budget exceeded".into());
    }
    let id = InstId(region.instructions.len() as u32);
    let result = ValueId(region.values.len() as u32);
    region.values.push(Value {
        ty: Type::I32,
        definition: Definition::Instruction(id, 0),
    });
    region.instructions.push(Instruction {
        block,
        op: Op::Const(0xFFFF),
        args: vec![],
        results: vec![result],
        state: None,
        commit: None,
        trap_after_fault: false,
        unmasked_word_store: false,
    });
    Ok((id, result))
}

/// Simplify without changing the CFG, effect order or guest-instruction count.
/// On any verification or budget error the entire original region is unchanged.
pub fn run(region: &mut Region, work_limit: usize) -> Result<Stats, String> {
    if region.blocks.len() > 64
        || region.entries.len() > 64
        || region.instructions.len() > MAX_INSTRUCTIONS
        || region.values.len() > MAX_VALUES
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
    // Bound variable-length metadata before cloning and calling the verifier.
    for block in &region.blocks {
        work.spend(block.params.len())?;
        work.spend(block.instructions.len())?;
        if let Some(term) = &block.terminator {
            for edge in term.edges() {
                work.spend(edge.args.len())?;
            }
        }
    }
    for inst in &region.instructions {
        work.spend(inst.args.len())?;
        work.spend(inst.results.len())?;
    }
    for state in &region.states {
        work.spend(24)?;
        work.spend(state.xmm.len())?;
        work.spend(state.x87.len())?;
    }
    for helper in &region.helpers {
        work.spend(helper.name.len())?;
        work.spend(helper.params.len())?;
        work.spend(helper.results.len())?;
        if let crate::ir::helper::HelperAbi::Outcome {
            fault_delivery: Some(name),
            ..
        } = &helper.abi
        {
            work.spend(name.len())?;
        }
    }
    verify(region).map_err(|e| e.0)?;
    let cfg = Cfg::compute(region)?;
    let mut order: Vec<_> = (0..region.blocks.len()).collect();
    order.sort_by_key(|&b| (cfg.dominates[b].iter().filter(|&&d| d).count(), b));
    let mut staged = region.clone();
    let mut aliases = vec![None; region.values.len()];
    let mut stats = Stats::default();
    for b in order {
        let mut schedule = Vec::new();
        for id in staged.blocks[b].instructions.clone() {
            work.spend(1)?;
            for arg in &mut staged.instructions[id.index()].args {
                *arg = resolve(&aliases, *arg, &mut work)?;
            }
            let mut removed = false;
            while pure(&staged.instructions[id.index()]) {
                work.spend(1)?;
                let inst = &staged.instructions[id.index()];
                let action = simplify(&staged, inst, &mut work)?;
                if matches!(action, Action::Keep) {
                    break;
                }
                match inst.op {
                    Op::VectorShuffle(_) => stats.shuffles += 1,
                    Op::VectorBinary(_) => stats.bitwise += 1,
                    _ => stats.lanes += 1,
                }
                match action {
                    Action::Alias(value) => {
                        let result = inst.results[0];
                        aliases[result.index()] = Some(resolve(&aliases, value, &mut work)?);
                        stats.aliases += 1;
                        removed = true;
                        break;
                    },
                    Action::Rewrite(op, args) => {
                        let inst = &mut staged.instructions[id.index()];
                        inst.op = op;
                        inst.args = args;
                        stats.rewritten += 1;
                    },
                    Action::Low16(source) => {
                        let (mask_id, mask) =
                            low16_mask(&mut staged, BlockId(b as u32), &mut work)?;
                        aliases.push(None);
                        schedule.push(mask_id);
                        let inst = &mut staged.instructions[id.index()];
                        inst.op = Op::Binary(Binary::And);
                        inst.args = vec![source, mask];
                        stats.rewritten += 1;
                    },
                    Action::Keep => unreachable!(),
                }
            }
            if !removed {
                schedule.push(id);
            }
        }
        staged.blocks[b].instructions = schedule;
    }
    // Canonical destinations precede aliases in dominance order. Resolve once,
    // then rewrite instruction, edge and every recovery-only use simultaneously.
    for i in 0..aliases.len() {
        if let Some(value) = aliases[i] {
            aliases[i] = Some(resolve(&aliases, value, &mut work)?);
        }
    }
    rewrite_values(&mut staged, |value| {
        if let Some(new) = aliases[value.index()] {
            *value = new;
        }
    });
    verify(&staged).map_err(|e| e.0)?;
    stats.work = work.used;
    *region = staged;
    Ok(stats)
}

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/simd_opt.rs"]
mod tests;
