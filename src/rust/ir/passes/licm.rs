//! Bounded, transactional loop-invariant code motion for pure SSA expressions.
//!
//! Never speculate CPU observations, guest memory, guards, helper calls, or
//! recovery points. No blocks/edges are added: the dispatcher budget and every
//! architectural snapshot remain at exactly the same control-flow positions.
use crate::ir::{analysis::cfg::Cfg, hir::*, ids::*, verify::verify};

pub const DEFAULT_WORK_BUDGET: usize = 262_144;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LicmStats {
    pub loops: usize,
    pub hoisted: usize,
}

struct Work(usize);
impl Work {
    fn charge(&mut self, amount: usize) -> Result<(), String> {
        self.0 = self
            .0
            .checked_sub(amount)
            .ok_or("LICM work budget exceeded")?;
        Ok(())
    }
}

#[derive(Debug)]
struct NaturalLoop {
    header: usize,
    preheader: usize,
    members: Vec<bool>,
}

/// Merge all dominating backedges to a header, then require a single existing
/// unconditional preheader. External entries are synthetic CFG roots and must
/// never acquire a dependence on a preheader that they can bypass.
fn natural_loops(region: &Region, cfg: &Cfg, work: &mut Work) -> Result<Vec<NaturalLoop>, String> {
    let n = region.blocks.len();
    let mut loops = Vec::new();
    for header in 0..n {
        work.charge(1)?;
        if region.entries.contains(&BlockId(header as u32)) {
            continue;
        }
        let mut members = vec![false; n];
        members[header] = true;
        let mut queue = Vec::new();
        let mut has_backedge = false;
        for &tail in &cfg.predecessors[header] {
            work.charge(1)?;
            if cfg.reachable[tail.index()] && cfg.dominates[tail.index()][header] {
                has_backedge = true;
                if !members[tail.index()] {
                    members[tail.index()] = true;
                    queue.push(tail.index());
                }
            }
        }
        if !has_backedge {
            continue;
        }
        while let Some(block) = queue.pop() {
            for pred in &cfg.predecessors[block] {
                work.charge(1)?;
                if cfg.reachable[pred.index()] && !members[pred.index()] {
                    members[pred.index()] = true;
                    queue.push(pred.index());
                }
            }
        }
        // Refuse side-entry / non-natural cycles, including another public root.
        let mut safe = true;
        let mut incoming = Vec::new();
        for block in 0..n {
            work.charge(1)?;
            if !members[block] {
                continue;
            }
            if !cfg.dominates[block][header] || region.entries.contains(&BlockId(block as u32)) {
                safe = false;
                break;
            }
            for pred in &cfg.predecessors[block] {
                work.charge(1)?;
                if !cfg.reachable[pred.index()] || members[pred.index()] {
                    continue;
                }
                if block != header {
                    safe = false;
                } else if !incoming.contains(&pred.index()) {
                    incoming.push(pred.index());
                }
            }
        }
        if !safe || incoming.len() != 1 {
            continue;
        }
        let preheader = incoming[0];
        if !matches!(
            &region.blocks[preheader].terminator,
            Some(Terminator::Branch(edge)) if edge.target.index() == header
        ) || !cfg.dominates[header][preheader]
        {
            continue;
        }
        loops.push(NaturalLoop {
            header,
            preheader,
            members,
        });
    }
    // Inner loops first: a value can subsequently move through an outer
    // preheader, but is never duplicated or moved out of an unrelated loop.
    loops.sort_by_key(|l| (l.members.iter().filter(|&&member| member).count(), l.header));
    Ok(loops)
}

fn speculatable(inst: &Instruction) -> bool {
    inst.results.len() == 1
        && inst.state.is_none()
        && inst.commit.is_none()
        && !inst.trap_after_fault
        && !inst.unmasked_word_store
        && matches!(
            &inst.op,
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

/// Hoist only definitions available at the *end* of the existing preheader.
/// The complete pass is a transaction: budget exhaustion or verifier rejection
/// leaves the input region untouched. A block parameter inside the loop is
/// conservatively variant; a separate phi pass may prove it redundant first.
pub fn run(region: &mut Region, work_budget: usize) -> Result<LicmStats, String> {
    if region.blocks.len() > 64
        || region.instructions.len() > 8192
        || region.values.len() > 16384
        || region.states.len() > 8192
        || region.helpers.len() > 1024
    {
        return Err("LICM region budget exceeded".into());
    }
    let mut work = Work(work_budget);
    work.charge(region.blocks.len() + region.instructions.len() + region.values.len())?;
    verify(region).map_err(|e| e.0)?;
    let cfg = Cfg::compute(region)?;
    let loops = natural_loops(region, &cfg, &mut work)?;
    if loops.is_empty() {
        return Ok(LicmStats::default());
    }
    let mut candidate = region.clone();
    let mut stats = LicmStats::default();
    for natural in loops {
        let mut order: Vec<_> = (0..candidate.blocks.len())
            .filter(|&b| natural.members[b])
            .collect();
        // Dominating definitions precede their uses; within each block retain
        // original instruction order. This also handles permuted block IDs.
        order.sort_by_key(|&b| cfg.dominates[b].iter().filter(|&&v| v).count());
        let mut selected = vec![false; candidate.instructions.len()];
        let mut hoisted = Vec::new();
        for &block in &order {
            for &id in &candidate.blocks[block].instructions {
                work.charge(1)?;
                let inst = &candidate.instructions[id.index()];
                if !speculatable(inst) {
                    continue;
                }
                let mut invariant = true;
                for &value in &inst.args {
                    work.charge(1)?;
                    let owner = match candidate.values[value.index()].definition {
                        Definition::Parameter(block, _) => block.index(),
                        Definition::Instruction(def, _) => {
                            candidate.instructions[def.index()].block.index()
                        },
                    };
                    if natural.members[owner] || !cfg.dominates[natural.preheader][owner] {
                        invariant = false;
                        break;
                    }
                }
                if invariant {
                    selected[id.index()] = true;
                    hoisted.push(id);
                    // Later dependent candidates see the planned destination.
                    candidate.instructions[id.index()].block = BlockId(natural.preheader as u32);
                }
            }
        }
        if hoisted.is_empty() {
            continue;
        }
        for &block in &order {
            work.charge(candidate.blocks[block].instructions.len())?;
            candidate.blocks[block]
                .instructions
                .retain(|id| !selected[id.index()]);
        }
        stats.loops += 1;
        stats.hoisted += hoisted.len();
        candidate.blocks[natural.preheader]
            .instructions
            .extend(hoisted);
    }
    if stats.hoisted != 0 {
        verify(&candidate).map_err(|e| e.0)?;
        *region = candidate;
    }
    Ok(stats)
}

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/licm.rs"]
mod tests;
