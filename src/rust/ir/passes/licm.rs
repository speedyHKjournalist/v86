//! Transactional LICM for total, immutable SSA expressions only.
//!
//! In particular, `!op.ordered()` is NOT a speculation proof: ReadGpr,
//! ReadFlags, ReadXmm and segment/stack reads observe CPU backing state.
//! Loads, guards, helpers, FP operations and budget/state observations stay put.
use crate::ir::{
    analysis::loops::{charge, LoopAnalysis},
    hir::{Definition, Instruction, Op, Region},
    verify::verify,
};

pub const DEFAULT_WORK_BUDGET: usize = 1 << 20;
#[derive(Default, Debug)]
pub struct LicmStats {
    pub loops: usize,
    pub hoisted: usize,
    pub work: usize,
}
fn speculatable(inst: &Instruction) -> bool {
    inst.results.len() == 1
        && inst.state.is_none()
        && inst.commit.is_none()
        && !inst.trap_after_fault
        && !inst.unmasked_word_store
        && matches!(
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
        )
}
/// A failed budget or verifier check leaves the caller's entire region unchanged.
/// CFG edges, parameters, StateMaps and effect chains are never rewritten.
pub fn run(region: &mut Region, max_work: usize) -> Result<LicmStats, String> {
    if region.states.len() > 8192 || region.helpers.len() > 8192 || max_work > 1 << 24 {
        return Err("LICM region/configuration budget exceeded".into());
    }
    let analysis = LoopAnalysis::compute(region, max_work)?;
    verify(region).map_err(|e| e.0)?;
    let mut remaining = max_work - analysis.work;
    // Most cold/linear regions have no eligible loop. Do not clone their arenas.
    if analysis.loops.iter().all(|lp| lp.preheader.is_none()) {
        return Ok(LicmStats {
            work: analysis.work,
            ..LicmStats::default()
        });
    }
    let mut candidate = region.clone();
    let mut stats = LicmStats::default();
    for natural_loop in &analysis.loops {
        let Some(preheader) = natural_loop.preheader else {
            continue;
        };
        stats.loops += 1;
        loop {
            let mut changed = false;
            for block in 0..candidate.blocks.len() {
                charge(&mut remaining, 1)?;
                if !natural_loop.members[block] {
                    continue;
                }
                // Allocation order need not be dominance order; revisit until fixed point.
                let mut kept = Vec::new();
                for id in candidate.blocks[block].instructions.clone() {
                    charge(&mut remaining, 1)?;
                    let inst = &candidate.instructions[id.index()];
                    let mut invariant = speculatable(inst);
                    if invariant {
                        for arg in &inst.args {
                            charge(&mut remaining, 1)?;
                            let owner = match candidate.values[arg.index()].definition {
                                Definition::Parameter(owner, _) => owner,
                                Definition::Instruction(def, _) => {
                                    candidate.instructions[def.index()].block
                                },
                            };
                            if natural_loop.members[owner.index()]
                                || !analysis.cfg.dominates[preheader.index()][owner.index()]
                            {
                                invariant = false;
                                break;
                            }
                        }
                    }
                    if invariant {
                        // Every input is available at the END of the preheader.
                        // Previously hoisted definitions are appended before their users.
                        candidate.instructions[id.index()].block = preheader;
                        candidate.blocks[preheader.index()].instructions.push(id);
                        changed = true;
                        stats.hoisted += 1;
                    } else {
                        kept.push(id);
                    }
                }
                candidate.blocks[block].instructions = kept;
            }
            if !changed {
                break;
            }
        }
    }
    verify(&candidate).map_err(|e| e.0)?;
    stats.work = max_work - remaining;
    *region = candidate;
    Ok(stats)
}
