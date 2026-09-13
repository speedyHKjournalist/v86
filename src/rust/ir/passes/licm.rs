//! Transactional, bounded LICM for total, CPU-independent SSA expressions.
//!
//! Only existing unconditional preheaders are used. The CFG, effect chain,
//! snapshots, fault sites and budget polls are never rewritten by this pass.
use crate::ir::{analysis::cfg::Cfg, hir::*, ids::*, verify::verify};

#[derive(Clone, Copy, Debug)]
pub struct LicmConfig {
    pub max_work: usize,
    pub max_hoisted: usize,
}
impl Default for LicmConfig {
    fn default() -> Self {
        Self {
            max_work: 1_000_000,
            max_hoisted: 256,
        }
    }
}
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LicmStats {
    pub loops: usize,
    /// Number of moves; an instruction may move through nested preheaders.
    pub hoisted: usize,
    pub work: usize,
}
struct Work {
    remaining: usize,
}
impl Work {
    fn spend(&mut self, amount: usize) -> Result<(), String> {
        self.remaining = self
            .remaining
            .checked_sub(amount)
            .ok_or_else(|| "LICM work budget exceeded".to_string())?;
        Ok(())
    }
}
struct NaturalLoop {
    header: usize,
    preheader: usize,
    body: Vec<bool>,
}

/// On any failure, including the final verifier, `region` is unchanged.
pub fn run(region: &mut Region, config: LicmConfig) -> Result<LicmStats, String> {
    if region.blocks.len() > 64
        || region.instructions.len() > 8192
        || region.values.len() > 16384
        || region.states.len() > 8192
    {
        return Err("LICM region budget exceeded".into());
    }
    verify(region).map_err(|e| e.0)?;
    let mut work = Work {
        remaining: config.max_work,
    };
    work.spend(1)?;
    let cfg = Cfg::compute(region)?;
    let loops = natural_loops(region, &cfg, &mut work)?;
    if loops.is_empty() {
        return Ok(LicmStats {
            work: config.max_work - work.remaining,
            ..LicmStats::default()
        });
    }
    let mut draft = region.clone();
    let mut stats = LicmStats::default();
    for natural in loops {
        stats.loops += 1;
        let mut order: Vec<_> = (0..draft.blocks.len())
            .filter(|&b| natural.body[b])
            .collect();
        order.sort_by_key(|&b| (cfg.dominates[b].iter().filter(|&&v| v).count(), b));
        let mut selected = vec![false; draft.instructions.len()];
        let mut moves = Vec::new();
        for b in order {
            for &id in &draft.blocks[b].instructions {
                work.spend(1)?;
                let inst = &draft.instructions[id.index()];
                if !movable(inst) {
                    continue;
                }
                let mut invariant = true;
                for &arg in &inst.args {
                    work.spend(1)?;
                    let owner = match draft.values[arg.index()].definition {
                        Definition::Parameter(block, _) => block.index(),
                        Definition::Instruction(def, _) => {
                            if selected[def.index()] {
                                continue;
                            }
                            draft.instructions[def.index()].block.index()
                        },
                    };
                    if natural.body[owner] || !cfg.dominates[natural.preheader][owner] {
                        invariant = false;
                        break;
                    }
                }
                if invariant {
                    if stats.hoisted >= config.max_hoisted {
                        return Err("LICM hoist budget exceeded".into());
                    }
                    selected[id.index()] = true;
                    moves.push(id);
                    stats.hoisted += 1;
                }
            }
        }
        // Selection is in definition-before-use order. Stable IDs preserve all
        // recovery-only references; only the instruction's owning block changes.
        for (b, block) in draft.blocks.iter_mut().enumerate() {
            if natural.body[b] {
                block.instructions.retain(|id| !selected[id.index()]);
            }
        }
        for id in moves {
            draft.instructions[id.index()].block = BlockId(natural.preheader as u32);
            draft.blocks[natural.preheader].instructions.push(id);
        }
    }
    verify(&draft).map_err(|e| e.0)?;
    stats.work = config.max_work - work.remaining;
    *region = draft;
    Ok(stats)
}

fn movable(inst: &Instruction) -> bool {
    if inst.state.is_some()
        || inst.commit.is_some()
        || inst.trap_after_fault
        || inst.unmasked_word_store
        || inst.results.len() != 1
    {
        return false;
    }
    // `!op.ordered()` is NOT sufficient: ReadGpr/ReadFlags/ReadXmm etc. observe
    // mutable CPU state, and future operations must be audited explicitly.
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

fn natural_loops(region: &Region, cfg: &Cfg, work: &mut Work) -> Result<Vec<NaturalLoop>, String> {
    let n = region.blocks.len();
    let mut latches = vec![Vec::new(); n];
    for (b, block) in region.blocks.iter().enumerate() {
        if !cfg.reachable[b] {
            continue;
        }
        for edge in block.terminator.as_ref().unwrap().edges() {
            work.spend(1)?;
            let header = edge.target.index();
            if cfg.dominates[b][header] {
                latches[header].push(b);
            }
        }
    }
    let mut result = Vec::new();
    for (header, tails) in latches.into_iter().enumerate() {
        work.spend(1)?;
        if tails.is_empty() || region.entries.contains(&BlockId(header as u32)) {
            continue;
        }
        let mut body = vec![false; n];
        body[header] = true;
        let mut stack = tails;
        while let Some(b) = stack.pop() {
            work.spend(1)?;
            if body[b] {
                continue;
            }
            body[b] = true;
            for pred in &cfg.predecessors[b] {
                work.spend(1)?;
                if cfg.reachable[pred.index()] {
                    stack.push(pred.index());
                }
            }
        }
        // Reject secondary entries (including external roots) and any body not
        // dominated by its header. This also keeps irreducible cycles untouched.
        if (0..n).any(|b| {
            body[b] && (!cfg.dominates[b][header] || region.entries.contains(&BlockId(b as u32)))
        }) {
            continue;
        }
        let mut outside: Vec<_> = cfg.predecessors[header]
            .iter()
            .map(|b| b.index())
            .filter(|&b| !body[b])
            .collect();
        outside.sort_unstable();
        outside.dedup();
        if outside.len() != 1 {
            continue;
        }
        let preheader = outside[0];
        if !cfg.reachable[preheader] || !cfg.dominates[header][preheader] {
            continue;
        }
        if !matches!(&region.blocks[preheader].terminator,
            Some(Terminator::Branch(edge)) if edge.target.index() == header)
        {
            continue;
        }
        let mut single_entry = true;
        for b in 0..n {
            if !body[b] || b == header {
                continue;
            }
            for pred in &cfg.predecessors[b] {
                work.spend(1)?;
                if !body[pred.index()] {
                    single_entry = false;
                }
            }
        }
        if single_entry {
            result.push(NaturalLoop {
                header,
                preheader,
                body,
            });
        }
    }
    result.sort_by_key(|l| (l.body.iter().filter(|&&b| b).count(), l.header));
    Ok(result)
}

#[cfg(test)]
mod tests;
