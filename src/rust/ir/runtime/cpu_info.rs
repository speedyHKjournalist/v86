//! Audited cold CPU helpers; the pinned CPU owns identification/MSR/TSC policy.
use crate::cpu::{cpu, global_pointers as gp, instructions_0f};
use crate::ir::helper::Outcome;
unsafe fn commit() -> u32 {
    *gp::instruction_counter = (*gp::instruction_counter).wrapping_add(1);
    Outcome::Invalidated as u32
}
unsafe fn ring0() -> bool {
    assert!(!cpu::in_jit);
    if *gp::cpl != 0 {
        cpu::trigger_gp(0);
        false
    } else {
        true
    }
}
#[no_mangle]
pub unsafe fn ir_cpuid() -> u32 {
    assert!(!cpu::in_jit);
    instructions_0f::instr_0FA2();
    commit()
}
#[no_mangle]
pub unsafe fn ir_rdtsc() -> u32 {
    assert!(!cpu::in_jit);
    if *gp::cpl != 0 && *gp::cr.offset(4) & cpu::CR4_TSD != 0 {
        cpu::trigger_gp(0);
        return Outcome::ControlTransferred as u32;
    }
    instructions_0f::instr_0F31();
    commit()
}
#[no_mangle]
pub unsafe fn ir_rdmsr() -> u32 {
    if !ring0() {
        return Outcome::ControlTransferred as u32;
    }
    // The audited body has no delivered faults after its CPL check. Unknown
    // indices retain its debug abort / release zero-result policy.
    instructions_0f::instr_0F32();
    commit()
}
#[no_mangle]
pub unsafe fn ir_wrmsr() -> u32 {
    if !ring0() {
        return Outcome::ControlTransferred as u32;
    }
    // APIC restrictions and unknown-index debug assertions remain CPU-owned.
    instructions_0f::instr_0F30();
    commit()
}
#[cfg(feature = "ir-test-hooks")]
#[no_mangle]
pub unsafe fn ir_test_tsc_reset(offset: u64) {
    cpu::tsc_offset = offset;
    cpu::tsc_last_value = 0;
    cpu::tsc_resolution = u64::MAX;
    cpu::tsc_number_of_same_readings = 0;
    cpu::tsc_speed = 1;
    #[cfg(debug_assertions)]
    {
        cpu::tsc_last_extra = 0;
    }
}
#[cfg(feature = "ir-test-hooks")]
#[no_mangle]
pub unsafe fn ir_test_tsc_state(field: u32) -> u64 {
    match field {
        0 => cpu::tsc_offset,
        1 => cpu::tsc_last_value,
        2 => cpu::tsc_resolution,
        3 => cpu::tsc_number_of_same_readings,
        4 => cpu::tsc_speed,
        5 => {
            #[cfg(debug_assertions)]
            {
                return cpu::tsc_last_extra;
            }
            #[cfg(not(debug_assertions))]
            {
                0
            }
        },
        _ => unreachable!(),
    }
}
