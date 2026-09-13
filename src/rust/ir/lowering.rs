use super::{
    backend::locals::allocate,
    hir::{Op, Region},
    mir::{Draft, HelperCall, MirData, MirRegion},
    types::Type,
    verify::verify,
};
use crate::wasmgen::wasm_builder::Signature;
use std::collections::HashMap;
#[derive(Debug, Eq, PartialEq)]
pub enum CompileError {
    InvalidIr(String),
    Unsupported(&'static str),
    Budget(&'static str),
}
pub fn lower(region: &Region) -> Result<MirRegion, CompileError> {
    lower_draft(region)?.finish()
}
pub fn lower_draft(region: &Region) -> Result<Draft<'_>, CompileError> {
    if region.blocks.len() > 64 || region.instructions.len() > 8192 || region.values.len() > 16384 {
        return Err(CompileError::Budget("IR region size"));
    }
    verify(region).map_err(|e| CompileError::InvalidIr(e.0))?;
    for state in &region.states {
        if !state.x87.is_empty() {
            return Err(CompileError::Unsupported(
                "extended state materialization pending",
            ));
        }
    }
    let mut imports = HashMap::new();
    let mut helpers: Vec<Option<HelperCall>> = (0..region.helpers.len()).map(|_| None).collect();
    for value in &region.values {
        if !matches!(
            value.ty,
            Type::I1
                | Type::I8
                | Type::I16
                | Type::I32
                | Type::I64
                | Type::V128
                | Type::LinearAddress
                | Type::RmwTicket
                | Type::Effect
        ) {
            return Err(CompileError::Unsupported("unsupported backend value"));
        }
    }
    for block in &region.blocks {
        for id in &block.instructions {
            let inst = &region.instructions[id.index()];
            if matches!(
                inst.op,
                Op::GuestLoad { .. }
                    | Op::Divide { .. }
                    | Op::GuestStore { .. }
                    | Op::PartialStore { .. }
                    | Op::GuestCheck { .. }
                    | Op::SegmentAddress { .. }
                    | Op::PopAddress { .. }
                    | Op::RmwLoad { .. }
                    | Op::RmwStore { .. }
                    | Op::CompareExchange8B { .. }
                    | Op::SseCheck
                    | Op::XmmLoad { .. }
                    | Op::XmmBinary { .. }
                    | Op::XmmShuffle { .. }
                    | Op::XmmTransferLoad { .. }
                    | Op::XmmInsertWord { .. }
                    | Op::XmmStore { .. }
                    | Op::XmmMaskedStore { .. }
            ) {
                if matches!(inst.op, Op::GuestLoad { bytes } | Op::GuestStore { bytes } | Op::PartialStore { bytes } if !matches!(bytes, 1 | 2 | 4))
                {
                    return Err(CompileError::Unsupported(
                        "wide guest memory lowering pending",
                    ));
                }
                if region.states[inst.state.unwrap().index()].resume
                    != super::state::ResumeKind::BeforeInstruction
                {
                    return Err(CompileError::InvalidIr(
                        "memory fault needs pre-instruction state".into(),
                    ));
                }
            }
            if let Op::CallHelper(id) = region.instructions[id.index()].op {
                if helpers[id.index()].is_some() {
                    continue;
                }
                let descriptor = &region.helpers[id.index()];
                let call = super::mir::call::legalize(descriptor)?;
                let fault_delivery = &call.fault_delivery;
                let signature = call.signature.clone();
                for (name, signature) in std::iter::once((&descriptor.name, signature.clone()))
                    .chain(
                        fault_delivery
                            .iter()
                            .map(|name| (name, Signature::new(&[], &[]))),
                    )
                {
                    if name.is_empty()
                        || name == "m"
                        || imports.get(name).is_some_and(|s| *s != signature)
                    {
                        return Err(CompileError::InvalidIr(
                            "helper import name/signature conflict".into(),
                        ));
                    }
                    imports.insert(name.clone(), signature);
                }
                helpers[id.index()] = Some(call);
            }
        }
    }
    let memory: Vec<_> = region
        .instructions
        .iter()
        .map(super::mir::memory::lower)
        .collect();
    for plan in memory.iter().flatten() {
        let extra = if let super::mir::memory::SlowResult::Rmw { read_value, .. } = &plan.result {
            Some(read_value)
        } else {
            None
        };
        for call in std::iter::once(&plan.call).chain(extra) {
            if imports
                .get(call.name)
                .is_some_and(|signature| *signature != call.signature)
            {
                return Err(CompileError::InvalidIr(
                    "memory/helper import signature conflict".into(),
                ));
            }
            imports.insert(call.name.to_owned(), call.signature.clone());
        }
    }
    let effects: Vec<_> = region
        .instructions
        .iter()
        .map(super::mir::effect::lower)
        .collect();
    for plan in effects.iter().flatten() {
        let call = plan.call();
        if imports
            .get(call.name)
            .is_some_and(|signature| *signature != call.signature)
        {
            return Err(CompileError::InvalidIr(
                "effect/helper import signature conflict".into(),
            ));
        }
        imports.insert(call.name.to_owned(), call.signature.clone());
    }
    let calls = region
        .instructions
        .iter()
        .map(|inst| super::mir::call::lower(region, inst, &helpers))
        .collect();
    let values = region
        .instructions
        .iter()
        .map(|inst| super::mir::value::lower(region, inst))
        .collect();
    let states = region
        .states
        .iter()
        .map(super::mir::materialize::lower)
        .collect();
    let allocation = allocate(region).map_err(CompileError::Budget)?;
    let control = super::mir::control::lower(region, &allocation)?;
    Ok(Draft {
        hir: region,
        data: MirData {
            ram_forwarding: vec![None; region.instructions.len()],
            value_types: region.values.iter().map(|v| v.ty).collect(),
            allocation,
            helpers,
            memory,
            effects,
            calls,
            control,
            values,
            states,
        },
    })
}
