//! Bounded, transactional LICM for non-trapping SSA expressions only.
//!
//! No CFG is rewritten. A loop must already have an unconditional, unique
//! preheader; memory operations, CPU reads, helpers and recovery points never
//! move. In particular, `!op.ordered()` is NOT a speculation proof.
use crate::ir::{analysis::cfg::Cfg, hir::*, ids::*, verify::verify};

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub max_work: usize,
    pub max_hoisted: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            max_work: 262_144,
            max_hoisted: 256,
        }
    }
}
#[derive(Default, Debug, Eq, PartialEq)]
pub struct Stats {
    pub loops: usize,
    pub hoisted: usize,
    pub work: usize,
}
struct Budget {
    remaining: usize,
}
impl Budget {
    fn charge(&mut self, amount: usize) -> Result<(), String> {
        self.remaining = self
            .remaining
            .checked_sub(amount)
            .ok_or("LICM work budget exceeded")?;
        Ok(())
    }
}
struct NaturalLoop {
    header: usize,
    preheader: usize,
    members: Vec<bool>,
}

/// All failures leave the caller's region unchanged, including budget failures
/// after finding or moving candidates in earlier (possibly nested) loops.
pub fn run(region: &mut Region, config: Config) -> Result<Stats, String> {
    if config.max_work > 1_048_576 || config.max_hoisted > 1024 {
        return Err("invalid LICM budget".into());
    }
    if region.blocks.len() > 64
        || region.instructions.len() > 8192
        || region.values.len() > 16384
        || region.states.len() > 4096
    {
        return Err("LICM region budget exceeded".into());
    }
    verify(region).map_err(|e| e.0)?;
    if config.max_hoisted == 0 {
        return Ok(Stats::default());
    }
    let mut budget = Budget {
        remaining: config.max_work,
    };
    let cfg = Cfg::compute(region)?;
    let loops = natural_loops(region, &cfg, &mut budget)?;
    let mut candidate = region.clone();
    let mut stats = Stats::default();
    for natural_loop in loops {
        stats.loops += 1;
        // SSA definitions in dominating blocks precede their uses, even when
        // the arena was not allocated in CFG or dominator order.
        let mut order: Vec<_> = (0..candidate.blocks.len())
            .filter(|&b| natural_loop.members[b])
            .collect();
        order.sort_by_key(|&b| cfg.dominates[b].iter().filter(|&&v| v).count());
        for b in order {
            for id in candidate.blocks[b].instructions.clone() {
                budget.charge(1)?;
                let inst = &candidate.instructions[id.index()];
                if !speculatable(inst) {
                    continue;
                }
                let mut invariant = true;
                for &value in &inst.args {
                    budget.charge(1)?;
                    let owner = match candidate.values[value.index()].definition {
                        Definition::Parameter(block, _) => block.index(),
                        Definition::Instruction(id, _) => {
                            candidate.instructions[id.index()].block.index()
                        },
                    };
                    if !cfg.dominates[natural_loop.preheader][owner] {
                        invariant = false;
                        break;
                    }
                }
                if !invariant {
                    continue;
                }
                if stats.hoisted == config.max_hoisted {
                    return Err("LICM hoist budget exceeded".into());
                }
                // Earlier hoists are now available at the end of the preheader.
                // Stable IDs preserve every StateMap and edge-only use.
                candidate.instructions[id.index()].block = BlockId(natural_loop.preheader as u32);
                candidate.blocks[natural_loop.preheader]
                    .instructions
                    .push(id);
                stats.hoisted += 1;
            }
            budget.charge(candidate.blocks[b].instructions.len())?;
            candidate.blocks[b]
                .instructions
                .retain(|id| candidate.instructions[id.index()].block.index() == b);
        }
    }
    verify(&candidate).map_err(|e| e.0)?;
    stats.work = config.max_work - budget.remaining;
    if stats.hoisted != 0 {
        *region = candidate;
    }
    Ok(stats)
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

fn natural_loops(
    region: &Region,
    cfg: &Cfg,
    budget: &mut Budget,
) -> Result<Vec<NaturalLoop>, String> {
    let n = region.blocks.len();
    let mut loops = Vec::new();
    for header in 0..n {
        budget.charge(1 + cfg.predecessors[header].len())?;
        if region.entries.contains(&BlockId(header as u32)) {
            continue;
        }
        let mut stack: Vec<_> = cfg.predecessors[header]
            .iter()
            .filter(|p| cfg.dominates[p.index()][header])
            .map(|p| p.index())
            .collect();
        if stack.is_empty() {
            continue;
        }
        // Union every latch of this header, rather than treating overlapping
        // backedges as independent loops with unsound preheaders.
        let mut members = vec![false; n];
        members[header] = true;
        while let Some(b) = stack.pop() {
            budget.charge(1)?;
            if members[b] {
                continue;
            }
            members[b] = true;
            budget.charge(cfg.predecessors[b].len())?;
            stack.extend(cfg.predecessors[b].iter().map(|p| p.index()));
        }
        budget.charge(n + cfg.predecessors[header].len())?;
        if (0..n).any(|b| {
            members[b]
                && (!cfg.dominates[b][header] || region.entries.contains(&BlockId(b as u32)))
        }) {
            continue;
        }
        let mut outside: Vec<_> = cfg.predecessors[header]
            .iter()
            .map(|p| p.index())
            .filter(|&p| !members[p])
            .collect();
        outside.sort_unstable();
        outside.dedup();
        if outside.len() != 1 {
            continue;
        }
        let preheader = outside[0];
        if !matches!(
            &region.blocks[preheader].terminator,
            Some(Terminator::Branch(edge)) if edge.target.index() == header
        ) {
            continue;
        }
        loops.push(NaturalLoop {
            header,
            preheader,
            members,
        });
    }
    // Inner loops first: an outer pass can then move their invariants again.
    loops.sort_by_key(|l| (l.members.iter().filter(|&&b| b).count(), l.header));
    Ok(loops)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod pipeline;
