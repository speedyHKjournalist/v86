//! Audited helper-state observation trimming.
//!
//! A helper can skip pre-call CPU StateMap materialization only when its
//! descriptor proves that it neither reads nor writes architectural state,
//! cannot fault/transfer/yield/invalidate, and its normal ABI preserves state.
//! The certificate is derived at lowering and enabled explicitly for Tier 2.

use super::{call::CallPlan, MirData};
use crate::ir::{
    helper::{ExceptionOwner, HelperAbi},
    hir::{Op, Region},
    ids::InstId,
    lowering::CompileError,
};

pub const DEFAULT_WORK_LIMIT: usize = 16_384;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    eligible: Vec<bool>,
    enabled: bool,
}

fn derive(region: &Region, calls: &[Option<CallPlan>], work_limit: usize) -> Result<Plan, CompileError> {
    if region.instructions.len() > work_limit || calls.len() != region.instructions.len() {
        return Err(CompileError::Budget("helper-state work"));
    }
    let mut eligible = vec![false; region.instructions.len()];
    for block in &region.blocks {
        for &id in &block.instructions {
            let inst = &region.instructions[id.index()];
            let Op::CallHelper(helper) = inst.op else { continue };
            let descriptor = region
                .helpers
                .get(helper.index())
                .ok_or_else(|| CompileError::InvalidIr("helper-state descriptor missing".into()))?;
            let Some(plan) = calls.get(id.index()).and_then(Option::as_ref) else {
                return Err(CompileError::InvalidIr("helper-state call plan missing".into()));
            };
            let normal_preserves = matches!(
                descriptor.abi,
                HelperAbi::Outcome {
                    normal_preserves_state: true,
                    fault_delivery: None,
                }
            );
            eligible[id.index()] = descriptor.effects.is_pure()
                && descriptor.exception_owner == ExceptionOwner::CannotFault
                && normal_preserves
                && plan.delivery.is_none()
                && plan.exits.is_empty()
                && plan.normal.is_some();
        }
    }
    Ok(Plan { eligible, enabled: false })
}

pub(crate) fn lower(
    region: &Region,
    calls: &[Option<CallPlan>],
    work_limit: usize,
) -> Result<Plan, CompileError> {
    match derive(region, calls, work_limit) {
        Ok(plan) => Ok(plan),
        Err(CompileError::Budget(_)) => Ok(Plan {
            eligible: vec![false; region.instructions.len()],
            enabled: false,
        }),
        Err(error) => Err(error),
    }
}

pub(super) fn verify(region: &Region, data: &MirData) -> Result<(), CompileError> {
    let expected = lower(region, &data.calls, DEFAULT_WORK_LIMIT)?;
    if data.helper_state.enabled || data.helper_state.eligible != expected.eligible {
        return Err(CompileError::InvalidIr(
            "invalid helper-state certificate".into(),
        ));
    }
    Ok(())
}

pub(super) fn enable(data: &mut MirData, work_limit: usize) -> Result<usize, CompileError> {
    if data.helper_state.eligible.len() > work_limit {
        return Err(CompileError::Budget("helper-state work"));
    }
    let count = data.helper_state.eligible.iter().filter(|&&eligible| eligible).count();
    data.helper_state.enabled = true;
    Ok(count)
}

pub(super) fn elided(data: &MirData, id: InstId) -> bool {
    data.helper_state.enabled
        && data
            .helper_state
            .eligible
            .get(id.index())
            .copied()
            .unwrap_or(false)
}

pub(super) fn eligible(data: &MirData, id: InstId) -> bool {
    data.helper_state
        .eligible
        .get(id.index())
        .copied()
        .unwrap_or(false)
}
