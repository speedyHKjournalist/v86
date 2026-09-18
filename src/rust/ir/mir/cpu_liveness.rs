//! CPU-only post-lowering liveness.
//!
//! Generic HIR keeps complete StateMaps so standalone emission and verification
//! remain unchanged. CPU StatePlans may instead recover exact lazy-FLAGS
//! backing. This certificate follows only values actually consumed by the CPU
//! plan and explicit guest semantics, allowing the CPU emitter to skip pure
//! value programs that are needed solely by standalone/concrete FLAGS recovery.

use super::{
    call::CallPlan,
    helper_state,
    materialize::StatePlan,
    value::Step,
    MirData,
};
use crate::ir::{
    hir::{Definition, Region, Terminator},
    ids::{InstId, StateId, ValueId},
    lowering::CompileError,
};
use std::collections::BTreeSet;

pub const DEFAULT_WORK_LIMIT: usize = 262_144;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    baseline_live: Vec<bool>,
    trimmed_live: Vec<bool>,
    enabled: bool,
    use_trimmed: bool,
}

fn spend(left: &mut usize, amount: usize) -> Result<(), CompileError> {
    *left = left
        .checked_sub(amount)
        .ok_or(CompileError::Budget("CPU liveness work"))?;
    Ok(())
}

fn expression_values(steps: &[Step], work: &mut Vec<ValueId>) {
    for step in steps {
        match step {
            Step::Value(value) => work.push(*value),
            Step::Packed {
                destination,
                source,
                ..
            } => {
                work.push(*destination);
                work.push(*source);
            },
            _ => (),
        }
    }
}

fn state_values(plan: &StatePlan, work: &mut Vec<ValueId>) {
    for write in &plan.cpu.writes {
        expression_values(&write.expression, work);
    }
    if let Some(base) = plan.cpu.count.base {
        work.push(base);
    }
    expression_values(&plan.decoded_next.expression, work);
}

fn used_states(
    region: &Region,
    calls: &[Option<CallPlan>],
    helper_plan: &helper_state::Plan,
    trim_helpers: bool,
) -> BTreeSet<StateId> {
    let mut states = BTreeSet::new();
    for block in &region.blocks {
        states.extend(block.entry_state);
        if let Some(Terminator::Exit(state)) = block.terminator {
            states.insert(state);
        }
        for id in &block.instructions {
            let inst = &region.instructions[id.index()];
            states.extend(inst.commit);
            if let Some(state) = inst.state {
                let trimmed_call = trim_helpers
                    && calls.get(id.index()).and_then(Option::as_ref).is_some()
                    && helper_state::plan_eligible(helper_plan, *id);
                if !trimmed_call {
                    states.insert(state);
                }
            }
        }
    }
    states
}

fn derive_mask(
    region: &Region,
    states: &[StatePlan],
    calls: &[Option<CallPlan>],
    helper_plan: &helper_state::Plan,
    trim_helpers: bool,
    work_limit: usize,
) -> Result<Vec<bool>, CompileError> {
    let mut left = work_limit;
    spend(
        &mut left,
        region.blocks.len() + region.instructions.len() + states.len(),
    )?;
    if states.len() != region.states.len() {
        return Err(CompileError::InvalidIr(
            "CPU liveness state-plan mismatch".into(),
        ));
    }

    let mut live = vec![false; region.instructions.len()];
    let mut seen_values = vec![false; region.values.len()];
    let mut work = Vec::new();

    for state in used_states(region, calls, helper_plan, trim_helpers) {
        let plan = states
            .get(state.index())
            .ok_or_else(|| CompileError::InvalidIr("CPU liveness state missing".into()))?;
        state_values(plan, &mut work);
    }

    for block in &region.blocks {
        let term = block
            .terminator
            .as_ref()
            .ok_or_else(|| CompileError::InvalidIr("CPU liveness terminator missing".into()))?;
        if let Terminator::CondBranch { condition, .. } = term {
            work.push(*condition);
        }
        for id in &block.instructions {
            let inst = &region.instructions[id.index()];
            if inst.op.ordered() || inst.state.is_some() || inst.commit.is_some() {
                if !live[id.index()] {
                    live[id.index()] = true;
                    work.extend(&inst.args);
                }
            }
        }
    }

    while let Some(value) = work.pop() {
        spend(&mut left, 1)?;
        let seen = seen_values
            .get_mut(value.index())
            .ok_or_else(|| CompileError::InvalidIr("CPU liveness value missing".into()))?;
        if *seen {
            continue;
        }
        *seen = true;
        let data = &region.values[value.index()];
        match data.definition {
            Definition::Instruction(id, _) => {
                if !live[id.index()] {
                    live[id.index()] = true;
                    work.extend(&region.instructions[id.index()].args);
                }
            },
            Definition::Parameter(block, parameter) => {
                let parameter = parameter as usize;
                for source in &region.blocks {
                    for edge in source
                        .terminator
                        .as_ref()
                        .ok_or_else(|| {
                            CompileError::InvalidIr("CPU liveness edge source missing".into())
                        })?
                        .edges()
                    {
                        if edge.target == block {
                            spend(&mut left, 1)?;
                            let value = edge.args.get(parameter).ok_or_else(|| {
                                CompileError::InvalidIr(
                                    "CPU liveness edge/parameter mismatch".into(),
                                )
                            })?;
                            work.push(*value);
                        }
                    }
                }
            },
        }
    }

    Ok(live)
}

pub(crate) fn lower(
    region: &Region,
    states: &[StatePlan],
    calls: &[Option<CallPlan>],
    helper_plan: &helper_state::Plan,
    work_limit: usize,
) -> Result<Plan, CompileError> {
    let fallback = || vec![true; region.instructions.len()];
    let baseline_live = match derive_mask(
        region,
        states,
        calls,
        helper_plan,
        false,
        work_limit,
    ) {
        Ok(live) => live,
        Err(CompileError::Budget(_)) => fallback(),
        Err(error) => return Err(error),
    };
    let trimmed_live = match derive_mask(
        region,
        states,
        calls,
        helper_plan,
        true,
        work_limit,
    ) {
        Ok(live) => live,
        Err(CompileError::Budget(_)) => fallback(),
        Err(error) => return Err(error),
    };
    Ok(Plan {
        baseline_live,
        trimmed_live,
        enabled: false,
        use_trimmed: false,
    })
}

pub(super) fn verify(region: &Region, data: &MirData) -> Result<(), CompileError> {
    let expected = lower(
        region,
        &data.states,
        &data.calls,
        &data.helper_state,
        DEFAULT_WORK_LIMIT,
    )?;
    if data.cpu_liveness.enabled
        || data.cpu_liveness.use_trimmed
        || data.cpu_liveness.baseline_live != expected.baseline_live
        || data.cpu_liveness.trimmed_live != expected.trimmed_live
    {
        return Err(CompileError::InvalidIr(
            "invalid CPU liveness certificate".into(),
        ));
    }
    Ok(())
}

pub(super) fn enable(data: &mut MirData, work_limit: usize) -> Result<usize, CompileError> {
    let use_trimmed = data.helper_state.enabled;
    let live = if use_trimmed {
        &data.cpu_liveness.trimmed_live
    } else {
        &data.cpu_liveness.baseline_live
    };
    if live.len() > work_limit {
        return Err(CompileError::Budget("CPU liveness work"));
    }
    let count = live
        .iter()
        .enumerate()
        .filter(|(index, live)| !**live && data.values[*index].is_some())
        .count();
    data.cpu_liveness.enabled = true;
    data.cpu_liveness.use_trimmed = use_trimmed;
    Ok(count)
}

pub(super) fn instruction_live(data: &MirData, id: InstId) -> bool {
    let live = if data.cpu_liveness.use_trimmed {
        &data.cpu_liveness.trimmed_live
    } else {
        &data.cpu_liveness.baseline_live
    };
    !data.cpu_liveness.enabled
        || live.get(id.index()).copied().unwrap_or(true)
        || !matches!(data.values.get(id.index()), Some(Some(_)))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        backend::wasm::{emit, emit_cpu, StateLayout},
        frontend::{
            decode::{GuestEip, LinearAddress},
            region::lift_cpu_cfg,
        },
        lowering::lower,
        passes::{run, PassConfig},
    };

    fn region(bytes: &[u8]) -> crate::ir::hir::Region {
        let mut region =
            lift_cpu_cfg(bytes, GuestEip(0x1000), LinearAddress(0x100000), true, 8).unwrap();
        run(&mut region, PassConfig::default()).unwrap();
        region
    }

    fn layout() -> StateLayout {
        StateLayout {
            gpr: 256,
            flags: 288,
            eip: 292,
            committed: 296,
            flag_operand: 300,
        }
    }

    #[test]
    fn cpu_liveness_elides_concrete_flags_without_changing_standalone() {
        // ADD EAX,EBX; ADD ECX,EDX; JNZ +2; NOP; NOP; NOP.
        // CPU recovery can carry exact lazy backing across both arithmetic
        // boundaries, while JNZ explicitly demands only ZF from the second ADD.
        let region = region(&[0x01, 0xD8, 0x01, 0xD1, 0x75, 0x02, 0x90, 0x90, 0x90]);
        let baseline = lower(&region).unwrap();
        let baseline_cpu = emit_cpu(&baseline, 100).unwrap().bytes;
        let baseline_standalone = emit(&baseline, layout(), 100).unwrap().bytes;

        let mut optimized = lower(&region).unwrap();
        let count = optimized
            .elide_dead_cpu_values(DEFAULT_WORK_LIMIT)
            .unwrap();
        assert!(count > 0, "scalar ALU should expose CPU-only dead flag values");
        let optimized_cpu = emit_cpu(&optimized, 100).unwrap().bytes;
        let optimized_standalone = emit(&optimized, layout(), 100).unwrap().bytes;

        assert!(
            optimized_cpu.len() < baseline_cpu.len(),
            "CPU liveness should remove emitted value programs"
        );
        assert_eq!(
            optimized_standalone, baseline_standalone,
            "CPU-only liveness must not alter standalone emission"
        );
    }

    #[test]
    fn extended_integer_flag_families_keep_exact_recovery() {
        let region = region(&[
            0xD1, 0xE0,             // SHL EAX, 1
            0xD1, 0xC9,             // ROR ECX, 1
            0x0F, 0xA3, 0xC8,       // BT EAX, ECX
            0xF3, 0x0F, 0xB8, 0xD8, // POPCNT EBX, EAX
            0x0F, 0xAF, 0xC3,       // IMUL EAX, EBX
            0xF8,                   // CLC
            0xFC,                   // CLD
            0x75, 0x00,             // JNZ
            0x90,
        ]);
        let mir = lower(&region).unwrap();
        assert!(
            mir.states.iter().all(|state| state.lazy_flags),
            "audited shift/bit/multiply/control states should keep exact recovery"
        );
    }

    #[test]
    fn mixed_eager_lazy_integer_flags_keep_exact_recovery() {
        // ADC/SBB eagerly materialize CF/AF/OF; INC/DEC eagerly preserve CF.
        // Their remaining arithmetic flags stay lazy in the baseline.
        let region = region(&[
            0x11, 0xD8, // ADC EAX, EBX
            0x19, 0xD1, // SBB ECX, EDX
            0x40,       // INC EAX
            0x49,       // DEC ECX
            0x75, 0x00, // JNZ
            0x90,
        ]);
        let mir = lower(&region).unwrap();
        assert!(
            mir.states.iter().all(|state| state.lazy_flags),
            "audited mixed eager/lazy integer states should use exact lazy recovery"
        );
    }
}
