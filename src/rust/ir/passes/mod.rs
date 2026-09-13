//! Bounded, verifier-checked pure dataflow passes. Memory and helpers never enter GVN.
use super::{hir::*, ids::*, verify::verify};
use std::collections::HashSet;
mod gvn;
pub mod licm;
mod merge;
mod prune;
#[derive(Clone, Copy)]
pub struct PassConfig {
    pub prune: bool,
    pub merge: bool,
    pub phis: bool,
    pub fold: bool,
    pub gvn: bool,
    pub licm: bool,
    pub dce: bool,
    pub rounds: usize,
}
impl Default for PassConfig {
    fn default() -> Self {
        Self {
            prune: true,
            merge: true,
            phis: true,
            fold: true,
            gvn: true,
            licm: true,
            dce: true,
            rounds: 2,
        }
    }
}
#[derive(Default, Debug)]
pub struct PassStats {
    pub branches: usize,
    pub unreachable: usize,
    pub cross_commoned: usize,
    pub merged: usize,
    pub phis: usize,
    pub folded: usize,
    pub commoned: usize,
    pub removed: usize,
    pub loops: usize,
    pub hoisted: usize,
}
pub fn run(region: &mut Region, config: PassConfig) -> Result<PassStats, String> {
    verify(region).map_err(|e| e.0)?;
    if config.rounds > 8 {
        return Err("pass iteration budget exceeded".into());
    }
    let mut stats = PassStats::default();
    for _ in 0..config.rounds {
        if config.merge {
            merge::run(region, &mut stats)?;
            verify(region).map_err(|e| e.0)?;
        }
        if config.phis {
            trivial_phis(region, &mut stats);
            verify(region).map_err(|e| e.0)?;
        }
        if config.fold {
            fold(region, &mut stats);
            verify(region).map_err(|e| e.0)?;
        }
        if config.prune {
            prune::run(region, &mut stats)?;
            verify(region).map_err(|e| e.0)?;
        }
        if config.gvn {
            gvn::run(region, &mut stats)?;
            verify(region).map_err(|e| e.0)?;
        }
        if config.licm {
            let result = licm::run(region, licm::DEFAULT_WORK_BUDGET)?;
            stats.loops += result.loops;
            stats.hoisted += result.hoisted;
        }
        if config.dce {
            dce(region, &mut stats);
            verify(region).map_err(|e| e.0)?;
        }
    }
    Ok(stats)
}
fn constant(region: &Region, value: ValueId) -> Option<u64> {
    match region.values[value.index()].definition {
        Definition::Instruction(id, _) => match region.instructions[id.index()].op {
            Op::Const(n) => Some(n),
            _ => None,
        },
        _ => None,
    }
}
fn fold(region: &mut Region, stats: &mut PassStats) {
    for block in region.blocks.clone() {
        for id in block.instructions {
            let inst = &region.instructions[id.index()];
            if inst.results.len() != 1 || inst.op.ordered() || matches!(inst.op, Op::Const(_)) {
                continue;
            }
            let args: Option<Vec<_>> = inst.args.iter().map(|&v| constant(region, v)).collect();
            let Some(args) = args else {
                continue;
            };
            let Some(bits) = region.values[inst.results[0].index()].ty.bits() else {
                continue;
            };
            let input_bits = inst
                .args
                .first()
                .and_then(|v| region.values[v.index()].ty.bits())
                .unwrap_or(bits);
            let mask = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
            let signed = |v: u64| ((v << (64 - input_bits)) as i64) >> (64 - input_bits);
            let value = match inst.op {
                Op::Binary(op) => match op {
                    Binary::Add => args[0].wrapping_add(args[1]),
                    Binary::Sub => args[0].wrapping_sub(args[1]),
                    Binary::Mul => args[0].wrapping_mul(args[1]),
                    Binary::And => args[0] & args[1],
                    Binary::Or => args[0] | args[1],
                    Binary::Xor => args[0] ^ args[1],
                    Binary::Shl => {
                        args[0].wrapping_shl((args[1] & if bits == 64 { 63 } else { 31 }) as u32)
                    },
                    Binary::Shr => args[0] >> (args[1] & if bits == 64 { 63 } else { 31 }),
                    Binary::Sar => {
                        (signed(args[0]) >> (args[1] & if bits == 64 { 63 } else { 31 })) as u64
                    },
                    Binary::Eq => (args[0] == args[1]) as u64,
                    Binary::Ult => (args[0] < args[1]) as u64,
                    Binary::Slt => (signed(args[0]) < signed(args[1])) as u64,
                },
                Op::CountLeadingZeros => {
                    if bits == 64 {
                        args[0].leading_zeros() as u64
                    } else {
                        (args[0] as u32).leading_zeros() as u64
                    }
                },
                Op::CountTrailingZeros => {
                    if bits == 64 {
                        args[0].trailing_zeros() as u64
                    } else {
                        (args[0] as u32).trailing_zeros() as u64
                    }
                },
                Op::PopulationCount => args[0].count_ones() as u64,
                Op::Select => {
                    if args[0] != 0 {
                        args[1]
                    } else {
                        args[2]
                    }
                },
                Op::Extend { signed: true } => signed(args[0]) as u64,
                Op::Extend { signed: false } | Op::Truncate => args[0],
                Op::Extract { lsb } => args[0] >> lsb,
                Op::Insert { lsb } => {
                    let width = region.values[inst.args[1].index()].ty.bits().unwrap();
                    let part = if width == 64 { u64::MAX } else { ((1u64 << width) - 1) << lsb };
                    (args[0] & !part) | args[1] << lsb
                },
                _ => continue,
            } & mask;
            let inst = &mut region.instructions[id.index()];
            inst.op = Op::Const(value);
            inst.args.clear();
            stats.folded += 1;
        }
    }
}
fn replace(region: &mut Region, old: ValueId, new: ValueId) {
    rewrite_values(region, |v| {
        if *v == old {
            *v = new;
        }
    });
}
fn rewrite_values(region: &mut Region, replace: impl Fn(&mut ValueId)) {
    for inst in &mut region.instructions {
        for arg in &mut inst.args {
            replace(arg);
        }
    }
    for state in &mut region.states {
        for value in &mut state.gpr {
            replace(value);
        }
        for value in &mut state.flags.arithmetic {
            replace(value);
        }
        replace(&mut state.flags.system);
        if let Some(value) = &mut state.flags.last_op1 {
            replace(value);
        }
        if let Some(value) = &mut state.flags.raw_zero {
            replace(value);
        }
        if let Some(value) = &mut state.flags.zero_is_lazy {
            replace(value);
        }
        if let Some(value) = &mut state.count_base {
            replace(value);
        }
        if let Some(value) = &mut state.next_value {
            replace(value);
        }
        for value in state.xmm.iter_mut().chain(&mut state.x87) {
            replace(value);
        }
        if let Some(rep) = &mut state.rep_progress {
            for value in rep {
                replace(value);
            }
        }
    }
    for block in &mut region.blocks {
        if let Some(term) = &mut block.terminator {
            if let Terminator::CondBranch { condition, .. } = term {
                replace(condition);
            }
            for edge in term.edges_mut() {
                for arg in &mut edge.args {
                    replace(arg);
                }
            }
        }
    }
}
fn dce(region: &mut Region, stats: &mut PassStats) {
    let mut live = HashSet::new();
    let mut work = Vec::new();
    for block in &region.blocks {
        if let Some(id) = block.entry_state {
            work.extend(region.states[id.index()].values());
        }
        let term = block.terminator.as_ref().unwrap();
        if let Terminator::Exit(id) = term {
            work.extend(region.states[id.index()].values());
        }
        if let Terminator::CondBranch { condition, .. } = term {
            work.push(*condition);
        }
        for edge in term.edges() {
            work.extend(&edge.args);
        }
        for id in &block.instructions {
            let inst = &region.instructions[id.index()];
            if inst.op.ordered() || inst.state.is_some() {
                live.insert(*id);
                work.extend(&inst.args);
                for id in [inst.state, inst.commit].into_iter().flatten() {
                    work.extend(region.states[id.index()].values());
                }
            }
        }
    }
    while let Some(value) = work.pop() {
        if let Definition::Instruction(id, _) = region.values[value.index()].definition {
            if live.insert(id) {
                work.extend(&region.instructions[id.index()].args);
            }
        }
    }
    for block in &mut region.blocks {
        let before = block.instructions.len();
        block.instructions.retain(|id| live.contains(id));
        stats.removed += before - block.instructions.len();
    }
}

fn trivial_phis(region: &mut Region, stats: &mut PassStats) {
    for b in 0..region.blocks.len() {
        if region.entries.contains(&BlockId(b as u32)) {
            continue;
        }
        for p in (0..region.blocks[b].params.len()).rev() {
            let param = region.blocks[b].params[p];
            let mut candidate = None;
            let mut differs = false;
            for block in &region.blocks {
                for edge in block.terminator.as_ref().unwrap().edges() {
                    if edge.target.index() != b {
                        continue;
                    }
                    let value = edge.args[p];
                    if value == param {
                        continue;
                    }
                    if candidate.is_some() && candidate != Some(value) {
                        differs = true;
                    }
                    candidate = Some(value);
                }
            }
            if differs {
                continue;
            }
            let Some(value) = candidate else {
                continue;
            };
            // Effect phis remain explicit chain roots until effect-aware CFG simplification.
            if region.values[param.index()].ty == super::types::Type::Effect {
                continue;
            }
            replace(region, param, value);
            region.blocks[b].params.remove(p);
            for block in &mut region.blocks {
                for edge in block.terminator.as_mut().unwrap().edges_mut() {
                    if edge.target.index() == b {
                        edge.args.remove(p);
                    }
                }
            }
            for (i, &v) in region.blocks[b].params.iter().enumerate() {
                region.values[v.index()].definition =
                    Definition::Parameter(BlockId(b as u32), i as u32);
            }
            stats.phis += 1;
        }
    }
}
