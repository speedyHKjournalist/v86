//! Audited terminal x87 register helper.
//!
//! Register-only D8-DF forms have no guest-memory access. The helper first
//! applies the architectural CR0.EM/TS guard, then synchronizes/discards the
//! legacy f64 x87 cache so the canonical F80 CPU state is authoritative. It
//! dispatches the same instruction bodies as the interpreter. Invalid nested
//! encodings are rejected before calling those bodies so a delivered #UD can
//! never be followed by an instruction commit.
use crate::{
    cpu::{cpu, fpu, global_pointers as gp, instructions},
    ir::helper::Outcome,
};

unsafe fn commit() -> u32 {
    *gp::instruction_counter = (*gp::instruction_counter).wrapping_add(1);
    Outcome::Invalidated as u32
}

unsafe fn ud() -> u32 {
    cpu::trigger_ud();
    Outcome::ControlTransferred as u32
}

fn valid(opcode: u32, group: u32, r: u32) -> bool {
    match opcode {
        0xD8 | 0xDC => true,
        0xD9 => match group {
            0 | 1 | 3 | 6 | 7 => true,
            2 => r == 0,
            4 => matches!(r, 0 | 1 | 4 | 5),
            5 => r <= 6,
            _ => false,
        },
        0xDA => match group {
            0..=3 => true,
            5 => r == 1,
            _ => false,
        },
        0xDB => match group {
            0..=3 | 5 | 6 => true,
            4 => r <= 4,
            _ => false,
        },
        0xDD => group <= 5,
        0xDE => group != 3 || r == 1,
        0xDF => match group {
            0..=3 | 5 | 6 => true,
            4 => r == 0,
            _ => false,
        },
        _ => false,
    }
}

#[no_mangle]
pub unsafe fn ir_x87_reg(opcode: u32, group: u32, r: u32, operand_size: u32) -> u32 {
    assert!(!cpu::in_jit);
    assert!((0xD8..=0xDF).contains(&opcode));
    assert!(group < 8 && r < 8 && matches!(operand_size, 16 | 32));

    // x87 (unlike SSE/MMX) raises #NM for either CR0.EM or CR0.TS.
    if !cpu::task_switch_test() {
        return Outcome::ControlTransferred as u32;
    }

    // The legacy JIT may have left exact-enough f64 shadow values live. IR x87
    // always executes against canonical F80 state; synchronize only after the
    // task-switch guard so a faulting instruction does not observe/mutate FPU
    // state before #NM.
    fpu::fpu_cache_barrier();

    if !valid(opcode, group, r) {
        return ud();
    }

    let r = r as i32;
    match (opcode, group) {
        (0xD8, 0) => instructions::instr_D8_0_reg(r),
        (0xD8, 1) => instructions::instr_D8_1_reg(r),
        (0xD8, 2) => instructions::instr_D8_2_reg(r),
        (0xD8, 3) => instructions::instr_D8_3_reg(r),
        (0xD8, 4) => instructions::instr_D8_4_reg(r),
        (0xD8, 5) => instructions::instr_D8_5_reg(r),
        (0xD8, 6) => instructions::instr_D8_6_reg(r),
        (0xD8, 7) => instructions::instr_D8_7_reg(r),

        (0xD9, 0) => instructions::instr16_D9_0_reg(r),
        (0xD9, 1) => instructions::instr16_D9_1_reg(r),
        (0xD9, 2) => instructions::instr16_D9_2_reg(r),
        (0xD9, 3) => instructions::instr16_D9_3_reg(r),
        (0xD9, 4) => instructions::instr16_D9_4_reg(r),
        (0xD9, 5) => instructions::instr16_D9_5_reg(r),
        (0xD9, 6) => instructions::instr16_D9_6_reg(r),
        (0xD9, 7) => instructions::instr16_D9_7_reg(r),

        (0xDA, 0) => instructions::instr_DA_0_reg(r),
        (0xDA, 1) => instructions::instr_DA_1_reg(r),
        (0xDA, 2) => instructions::instr_DA_2_reg(r),
        (0xDA, 3) => instructions::instr_DA_3_reg(r),
        (0xDA, 5) => instructions::instr_DA_5_reg(r),

        (0xDB, 0) => instructions::instr_DB_0_reg(r),
        (0xDB, 1) => instructions::instr_DB_1_reg(r),
        (0xDB, 2) => instructions::instr_DB_2_reg(r),
        (0xDB, 3) => instructions::instr_DB_3_reg(r),
        (0xDB, 4) => instructions::instr_DB_4_reg(r),
        (0xDB, 5) => instructions::instr_DB_5_reg(r),
        (0xDB, 6) => instructions::instr_DB_6_reg(r),

        (0xDC, 0) => instructions::instr_DC_0_reg(r),
        (0xDC, 1) => instructions::instr_DC_1_reg(r),
        (0xDC, 2) => instructions::instr_DC_2_reg(r),
        (0xDC, 3) => instructions::instr_DC_3_reg(r),
        (0xDC, 4) => instructions::instr_DC_4_reg(r),
        (0xDC, 5) => instructions::instr_DC_5_reg(r),
        (0xDC, 6) => instructions::instr_DC_6_reg(r),
        (0xDC, 7) => instructions::instr_DC_7_reg(r),

        (0xDD, 0) => instructions::instr16_DD_0_reg(r),
        (0xDD, 1) => instructions::instr16_DD_1_reg(r),
        (0xDD, 2) => instructions::instr16_DD_2_reg(r),
        (0xDD, 3) => instructions::instr16_DD_3_reg(r),
        (0xDD, 4) => instructions::instr16_DD_4_reg(r),
        (0xDD, 5) => instructions::instr16_DD_5_reg(r),

        (0xDE, 0) => instructions::instr_DE_0_reg(r),
        (0xDE, 1) => instructions::instr_DE_1_reg(r),
        (0xDE, 2) => instructions::instr_DE_2_reg(r),
        (0xDE, 3) => instructions::instr_DE_3_reg(r),
        (0xDE, 4) => instructions::instr_DE_4_reg(r),
        (0xDE, 5) => instructions::instr_DE_5_reg(r),
        (0xDE, 6) => instructions::instr_DE_6_reg(r),
        (0xDE, 7) => instructions::instr_DE_7_reg(r),

        (0xDF, 0) => instructions::instr_DF_0_reg(r),
        (0xDF, 1) => instructions::instr_DF_1_reg(r),
        (0xDF, 2) => instructions::instr_DF_2_reg(r),
        (0xDF, 3) => instructions::instr_DF_3_reg(r),
        (0xDF, 4) => instructions::instr_DF_4_reg(r),
        (0xDF, 5) => instructions::instr_DF_5_reg(r),
        (0xDF, 6) => instructions::instr_DF_6_reg(r),
        _ => unreachable!("validated x87 register dispatch"),
    }

    commit()
}

#[cfg(feature = "ir-test-hooks")]
#[no_mangle]
pub unsafe fn ir_test_x87_seed() {
    use crate::softfloat::F80;

    fpu::fpu_discard_cache();
    F80::clear_exception_flags();
    fpu::fpu_finit();
    *gp::fpu_stack_ptr = 0;
    *gp::fpu_stack_empty = 0;
    *gp::fpu_status_word = 0;
    for index in 0..8 {
        fpu::fpu_write_st(index, F80::of_i32(index + 1));
    }
}
