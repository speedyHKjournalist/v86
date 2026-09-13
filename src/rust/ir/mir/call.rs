//! Per-call-site observation, staged results and outcome edges.
use crate::ir::{
    helper::{after_call, AdapterAction, HelperAbi, HelperDescriptor, Outcome},
    hir::{Instruction, Op, Region},
    ids::{HelperId, StateId, ValueId},
    lowering::CompileError,
    mir::HelperCall,
    state::ResumeKind,
    types::Type,
};
use crate::wasmgen::wasm_builder::{Signature, WasmType};
pub fn legalize(descriptor: &HelperDescriptor) -> Result<HelperCall, CompileError> {
    descriptor
        .validate()
        .map_err(|message| CompileError::InvalidIr(message.into()))?;
    if descriptor
        .params
        .iter()
        .chain(&descriptor.results)
        .any(|t| *t == Type::V128)
    {
        return Err(CompileError::Unsupported(
            "vector helper requires explicit scratch ABI",
        ));
    }
    let (fault_delivery, normal_preserves_state, cpu_exit) = match &descriptor.abi {
        HelperAbi::Outcome {
            fault_delivery,
            normal_preserves_state,
        } => (fault_delivery.clone(), *normal_preserves_state, false),
        HelperAbi::CpuExit | HelperAbi::CpuRep => (None, false, true),
        HelperAbi::Unadapted => return Err(CompileError::Unsupported("unadapted helper ABI")),
    };
    // Continuing SSA cannot observe writes without explicit reloads.
    if !normal_preserves_state && !cpu_exit {
        return Err(CompileError::Unsupported("helper state reload pending"));
    }
    let caller_fault =
        after_call(descriptor, Outcome::FaultNeedsDelivery) == Ok(AdapterAction::RestoreAndDeliver);
    if caller_fault != fault_delivery.is_some() {
        return Err(CompileError::InvalidIr(
            "helper fault delivery adapter mismatch".into(),
        ));
    }
    let wasm_type = |t: &Type| if *t == Type::I64 { WasmType::I64 } else { WasmType::I32 };
    let params: Vec<_> = descriptor.params.iter().map(wasm_type).collect();
    let results: Vec<_> = std::iter::once(WasmType::I32)
        .chain(descriptor.results.iter().map(wasm_type))
        .collect();
    let signature = Signature::new(&params, &results);
    Ok(HelperCall {
        name: descriptor.name.clone(),
        cpu_exit,
        signature,
        fault_delivery: fault_delivery.clone(),
        exit_outcomes: [
            Outcome::ControlTransferred,
            Outcome::Yield,
            Outcome::Invalidated,
        ]
        .into_iter()
        .filter(|&outcome| after_call(descriptor, outcome) == Ok(AdapterAction::ExitWithoutRestore))
        .map(|outcome| outcome as u32)
        .collect(),
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observation {
    CapturedState,
    DecodedNextPc,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultSlot {
    pub value: ValueId,
    pub ty: Type,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivery {
    pub outcome: u32,
    pub restore: StateId,
    pub name: String,
    pub signature: Signature,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallPlan {
    pub helper: HelperId,
    pub state: StateId,
    pub cpu_observation: Observation,
    pub standalone_observation: Observation,
    pub args: Vec<ValueId>,
    /// Pop order for multi-results; assignments occur only after the outcome check.
    pub staged: Vec<ResultSlot>,
    pub delivery: Option<Delivery>,
    pub exits: Vec<u32>,
    pub normal: Option<u32>,
}
pub fn lower(
    region: &Region,
    inst: &Instruction,
    helpers: &[Option<HelperCall>],
) -> Option<CallPlan> {
    let Op::CallHelper(helper) = inst.op else {
        return None;
    };
    let call = helpers[helper.index()].as_ref()?;
    let state = inst.state.unwrap();
    Some(CallPlan {
        helper,
        state,
        cpu_observation: if matches!(
            region.states[state.index()].resume,
            ResumeKind::BeforeInstruction | ResumeKind::RepProgress
        ) {
            Observation::DecodedNextPc
        } else {
            Observation::CapturedState
        },
        standalone_observation: Observation::CapturedState,
        args: inst.args[..inst.args.len() - 1].to_vec(),
        staged: inst.results[..inst.results.len() - 1]
            .iter()
            .rev()
            .map(|&value| ResultSlot {
                value,
                ty: region.values[value.index()].ty,
            })
            .collect(),
        delivery: call.fault_delivery.as_ref().map(|name| Delivery {
            outcome: Outcome::FaultNeedsDelivery as u32,
            restore: state,
            name: name.clone(),
            signature: Signature::new(&[], &[]),
        }),
        exits: call.exit_outcomes.clone(),
        normal: (!call.cpu_exit).then_some(Outcome::Normal as u32),
    })
}
pub fn verify(
    region: &Region,
    helpers: &[Option<HelperCall>],
    plans: &[Option<CallPlan>],
) -> Result<(), CompileError> {
    let invalid = || CompileError::InvalidIr("invalid lowered helper call plan".into());
    if helpers.len() != region.helpers.len() || plans.len() != region.instructions.len() {
        return Err(invalid());
    }
    for (descriptor, call) in region.helpers.iter().zip(helpers) {
        if let Some(call) = call {
            if legalize(descriptor)? != *call {
                return Err(invalid());
            }
        }
    }
    let mut live = vec![false; region.instructions.len()];
    for block in &region.blocks {
        for id in &block.instructions {
            live[id.index()] = true;
        }
    }
    for (index, (inst, plan)) in region.instructions.iter().zip(plans).enumerate() {
        if live[index] && matches!(inst.op, Op::CallHelper(_)) && plan.is_none()
            || lower(region, inst, helpers).as_ref() != plan.as_ref()
        {
            return Err(invalid());
        }
    }
    Ok(())
}
