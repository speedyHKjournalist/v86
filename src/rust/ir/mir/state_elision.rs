//! Conservative CPU backing-state write elision for pure CFG regions.
//!
//! The certificate only removes writes that reconstruct values proven equal to
//! the CPU backing state present when the IR entry was invoked. Regions with any
//! resumable memory/helper/commit observation are rejected wholesale; budget
//! recovery and terminal exits are the only observation sites allowed.

use super::{materialize::StatePlan, value::Address, MirData};
use crate::{
    cpu::global_pointers as gp,
    ir::{
        hir::{Definition, Op, Region, Terminator},
        ids::{StateId, ValueId},
        lowering::CompileError,
    },
};

pub const DEFAULT_WORK_LIMIT: usize = 262_144;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Initial {
    Gpr(u8),
    Xmm(u8),
    FlagSystem,
    FlagBit(u8),
    FlagOperand,
    RawFlags,
    RawZero,
    FlagChanges,
    ZeroLazy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Origin {
    Unknown,
    Initial(Initial),
    Other,
}

impl Origin {
    fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unknown, value) | (value, Self::Unknown) => value,
            (Self::Initial(a), Self::Initial(b)) if a == b => self,
            (Self::Other, Self::Other) => Self::Other,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    masks: Vec<Vec<bool>>,
    enabled: bool,
}

fn spend(left: &mut usize, amount: usize) -> Result<(), CompileError> {
    *left = left
        .checked_sub(amount)
        .ok_or(CompileError::Budget("MIR state elision work"))?;
    Ok(())
}

fn disabled(states: &[StatePlan]) -> Plan {
    Plan {
        masks: states
            .iter()
            .map(|state| vec![false; state.cpu.writes.len()])
            .collect(),
        enabled: false,
    }
}

fn special_origin(region: &Region, value: ValueId, origins: &[Origin]) -> Origin {
    let Definition::Instruction(id, result) = region.values[value.index()].definition else {
        return Origin::Unknown;
    };
    if result != 0 {
        return Origin::Other;
    }
    let inst = &region.instructions[id.index()];
    match inst.op {
        Op::ReadGpr(reg) => Origin::Initial(Initial::Gpr(reg)),
        Op::ReadXmm(reg) => Origin::Initial(Initial::Xmm(reg)),
        Op::ReadFlags => Origin::Initial(Initial::FlagSystem),
        Op::ReadRawFlags => Origin::Initial(Initial::RawFlags),
        Op::ReadFlagOperand => Origin::Initial(Initial::FlagOperand),
        Op::ReadFlagChanges => Origin::Initial(Initial::FlagChanges),
        Op::Extract { lsb } if inst.args.len() == 1 => match origins[inst.args[0].index()] {
            Origin::Initial(Initial::FlagSystem) => [0u8, 2, 4, 6, 7, 11]
                .iter()
                .position(|&bit| bit == lsb)
                .map(|bit| Origin::Initial(Initial::FlagBit(bit as u8)))
                .unwrap_or(Origin::Other),
            Origin::Initial(Initial::RawFlags) if lsb == 6 => {
                Origin::Initial(Initial::RawZero)
            },
            Origin::Initial(Initial::FlagChanges) if lsb == 6 => {
                Origin::Initial(Initial::ZeroLazy)
            },
            _ => Origin::Other,
        },
        _ => Origin::Other,
    }
}

fn origins(region: &Region, left: &mut usize) -> Result<Vec<Origin>, CompileError> {
    spend(left, region.values.len() + region.instructions.len())?;
    let mut result = vec![Origin::Unknown; region.values.len()];

    // Instruction roots that do not depend on block-parameter facts can be
    // seeded immediately. Other instruction results are conservatively Other.
    for (index, value) in region.values.iter().enumerate() {
        if let Definition::Instruction(_, _) = value.definition {
            result[index] = special_origin(region, ValueId(index as u32), &result);
        }
    }

    let rounds = region.blocks.len().saturating_add(2);
    for _ in 0..rounds {
        let mut changed = false;
        for (b, block) in region.blocks.iter().enumerate() {
            spend(left, 1 + block.params.len())?;
            for (p, &param) in block.params.iter().enumerate() {
                let next = if region.entries.contains(&crate::ir::ids::BlockId(b as u32)) {
                    Origin::Other
                } else {
                    let mut incoming = Origin::Unknown;
                    let mut seen = false;
                    for source in &region.blocks {
                        for edge in source.terminator.as_ref().unwrap().edges() {
                            if edge.target.index() == b {
                                spend(left, 1)?;
                                let arg = *edge.args.get(p).ok_or_else(|| {
                                    CompileError::InvalidIr(
                                        "state-elision edge/parameter mismatch".into(),
                                    )
                                })?;
                                incoming = incoming.join(result[arg.index()]);
                                seen = true;
                            }
                        }
                    }
                    if seen { incoming } else { Origin::Other }
                };
                if next != result[param.index()] {
                    result[param.index()] = next;
                    changed = true;
                }
            }
        }

        // Extracts of incoming flag bundles can become entry-equivalent after
        // phi facts settle. Re-evaluate all instruction-defined values.
        for index in 0..region.values.len() {
            if matches!(region.values[index].definition, Definition::Instruction(_, _)) {
                let next = special_origin(region, ValueId(index as u32), &result);
                if next != result[index] {
                    result[index] = next;
                    changed = true;
                }
            }
        }
        if !changed {
            for origin in &mut result {
                if *origin == Origin::Unknown {
                    *origin = Origin::Other;
                }
            }
            return Ok(result);
        }
    }
    Err(CompileError::Budget("MIR state elision convergence"))
}

fn is_initial(origins: &[Origin], value: ValueId, initial: Initial) -> bool {
    origins[value.index()] == Origin::Initial(initial)
}

fn derive(region: &Region, states: &[StatePlan], work_limit: usize) -> Result<Plan, CompileError> {
    let mut left = work_limit;
    spend(&mut left, region.blocks.len() + region.states.len())?;
    if region.entries.len() != 1
        || region
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .map(|id| &region.instructions[id.index()])
            .any(|inst| {
                (inst.state.is_some() || inst.commit.is_some()) && inst.op != Op::PollBudget
            })
    {
        return Ok(disabled(states));
    }

    let origins = origins(region, &mut left)?;
    let mut masks = Vec::with_capacity(region.states.len());
    for (index, state) in region.states.iter().enumerate() {
        let plan = states.get(index).ok_or_else(|| {
            CompileError::InvalidIr("state-elision plan/state mismatch".into())
        })?;
        spend(&mut left, plan.cpu.writes.len())?;

        let arithmetic_clean = state
            .flags
            .arithmetic
            .iter()
            .enumerate()
            .all(|(bit, &value)| is_initial(&origins, value, Initial::FlagBit(bit as u8)));
        let flags_clean = arithmetic_clean
            && is_initial(&origins, state.flags.system, Initial::FlagSystem)
            && state
                .flags
                .raw_zero
                .is_some_and(|value| is_initial(&origins, value, Initial::RawZero))
            && state
                .flags
                .zero_is_lazy
                .is_some_and(|value| is_initial(&origins, value, Initial::ZeroLazy));

        let mut mask = vec![false; plan.cpu.writes.len()];
        for (write_index, write) in plan.cpu.writes.iter().enumerate() {
            mask[write_index] = match write.address {
                Address::Gpr(reg) => state
                    .gpr
                    .get(reg as usize)
                    .is_some_and(|&value| is_initial(&origins, value, Initial::Gpr(reg))),
                Address::FlagOperand => state
                    .flags
                    .last_op1
                    .is_some_and(|value| is_initial(&origins, value, Initial::FlagOperand)),
                Address::Flags => flags_clean,
                Address::Absolute(address)
                    if address == gp::last_result as u32
                        || address == gp::last_op_size as u32
                        || address == gp::flags_changed as u32 =>
                {
                    flags_clean
                },
                Address::Absolute(address) => state.xmm.iter().enumerate().any(|(reg, &value)| {
                    address == gp::get_reg_xmm_offset(reg as u32)
                        && is_initial(&origins, value, Initial::Xmm(reg as u8))
                }),
                Address::Eip | Address::Committed => false,
            };
        }
        masks.push(mask);
    }
    Ok(Plan {
        masks,
        enabled: false,
    })
}

pub(super) fn lower(
    region: &Region,
    states: &[StatePlan],
    work_limit: usize,
) -> Result<Plan, CompileError> {
    derive(region, states, work_limit)
}

pub(super) fn verify(region: &Region, data: &MirData) -> Result<(), CompileError> {
    let expected = derive(region, &data.states, DEFAULT_WORK_LIMIT)?;
    if data.state_elision.enabled || data.state_elision.masks != expected.masks {
        return Err(CompileError::InvalidIr(
            "invalid CPU state-elision certificate".into(),
        ));
    }
    Ok(())
}

pub(super) fn enable(data: &mut MirData, work_limit: usize) -> Result<usize, CompileError> {
    let work = data
        .state_elision
        .masks
        .iter()
        .try_fold(data.state_elision.masks.len(), |n, mask| n.checked_add(mask.len()))
        .ok_or(CompileError::Budget("MIR state elision work"))?;
    if work > work_limit {
        return Err(CompileError::Budget("MIR state elision work"));
    }
    let count = data
        .state_elision
        .masks
        .iter()
        .flatten()
        .filter(|&&skip| skip)
        .count();
    data.state_elision.enabled = true;
    Ok(count)
}

pub(super) fn elided(data: &MirData, state: StateId, write: usize) -> bool {
    data.state_elision.enabled
        && data
            .state_elision
            .masks
            .get(state.index())
            .and_then(|mask| mask.get(write))
            .copied()
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        backend::wasm::emit_cpu,
        frontend::{
            decode::{GuestEip, LinearAddress},
            region::lift_cpu_cfg,
        },
        lowering::lower,
        passes::{run, PassConfig},
    };

    fn optimized(bytes: &[u8]) -> crate::ir::mir::MirRegion {
        let mut region =
            lift_cpu_cfg(bytes, GuestEip(0x1000), LinearAddress(0x100000), true, 8).unwrap();
        run(&mut region, PassConfig::default()).unwrap();
        lower(&region).unwrap()
    }

    #[test]
    fn pure_cfg_elides_entry_equivalent_register_and_flag_writes() {
        // JECXZ and MOV do not modify FLAGS; the join therefore carries the
        // original flag backing across multiple blocks.
        let mut mir = optimized(&[0xE3, 2, 0x89, 0xD8, 0x90]);
        let count = enable(&mut mir.data, DEFAULT_WORK_LIMIT).unwrap();
        assert!(count > 0);
        let mut skipped_flags = 0;
        let mut skipped_gprs = 0;
        for (state, mask) in mir.states.iter().zip(&mir.state_elision.masks) {
            for (write, &skip) in state.cpu.writes.iter().zip(mask) {
                if !skip {
                    continue;
                }
                match write.address {
                    Address::Flags | Address::FlagOperand => skipped_flags += 1,
                    Address::Absolute(a)
                        if a == gp::last_result as u32
                            || a == gp::last_op_size as u32
                            || a == gp::flags_changed as u32 =>
                    {
                        skipped_flags += 1;
                    },
                    Address::Gpr(_) => skipped_gprs += 1,
                    _ => (),
                }
            }
        }
        assert!(skipped_flags > 0);
        assert!(skipped_gprs > 0);
        emit_cpu(&mir, 32).unwrap();
    }

    #[test]
    fn changed_flags_remain_materialized_and_memory_disables_the_pass() {
        let mut flags = optimized(&[0x40, 0x75, 0, 0x90]);
        enable(&mut flags.data, DEFAULT_WORK_LIMIT).unwrap();
        assert!(flags
            .states
            .iter()
            .zip(&flags.state_elision.masks)
            .any(|(state, mask)| state.cpu.writes.iter().zip(mask).any(
                |(write, &skip)| write.address == Address::Flags && !skip
            )));

        let mut memory = optimized(&[0x8B, 0x06, 0x90]);
        assert_eq!(enable(&mut memory.data, DEFAULT_WORK_LIMIT).unwrap(), 0);
        assert!(memory.state_elision.masks.iter().flatten().all(|skip| !skip));
    }

    #[test]
    fn enabling_is_bounded_and_atomic() {
        let mut mir = optimized(&[0xEB, 0xFE]);
        assert!(enable(&mut mir.data, 0).is_err());
        assert!(!mir.state_elision.enabled);
        assert!(enable(&mut mir.data, DEFAULT_WORK_LIMIT).unwrap() > 0);
        assert!(mir.state_elision.enabled);
    }
}
