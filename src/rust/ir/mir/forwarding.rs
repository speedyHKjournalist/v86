//! Guarded ordinary-RAM forwarding and loop-invariant load caching.
//!
//! Static certificates never license reuse of a slow access. Runtime validity is
//! established only by a successful native RAM guard/load (or by a committed
//! native scalar store for the intra-block chain). Slow paths clear all loop
//! caches before page walking/MMIO callbacks, preserving mapping and exception
//! ordering.
use super::{
    effect::EffectPlan,
    memory::{Argument, MemoryPlan, NativeMemory, RamGuard, SlowResult},
    value::{Reading, Step},
    MirData,
};
use crate::ir::{ids::*, lowering::CompileError};

pub const DEFAULT_WORK_LIMIT: usize = 262_144;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Forwarding {
    /// Reset validity before the first guarded source of each intra-block chain.
    Begin,
    /// A preceding load/store source in this block with an exact address proof.
    Reuse { previous: InstId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoopForwarding {
    pub slot: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoopPlan {
    pub(super) instructions: Vec<Option<LoopForwarding>>,
    pub(super) resets: Vec<Vec<usize>>,
    pub(super) slots: usize,
}
impl LoopPlan {
    pub(crate) fn disabled(instructions: usize, blocks: usize) -> Self {
        Self {
            instructions: vec![None; instructions],
            resets: vec![vec![]; blocks],
            slots: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AliasProof {
    Exact,
    Disjoint,
    MayAlias,
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

fn eligible_load(plan: &MemoryPlan) -> bool {
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

fn eligible_store(plan: &MemoryPlan) -> bool {
    let NativeMemory::ScalarStore {
        value,
        commit: Some(commit),
    } = plan.native
    else {
        return false;
    };
    let bytes = plan.guard.bytes;
    matches!(bytes, 1 | 2 | 4)
        && plan.guard == RamGuard::new(bytes, true)
        && plan.result
            == (SlowResult::Store {
                commit: Some(commit),
                trap_after_fault: false,
            })
        && plan.call.name == "ir_memory_write"
        && plan.call.args
            == [
                Argument::Value(plan.address),
                Argument::Value(value),
                Argument::I32(bytes as i32),
            ]
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
        .ok_or(CompileError::Budget("MIR RAM optimization work"))?;
    Ok(())
}

fn address_keys(data: &MirData, left: &mut usize) -> Result<Vec<Option<AddressKey>>, CompileError> {
    let mut addresses = vec![None; data.value_types.len()];
    for effect in data.effects.iter().flatten() {
        spend(left, 1)?;
        if let Some((value, key)) = segment(effect) {
            let slot = addresses.get_mut(value.index()).ok_or_else(|| {
                CompileError::InvalidIr("RAM proof address outside value arena".into())
            })?;
            *slot = Some(key);
        }
    }
    Ok(addresses)
}

fn key_for(addresses: &[Option<AddressKey>], plan: &MemoryPlan) -> AddressKey {
    addresses
        .get(plan.address.index())
        .copied()
        .flatten()
        .unwrap_or(AddressKey::Value(plan.address))
}

fn constant_i32(data: &MirData, value: ValueId) -> Option<u32> {
    let plan = data.values.get(value.index())?.as_ref()?;
    match plan.steps.as_slice() {
        [Step::I32(n)] => Some(*n as u32),
        _ => None,
    }
}

fn native_offsets_disjoint(a: u32, a_bytes: u8, b: u32, b_bytes: u8) -> bool {
    // Native page translations preserve the low 12 address bits. Distinct
    // virtual pages may alias the same physical page, so full linear-address
    // inequality is insufficient; byte offsets within a physical page must be
    // disjoint even under the worst-case page alias.
    (0..a_bytes).all(|i| {
        (0..b_bytes).all(|j| {
            (a.wrapping_add(i as u32) & 4095) != (b.wrapping_add(j as u32) & 4095)
        })
    })
}

fn alias(
    data: &MirData,
    a: AddressKey,
    a_bytes: u8,
    b: AddressKey,
    b_bytes: u8,
) -> AliasProof {
    if a == b && a_bytes == b_bytes {
        return AliasProof::Exact;
    }
    let constants = match (a, b) {
        (
            AddressKey::Segment {
                base: abase,
                null_byte: anull,
                offset: ao,
            },
            AddressKey::Segment {
                base: bbase,
                null_byte: bnull,
                offset: bo,
            },
        ) if abase == bbase && anull == bnull => {
            constant_i32(data, ao).zip(constant_i32(data, bo))
        },
        (AddressKey::Value(a), AddressKey::Value(b)) => {
            constant_i32(data, a).zip(constant_i32(data, b))
        },
        _ => None,
    };
    match constants {
        Some((a, b)) if native_offsets_disjoint(a, a_bytes, b, b_bytes) => {
            AliasProof::Disjoint
        },
        _ => AliasProof::MayAlias,
    }
}

fn validate_arenas(data: &MirData) -> Result<(), CompileError> {
    let n = data.memory.len();
    if data.control.blocks.len() > 64
        || n > 8192
        || data.value_types.len() > 16384
        || data.value_blocks.len() != data.value_types.len()
    {
        return Err(CompileError::Budget("MIR RAM optimization region"));
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
            "inconsistent RAM optimization arenas".into(),
        ));
    }
    Ok(())
}

/// Derive the intra-block certificate. A proven-disjoint committed native store
/// may remain between a source load and a reuse. Its slow path returns from the
/// entry, so only the callback-free native continuation can reach the reuse.
fn plan_with_loops(
    data: &MirData,
    loops: &LoopPlan,
    work_limit: usize,
) -> Result<Vec<Option<Forwarding>>, CompileError> {
    validate_arenas(data)?;
    let n = data.memory.len();
    if loops.instructions.len() != n || loops.resets.len() != data.control.blocks.len() {
        return Err(CompileError::InvalidIr(
            "invalid loop RAM certificate dimensions".into(),
        ));
    }
    let mut left = work_limit;
    spend(&mut left, n + data.value_types.len())?;
    let mut result = vec![None; n];
    let addresses = address_keys(data, &mut left)?;
    for block in &data.control.blocks {
        let mut previous: Option<(AddressKey, u8, InstId)> = None;
        for &id in &block.instructions {
            spend(&mut left, 1)?;
            let index = id.index();
            if loops.instructions[index].is_some() {
                previous = None;
                continue;
            }
            let Some(memory) = data.memory.get(index) else {
                return Err(CompileError::InvalidIr(
                    "forwarding instruction outside arena".into(),
                ));
            };
            if let Some(poll) = &data.control.polls[index] {
                if poll.cost == 1 {
                    continue;
                }
                previous = None;
                continue;
            }
            if let Some(memory) = memory {
                let key = key_for(&addresses, memory);
                #[cfg(test)]
                if previous.is_some() {
                    eprintln!(
                        "IR11TRACE block={} inst={} memory load={} store={} key={:?} prev={:?}",
                        block.instructions.first().map(|id| id.index()).unwrap_or(usize::MAX),
                        index,
                        eligible_load(memory),
                        eligible_store(memory),
                        key,
                        previous
                    );
                }
                if eligible_load(memory) {
                    if let Some((old_key, bytes, old)) = previous {
                        if alias(data, old_key, bytes, key, memory.guard.bytes) == AliasProof::Exact {
                            if result[old.index()].is_none() {
                                result[old.index()] = Some(Forwarding::Begin);
                            }
                            result[index] = Some(Forwarding::Reuse { previous: old });
                        }
                    }
                    // A load slow path can invoke MMIO/page-walk callbacks, so it
                    // becomes the only valid source for following reuse.
                    previous = Some((key, memory.guard.bytes, id));
                    continue;
                }
                if eligible_store(memory) {
                    if let Some((old_key, old_bytes, _)) = previous {
                        if alias(data, old_key, old_bytes, key, memory.guard.bytes)
                            == AliasProof::Disjoint
                        {
                            // Keep the old cache. A slow store cannot continue;
                            // the continuing native store is proven non-aliasing.
                            continue;
                        }
                    }
                    // Exact store->load reuse remains supported. May-alias stores
                    // kill the old source but may seed their own exact chain.
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
                    if value.steps.iter().all(|step| {
                        !matches!(
                            step,
                            Step::Read {
                                cpu: Reading::Call { .. },
                                ..
                            }
                        )
                    }) {
                        continue;
                    }
                }
            }
            #[cfg(test)]
            if previous.is_some() {
                eprintln!(
                    "IR11TRACE kill inst={} effect={} call={} poll={} value={:?}",
                    index,
                    data.effects[index].is_some(),
                    data.calls[index].is_some(),
                    data.control.polls[index].is_some(),
                    data.values[index]
                );
            }
            previous = None;
        }
    }
    Ok(result)
}

fn plan(data: &MirData, work_limit: usize) -> Result<Vec<Option<Forwarding>>, CompileError> {
    plan_with_loops(data, &data.ram_loop_cache, work_limit)
}

fn value_invariant(
    data: &MirData,
    value: ValueId,
    members: u64,
    visiting: &mut [bool],
    left: &mut usize,
) -> Result<bool, CompileError> {
    spend(left, 1)?;
    let index = value.index();
    let block = data
        .value_blocks
        .get(index)
        .copied()
        .flatten()
        .ok_or_else(|| CompileError::InvalidIr("missing MIR value provenance".into()))?;
    if members & (1u64 << block.index()) == 0 {
        return Ok(true);
    }
    if constant_i32(data, value).is_some() {
        return Ok(true);
    }
    if *visiting
        .get(index)
        .ok_or_else(|| CompileError::InvalidIr("value provenance outside arena".into()))?
    {
        return Ok(false);
    }
    visiting[index] = true;
    let invariant = match data.values.get(index).and_then(Option::as_ref) {
        Some(plan) => {
            let mut ok = true;
            for step in &plan.steps {
                spend(left, 1)?;
                ok &= match step {
                    Step::Value(value) => value_invariant(data, *value, members, visiting, left)?,
                    Step::Packed {
                        destination,
                        source,
                        ..
                    } => {
                        value_invariant(data, *destination, members, visiting, left)?
                            && value_invariant(data, *source, members, visiting, left)?
                    },
                    Step::Read { .. } => false,
                    Step::I32(_)
                    | Step::I64(_)
                    | Step::Scalar(_)
                    | Step::Simd(_)
                    | Step::Lane { .. }
                    | Step::Shuffle(_) => true,
                };
                if !ok {
                    break;
                }
            }
            ok
        },
        None => false,
    };
    visiting[index] = false;
    Ok(invariant)
}

fn invariant_address(
    data: &MirData,
    key: AddressKey,
    members: u64,
    left: &mut usize,
) -> Result<bool, CompileError> {
    let value = match key {
        AddressKey::Value(value) => value,
        AddressKey::Segment { offset, .. } => offset,
    };
    value_invariant(
        data,
        value,
        members,
        &mut vec![false; data.value_types.len()],
        left,
    )
}

fn loop_plan(data: &MirData, work_limit: usize) -> Result<LoopPlan, CompileError> {
    validate_arenas(data)?;
    let nblocks = data.control.blocks.len();
    let ninst = data.memory.len();
    let mut result = LoopPlan::disabled(ninst, nblocks);
    if nblocks == 0 {
        return Ok(result);
    }
    let mut left = work_limit;
    spend(&mut left, ninst + data.value_types.len() + nblocks)?;
    let addresses = address_keys(data, &mut left)?;

    let mut predecessors = vec![Vec::<usize>::new(); nblocks];
    for (block, cfg) in data.control.blocks.iter().enumerate() {
        spend(&mut left, 1)?;
        for edge in cfg.terminator.edges() {
            let target = edge.target.index();
            if target >= nblocks {
                return Err(CompileError::InvalidIr(
                    "loop edge outside MIR CFG".into(),
                ));
            }
            if !predecessors[target].contains(&block) {
                predecessors[target].push(block);
            }
        }
    }

    let all = if nblocks == 64 {
        u64::MAX
    } else {
        (1u64 << nblocks) - 1
    };
    let mut dominates = vec![all; nblocks];
    for block in 0..nblocks {
        if data.control.entries.contains(&BlockId(block as u32)) || predecessors[block].is_empty() {
            dominates[block] = 1u64 << block;
        }
    }
    loop {
        let mut changed = false;
        for block in 0..nblocks {
            spend(&mut left, 1)?;
            if data.control.entries.contains(&BlockId(block as u32))
                || predecessors[block].is_empty()
            {
                continue;
            }
            let mut next = all;
            for &pred in &predecessors[block] {
                spend(&mut left, 1)?;
                next &= dominates[pred];
            }
            next |= 1u64 << block;
            if next != dominates[block] {
                dominates[block] = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut loops = Vec::<(u64, usize, usize)>::new();
    for header in 0..nblocks {
        spend(&mut left, 1)?;
        if data.control.entries.contains(&BlockId(header as u32)) {
            continue;
        }
        let latches: Vec<_> = predecessors[header]
            .iter()
            .copied()
            .filter(|&pred| dominates[pred] & (1u64 << header) != 0)
            .collect();
        if latches.is_empty() {
            continue;
        }
        let mut members = 1u64 << header;
        let mut stack = latches;
        while let Some(block) = stack.pop() {
            spend(&mut left, 1)?;
            if members & (1u64 << block) != 0 {
                continue;
            }
            members |= 1u64 << block;
            for &pred in &predecessors[block] {
                if members & (1u64 << pred) == 0 {
                    stack.push(pred);
                }
            }
        }
        let valid = (0..nblocks)
            .filter(|&b| members & (1u64 << b) != 0)
            .all(|b| dominates[b] & (1u64 << header) != 0);
        if !valid {
            continue;
        }
        let outside: Vec<_> = predecessors[header]
            .iter()
            .copied()
            .filter(|&p| members & (1u64 << p) == 0)
            .collect();
        #[cfg(test)]
        eprintln!(
            "IR11LOOP header={} members={:#x} entries={:?} preds={:?} outside={:?} valid={}",
            header,
            members,
            data.control.entries,
            predecessors[header],
            outside,
            valid
        );
        if outside.len() != 1 {
            continue;
        }
        let preheader = outside[0];
        if !matches!(
            &data.control.blocks[preheader].terminator,
            super::control::Terminator::Jump(edge) if edge.target.index() == header
        ) {
            continue;
        }
        loops.push((members, preheader, header));
    }
    // Inner loops receive certificates first. An instruction is owned by at most
    // one loop cache, avoiding ambiguous reset scopes for nested loops.
    loops.sort_by_key(|(members, _, header)| (members.count_ones(), *header));

    for (members, preheader, _) in loops {
        let mut safe = true;
        let mut candidates = Vec::<(InstId, AddressKey, u8)>::new();
        for block in 0..nblocks {
            if members & (1u64 << block) == 0 {
                continue;
            }
            for &id in &data.control.blocks[block].instructions {
                spend(&mut left, 1)?;
                let index = id.index();
                if data.calls[index].is_some() {
                    safe = false;
                    break;
                }
                if let Some(effect) = &data.effects[index] {
                    if segment(effect).is_none() {
                        safe = false;
                        break;
                    }
                }
                if let Some(poll) = &data.control.polls[index] {
                    if poll.cost != 1 {
                        safe = false;
                        break;
                    }
                }
                if let Some(memory) = &data.memory[index] {
                    if !eligible_load(memory) {
                        // Stores/RMW/vector memory could change RAM or mappings
                        // between iterations. This conservative loop cache handles
                        // read-only ordinary-RAM loops only.
                        safe = false;
                        break;
                    }
                    let key = key_for(&addresses, memory);
                    if invariant_address(data, key, members, &mut left)? {
                        candidates.push((id, key, memory.guard.bytes));
                    }
                }
            }
            if !safe {
                break;
            }
        }
        if !safe || candidates.is_empty() {
            continue;
        }

        let mut groups = Vec::<(AddressKey, u8, usize)>::new();
        for (id, key, bytes) in candidates {
            if result.instructions[id.index()].is_some() {
                continue;
            }
            let slot = groups
                .iter()
                .find_map(|(old, old_bytes, slot)| {
                    (alias(data, *old, *old_bytes, key, bytes) == AliasProof::Exact)
                        .then_some(*slot)
                })
                .unwrap_or_else(|| {
                    let slot = result.slots;
                    result.slots += 1;
                    groups.push((key, bytes, slot));
                    result.resets[preheader].push(slot);
                    slot
                });
            result.instructions[id.index()] = Some(LoopForwarding { slot });
        }
    }
    for resets in &mut result.resets {
        resets.sort_unstable();
        resets.dedup();
    }
    Ok(result)
}

pub(super) fn optimize(data: &mut MirData, work_limit: usize) -> Result<usize, CompileError> {
    let next = plan(data, work_limit)?;
    let count = next
        .iter()
        .filter(|p| matches!(p, Some(Forwarding::Reuse { .. })))
        .count();
    data.ram_forwarding = next;
    Ok(count)
}

pub(super) fn optimize_loops(
    data: &mut MirData,
    work_limit: usize,
) -> Result<usize, CompileError> {
    let next_loop = loop_plan(data, work_limit)?;
    // Keep the update transactional: derive the compatible intra-block
    // certificate before publishing either plan.
    let next_forward = plan_with_loops(data, &next_loop, DEFAULT_WORK_LIMIT)?;
    let count = next_loop.instructions.iter().filter(|p| p.is_some()).count();
    data.ram_loop_cache = next_loop;
    data.ram_forwarding = next_forward;
    Ok(count)
}

pub(super) fn verify(data: &MirData) -> Result<(), CompileError> {
    if data.ram_forwarding.len() != data.memory.len()
        || data.ram_loop_cache.instructions.len() != data.memory.len()
        || data.ram_loop_cache.resets.len() != data.control.blocks.len()
    {
        return Err(CompileError::InvalidIr(
            "invalid RAM optimization certificate length".into(),
        ));
    }
    let loop_enabled = data.ram_loop_cache.slots != 0
        || data.ram_loop_cache.instructions.iter().any(Option::is_some)
        || data.ram_loop_cache.resets.iter().any(|r| !r.is_empty());
    if loop_enabled && data.ram_loop_cache != loop_plan(data, DEFAULT_WORK_LIMIT)? {
        return Err(CompileError::InvalidIr(
            "invalid loop RAM cache certificate".into(),
        ));
    }
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
