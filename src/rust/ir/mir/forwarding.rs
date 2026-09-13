//! Guarded, intra-block ordinary-RAM read forwarding. A static chain is not a
//! license to reuse a slow read: the emitter carries a separate runtime valid
//! bit which is set ONLY by a successful native RAM guard/load. MMIO, callbacks,
//! page crossings and faults keep their original ordered slow paths.
use super::{
    effect::EffectPlan,
    memory::{Argument, MemoryPlan, NativeMemory, RamGuard, SlowResult},
    value::Step,
    MirData,
};
use crate::ir::{ids::*, lowering::CompileError};

pub const DEFAULT_WORK_LIMIT: usize = 262_144;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Forwarding {
    /// Reset validity before the first guarded read of each chain, on every visit.
    Begin,
    /// A preceding candidate in this block, with the same address and width.
    Reuse { previous: InstId },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AddressKey {
    Value(ValueId),
    Segment {
        base: u32,
        null_byte: u32,
        offset: ValueId,
    },
}

fn eligible(plan: &MemoryPlan) -> bool {
    let NativeMemory::ScalarLoad {
        result,
        ticket: None,
    } = plan.native
    else {
        return false;
    };
    let bytes = plan.guard.bytes;
    matches!(bytes, 1 | 2 | 4)
        && plan.guard == RamGuard::new(bytes, false)
        && plan.result
            == (SlowResult::Packed {
                result,
                trap_after_fault: false,
            })
        && plan.call.name == "ir_memory_read"
        && plan.call.args == [Argument::Value(plan.address), Argument::I32(bytes as i32)]
}
fn segment(plan: &EffectPlan) -> Option<(ValueId, AddressKey)> {
    match *plan {
        EffectPlan::Address {
            base,
            null_byte,
            offset,
            result,
            ref call,
            trap_after_fault: false,
            ..
        } if call.name == "ir_segment_address" => Some((
            result,
            AddressKey::Segment {
                base,
                null_byte,
                offset,
            },
        )),
        _ => None,
    }
}
fn spend(left: &mut usize, amount: usize) -> Result<(), CompileError> {
    *left = left
        .checked_sub(amount)
        .ok_or(CompileError::Budget("MIR RAM forwarding work"))?;
    Ok(())
}

/// Derive the full certificate from owned, already legalized machine plans.
/// No HIR, runtime memory, CPU state or mutable compiler context is consulted.
fn plan(data: &MirData, work_limit: usize) -> Result<Vec<Option<Forwarding>>, CompileError> {
    let n = data.memory.len();
    if data.control.blocks.len() > 64 || n > 8192 || data.value_types.len() > 16384 {
        return Err(CompileError::Budget("MIR RAM forwarding region"));
    }
    if [
        data.effects.len(),
        data.calls.len(),
        data.values.len(),
        data.control.polls.len(),
    ]
    .iter()
    .any(|&len| len != n)
    {
        return Err(CompileError::InvalidIr(
            "inconsistent forwarding arenas".into(),
        ));
    }
    let mut left = work_limit;
    spend(&mut left, n + data.value_types.len())?;
    let mut result = vec![None; n];
    let mut addresses = vec![None; data.value_types.len()];
    // Segment expressions remain at their original positions. Canonical keys
    // identify repeated resolutions but never remove the segment-null guard.
    for effect in data.effects.iter().flatten() {
        spend(&mut left, 1)?;
        if let Some((value, key)) = segment(effect) {
            let slot = addresses.get_mut(value.index()).ok_or_else(|| {
                CompileError::InvalidIr("forwarding address outside value arena".into())
            })?;
            *slot = Some(key);
        }
    }
    for block in &data.control.blocks {
        let mut previous: Option<(AddressKey, u8, InstId)> = None;
        for &id in &block.instructions {
            spend(&mut left, 1)?;
            let index = id.index();
            let Some(memory) = data.memory.get(index) else {
                return Err(CompileError::InvalidIr(
                    "forwarding instruction outside arena".into(),
                ));
            };
            if let Some(poll) = &data.control.polls[index] {
                // The checked one-unit poll only updates a Wasm budget local on
                // continuation. Its materialization arm RETURNS from the entry;
                // it cannot resume with a stale cache after observing CPU state.
                if poll.cost == 1 {
                    continue;
                }
                previous = None;
                continue;
            }
            if let Some(memory) = memory {
                if eligible(memory) {
                    let key = addresses
                        .get(memory.address.index())
                        .copied()
                        .flatten()
                        .unwrap_or(AddressKey::Value(memory.address));
                    if let Some((old_key, bytes, old)) = previous {
                        if key == old_key && bytes == memory.guard.bytes {
                            if result[old.index()].is_none() {
                                result[old.index()] = Some(Forwarding::Begin);
                            }
                            result[index] = Some(Forwarding::Reuse { previous: old });
                        }
                    }
                    previous = Some((key, memory.guard.bytes, id));
                    continue;
                }
            } else if let Some(effect) = &data.effects[index] {
                if segment(effect).is_some() {
                    continue;
                }
            } else if data.calls[index].is_none() && data.control.polls[index].is_none() {
                if let Some(value) = &data.values[index] {
                    spend(&mut left, value.steps.len())?;
                    // CPU observations (including helper reads) are conservative
                    // barriers, even if the corresponding HIR node is unordered.
                    if value
                        .steps
                        .iter()
                        .all(|step| !matches!(step, Step::Read { .. }))
                    {
                        continue;
                    }
                }
            }
            // Stores, RMW, checks, division, calls, vector reads and every
            // unrecognized action terminate the chain regardless of alias class.
            previous = None;
        }
    }
    Ok(result)
}

pub(super) fn optimize(data: &mut MirData, work_limit: usize) -> Result<usize, CompileError> {
    let next = plan(data, work_limit)?;
    let count = next
        .iter()
        .filter(|p| matches!(p, Some(Forwarding::Reuse { .. })))
        .count();
    // Planning is read-only; a failed bound/validation cannot partially enable
    // forwarding. No SSA use, recovery map or allocated local is rewritten.
    data.ram_forwarding = next;
    Ok(count)
}

pub(super) fn verify(data: &MirData) -> Result<(), CompileError> {
    if data.ram_forwarding.len() != data.memory.len() {
        return Err(CompileError::InvalidIr(
            "invalid RAM forwarding certificate length".into(),
        ));
    }
    // All-None is the explicit disabled form. Otherwise require the COMPLETE
    // deterministic certificate, preventing a reuse without its initializing read.
    if data.ram_forwarding.iter().any(Option::is_some)
        && data.ram_forwarding != plan(data, DEFAULT_WORK_LIMIT)?
    {
        return Err(CompileError::InvalidIr(
            "invalid RAM forwarding certificate".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../../tests/ir/semantics/mir_forwarding.rs"]
mod tests;
