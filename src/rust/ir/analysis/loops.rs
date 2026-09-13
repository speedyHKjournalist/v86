//! Bounded natural-loop discovery. No CFG edges or recovery points are rewritten.
use super::cfg::Cfg;
use crate::ir::{hir::*, ids::BlockId};

pub(crate) const MAX_BLOCKS: usize = 64;

#[derive(Debug)]
pub(crate) enum LoopError {
    Budget(&'static str),
    Invalid(String),
}

pub(crate) struct WorkBudget {
    limit: usize,
    used: usize,
}
impl WorkBudget {
    pub fn new(limit: usize) -> Self {
        Self { limit, used: 0 }
    }
    pub fn charge(&mut self, amount: usize) -> Result<(), LoopError> {
        let next = self
            .used
            .checked_add(amount)
            .ok_or(LoopError::Budget("loop work overflow"))?;
        if next > self.limit {
            return Err(LoopError::Budget("loop work budget exceeded"));
        }
        self.used = next;
        Ok(())
    }
    pub fn used(&self) -> usize {
        self.used
    }
}

#[derive(Debug)]
pub(crate) struct NaturalLoop {
    pub header: BlockId,
    pub latches: Vec<BlockId>,
    pub members: Vec<bool>,
    /// An existing, unique outside predecessor whose only successor is the header.
    pub preheader: Option<BlockId>,
}

pub(crate) struct LoopAnalysis {
    pub cfg: Cfg,
    /// Inner loops precede their containing loops, deterministically.
    pub loops: Vec<NaturalLoop>,
}
impl LoopAnalysis {
    pub fn compute(region: &Region, work: &mut WorkBudget) -> Result<Self, LoopError> {
        let n = region.blocks.len();
        if n > MAX_BLOCKS {
            return Err(LoopError::Budget("loop block budget exceeded"));
        }
        // Dominance/verifier work is independently bounded by MAX_BLOCKS.
        work.charge(n * n)?;
        let cfg = Cfg::compute(region).map_err(|e| LoopError::Invalid(e.into()))?;
        let mut latches = vec![Vec::new(); n];
        for (tail, block) in region.blocks.iter().enumerate() {
            work.charge(1)?;
            if !cfg.reachable[tail] {
                continue;
            }
            for edge in block.terminator.as_ref().unwrap().edges() {
                work.charge(1)?;
                let header = edge.target.index();
                if cfg.dominates[tail][header] {
                    let tail = BlockId(tail as u32);
                    if !latches[header].contains(&tail) {
                        latches[header].push(tail);
                    }
                }
            }
        }
        let mut loops = Vec::new();
        for (h, tails) in latches.into_iter().enumerate() {
            work.charge(1)?;
            if tails.is_empty() {
                continue;
            }
            let header = BlockId(h as u32);
            let mut members = vec![false; n];
            members[h] = true;
            let mut stack = tails.clone();
            let mut reducible = true;
            while let Some(block) = stack.pop() {
                work.charge(1)?;
                let b = block.index();
                if members[b] {
                    continue;
                }
                if !cfg.dominates[b][h] {
                    reducible = false;
                    break;
                }
                members[b] = true;
                for &pred in &cfg.predecessors[b] {
                    work.charge(1)?;
                    if cfg.reachable[pred.index()] && !members[pred.index()] {
                        stack.push(pred);
                    }
                }
            }
            if !reducible {
                continue;
            }
            let mut outside = Vec::new();
            for &pred in &cfg.predecessors[h] {
                work.charge(1)?;
                if cfg.reachable[pred.index()] && !members[pred.index()] && !outside.contains(&pred)
                {
                    outside.push(pred);
                }
            }
            let has_external_entry = region.entries.iter().any(|entry| members[entry.index()]);
            let preheader = if !has_external_entry && outside.len() == 1 {
                let pred = outside[0];
                match region.blocks[pred.index()].terminator.as_ref().unwrap() {
                    Terminator::Branch(edge)
                        if edge.target == header && cfg.dominates[h][pred.index()] =>
                    {
                        Some(pred)
                    },
                    _ => None,
                }
            } else {
                None
            };
            loops.push(NaturalLoop {
                header,
                latches: tails,
                members,
                preheader,
            });
        }
        loops.sort_by_key(|lp| (lp.members.iter().filter(|&&v| v).count(), lp.header.0));
        Ok(Self { cfg, loops })
    }
}
