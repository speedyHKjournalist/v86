//! Guarded batching does not change the unit-count policy or any recovery map.
//! It adds a second body usable only when all original credits are present.
use super::{value::{Reading, Step}, MirData};
use crate::ir::{ids::BlockId, lowering::CompileError};
pub const DEFAULT_WORK_LIMIT: usize = 262_144;
const MAX_BODY_INSTRUCTIONS: usize = 512;
const MAX_POLLS: u32 = 31;
const MAX_BATCHES: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Batch {
    /// First duplicated instruction. Its preceding poll (or the block-entry
    /// poll for zero) remains on the mandatory, unspeculated path.
    pub start: usize,
    pub cost: u32,
}
fn observes(data: &MirData, id: crate::ir::ids::InstId) -> bool {
    let i = id.index();
    if data.memory[i].is_some() || data.effects[i].is_some() || data.calls[i].is_some() {
        return true;
    }
    if data.control.polls[i].is_some() { return false; }
    let Some(value) = &data.values[i] else { return true; };
    // Backing-state loads do not observe the host. Opaque imports do, even
    // when a particular implementation happens to preserve CPU state.
    value.steps.iter().any(|step| matches!(step, Step::Read { cpu: Reading::Call { .. }, .. }
        | Step::Read { standalone: Reading::Call { .. }, .. }))
}
fn certificate(data: &MirData, id: BlockId) -> Option<Batch> {
    let block = &data.control.blocks[id.index()];
    if block.budget_cost != 1 || data.helpers.iter().flatten().any(|h| h.starts_interrupt_shadow) {
        return None;
    }
    let last_observer = block.instructions.iter().rposition(|&id| observes(data, id));
    let start = if last_observer.is_some() || block.recovery.is_none() {
        // Do not speculate over a load/store/helper. Its following budget poll
        // checks both remaining credit and the active code epoch as before.
        let after = last_observer.map_or(0, |i| i + 1);
        after + block.instructions[after..].iter().position(|id|
            data.control.polls[id.index()].as_ref().is_some_and(|p| p.cost == 1))? + 1
    } else { 0 };
    if block.instructions.len() - start > MAX_BODY_INSTRUCTIONS { return None; }
    let mut cost = 0u32;
    for &id in &block.instructions[start..] {
        if let Some(poll) = &data.control.polls[id.index()] {
            if poll.cost != 1 { return None; }
            cost = cost.checked_add(1)?;
        }
    }
    (2..=MAX_POLLS).contains(&cost).then_some(Batch { start, cost })
}
pub(super) fn enable(data: &mut MirData, work_limit: usize) -> Result<usize, CompileError> {
    let values = data.values.iter().flatten().try_fold(0usize, |n, v| n.checked_add(v.steps.len()))
        .ok_or(CompileError::Budget("budget batch work"))?;
    let work = data.control.blocks.iter().try_fold(values, |total, b|
        total.checked_add(b.instructions.len()).and_then(|n| n.checked_add(data.helpers.len())))
        .ok_or(CompileError::Budget("budget batch work"))?;
    if work > work_limit { return Err(CompileError::Budget("budget batch work")); }
    let mut batches = vec![None; data.control.blocks.len()];
    let mut count = 0;
    for (index, batch) in batches.iter_mut().enumerate() {
        if count == MAX_BATCHES { break; }
        *batch = certificate(data, BlockId(index as u32));
        count += usize::from(batch.is_some());
    }
    data.poll_batches = batches;
    Ok(count)
}
pub(super) fn verify(data: &MirData) -> Result<(), CompileError> {
    if data.poll_batches.len() != data.control.blocks.len()
        || data.poll_batches.iter().flatten().count() > MAX_BATCHES
        || data.poll_batches.iter().enumerate().any(|(index, batch)|
            batch.is_some() && *batch != certificate(data, BlockId(index as u32))) {
        return Err(CompileError::InvalidIr("invalid pure budget batch certificate".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{frontend::{decode::{GuestEip, LinearAddress}, region::lift_cpu_cfg},
        lowering::lower, passes::{run, PassConfig}};
    fn make(bytes: &[u8]) -> crate::ir::mir::MirRegion {
        let mut r = lift_cpu_cfg(bytes, GuestEip(0x100000), LinearAddress(0x100000), true, 64).unwrap();
        run(&mut r, PassConfig::default()).unwrap();
        lower(&r).unwrap()
    }
    #[test]
    fn bounded_owned_proof_rejects_forged_cost_and_budget_failure_is_transactional() {
        let mut mir = make(&[0x40,0x43,0x46,0x4B,0xEB,0xFA]);
        assert!(mir.batch_pure_budget_polls(0).is_err());
        assert!(mir.poll_batches.iter().all(Option::is_none));
        assert!(mir.batch_pure_budget_polls(DEFAULT_WORK_LIMIT).unwrap() > 0);
        mir.verify().unwrap();
        let saved = mir.poll_batches.clone();
        assert!(mir.batch_pure_budget_polls(0).is_err());
        assert_eq!(mir.poll_batches, saved);
        let at = saved.iter().position(Option::is_some).unwrap();
        let original = saved[at].unwrap();
        mir.data.poll_batches[at] = Some(Batch { cost: original.cost + 1, ..original });
        assert!(mir.verify().is_err());
        mir.data.poll_batches = saved;
        mir.verify().unwrap();
    }
    #[test]
    fn stores_loads_observers_and_interrupt_shadow_do_not_enter_a_pure_batch() {
        for bytes in [&[0x40,0x43,0x46,0x89,0x06,0x47][..],
            &[0x40,0x43,0x46,0x8B,0x06,0x47][..],
            &[0x40,0x43,0x46,0x0F,0x31,0x47][..],
            &[0xFB,0x40,0x43,0x46,0x47][..]] {
            let mut mir = make(bytes);
            mir.batch_pure_budget_polls(DEFAULT_WORK_LIMIT).unwrap();
            for (i, block) in mir.control.blocks.iter().enumerate() {
                if let Some(batch) = mir.poll_batches[i] {
                    for id in &block.instructions[batch.start..] { assert!(!observes(&mir, *id)); }
                }
            }
            if bytes[0] == 0xFB { assert!(mir.poll_batches.iter().all(Option::is_none)); }
            mir.verify().unwrap();
        }
    }
}
