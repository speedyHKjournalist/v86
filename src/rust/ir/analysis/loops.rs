//! Natural loops with an existing, dedicated preheader. No CFG is rewritten.
use super::cfg::Cfg;
use crate::ir::{
    hir::{Region, Terminator},
    ids::BlockId,
};

#[derive(Clone, Debug)]
pub struct NaturalLoop {
    pub header: BlockId,
    pub preheader: BlockId,
    pub members: Vec<bool>,
}

/// Shared work budget for discovery and code motion. Graph size is separately bounded.
pub struct WorkBudget {
    remaining: usize,
}
impl WorkBudget {
    pub fn new(limit: usize) -> Self {
        Self { remaining: limit }
    }
    pub fn spend(&mut self, amount: usize) -> Result<(), String> {
        self.remaining = self
            .remaining
            .checked_sub(amount)
            .ok_or("IR optimization work budget exceeded")?;
        Ok(())
    }
}

pub fn discover(
    region: &Region,
    cfg: &Cfg,
    work: &mut WorkBudget,
) -> Result<Vec<NaturalLoop>, String> {
    let n = region.blocks.len();
    let mut result = Vec::new();
    for header in 0..n {
        work.spend(1)?;
        // A region entry has a synthetic external predecessor, not a preheader.
        if region.entries.contains(&BlockId(header as u32)) {
            continue;
        }
        let mut members = vec![false; n];
        members[header] = true;
        let mut queue = Vec::new();
        let mut has_backedge = false;
        for &latch in &cfg.predecessors[header] {
            work.spend(1)?;
            if cfg.reachable[latch.index()] && cfg.dominates[latch.index()][header] {
                has_backedge = true;
                if !members[latch.index()] {
                    members[latch.index()] = true;
                    queue.push(latch);
                }
            }
        }
        if !has_backedge {
            continue;
        }
        // Union all natural backedge loops with the same header. Never walk
        // backwards through the header into its outside predecessors.
        while let Some(block) = queue.pop() {
            for &pred in &cfg.predecessors[block.index()] {
                work.spend(1)?;
                if cfg.reachable[pred.index()] && !members[pred.index()] {
                    members[pred.index()] = true;
                    queue.push(pred);
                }
            }
        }
        if members.iter().enumerate().any(|(b, &inside)| {
            inside && (!cfg.dominates[b][header] || region.entries.contains(&BlockId(b as u32)))
        }) {
            continue;
        }
        let mut outside = Vec::new();
        for &pred in &cfg.predecessors[header] {
            work.spend(1)?;
            if !members[pred.index()] && !outside.contains(&pred) {
                outside.push(pred);
            }
        }
        if outside.len() != 1 {
            continue;
        }
        let preheader = outside[0];
        if !cfg.dominates[header][preheader.index()] {
            continue;
        }
        match region.blocks[preheader.index()].terminator.as_ref() {
            Some(Terminator::Branch(edge)) if edge.target.index() == header => {},
            // Do not insert blocks, split critical edges, or execute a value
            // on a preheader path that can bypass the loop altogether.
            _ => continue,
        }
        result.push(NaturalLoop {
            header: BlockId(header as u32),
            preheader,
            members,
        });
    }
    // Inner loops first; header ID breaks ties deterministically. The CFG stays
    // fixed, so an inner preheader may subsequently be optimized by an outer loop.
    result.sort_by_key(|lp| (lp.members.iter().filter(|&&b| b).count(), lp.header.0));
    Ok(result)
}
