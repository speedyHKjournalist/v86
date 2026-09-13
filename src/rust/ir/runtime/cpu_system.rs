//! Explicit terminal adapters for privilege and execution-context changes.
use crate::cpu::{cpu, global_pointers as gp, instructions, instructions_0f};
use crate::ir::helper::Outcome;

unsafe fn commit() -> u32 {
    *gp::instruction_counter = (*gp::instruction_counter).wrapping_add(1);
    Outcome::Invalidated as u32
}
unsafe fn gp_fault() -> u32 {
    cpu::trigger_gp(0);
    Outcome::ControlTransferred as u32
}
#[no_mangle]
pub unsafe fn ir_cli() -> u32 {
    assert!(!cpu::in_jit);
    // Reuse the CPU's explicit success predicate (including its VME/PVI policy).
    if !instructions::instr_FA_without_fault() {
        return gp_fault();
    }
    commit()
}
#[no_mangle]
pub unsafe fn ir_clts() -> u32 {
    assert!(!cpu::in_jit);
    if *gp::cpl != 0 {
        return gp_fault();
    }
    instructions_0f::instr_0F06();
    commit()
}
#[no_mangle]
pub unsafe fn ir_wbinvd() -> u32 {
    assert!(!cpu::in_jit);
    if *gp::cpl != 0 {
        return gp_fault();
    }
    instructions_0f::instr_0F09();
    commit()
}
#[no_mangle]
pub unsafe fn ir_sysenter() -> u32 {
    assert!(!cpu::in_jit);
    // The audited body has no guest-fault path after this guard. It owns CS/SS,
    // mode/CPL, flags, ESP/EIP and fetch-context invalidation on success.
    if !*gp::protected_mode || *gp::sysenter_cs & 0xFFFC == 0 {
        return gp_fault();
    }
    instructions_0f::instr_0F34();
    commit()
}
#[no_mangle]
pub unsafe fn ir_sysexit() -> u32 {
    assert!(!cpu::in_jit);
    if !*gp::protected_mode || *gp::cpl != 0 || *gp::sysenter_cs & 0xFFFC == 0 {
        return gp_fault();
    }
    instructions_0f::instr_0F35();
    commit()
}

#[no_mangle]
pub unsafe fn ir_hlt() -> u32 {
    assert!(!cpu::in_jit);
    if *gp::cpl != 0 {
        return gp_fault();
    }
    // The CPU owns in_hlt, timer observation, halt notification and immediate IRQ
    // delivery. Commit after those observations, matching the semantic body.
    instructions::instr_F4();
    commit()
}
