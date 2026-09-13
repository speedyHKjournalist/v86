//! Transactional loop-invariant code motion for total, pure SSA expressions.
//!
//! CPU observations, memory, guards, helpers, and budget polls are never moved.
//! Only existing single-successor preheaders are used; no CFG or recovery point
//! is introduced, removed, or retimed by this pass.
use crate::ir::{analysis::cfg::Cfg, hir::*, ids::*, verify::verify};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Planning work only. Verifier cost is separately bounded by arena limits.
    pub work: usize,
    pub hoisted: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            work: 2_000_000,
            hoisted: 4096,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Stats {
    pub loops: usize,
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
            .ok_or("LICM work budget exceeded")?;
        Ok(())
    }
}
struct NaturalLoop {
    header: usize,
    preheader: usize,
    members: Vec<bool>,
}

/// An explicit total-operation allowlist, deliberately stricter than `!ordered`.
/// ReadGpr/ReadFlags/etc. observe mutable CPU backing state despite having no
/// effect-token result. A future opcode must be audited before it is added here.
fn movable(inst: &Instruction) -> bool {
    inst.state.is_none()
        && inst.commit.is_none()
        && !inst.trap_after_fault
        && !inst.unmasked_word_store
        && inst.results.len() == 1
        && matches!(
            inst.op,
            Op::Const(_)
                | Op::Binary(
                    Binary::Add
                        | Binary::Sub
                        | Binary::Mul
                        | Binary::And
                        | Binary::Or
                        | Binary::Xor
                        | Binary::Shl
                        | Binary::Shr
                        | Binary::Sar
                        | Binary::Eq
                        | Binary::Ult
                        | Binary::Slt
                )
                | Op::Select
                | Op::Extend { .. }
                | Op::Truncate
                | Op::Extract { .. }
                | Op::Insert { .. }
                | Op::CountLeadingZeros
                | Op::CountTrailingZeros
                | Op::PopulationCount
                | Op::VectorBitmask { .. }
                | Op::VectorShuffle(_)
                | Op::VectorExtract { .. }
                | Op::VectorReplace { .. }
        )
}

fn loops(region: &Region, cfg: &Cfg, work: &mut Work) -> Result<Vec<NaturalLoop>, String> {
    let n = region.blocks.len();
    let mut result = Vec::new();
    for header in 0..n {
        work.spend(1 + cfg.predecessors[header].len())?;
        if region.entries.contains(&BlockId(header as u32)) {
            continue;
        }
        // Union all latches for one header: processing backedges independently
        // could mistake another latch for an outside predecessor.
        let mut stack: Vec<_> = cfg.predecessors[header]
            .iter()
            .map(|p| p.index())
            .filter(|&p| cfg.dominates[p][header])
            .collect();
        if stack.is_empty() {
            continue;
        }
        let mut members = vec![false; n];
        members[header] = true;
        while let Some(block) = stack.pop() {
            work.spend(1)?;
            if members[block] {
                continue;
            }
            members[block] = true;
            work.spend(cfg.predecessors[block].len())?;
            stack.extend(cfg.predecessors[block].iter().map(|p| p.index()));
        }
        // Reject external roots and irreducible side entries. The same checks
        // also guard against treating a synthetic-root edge as a preheader.
        let mut valid = true;
        for block in 0..n {
            work.spend(1)?;
            if !members[block] {
                continue;
            }
            if !cfg.dominates[block][header] || region.entries.contains(&BlockId(block as u32)) {
                valid = false;
            }
            if block != header {
                work.spend(cfg.predecessors[block].len())?;
                valid &= cfg.predecessors[block].iter().all(|p| members[p.index()]);
            }
        }
        if !valid {
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
        if !cfg.dominates[header][preheader]
            || !matches!(
                &region.blocks[preheader].terminator,
                Some(Terminator::Branch(edge)) if edge.target.index() == header
            )
        {
            continue;
        }
        result.push(NaturalLoop {
            header,
            preheader,
            members,
        });
    }
    // Inner loops first, deterministic even when block arena order is unrelated
    // to dominance. A subsequent outer-loop pass may move the expression again.
    result.sort_by_key(|l| (l.members.iter().filter(|&&m| m).count(), l.header));
    Ok(result)
}

/// Optimize a verified region, or leave it and all its arenas unchanged on error.
/// The hoist limit is a successful partial optimization, not a compiler failure.
pub fn run(region: &mut Region, limits: Limits) -> Result<Stats, String> {
    if region.blocks.len() > 64
        || region.entries.len() > 64
        || region.instructions.len() > 8192
        || region.values.len() > 16384
        || region.states.len() > 8192
    {
        return Err("LICM region budget exceeded".into());
    }
    verify(region).map_err(|e| e.0)?;
    let mut work = Work {
        remaining: limits.work,
    };
    work.spend(region.blocks.len().saturating_mul(region.blocks.len()))?;
    let cfg = Cfg::compute(region)?;
    let loops = loops(region, &cfg, &mut work)?;
    let mut stats = Stats {
        loops: loops.len(),
        ..Stats::default()
    };
    if loops.is_empty() || limits.hoisted == 0 {
        stats.work = limits.work - work.remaining;
        return Ok(stats);
    }
    // Only instruction placement changes during planning. Stable arena/value
    // IDs, effect chains, branch arguments, and StateMaps remain untouched.
    let mut placement: Vec<_> = region
        .blocks
        .iter()
        .map(|b| b.instructions.clone())
        .collect();
    let mut owners: Vec<_> = region
        .instructions
        .iter()
        .map(|i| i.block.index())
        .collect();
    let depth: Vec<_> = cfg
        .dominates
        .iter()
        .map(|row| row.iter().filter(|&&d| d).count())
        .collect();
    for l in loops {
        let mut order: Vec<_> = (0..region.blocks.len()).filter(|&b| l.members[b]).collect();
        order.sort_by_key(|&b| (depth[b], b));
        for block in order {
            let instructions = std::mem::take(&mut placement[block]);
            for id in instructions {
                work.spend(1)?;
                let inst = &region.instructions[id.index()];
                let mut invariant = stats.hoisted < limits.hoisted && movable(inst);
                if invariant {
                    work.spend(inst.args.len())?;
                    invariant = inst.args.iter().all(|value| {
                        let owner = match region.values[value.index()].definition {
                            Definition::Parameter(owner, _) => owner.index(),
                            Definition::Instruction(id, _) => owners[id.index()],
                        };
                        !l.members[owner] && cfg.dominates[l.preheader][owner]
                    });
                }
                if invariant {
                    owners[id.index()] = l.preheader;
                    placement[l.preheader].push(id);
                    stats.hoisted += 1;
                } else {
                    placement[block].push(id);
                }
            }
        }
    }
    stats.work = limits.work - work.remaining;
    if stats.hoisted != 0 {
        let mut candidate = region.clone();
        for (block, instructions) in candidate.blocks.iter_mut().zip(placement) {
            block.instructions = instructions;
        }
        for (inst, owner) in candidate.instructions.iter_mut().zip(owners) {
            inst.block = BlockId(owner as u32);
        }
        verify(&candidate).map_err(|e| format!("LICM result: {}", e.0))?;
        *region = candidate;
    }
    Ok(stats)
}

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/licm.rs"]
mod tests;
