//! CPU-only post-lowering liveness.
//!
//! Generic HIR keeps complete StateMaps so standalone emission and verification
//! remain unchanged. CPU StatePlans may instead recover exact lazy-FLAGS
//! backing. This certificate follows only values actually consumed by the CPU
//! plan and explicit guest semantics, allowing the CPU emitter to skip pure
//! value programs that are needed solely by standalone/concrete FLAGS recovery.

use super::{materialize::StatePlan, value::Step, MirData};
use crate::ir::{
    hir::{Definition, Region, Terminator},
    ids::{InstId, StateId, ValueId},
    lowering::CompileError,
};
use std::collections::BTreeSet;

pub const DEFAULT_WORK_LIMIT: usize = 262_144;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    live: Vec<bool>,
    enabled: bool,
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

fn used_states(region: &Region) -> BTreeSet<StateId> {
    let mut states = BTreeSet::new();
    for block in &region.blocks {
        states.extend(block.entry_state);
        if let Some(Terminator::Exit(state)) = block.terminator {
            states.insert(state);
        }
        for id in &block.instructions {
            let inst = &region.instructions[id.index()];
            states.extend(inst.state);
            states.extend(inst.commit);
        }
    }
    states
}

fn derive(
    region: &Region,
    states: &[StatePlan],
    work_limit: usize,
) -> Result<Plan, CompileError> {
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
    let mut work = Vec::new();

    for state in used_states(region) {
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
        let data = region
            .values
            .get(value.index())
            .ok_or_else(|| CompileError::InvalidIr("CPU liveness value missing".into()))?;
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

    Ok(Plan {
        live,
        enabled: false,
    })
}

pub(crate) fn lower(
    region: &Region,
    states: &[StatePlan],
    work_limit: usize,
) -> Result<Plan, CompileError> {
    match derive(region, states, work_limit) {
        Ok(plan) => Ok(plan),
        Err(CompileError::Budget(_)) => Ok(Plan {
            live: vec![true; region.instructions.len()],
            enabled: false,
        }),
        Err(error) => Err(error),
    }
}

pub(super) fn verify(region: &Region, data: &MirData) -> Result<(), CompileError> {
    let expected = lower(region, &data.states, DEFAULT_WORK_LIMIT)?;
    if data.cpu_liveness.enabled || data.cpu_liveness.live != expected.live {
        return Err(CompileError::InvalidIr(
            "invalid CPU liveness certificate".into(),
        ));
    }
    Ok(())
}

pub(super) fn enable(data: &mut MirData, work_limit: usize) -> Result<usize, CompileError> {
    if data.cpu_liveness.live.len() > work_limit {
        return Err(CompileError::Budget("CPU liveness work"));
    }
    let count = data
        .cpu_liveness
        .live
        .iter()
        .enumerate()
        .filter(|(index, live)| !**live && data.values[*index].is_some())
        .count();
    data.cpu_liveness.enabled = true;
    Ok(count)
}

pub(super) fn instruction_live(data: &MirData, id: InstId) -> bool {
    !data.cpu_liveness.enabled
        || data
            .cpu_liveness
            .live
            .get(id.index())
            .copied()
            .unwrap_or(true)
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
    fn partial_flag_mutation_disables_lazy_recovery() {
        // INC preserves CF with a mixed eager/lazy backing layout. Until that
        // exact layout is modeled, its post-instruction state stays canonical.
        let region = region(&[0x40, 0x75, 0x00, 0x90]);
        let mir = lower(&region).unwrap();
        assert!(
            mir.states.iter().any(|state| !state.lazy_flags),
            "INC path must retain at least one canonical FLAGS recovery state"
        );
    }
}
