//! Bounded natural-loop discovery. Never synthesizes entries or changes the CFG.
use super::cfg::Cfg;
use crate::ir::{
    hir::{Region, Terminator},
    ids::BlockId,
};

#[derive(Debug)]
pub struct NaturalLoop {
    pub header: BlockId,
    pub members: Vec<bool>,
    /// A unique, unconditional predecessor outside the loop. No edge splitting.
    pub preheader: Option<BlockId>,
}
#[derive(Debug)]
pub struct LoopAnalysis {
    pub cfg: Cfg,
    /// Innermost first, deterministic even when block allocation is not RPO.
    pub loops: Vec<NaturalLoop>,
    pub work: usize,
}
pub(crate) fn charge(remaining: &mut usize, cost: usize) -> Result<(), &'static str> {
    *remaining = remaining
        .checked_sub(cost)
        .ok_or("LICM work budget exceeded")?;
    Ok(())
}
impl LoopAnalysis {
    pub fn compute(region: &Region, max_work: usize) -> Result<Self, &'static str> {
        if region.blocks.len() > 64
            || region.instructions.len() > 8192
            || region.values.len() > 16384
        {
            return Err("LICM region budget exceeded");
        }
        let mut remaining = max_work;
        charge(&mut remaining, 1)?;
        let cfg = Cfg::compute(region)?;
        let n = region.blocks.len();
        let mut loops = Vec::new();
        for header in 0..n {
            charge(&mut remaining, 1)?;
            if !cfg.reachable[header] {
                continue;
            }
            let mut members = vec![false; n];
            members[header] = true;
            let mut pending = Vec::new();
            let mut found = false;
            for &pred in &cfg.predecessors[header] {
                charge(&mut remaining, 1)?;
                if cfg.reachable[pred.index()] && cfg.dominates[pred.index()][header] {
                    found = true;
                    if !members[pred.index()] {
                        members[pred.index()] = true;
                        pending.push(pred);
                    }
                }
            }
            if !found {
                continue;
            }
            let mut reducible = true;
            while let Some(block) = pending.pop() {
                for &pred in &cfg.predecessors[block.index()] {
                    charge(&mut remaining, 1)?;
                    if !cfg.reachable[pred.index()] {
                        continue;
                    }
                    if !cfg.dominates[pred.index()][header] {
                        reducible = false;
                    }
                    if !members[pred.index()] {
                        members[pred.index()] = true;
                        pending.push(pred);
                    }
                }
            }
            // An external entry into the cycle invalidates any preheader proof.
            if !reducible || region.entries.iter().any(|b| members[b.index()]) {
                continue;
            }
            let mut outside = Vec::new();
            for &pred in &cfg.predecessors[header] {
                charge(&mut remaining, 1)?;
                if cfg.reachable[pred.index()] && !members[pred.index()] && !outside.contains(&pred)
                {
                    outside.push(pred);
                }
            }
            let preheader = if outside.len() == 1 {
                let pred = outside[0];
                match &region.blocks[pred.index()].terminator {
                    Some(Terminator::Branch(edge))
                        if edge.target.index() == header && cfg.dominates[header][pred.index()] =>
                    {
                        Some(pred)
                    },
                    _ => None,
                }
            } else {
                None
            };
            loops.push(NaturalLoop {
                header: BlockId(header as u32),
                members,
                preheader,
            });
        }
        loops.sort_by_key(|l| (l.members.iter().filter(|&&v| v).count(), l.header));
        Ok(Self {
            cfg,
            loops,
            work: max_work - remaining,
        })
    }
}
