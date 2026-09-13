//! Transactional, bounded LICM for non-trapping, CPU-state-independent SSA values.
//! Memory proofs and CPU reads are deliberately outside this pass's contract.
use crate::ir::{
    analysis::{
        cfg::Cfg,
        loops::{discover, WorkBudget},
    },
    hir::{Definition, Instruction, Op, Region},
    ids::BlockId,
    verify::verify,
};

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub max_work: usize,
    pub max_hoists: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            max_work: 1_000_000,
            max_hoists: 8192,
        }
    }
}
#[derive(Default, Debug)]
pub struct Stats {
    pub loops: usize,
    /// Motion steps, including an instruction moved through nested preheaders.
    pub hoisted: usize,
}

fn movable(inst: &Instruction) -> bool {
    if inst.results.len() != 1
        || inst.state.is_some()
        || inst.commit.is_some()
        || inst.trap_after_fault
        || inst.unmasked_word_store
    {
        return false;
    }
    // !ordered() alone is NOT sufficient: ReadGpr/ReadFlags/ReadXmm and
    // segment/stack reads observe mutable backing state despite having no token.
    matches!(
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

pub fn run(region: &mut Region, config: Config) -> Result<Stats, String> {
    // Check before invoking the quadratic dominator/verifier analyses.
    if region.blocks.len() > 64 || region.instructions.len() > 8192 || region.values.len() > 16384 {
        return Err("LICM region budget exceeded".into());
    }
    let mut work = WorkBudget::new(config.max_work.min(1_000_000));
    work.spend(1)?;
    verify(region).map_err(|e| e.0)?;
    let cfg = Cfg::compute(region)?;
    let loops = discover(region, &cfg, &mut work)?;
    let mut stats = Stats {
        loops: loops.len(),
        hoisted: 0,
    };
    // Plan solely on block lists and virtual ownership; no IR or recovery map
    // changes until *all* loops and budget checks have succeeded.
    let mut lists: Vec<_> = region
        .blocks
        .iter()
        .map(|b| b.instructions.clone())
        .collect();
    let original_owners: Vec<_> = region.instructions.iter().map(|i| i.block).collect();
    let mut owners = original_owners.clone();
    for lp in loops {
        let preheader = lp.preheader.index();
        loop {
            let mut changed = false;
            for b in 0..region.blocks.len() {
                work.spend(1)?;
                if !lp.members[b] {
                    continue;
                }
                let mut kept = Vec::with_capacity(lists[b].len());
                // Source order is preserved for pinned nodes, and destinations
                // are appended in dependency order, even for reverse block IDs.
                for &id in &lists[b].clone() {
                    work.spend(1)?;
                    let inst = &region.instructions[id.index()];
                    let mut available = movable(inst);
                    if available {
                        for &value in &inst.args {
                            work.spend(1)?;
                            let owner = match region.values[value.index()].definition {
                                Definition::Parameter(block, _) => block,
                                Definition::Instruction(def, _) => owners[def.index()],
                            };
                            if !cfg.dominates[preheader][owner.index()] {
                                available = false;
                                break;
                            }
                        }
                    }
                    if available {
                        if stats.hoisted >= config.max_hoists.min(8192) {
                            return Err("LICM hoist budget exceeded".into());
                        }
                        lists[preheader].push(id);
                        owners[id.index()] = BlockId(preheader as u32);
                        stats.hoisted += 1;
                        changed = true;
                    } else {
                        kept.push(id);
                    }
                }
                lists[b] = kept;
            }
            if !changed {
                break;
            }
        }
    }
    if stats.hoisted == 0 {
        return Ok(stats);
    }
    for (block, list) in region.blocks.iter_mut().zip(&mut lists) {
        std::mem::swap(&mut block.instructions, list);
    }
    for (inst, &owner) in region.instructions.iter_mut().zip(&owners) {
        inst.block = owner;
    }
    if let Err(error) = verify(region) {
        // A failed postcondition must not leak partially transformed code.
        for (block, list) in region.blocks.iter_mut().zip(&mut lists) {
            std::mem::swap(&mut block.instructions, list);
        }
        for (inst, owner) in region.instructions.iter_mut().zip(original_owners) {
            inst.block = owner;
        }
        return Err(error.0);
    }
    Ok(stats)
}

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/licm.rs"]
mod tests;
