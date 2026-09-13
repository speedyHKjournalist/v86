//! Conservative, transactional loop-invariant code motion.
//!
//! Only total, pure SSA operations may move to an existing preheader. In
//! particular `!Op::ordered()` is NOT a sufficient speculation proof: CPU
//! backing-state reads are not ordered but must remain at their original site.
use crate::ir::{analysis::cfg::Cfg, hir::*, ids::*, verify::verify};

#[derive(Clone, Copy, Debug)]
pub struct LicmConfig {
    pub max_work: usize,
}
impl Default for LicmConfig {
    fn default() -> Self {
        Self {
            max_work: 1_000_000,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LicmStats {
    pub natural_loops: usize,
    pub optimized_loops: usize,
    /// Motions, not distinct IDs: a value may leave multiple nested loops.
    pub hoisted: usize,
    pub work: usize,
}
struct Budget {
    remaining: usize,
}
impl Budget {
    fn spend(&mut self, amount: usize) -> Result<(), String> {
        self.remaining = self
            .remaining
            .checked_sub(amount)
            .ok_or("LICM work budget exceeded")?;
        Ok(())
    }
}
struct NaturalLoop {
    header: BlockId,
    members: Vec<bool>,
}

/// Does not mutate the caller's region on invalid input, exhausted work, or a
/// failed post-transform verifier. IDs and all recovery maps remain stable.
pub fn run(region: &mut Region, config: LicmConfig) -> Result<LicmStats, String> {
    if region.blocks.len() > 64
        || region.instructions.len() > 8192
        || region.values.len() > 16384
        || region.states.len() > 16384
        || region.helpers.len() > 1024
    {
        return Err("LICM region budget exceeded".into());
    }
    let mut budget = Budget {
        remaining: config.max_work,
    };
    // Account for input traversal before verification/cloning. Bound the
    // variable-size recovery vectors as well as the instruction arenas.
    budget.spend(region.blocks.len() + region.values.len() + region.instructions.len())?;
    for instruction in &region.instructions {
        budget.spend(instruction.args.len() + instruction.results.len())?;
    }
    for state in &region.states {
        budget.spend(32 + state.xmm.len() + state.x87.len())?;
    }
    for block in &region.blocks {
        budget.spend(block.params.len() + block.instructions.len())?;
        if let Some(term) = &block.terminator {
            for edge in term.edges() {
                budget.spend(1 + edge.args.len())?;
            }
        }
    }
    verify(region).map_err(|error| error.0)?;
    let cfg = Cfg::compute(region)?;
    let mut loops = discover(region, &cfg, &mut budget)?;
    // Inner loops first. A value moved to an inner preheader can subsequently
    // leave an enclosing loop. Block IDs break ties deterministically.
    loops.sort_by_key(|loop_| {
        (
            loop_.members.iter().filter(|&&member| member).count(),
            loop_.header.0,
        )
    });
    let mut stats = LicmStats {
        natural_loops: loops.len(),
        ..LicmStats::default()
    };
    if loops.is_empty() {
        stats.work = config.max_work - budget.remaining;
        return Ok(stats);
    }
    let mut candidate = region.clone();
    for loop_ in &loops {
        let Some(preheader) = preheader(&candidate, &cfg, loop_, &mut budget)? else {
            continue;
        };
        let count = hoist(&mut candidate, &cfg, loop_, preheader, &mut budget)?;
        stats.hoisted += count;
        stats.optimized_loops += usize::from(count != 0);
    }
    verify(&candidate).map_err(|error| error.0)?;
    stats.work = config.max_work - budget.remaining;
    if stats.hoisted != 0 {
        *region = candidate;
    }
    Ok(stats)
}

fn discover(region: &Region, cfg: &Cfg, budget: &mut Budget) -> Result<Vec<NaturalLoop>, String> {
    let n = region.blocks.len();
    let mut latches = vec![Vec::new(); n];
    for (source, block) in region.blocks.iter().enumerate() {
        for edge in block.terminator.as_ref().unwrap().edges() {
            budget.spend(1)?;
            if cfg.reachable[source] && cfg.dominates[source][edge.target.index()] {
                latches[edge.target.index()].push(BlockId(source as u32));
            }
        }
    }
    let mut loops = Vec::new();
    for (header, incoming) in latches.into_iter().enumerate() {
        if incoming.is_empty() {
            continue;
        }
        let mut members = vec![false; n];
        members[header] = true;
        let mut work = incoming;
        let mut single_entry = true;
        while let Some(block) = work.pop() {
            budget.spend(1)?;
            let index = block.index();
            if members[index] || !cfg.reachable[index] {
                continue;
            }
            if !cfg.dominates[index][header] {
                single_entry = false;
                break;
            }
            members[index] = true;
            for &pred in &cfg.predecessors[index] {
                budget.spend(1)?;
                if cfg.reachable[pred.index()] {
                    work.push(pred);
                }
            }
        }
        if single_entry {
            loops.push(NaturalLoop {
                header: BlockId(header as u32),
                members,
            });
        }
    }
    Ok(loops)
}

fn preheader(
    region: &Region,
    cfg: &Cfg,
    loop_: &NaturalLoop,
    budget: &mut Budget,
) -> Result<Option<BlockId>, String> {
    if region
        .entries
        .iter()
        .any(|entry| loop_.members[entry.index()])
    {
        return Ok(None);
    }
    let mut outside = None;
    for (member, &inside) in loop_.members.iter().enumerate() {
        if !inside {
            continue;
        }
        for &pred in &cfg.predecessors[member] {
            budget.spend(1)?;
            if !cfg.reachable[pred.index()] || loop_.members[pred.index()] {
                continue;
            }
            if member != loop_.header.index() || outside.is_some_and(|old| old != pred) {
                return Ok(None);
            }
            outside = Some(pred);
        }
    }
    let Some(preheader) = outside else {
        return Ok(None);
    };
    if !cfg.dominates[loop_.header.index()][preheader.index()] {
        return Ok(None);
    }
    // Do not introduce a new block or move work onto a bypass path. Creating
    // preheaders requires a separate recovery-map/dispatcher transformation.
    match region.blocks[preheader.index()]
        .terminator
        .as_ref()
        .unwrap()
    {
        Terminator::Branch(edge) if edge.target == loop_.header => Ok(Some(preheader)),
        _ => Ok(None),
    }
}

fn speculatable(instruction: &Instruction) -> bool {
    instruction.results.len() == 1
        && instruction.state.is_none()
        && instruction.commit.is_none()
        && !instruction.trap_after_fault
        && !instruction.unmasked_word_store
        && matches!(
            instruction.op,
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

fn hoist(
    region: &mut Region,
    cfg: &Cfg,
    loop_: &NaturalLoop,
    preheader: BlockId,
    budget: &mut Budget,
) -> Result<usize, String> {
    let mut order: Vec<_> = (0..region.blocks.len())
        .filter(|&b| loop_.members[b])
        .collect();
    order.sort_by_key(|&b| (cfg.dominates[b].iter().filter(|&&d| d).count(), b));
    let mut selected = vec![false; region.instructions.len()];
    let mut moves = Vec::new();
    loop {
        let previous = moves.len();
        for &block in &order {
            for &id in &region.blocks[block].instructions {
                budget.spend(1)?;
                let instruction = &region.instructions[id.index()];
                if selected[id.index()] || !speculatable(instruction) {
                    continue;
                }
                let mut invariant = true;
                for &arg in &instruction.args {
                    budget.spend(1)?;
                    let owner = match region.values[arg.index()].definition {
                        Definition::Parameter(block, _) => block,
                        Definition::Instruction(definition, _) => {
                            if selected[definition.index()] {
                                continue;
                            }
                            region.instructions[definition.index()].block
                        },
                    };
                    if loop_.members[owner.index()]
                        || !cfg.dominates[preheader.index()][owner.index()]
                    {
                        invariant = false;
                        break;
                    }
                }
                if invariant {
                    selected[id.index()] = true;
                    moves.push(id);
                }
            }
        }
        if previous == moves.len() {
            break;
        }
    }
    if moves.is_empty() {
        return Ok(0);
    }
    for &block in &order {
        budget.spend(region.blocks[block].instructions.len())?;
        region.blocks[block]
            .instructions
            .retain(|id| !selected[id.index()]);
    }
    for &id in &moves {
        region.instructions[id.index()].block = preheader;
    }
    let count = moves.len();
    region.blocks[preheader.index()].instructions.extend(moves);
    Ok(count)
}

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/licm.rs"]
mod tests;
