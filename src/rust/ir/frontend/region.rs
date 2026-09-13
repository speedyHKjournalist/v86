//! Reachable bytecode CFGs composed from the existing single-instruction lifter.
use super::{
    decode::{decode, DecodedInstruction, GuestEip, LinearAddress},
    integer::IntegerBuilder,
    lift::lift_cpu_with_rep_budget,
};
use crate::ir::{
    hir::*,
    ids::*,
    lowering::CompileError,
    state::{FlagState, ResumeKind, StateMap},
    types::Type,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct Frame {
    gpr: [ValueId; 8],
    flags: FlagState,
    xmm: Vec<ValueId>,
    count: ValueId,
    effect: ValueId,
}
impl Frame {
    fn params(r: &mut Region, block: BlockId, xmm: bool) -> Self {
        Self {
            gpr: std::array::from_fn(|_| r.param(block, Type::I32)),
            flags: FlagState {
                arithmetic: std::array::from_fn(|_| r.param(block, Type::I1)),
                system: r.param(block, Type::I32),
                last_op1: Some(r.param(block, Type::I32)),
                raw_zero: Some(r.param(block, Type::I1)),
                zero_is_lazy: Some(r.param(block, Type::I1)),
            },
            xmm: if xmm { (0..8).map(|_| r.param(block, Type::V128)).collect() } else { vec![] },
            count: r.param(block, Type::I32),
            effect: r.param(block, Type::Effect),
        }
    }
    fn args(&self) -> Vec<ValueId> {
        let mut args = self.gpr.to_vec();
        args.extend(self.flags.arithmetic);
        args.push(self.flags.system);
        args.push(self.flags.last_op1.unwrap());
        args.push(self.flags.raw_zero.unwrap());
        args.push(self.flags.zero_is_lazy.unwrap());
        args.extend(&self.xmm);
        args.extend([self.count, self.effect]);
        args
    }
    fn recovery(&self, i: &DecodedInstruction) -> StateMap {
        StateMap {
            instruction_pc: i.instruction_pc,
            next_pc: i.next_pc,
            next_value: None,
            resume: ResumeKind::BeforeInstruction,
            gpr: self.gpr,
            flags: self.flags.clone(),
            xmm: self.xmm.clone(),
            x87: vec![],
            committed_instructions: 0,
            count_base: Some(self.count),
            rep_progress: None,
        }
    }
}
struct Fragment {
    decoded: DecodedInstruction,
    ir: Region,
    stop: bool,
}
fn offset(pc: GuestEip, start: GuestEip, len: usize) -> Option<usize> {
    let n = pc.0.wrapping_sub(start.0) as usize;
    (n < len).then_some(n)
}
fn successor(state: &StateMap, start: GuestEip, len: usize) -> Option<usize> {
    (state.resume == ResumeKind::AfterInstruction && state.next_value.is_none())
        .then(|| offset(state.next_pc, start, len))
        .flatten()
}
fn invalid(message: &'static str) -> CompileError {
    CompileError::Unsupported(message)
}

pub fn lift_cpu_cfg(
    bytes: &[u8],
    pc: GuestEip,
    linear: LinearAddress,
    default_32: bool,
    rep_budget: u32,
) -> Result<Region, CompileError> {
    if bytes.is_empty() {
        return Err(invalid("empty CFG snapshot"));
    }
    if bytes.len() > 15 * 128 {
        return Err(CompileError::Budget("CFG code snapshot"));
    }
    let mut fragments: BTreeMap<usize, Fragment> = BTreeMap::new();
    let mut pending = BTreeSet::from([0usize]);
    while let Some(at) = pending.pop_first() {
        if fragments.contains_key(&at) {
            continue;
        }
        if fragments.len() >= 128 {
            return Err(CompileError::Budget("CFG decoded instructions"));
        }
        let decoded = decode(
            &bytes[at..],
            GuestEip(pc.0.wrapping_add(at as u32)),
            LinearAddress(linear.0.wrapping_add(at as u32)),
            default_32,
        )
        .map_err(|_| invalid("CFG decode stop"))?;
        let end = at + decoded.length as usize;
        if fragments
            .iter()
            .any(|(&other, f)| at < other + f.decoded.length as usize && other < end)
        {
            return Err(invalid("overlapping guest instruction streams"));
        }
        let ir = lift_cpu_with_rep_budget(
            &bytes[at..end],
            decoded.instruction_pc,
            decoded.linear_pc,
            default_32,
            rep_budget,
        )?;
        // These adapters already own completion/exit; do not create an internal continuation.
        let stop = ir.instructions.iter().any(|i| {
            i.commit.is_some() || matches!(i.op, Op::CallHelper(_) | Op::CompareExchange8B { .. })
        });
        if !stop {
            for block in &ir.blocks {
                if let Some(Terminator::Exit(state)) = block.terminator {
                    if let Some(next) = successor(&ir.states[state.index()], pc, bytes.len()) {
                        pending.insert(next);
                    }
                }
            }
        }
        fragments.insert(at, Fragment { decoded, ir, stop });
    }
    let block_count = 1 + fragments.values().map(|f| f.ir.blocks.len()).sum::<usize>();
    if block_count > 64 {
        return Err(CompileError::Budget("CFG block count"));
    }
    let xmm = fragments.values().any(|f| {
        f.ir.instructions
            .iter()
            .any(|i| matches!(i.op, Op::ReadXmm(_)))
    });
    let seed = IntegerBuilder::new();
    let mut b = IntegerBuilder::new();
    // Mode/selector-changing adapters terminate the region. These reads therefore
    // remain invariant until exit, including across guest backedges.
    let mut entry_reads: Vec<(Op, ValueId)> = vec![];
    for fragment in fragments.values() {
        for inst in &fragment.ir.instructions {
            if matches!(inst.op, Op::ReadSegment(_) | Op::ReadStack32)
                && !entry_reads.iter().any(|(op, _)| *op == inst.op)
            {
                let value = b.node(
                    inst.op.clone(),
                    vec![],
                    fragment.ir.values[inst.results[0].index()].ty,
                );
                entry_reads.push((inst.op.clone(), value));
            }
        }
    }
    if xmm {
        b.xmm = (0..8)
            .map(|r| b.node(Op::ReadXmm(r), vec![], Type::V128))
            .collect();
    }
    let zero = b.constant(0, Type::I32);
    let initial = Frame {
        gpr: b.gpr,
        flags: b.flags.clone(),
        xmm: b.xmm.clone(),
        count: zero,
        effect: b.effect,
    };
    let mut roots = BTreeMap::new();
    for (&at, fragment) in &fragments {
        let block = b.region.block(false);
        let frame = Frame::params(&mut b.region, block, xmm);
        let state = b.region.state(frame.recovery(&fragment.decoded));
        b.region.blocks[block.index()].entry_state = Some(state);
        roots.insert(at, (block, frame));
    }
    b.region.terminate(
        b.block,
        Terminator::Branch(Edge {
            target: roots[&0].0,
            args: initial.args(),
        }),
    );
    for (&at, fragment) in &fragments {
        graft(
            &mut b.region,
            fragment,
            &seed,
            &roots,
            &entry_reads,
            at,
            pc,
            bytes.len(),
        )?;
        if b.region.instructions.len() > 8192 || b.region.values.len() > 16384 {
            return Err(CompileError::Budget("CFG IR size"));
        }
    }
    crate::ir::verify::verify(&b.region).map_err(|e| CompileError::InvalidIr(e.0))?;
    Ok(b.region)
}
fn graft(
    out: &mut Region,
    fragment: &Fragment,
    seed: &IntegerBuilder,
    roots: &BTreeMap<usize, (BlockId, Frame)>,
    entry_reads: &[(Op, ValueId)],
    at: usize,
    pc: GuestEip,
    len: usize,
) -> Result<(), CompileError> {
    let src = &fragment.ir;
    let (root, frame) = &roots[&at];
    if src.entries != vec![BlockId(0)]
        || src.blocks[0].instructions.len() < seed.region.instructions.len()
    {
        return Err(invalid("CFG fragment entry shape"));
    }
    for (old, expected) in src.instructions.iter().zip(&seed.region.instructions) {
        if old.op != expected.op || old.args != expected.args || old.results != expected.results {
            return Err(invalid("CFG fragment input contract"));
        }
    }
    let mut values = vec![None; src.values.len()];
    for (&old, &new) in seed.gpr.iter().zip(&frame.gpr) {
        values[old.index()] = Some(new);
    }
    for (&old, &new) in seed.flags.arithmetic.iter().zip(&frame.flags.arithmetic) {
        values[old.index()] = Some(new);
    }
    for (old, new) in [
        (seed.flags.system, frame.flags.system),
        (seed.flags.last_op1.unwrap(), frame.flags.last_op1.unwrap()),
        (seed.flags.raw_zero.unwrap(), frame.flags.raw_zero.unwrap()),
        (
            seed.flags.zero_is_lazy.unwrap(),
            frame.flags.zero_is_lazy.unwrap(),
        ),
        (seed.effect, frame.effect),
    ] {
        values[old.index()] = Some(new);
    }
    let mut blocks = vec![*root];
    let mut effects = vec![frame.effect];
    let mut extra_effect = vec![false];
    for old in src.blocks.iter().skip(1) {
        let block = out.block(false);
        blocks.push(block);
        let mut effect = None;
        for &param in &old.params {
            let ty = src.values[param.index()].ty;
            let new = out.param(block, ty);
            values[param.index()] = Some(new);
            if ty == Type::Effect {
                effect = Some(new);
            }
        }
        extra_effect.push(effect.is_none());
        effects.push(effect.unwrap_or_else(|| out.param(block, Type::Effect)));
    }
    let helper_base = out.helpers.len() as u32;
    out.helpers.extend(src.helpers.clone());
    let mut instructions = vec![None; src.instructions.len()];
    let get = |map: &[Option<ValueId>], v: ValueId| {
        map.get(v.index())
            .copied()
            .flatten()
            .ok_or_else(|| invalid("unmapped CFG value"))
    };
    for (index, old_block) in src.blocks.iter().enumerate() {
        for id in &old_block.instructions {
            if id.index() < seed.region.instructions.len() {
                continue;
            }
            let inst = &src.instructions[id.index()];
            if let Some((_, value)) = entry_reads.iter().find(|(op, _)| *op == inst.op) {
                values[inst.results[0].index()] = Some(*value);
                continue;
            }
            if let Op::ReadXmm(reg) = inst.op {
                values[inst.results[0].index()] = Some(frame.xmm[reg as usize]);
                continue;
            }
            let mut op = inst.op.clone();
            if let Op::CallHelper(ref mut helper) = op {
                helper.0 += helper_base;
            }
            let args = inst
                .args
                .iter()
                .map(|&v| get(&values, v))
                .collect::<Result<Vec<_>, _>>()?;
            let types: Vec<_> = inst
                .results
                .iter()
                .map(|v| src.values[v.index()].ty)
                .collect();
            let new = out.append(blocks[index], op, args, &types, None);
            let new_id = *out.blocks[blocks[index].index()]
                .instructions
                .last()
                .unwrap();
            instructions[id.index()] = Some(new_id);
            for (&old, &new) in inst.results.iter().zip(&new) {
                values[old.index()] = Some(new);
            }
            if inst.op.ordered() {
                effects[index] = *new.last().unwrap();
            }
        }
    }
    let mut states = vec![];
    for old in &src.states {
        let mut state = old.clone();
        for value in &mut state.gpr {
            *value = get(&values, *value)?;
        }
        for value in &mut state.flags.arithmetic {
            *value = get(&values, *value)?;
        }
        state.flags.system = get(&values, state.flags.system)?;
        for v in [
            &mut state.flags.last_op1,
            &mut state.flags.raw_zero,
            &mut state.flags.zero_is_lazy,
            &mut state.next_value,
        ] {
            if let Some(value) = v {
                *value = get(&values, *value)?;
            }
        }
        if state.xmm.is_empty() {
            state.xmm = frame.xmm.clone();
        } else {
            for value in &mut state.xmm {
                *value = get(&values, *value)?;
            }
        }
        if !state.x87.is_empty() {
            return Err(invalid("CFG extended state pending"));
        }
        if let Some(rep) = &mut state.rep_progress {
            for value in rep {
                *value = get(&values, *value)?;
            }
        }
        state.count_base = Some(frame.count);
        states.push(out.state(state));
    }
    for (index, id) in instructions.iter().enumerate() {
        if let Some(id) = id {
            let old = &src.instructions[index];
            let inst = &mut out.instructions[id.index()];
            inst.state = old.state.map(|id| states[id.index()]);
            inst.commit = old.commit.map(|id| states[id.index()]);
            inst.trap_after_fault = old.trap_after_fault;
            inst.unmasked_word_store = old.unmasked_word_store;
        }
    }
    for (index, block) in src.blocks.iter().enumerate() {
        let destination = blocks[index];
        if index != 0 {
            out.blocks[destination.index()].entry_state =
                block.entry_state.map(|id| states[id.index()]);
        }
        let edge = |edge: &Edge| -> Result<Edge, CompileError> {
            let mut args = edge
                .args
                .iter()
                .map(|&v| get(&values, v))
                .collect::<Result<Vec<_>, _>>()?;
            if extra_effect[edge.target.index()] {
                args.push(effects[index]);
            }
            Ok(Edge {
                target: blocks[edge.target.index()],
                args,
            })
        };
        let term = match block.terminator.as_ref().unwrap() {
            Terminator::Branch(next) => Terminator::Branch(edge(next)?),
            Terminator::CondBranch {
                condition,
                taken,
                not_taken,
            } => Terminator::CondBranch {
                condition: get(&values, *condition)?,
                taken: edge(taken)?,
                not_taken: edge(not_taken)?,
            },
            Terminator::Exit(old) => {
                let id = states[old.index()];
                let state = out.states[id.index()].clone();
                if let Some(next) = if fragment.stop { None } else { successor(&state, pc, len) } {
                    let offset = out.append(
                        destination,
                        Op::Const(state.committed_instructions as u64),
                        vec![],
                        &[Type::I32],
                        None,
                    )[0];
                    let count = out.append(
                        destination,
                        Op::Binary(Binary::Add),
                        vec![frame.count, offset],
                        &[Type::I32],
                        None,
                    )[0];
                    let next_frame = Frame {
                        gpr: state.gpr,
                        flags: state.flags,
                        xmm: state.xmm,
                        count,
                        effect: effects[index],
                    };
                    Terminator::Branch(Edge {
                        target: roots[&next].0,
                        args: next_frame.args(),
                    })
                } else {
                    Terminator::Exit(id)
                }
            },
        };
        out.terminate(destination, term);
    }
    Ok(())
}
