//! Test-only Wasm exports. Never compiled into a normal production artifact.
use crate::{
    analysis::{analyze_decoded, analyze_step, analyze_step_legacy},
    cpu::memory,
    cpu_context::CpuContext,
    decode::{decode, GuestEip, LinearAddress},
    state_flags::CachedStateFlags,
};

/// Return a bitmask of disagreements; 0 means exact agreement.
/// The test harness supplies complete, <=15-byte instructions well inside a RAM page.
#[no_mangle]
pub fn ir_test_compare_analysis(physical: u32, length: u32, mode32: u32) -> u32 {
    if length == 0
        || length > 15
        || physical & 4095 > 4096 - 32
        || memory::in_mapped_range(physical)
        || memory::in_mapped_range(physical + 31)
    {
        return 0x40000000;
    }
    let mut bytes = [0; 15];
    for i in 0..length {
        bytes[i as usize] = memory::read8_no_mmap_check(physical + i) as u8;
    }
    let decoded = match decode(
        &bytes[..length as usize],
        GuestEip(0x2000),
        LinearAddress(0x12000),
        mode32 != 0,
    ) {
        Ok(decoded) => decoded,
        Err(_) => return 0x80000000,
    };
    let mut legacy = CpuContext {
        eip: physical,
        prefixes: 0,
        cs_offset: 0x10000,
        state_flags: CachedStateFlags::of_u32((mode32 != 0) as u32),
    };
    let old = analyze_step_legacy(&mut legacy);
    let new = analyze_decoded(&decoded);
    let mut shared = legacy.clone();
    shared.eip = physical;
    let actual = analyze_step(&mut shared, 0x12000);
    let integration_difference =
        actual != old || shared.eip != legacy.eip || shared.prefixes != legacy.prefixes;
    let prefixes = decoded.prefixes;
    let packed_prefixes = prefixes.segment.map_or(0, |s| s + 1)
        | if prefixes.operand { crate::prefix::PREFIX_66 } else { 0 }
        | if prefixes.address { crate::prefix::PREFIX_67 } else { 0 }
        | if prefixes.repne { crate::prefix::PREFIX_F2 } else { 0 }
        | if prefixes.rep { crate::prefix::PREFIX_F3 } else { 0 };
    let ea_difference = if let Some(ea) = &decoded.ea {
        let mut ea_context = legacy.clone();
        ea_context.eip = physical + decoded.modrm_offset.unwrap() as u32 + 1;
        let old_ea = crate::modrm::decode(&mut ea_context, decoded.modrm.unwrap());
        !old_ea.matches_shared(ea, decoded.prefixes.segment)
    } else {
        false
    };
    ((legacy.eip - physical != decoded.length as u32) as u32)
        | ((legacy.prefixes != packed_prefixes) as u32) << 1
        | ((old != new) as u32) << 2
        | (ea_difference as u32) << 3
        | (integration_difference as u32) << 4
}

#[no_mangle]
pub fn ir_test_snapshot_length(physical: u32) -> u32 {
    let context = CpuContext {
        eip: physical,
        prefixes: 0,
        cs_offset: 0,
        state_flags: CachedStateFlags::EMPTY,
    };
    context
        .instruction_snapshot()
        .map_or(0, |s| s.length as u32)
}

#[no_mangle]
pub fn ir_test_snapshot_decode(physical: u32) -> u32 {
    let context = CpuContext {
        eip: physical,
        prefixes: 0,
        cs_offset: 0,
        state_flags: CachedStateFlags::of_u32(1),
    };
    let Some(snapshot) = context.instruction_snapshot() else {
        return 0;
    };
    match decode(
        &snapshot.bytes[..snapshot.length],
        GuestEip(0),
        LinearAddress(0),
        true,
    ) {
        Ok(_) => 1,
        Err(crate::decode::DecodeStop::Incomplete { .. }) => 2,
        Err(_) => 3,
    }
}
