//! Transactional LICM for total, effect-free SSA expressions only.
//!
//! A value must be available at an existing unconditional preheader. CPU reads,
//! memory, guards, helpers, state observations, division and budget polls stay put.
use crate::ir::{
    analysis::loops::{LoopAnalysis, LoopError, WorkBudget, MAX_BLOCKS},
    hir::*,
    ids::*,
    verify::verify,
};

#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Transformation work units; dominance/verifiers also have hard arena limits.
    pub max_work: usize,
    /// Counts moves, including a value moved through nested loop preheaders.
    pub max_hoists: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            max_work: 1_048_576,
            max_hoists: 4096,
        }
    }
}
#[derive(Default, Debug)]
pub struct Stats {
    pub loops_seen: usize,
    pub loops_changed: usize,
    pub hoisted: usize,
    pub work: usize,
    /// An exhausted optimization leaves the original HIR available for compilation.
    pub budget_exhausted: bool,
    pub budget_reason: Option<&'static str>,
}

fn check_limits(region: &Region, config: Config) -> Result<(), LoopError> {
    if region.blocks.len() > MAX_BLOCKS
        || region.instructions.len() > 8192
        || region.values.len() > 16384
        || region.states.len() > 8192
        || region.helpers.len() > 8192
        || config.max_work > 16_777_216
        || config.max_hoists > 8192
    {
        return Err(LoopError::Budget(
            "LICM region/configuration budget exceeded",
        ));
    }
    Ok(())
}
fn movable(inst: &Instruction) -> bool {
    // Keep an explicit speculatability whitelist. Neither !ordered() nor GVN
    // eligibility proves that executing an expression on an extra path is safe.
    let total = matches!(
        inst.op,
        Op::Const(_)
            | Op::Binary(_)
            | Op::Select
            | Op::Extend { .. }
            | Op::Truncate
            | Op::Extract { .. }
            | Op::Insert { .. }
            | Op::CountLeadingZeros
            | Op::CountTrailingZeros
            | Op::PopulationCount
            | Op::LinearOffset
            | Op::VectorBitmask { .. }
            | Op::VectorBinary(_)
            | Op::VectorShuffle(_)
            | Op::VectorExtract { .. }
            | Op::VectorReplace { .. }
    );
    total
        && inst.results.len() == 1
        && inst.state.is_none()
        && inst.commit.is_none()
        && !inst.trap_after_fault
        && !inst.unmasked_word_store
}
fn available(
    region: &Region,
    value: ValueId,
    members: &[bool],
    preheader: BlockId,
    analysis: &LoopAnalysis,
) -> bool {
    let owner = match region.values[value.index()].definition {
        Definition::Parameter(block, _) => block,
        Definition::Instruction(id, _) => region.instructions[id.index()].block,
    };
    !members[owner.index()] && analysis.cfg.dominates[preheader.index()][owner.index()]
}

/// Failure is atomic: the caller retains the exact original Region.
pub fn run(region: &mut Region, config: Config) -> Result<Stats, String> {
    let mut work = WorkBudget::new(config.max_work);
    match run_inner(region, config, &mut work) {
        Ok(stats) => Ok(stats),
        Err(LoopError::Budget(reason)) => Ok(Stats {
            work: work.used(),
            budget_exhausted: true,
            budget_reason: Some(reason),
            ..Stats::default()
        }),
        Err(LoopError::Invalid(reason)) => Err(reason),
    }
}
fn run_inner(
    region: &mut Region,
    config: Config,
    work: &mut WorkBudget,
) -> Result<Stats, LoopError> {
    check_limits(region, config)?;
    verify(region).map_err(|e| LoopError::Invalid(e.0))?;
    let analysis = LoopAnalysis::compute(region, work)?;
    let mut candidate = region.clone();
    let mut stats = Stats {
        loops_seen: analysis.loops.len(),
        ..Stats::default()
    };
    for lp in &analysis.loops {
        let Some(preheader) = lp.preheader else {
            continue;
        };
        let before = stats.hoisted;
        let mut blocks: Vec<_> = (0..candidate.blocks.len())
            .filter(|&b| lp.members[b])
            .collect();
        // A strict dominator has fewer dominators. SSA producers are consequently
        // visited before their users even when block allocation order is reversed.
        blocks.sort_by_key(|&b| (analysis.cfg.dominates[b].iter().filter(|&&v| v).count(), b));
        for b in blocks {
            work.charge(candidate.blocks[b].instructions.len())?;
            let mut kept = Vec::with_capacity(candidate.blocks[b].instructions.len());
            for id in candidate.blocks[b].instructions.clone() {
                work.charge(1)?;
                let inst = &candidate.instructions[id.index()];
                let mut invariant = movable(inst);
                if invariant {
                    for &arg in &inst.args {
                        work.charge(1)?;
                        if !available(&candidate, arg, &lp.members, preheader, &analysis) {
                            invariant = false;
                            break;
                        }
                    }
                }
                if !invariant {
                    kept.push(id);
                    continue;
                }
                if stats.hoisted == config.max_hoists {
                    return Err(LoopError::Budget("LICM hoist budget exceeded"));
                }
                // Appending preserves all existing preheader observations. Earlier
                // moves are already present, so same-preheader operands stay ordered.
                candidate.instructions[id.index()].block = preheader;
                candidate.blocks[preheader.index()].instructions.push(id);
                stats.hoisted += 1;
            }
            candidate.blocks[b].instructions = kept;
        }
        stats.loops_changed += usize::from(stats.hoisted != before);
    }
    verify(&candidate).map_err(|e| LoopError::Invalid(e.0))?;
    stats.work = work.used();
    *region = candidate;
    Ok(stats)
}

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/licm.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/licm_observation.rs"]
mod observation_tests;
