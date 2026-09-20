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
                raw_flags: Some(r.param(block, Type::I32)),
                lazy_mask: Some(r.param(block, Type::I32)),
                last_result: Some(r.param(block, Type::I32)),
                last_op_size: Some(r.param(block, Type::I32)),
                backing_valid: Some(r.param(block, Type::I1)),
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
        args.push(self.flags.raw_flags.unwrap());
        args.push(self.flags.lazy_mask.unwrap());
        args.push(self.flags.last_result.unwrap());
        args.push(self.flags.last_op_size.unwrap());
        args.push(self.flags.backing_valid.unwrap());
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
    span: usize,
    ir: Region,
    stop: bool,
}
fn offset(pc: GuestEip, start: GuestEip, len: usize) -> Option<usize> {
    let n = pc.0.wrapping_sub(start.0) as usize;
    (n < len).then_some(n)
}
#[derive(Clone, Copy)]
pub struct CfgSource<'a> {
    pub bytes: &'a [u8],
    pub pc: GuestEip,
    pub linear: LinearAddress,
}
#[derive(Clone, Copy, Debug)]
pub struct PredictedEdge {
    pub from: GuestEip,
    pub target: GuestEip,
}
fn locate<'a>(sources: &'a [CfgSource<'a>], pc: GuestEip) -> Option<(CfgSource<'a>, usize)> {
    sources.iter().find_map(|s| offset(pc, s.pc, s.bytes.len()).map(|at| (*s, at)))
}
fn successor(state: &StateMap, sources: &[CfgSource<'_>]) -> Option<usize> {
    (state.resume == ResumeKind::AfterInstruction && state.next_value.is_none())
        .then(|| locate(sources, state.next_pc).map(|_| state.next_pc.0 as usize))
        .flatten()
}
fn predicted(state: &StateMap, sources: &[CfgSource<'_>], edges: &[PredictedEdge]) -> Option<usize> {
    if state.resume != ResumeKind::AfterInstruction || state.next_value.is_none() { return None; }
    edges.iter().find(|e| e.from == state.instruction_pc && locate(sources, e.target).is_some())
        .map(|e| e.target.0 as usize)
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
    lift_cpu_cfg_sources(&[CfgSource { bytes, pc, linear }], &[], default_32, rep_budget)
}
/// Compose separately captured hot regions into one SSA graph. Predictions only
/// discover candidate edges; every dynamic edge retains an exact target guard.
pub fn lift_cpu_cfg_sources(
    sources: &[CfgSource<'_>],
    predictions: &[PredictedEdge],
    default_32: bool,
    rep_budget: u32,
) -> Result<Region, CompileError> {
    if sources.is_empty() || sources.iter().any(|s| s.bytes.is_empty()) {
        return Err(invalid("empty CFG snapshot"));
    }
    if sources.len() > 4 || sources.iter().any(|s| s.bytes.len() > 15 * 128)
        || predictions.len() > 4 {
        return Err(CompileError::Budget("CFG code snapshot"));
    }
    let entry_pc = sources[0].pc;
    for (i, source) in sources.iter().enumerate() {
        if source.linear.0.wrapping_sub(source.pc.0) != sources[0].linear.0.wrapping_sub(entry_pc.0) {
            return Err(invalid("incompatible CFG sources"));
        }
        // Overlapping immutable windows are common for hot side entries. Share
        // identical bytes, but still reject conflicting snapshots and later
        // reject targets entering the middle of an already decoded instruction.
        for old in &sources[..i] {
            for (at, byte) in source.bytes.iter().enumerate() {
                if let Some(other) = offset(GuestEip(source.pc.0.wrapping_add(at as u32)), old.pc, old.bytes.len()) {
                    if *byte != old.bytes[other] { return Err(invalid("conflicting CFG source bytes")); }
                }
            }
        }
    }
    let mut fragments: BTreeMap<usize, Fragment> = BTreeMap::new();
    let mut pending = BTreeSet::from([entry_pc.0 as usize]);
    while let Some(address) = pending.pop_first() {
        if fragments.contains_key(&address) {
            continue;
        }
        let (source, at) = locate(sources, GuestEip(address as u32))
            .ok_or_else(|| invalid("missing CFG source"))?;
        let CfgSource { bytes, pc, linear } = source;
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
        let span = if decoded.encoding.opcode == 0xFB {
            super::sti::extent(
                &bytes[at..],
                decoded.instruction_pc,
                decoded.linear_pc,
                default_32,
            )?
        } else {
            decoded.length as usize
        };
        let end = at + span;
        if fragments
            .iter()
            .any(|(&other, f)|
                (address as u32).wrapping_sub(other as u32) < f.span as u32
                    || (other as u32).wrapping_sub(address as u32) < span as u32)
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
        // Scalar stores can continue through the backend's guarded RAM path.
        // Cold/MMIO writes and aliases of any immutable code dependency still
        // commit then exit there. Other commit-bearing adapters own their exit.
        let incomplete_tail = end < bytes.len()
            && decode(
                &bytes[end..],
                decoded.next_pc,
                LinearAddress(linear.0.wrapping_add(end as u32)),
                default_32,
            ).is_err();
        let stop = decoded.encoding.opcode == 0xFB
            || ir.instructions.iter().any(|i| {
                (i.commit.is_some()
                    && (incomplete_tail
                        || !matches!(i.op, Op::GuestStore { .. } | Op::RmwStore { .. })))
                    || matches!(i.op, Op::CompareExchange8B { .. })
                    || match i.op {
                        Op::CallHelper(id) => matches!(
                            ir.helpers[id.index()].abi,
                            crate::ir::helper::HelperAbi::CpuExit
                                | crate::ir::helper::HelperAbi::CpuRep
                        ),
                        _ => false,
                    }
            });
        if !stop {
            for block in &ir.blocks {
                if let Some(Terminator::Exit(state)) = block.terminator {
                    let state = &ir.states[state.index()];
                    if let Some(next) = successor(state, sources)
                        .or_else(|| predicted(state, sources, predictions)) {
                        pending.insert(next);
                    }
                }
            }
        }
        fragments.insert(
            address,
            Fragment {
                decoded,
                span,
                ir,
                stop,
            },
        );
    }
    let block_count = 1 + fragments.values().map(|f| f.ir.blocks.len()).sum::<usize>();
    if block_count > 64 {
        return Err(CompileError::Budget("CFG block count"));
    }
    let xmm = fragments.values().any(|f| {
        f.ir.states.iter().any(|s| !s.xmm.is_empty())
            || f.ir
                .instructions
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
            target: roots[&(entry_pc.0 as usize)].0,
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
            sources,
            predictions,
        )?;
        if b.region.instructions.len() > 8192 || b.region.values.len() > 16384 || b.region.blocks.len() > 64 {
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
    sources: &[CfgSource<'_>],
    predictions: &[PredictedEdge],
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
        (
            seed.flags.raw_flags.unwrap(),
            frame.flags.raw_flags.unwrap(),
        ),
        (
            seed.flags.lazy_mask.unwrap(),
            frame.flags.lazy_mask.unwrap(),
        ),
        (
            seed.flags.last_result.unwrap(),
            frame.flags.last_result.unwrap(),
        ),
        (
            seed.flags.last_op_size.unwrap(),
            frame.flags.last_op_size.unwrap(),
        ),
        (
            seed.flags.backing_valid.unwrap(),
            frame.flags.backing_valid.unwrap(),
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
            &mut state.flags.raw_flags,
            &mut state.flags.lazy_mask,
            &mut state.flags.last_result,
            &mut state.flags.last_op_size,
            &mut state.flags.backing_valid,
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
                if let Some(next) = if fragment.stop { None } else {
                    successor(&state, sources).or_else(|| predicted(&state, sources, predictions))
                } {
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
                    let next = Edge {
                        target: roots[&next].0,
                        args: next_frame.args(),
                    };
                    if let Some(target) = state.next_value {
                        let expected = out.append(destination, Op::Const(
                            out.states[out.blocks[next.target.index()].entry_state.unwrap().index()].instruction_pc.0 as u64
                        ), vec![], &[Type::I32], None)[0];
                        let condition = out.append(destination, Op::Binary(Binary::Eq),
                            vec![target, expected], &[Type::I1], None)[0];
                        let fallback = out.block(false);
                        out.blocks[fallback.index()].entry_state = Some(id);
                        out.terminate(fallback, Terminator::Exit(id));
                        Terminator::CondBranch { condition, taken: next,
                            not_taken: Edge { target: fallback, args: vec![] } }
                    } else { Terminator::Branch(next) }
                } else {
                    Terminator::Exit(id)
                }
            },
        };
        out.terminate(destination, term);
    }
    Ok(())
}
