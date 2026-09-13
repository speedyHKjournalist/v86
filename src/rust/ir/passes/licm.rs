//! Conservative LICM for natural loops with an existing unconditional preheader.
//!
//! Only total, pure SSA expressions may move. CPU reads are NOT pure, even when
//! `Op::ordered()` is false. Memory, checks, helpers, polls and recovery points
//! retain their order. No address/permission proof is inferred by this pass.
use crate::ir::{analysis::cfg::Cfg, helper::HelperAbi, hir::*, ids::*, verify::verify};

pub const DEFAULT_WORK_LIMIT: usize = 1_000_000;
const MAX_METADATA_ITEMS: usize = 131_072;

#[derive(Default, Debug, Clone, Copy, Eq, PartialEq)]
pub struct Stats {
    pub loops: usize,
    /// Motions, not unique instructions: a value can leave two nested loops.
    pub hoisted: usize,
    pub work: usize,
}

struct Work {
    remaining: usize,
    used: usize,
}
impl Work {
    fn spend(&mut self, amount: usize) -> Result<(), String> {
        self.remaining = self
            .remaining
            .checked_sub(amount)
            .ok_or("LICM work budget exceeded")?;
        self.used += amount;
        Ok(())
    }
}

struct NaturalLoop {
    header: usize,
    preheader: usize,
    members: Vec<bool>,
}

// Arena counts do not bound nested vectors and strings. Audit their aggregate
// size before cloning or verifying, without following potentially invalid IDs.
fn check_limits(region: &Region) -> Result<(), String> {
    if region.blocks.len() > 64
        || region.entries.len() > 64
        || region.instructions.len() > 8192
        || region.values.len() > 16384
        || region.states.len() > 8192
        || region.helpers.len() > 1024
    {
        return Err("LICM region budget exceeded".into());
    }
    let mut remaining = MAX_METADATA_ITEMS;
    let mut take = |amount: usize| -> Result<(), String> {
        remaining = remaining
            .checked_sub(amount)
            .ok_or("LICM metadata budget exceeded")?;
        Ok(())
    };
    for block in &region.blocks {
        take(block.params.len())?;
        take(block.instructions.len())?;
        if let Some(term) = &block.terminator {
            for edge in term.edges() {
                take(edge.args.len())?;
            }
        }
    }
    for inst in &region.instructions {
        take(inst.args.len())?;
        take(inst.results.len())?;
    }
    for state in &region.states {
        take(state.xmm.len())?;
        take(state.x87.len())?;
    }
    for helper in &region.helpers {
        take(helper.name.len())?;
        take(helper.params.len())?;
        take(helper.results.len())?;
        if let HelperAbi::Outcome { fault_delivery: Some(name), .. } = &helper.abi {
            take(name.len())?;
        }
    }
    Ok(())
}

fn eligible(inst: &Instruction) -> bool {
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
                | Op::VectorBitmask { .. }
                | Op::VectorBinary(_)
                | Op::VectorShuffle(_)
                | Op::VectorExtract { .. }
                | Op::VectorReplace { .. }
        )
}

fn discover(region: &Region, cfg: &Cfg, work: &mut Work) -> Result<Vec<NaturalLoop>, String> {
    let n = region.blocks.len();
    let mut loops = Vec::new();
    for header in 0..n {
        work.spend(1)?;
        // An external entry has a synthetic predecessor, not a real preheader.
        if region.entries.contains(&BlockId(header as u32)) {
            continue;
        }
        let mut members = vec![false; n];
        members[header] = true;
        let mut stack = Vec::new();
        let mut has_backedge = false;
        for pred in &cfg.predecessors[header] {
            work.spend(1)?;
            if cfg.dominates[pred.index()][header] {
                has_backedge = true;
                if !members[pred.index()] {
                    members[pred.index()] = true;
                    stack.push(pred.index());
                }
            }
        }
        if !has_backedge {
            continue;
        }
        // Union all latches of this header; stop the reverse walk at the header.
        while let Some(block) = stack.pop() {
            for pred in &cfg.predecessors[block] {
                work.spend(1)?;
                if !members[pred.index()] {
                    members[pred.index()] = true;
                    stack.push(pred.index());
                }
            }
        }
        let mut valid = true;
        for block in 0..n {
            work.spend(1)?;
            if !members[block] {
                continue;
            }
            valid &= cfg.dominates[block][header]
                && !region.entries.contains(&BlockId(block as u32));
            if block != header {
                for pred in &cfg.predecessors[block] {
                    work.spend(1)?;
                    valid &= members[pred.index()];
                }
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
        if !matches!(
            region.blocks[preheader].terminator.as_ref(),
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
    // Inner first. The graph is unchanged, so dominance remains valid throughout.
    loops.sort_by_key(|l| (l.members.iter().filter(|&&member| member).count(), l.header));
    Ok(loops)
}

/// Atomically optimize a verified region. An error leaves the caller's arenas,
/// scheduling and recovery maps unchanged. The work limit covers discovery and
/// candidate/operand visits; arena and aggregate metadata caps bound the input
/// to verification and cloning, whose internal work is not charged to `work`.
pub fn run(region: &mut Region, work_limit: usize) -> Result<Stats, String> {
    check_limits(region)?;
    let mut work = Work {
        remaining: work_limit,
        used: 0,
    };
    work.spend(1)?;
    verify(region).map_err(|e| e.0)?;
    let cfg = Cfg::compute(region)?;
    let loops = discover(region, &cfg, &mut work)?;
    // Acyclic regions and loops without a legal preheader have no motion to
    // plan. Keep input validation, but avoid cloning all HIR arenas here.
    if loops.is_empty() {
        return Ok(Stats { work: work.used, ..Stats::default() });
    }
    let mut staged = region.clone();
    let mut stats = Stats::default();
    for natural in loops {
        stats.loops += 1;
        let mut order: Vec<_> = (0..staged.blocks.len())
            .filter(|&b| natural.members[b])
            .collect();
        // A definition precedes a dominated use, regardless of arena numbering.
        order.sort_by_key(|&b| (cfg.dominates[b].iter().filter(|&&d| d).count(), b));
        let mut moved = vec![false; staged.instructions.len()];
        let mut hoisted = Vec::new();
        for &block in &order {
            for id in staged.blocks[block].instructions.clone() {
                work.spend(1)?;
                let inst = &staged.instructions[id.index()];
                if !eligible(inst) {
                    continue;
                }
                let mut invariant = true;
                for value in &inst.args {
                    work.spend(1)?;
                    let owner = match staged.values[value.index()].definition {
                        Definition::Parameter(owner, _) => owner.index(),
                        Definition::Instruction(def, _) => {
                            staged.instructions[def.index()].block.index()
                        }
                    };
                    invariant &=
                        !natural.members[owner] && cfg.dominates[natural.preheader][owner];
                }
                if invariant {
                    // Updating ownership makes dependent expressions available
                    // later in this scan. Append in exactly that dependency order.
                    staged.instructions[id.index()].block = BlockId(natural.preheader as u32);
                    moved[id.index()] = true;
                    hoisted.push(id);
                    stats.hoisted += 1;
                }
            }
        }
        for block in order {
            staged.blocks[block].instructions.retain(|id| !moved[id.index()]);
        }
        staged.blocks[natural.preheader].instructions.extend(hoisted);
    }
    // Loop discovery can succeed without finding an invariant. Do not replace
    // unchanged arenas or verify the same graph twice in that case.
    if stats.hoisted != 0 {
        verify(&staged).map_err(|e| e.0)?;
        *region = staged;
    }
    stats.work = work.used;
    Ok(stats)
}

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/licm.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/licm_pipeline.rs"]
mod pipeline_tests;

#[cfg(test)]
mod no_motion_tests {
    use super::*;
    use crate::ir::{
        frontend::{decode::GuestEip, integer::IntegerBuilder},
        state::{ResumeKind, StateMap},
        types::Type,
    };

    fn fixture(looping: bool) -> Region {
        let mut b = IntegerBuilder::new();
        if looping {
            let header = b.region.block(false);
            let effect = b.region.param(header, Type::Effect);
            let count = b.region.param(header, Type::I32);
            b.region.terminate(
                b.block,
                Terminator::Branch(Edge { target: header, args: vec![b.effect, b.gpr[0]] }),
            );
            let next = b.region.append(
                header,
                Op::Binary(Binary::Add),
                vec![count, count],
                &[Type::I32],
                None,
            )[0];
            b.region.terminate(
                header,
                Terminator::Branch(Edge { target: header, args: vec![effect, next] }),
            );
        } else {
            let state = b.region.state(StateMap {
                instruction_pc: GuestEip(0x1000),
                next_pc: GuestEip(0x1001),
                next_value: None,
                resume: ResumeKind::BeforeInstruction,
                gpr: b.gpr,
                flags: b.flags.clone(),
                xmm: vec![],
                x87: vec![],
                committed_instructions: 0,
                count_base: None,
                rep_progress: None,
            });
            b.region.terminate(b.block, Terminator::Exit(state));
        }
        b.region
    }

    #[test]
    fn unchanged_regions_retain_their_original_arenas() {
        for looping in [false, true] {
            let mut region = fixture(looping);
            verify(&region).unwrap();
            let before = format!("{region:?}");
            let blocks = region.blocks.as_ptr();
            let instructions = region.instructions.as_ptr();
            let values = region.values.as_ptr();
            let result = run(&mut region, DEFAULT_WORK_LIMIT).unwrap();
            assert_eq!(result.loops, usize::from(looping));
            assert_eq!(result.hoisted, 0);
            assert!(result.work > 0);
            assert_eq!(region.blocks.as_ptr(), blocks);
            assert_eq!(region.instructions.as_ptr(), instructions);
            assert_eq!(region.values.as_ptr(), values);
            assert_eq!(format!("{region:?}"), before);
        }
    }

    #[test]
    fn no_motion_fast_path_still_checks_input_and_work_budget() {
        let mut region = fixture(false);
        let before = format!("{region:?}");
        assert!(run(&mut region, 0).unwrap_err().contains("budget"));
        assert_eq!(format!("{region:?}"), before);
        region.instructions[0].block = BlockId(u32::MAX);
        let invalid = format!("{region:?}");
        assert!(run(&mut region, DEFAULT_WORK_LIMIT).is_err());
        assert_eq!(format!("{region:?}"), invalid);
    }

    #[test]
    fn metadata_limits_precede_verification_of_invalid_ids() {
        for kind in 0..11 {
            let mut region = fixture(false);
            let value = region.instructions[0].results[0];
            match kind {
                0 => region.blocks[0].instructions = vec![InstId(u32::MAX); MAX_METADATA_ITEMS + 1],
                1 => region.blocks[0].params = vec![value; MAX_METADATA_ITEMS + 1],
                2 => region.instructions[0].args = vec![value; MAX_METADATA_ITEMS + 1],
                3 => region.instructions[0].results = vec![value; MAX_METADATA_ITEMS + 1],
                4 => {
                    region.blocks[0].terminator = Some(Terminator::Branch(Edge {
                        target: BlockId(0),
                        args: vec![value; MAX_METADATA_ITEMS + 1],
                    }));
                }
                5 => region.states[0].xmm = vec![value; MAX_METADATA_ITEMS + 1],
                6 => region.states[0].x87 = vec![value; MAX_METADATA_ITEMS + 1],
                7 => region.helpers.push(crate::ir::helper::HelperDescriptor::conservative(
                    "h".repeat(MAX_METADATA_ITEMS + 1),
                    vec![],
                    vec![],
                )),
                8 => region.helpers.push(crate::ir::helper::HelperDescriptor::conservative(
                    "helper".into(),
                    vec![Type::I32; MAX_METADATA_ITEMS + 1],
                    vec![],
                )),
                9 => region.helpers.push(crate::ir::helper::HelperDescriptor::conservative(
                    "helper".into(),
                    vec![],
                    vec![Type::I32; MAX_METADATA_ITEMS + 1],
                )),
                _ => {
                    // Individually small vectors must share the same total cap.
                    region.instructions[0].args = vec![value; MAX_METADATA_ITEMS / 2];
                    region.instructions[0].results = vec![value; MAX_METADATA_ITEMS / 2];
                }
            }
            let before = format!("{region:?}");
            assert_eq!(
                run(&mut region, DEFAULT_WORK_LIMIT).unwrap_err(),
                "LICM metadata budget exceeded"
            );
            assert_eq!(format!("{region:?}"), before);
        }
    }
}
