//! Explicit semantic calls: guard and ordered load precede any destination update.
//! Retains baseline floating-point/NaN/rounding behavior, including its limitations.
use crate::cpu::{cpu, fpu, global_pointers as gp, instructions_0f as sem};
use crate::ir::helper::Outcome;

#[derive(PartialEq, Eq)]
struct ContinuationContext {
    epoch: u64,
    pc: i32,
    previous_pc: i32,
    count: u32,
    controls: [i32; 5],
    mode: [u32; 8],
    segments: [(u16, i32, u32, u8, bool); 8],
    descriptors: [i32; 5],
}
impl ContinuationContext {
    unsafe fn capture() -> Self {
        Self {
            epoch: super::live::continuation_epoch(),
            pc: *gp::instruction_pointer,
            previous_pc: *gp::previous_ip,
            count: *gp::instruction_counter,
            controls: std::array::from_fn(|i| *gp::cr.add(i)),
            mode: [
                *gp::protected_mode as u32,
                *gp::is_32 as u32,
                *gp::stack_size_32 as u32,
                *gp::cpl as u32,
                *gp::in_hlt as u32,
                *gp::prefixes as u32,
                (*gp::state_flags).to_u32(),
                (*gp::flags as u32) & !0x8D5, // all non-arithmetic FLAGS affect continuation policy
            ],
            descriptors: [
                *gp::gdtr_offset,
                *gp::gdtr_size,
                *gp::idtr_offset,
                *gp::idtr_size,
                *gp::tss_size_32 as i32,
            ],
            segments: std::array::from_fn(|i| {
                (
                    *gp::sreg.add(i),
                    *gp::segment_offsets.add(i),
                    *gp::segment_limits.add(i),
                    *gp::segment_access_bytes.add(i),
                    *gp::segment_is_null.add(i),
                )
            }),
        }
    }
}
unsafe fn finish(success: bool) -> u32 {
    if success {
        Outcome::Normal as u32
    } else {
        Outcome::ControlTransferred as u32
    }
}
#[no_mangle]
pub unsafe fn ir_sse_fp_reg_continue(
    op: u32,
    source: i32,
    destination: i32,
    immediate: i32,
) -> u32 {
    assert!(!cpu::in_jit && (0..8).contains(&source) && (0..8).contains(&destination));
    if !cpu::task_switch_test_mmx() {
        return finish(false);
    }
    fpu::fpu_cache_barrier();
    match op {
        0x0F2A => {
            sem::instr_0F2A(cpu::read_mmx64s(source), destination);
        },
        0x0F2C => {
            sem::instr_0F2C(cpu::read_xmm64s(source), destination);
        },
        0x0F2D => {
            sem::instr_0F2D(cpu::read_xmm64s(source), destination);
        },
        0x0F2E => {
            sem::instr_0F2E(cpu::read_xmm_f32(source), destination);
        },
        0x0F2F => {
            sem::instr_0F2F(cpu::read_xmm_f32(source), destination);
        },
        0x0F51 => {
            sem::instr_0F51(cpu::read_xmm128s(source), destination);
        },
        0x0F52 => {
            sem::instr_0F52(cpu::read_xmm128s(source), destination);
        },
        0x0F53 => {
            sem::instr_0F53(cpu::read_xmm128s(source), destination);
        },
        0x0F58 => {
            sem::instr_0F58(cpu::read_xmm128s(source), destination);
        },
        0x0F59 => {
            sem::instr_0F59(cpu::read_xmm128s(source), destination);
        },
        0x0F5A => {
            sem::instr_0F5A(cpu::read_xmm64s(source), destination);
        },
        0x0F5B => {
            sem::instr_0F5B(cpu::read_xmm128s(source), destination);
        },
        0x0F5C => {
            sem::instr_0F5C(cpu::read_xmm128s(source), destination);
        },
        0x0F5D => {
            sem::instr_0F5D(cpu::read_xmm128s(source), destination);
        },
        0x0F5E => {
            sem::instr_0F5E(cpu::read_xmm128s(source), destination);
        },
        0x0F5F => {
            sem::instr_0F5F(cpu::read_xmm128s(source), destination);
        },
        0x0FC2 => {
            sem::instr_0FC2(cpu::read_xmm128s(source), destination, immediate);
        },
        0x660F2A => {
            sem::instr_660F2A(cpu::read_mmx64s(source), destination);
            cpu::transition_fpu_to_mmx();
        },
        0x660F2C => {
            sem::instr_660F2C(cpu::read_xmm128s(source), destination);
        },
        0x660F2D => {
            sem::instr_660F2D(cpu::read_xmm128s(source), destination);
        },
        0x660F2E => {
            sem::instr_660F2E(cpu::read_xmm64s(source), destination);
        },
        0x660F2F => {
            sem::instr_660F2F(cpu::read_xmm64s(source), destination);
        },
        0x660F51 => {
            sem::instr_660F51(cpu::read_xmm128s(source), destination);
        },
        0x660F58 => {
            sem::instr_660F58(cpu::read_xmm128s(source), destination);
        },
        0x660F59 => {
            sem::instr_660F59(cpu::read_xmm128s(source), destination);
        },
        0x660F5A => {
            sem::instr_660F5A(cpu::read_xmm128s(source), destination);
        },
        0x660F5B => {
            sem::instr_660F5B(cpu::read_xmm128s(source), destination);
        },
        0x660F5C => {
            sem::instr_660F5C(cpu::read_xmm128s(source), destination);
        },
        0x660F5D => {
            sem::instr_660F5D(cpu::read_xmm128s(source), destination);
        },
        0x660F5E => {
            sem::instr_660F5E(cpu::read_xmm128s(source), destination);
        },
        0x660F5F => {
            sem::instr_660F5F(cpu::read_xmm128s(source), destination);
        },
        0x660F7C => {
            sem::instr_660F7C(cpu::read_xmm128s(source), destination);
        },
        0x660F7D => {
            sem::instr_660F7D(cpu::read_xmm128s(source), destination);
        },
        0x660FC2 => {
            sem::instr_660FC2(cpu::read_xmm128s(source), destination, immediate);
        },
        0x660FD0 => {
            sem::instr_660FD0(cpu::read_xmm128s(source), destination);
        },
        0x660FE6 => {
            sem::instr_660FE6(cpu::read_xmm128s(source), destination);
        },
        0xF20F2A => {
            sem::instr_F20F2A(cpu::read_reg32(source), destination);
        },
        0xF20F2C => {
            sem::instr_F20F2C(cpu::read_xmm64s(source), destination);
        },
        0xF20F2D => {
            sem::instr_F20F2D(cpu::read_xmm64s(source), destination);
        },
        0xF20F51 => {
            sem::instr_F20F51(cpu::read_xmm64s(source), destination);
        },
        0xF20F58 => {
            sem::instr_F20F58(cpu::read_xmm64s(source), destination);
        },
        0xF20F59 => {
            sem::instr_F20F59(cpu::read_xmm64s(source), destination);
        },
        0xF20F5A => {
            sem::instr_F20F5A(cpu::read_xmm64s(source), destination);
        },
        0xF20F5C => {
            sem::instr_F20F5C(cpu::read_xmm64s(source), destination);
        },
        0xF20F5D => {
            sem::instr_F20F5D(cpu::read_xmm64s(source), destination);
        },
        0xF20F5E => {
            sem::instr_F20F5E(cpu::read_xmm64s(source), destination);
        },
        0xF20F5F => {
            sem::instr_F20F5F(cpu::read_xmm64s(source), destination);
        },
        0xF20F7C => {
            sem::instr_F20F7C(cpu::read_xmm128s(source), destination);
        },
        0xF20F7D => {
            sem::instr_F20F7D(cpu::read_xmm128s(source), destination);
        },
        0xF20FC2 => {
            sem::instr_F20FC2(cpu::read_xmm64s(source), destination, immediate);
        },
        0xF20FD0 => {
            sem::instr_F20FD0(cpu::read_xmm128s(source), destination);
        },
        0xF20FE6 => {
            sem::instr_F20FE6(cpu::read_xmm128s(source), destination);
        },
        0xF30F2A => {
            sem::instr_F30F2A(cpu::read_reg32(source), destination);
        },
        0xF30F2C => {
            sem::instr_F30F2C(cpu::read_xmm_f32(source), destination);
        },
        0xF30F2D => {
            sem::instr_F30F2D(cpu::read_xmm_f32(source), destination);
        },
        0xF30F51 => {
            sem::instr_F30F51(cpu::read_xmm_f32(source), destination);
        },
        0xF30F52 => {
            sem::instr_F30F52(cpu::read_xmm_f32(source), destination);
        },
        0xF30F53 => {
            sem::instr_F30F53(cpu::read_xmm_f32(source), destination);
        },
        0xF30F58 => {
            sem::instr_F30F58(cpu::read_xmm_f32(source), destination);
        },
        0xF30F59 => {
            sem::instr_F30F59(cpu::read_xmm_f32(source), destination);
        },
        0xF30F5A => {
            sem::instr_F30F5A(cpu::read_xmm_f32(source), destination);
        },
        0xF30F5B => {
            sem::instr_F30F5B(cpu::read_xmm128s(source), destination);
        },
        0xF30F5C => {
            sem::instr_F30F5C(cpu::read_xmm_f32(source), destination);
        },
        0xF30F5D => {
            sem::instr_F30F5D(cpu::read_xmm_f32(source), destination);
        },
        0xF30F5E => {
            sem::instr_F30F5E(cpu::read_xmm_f32(source), destination);
        },
        0xF30F5F => {
            sem::instr_F30F5F(cpu::read_xmm_f32(source), destination);
        },
        0xF30FC2 => {
            sem::instr_F30FC2(cpu::read_xmm64s(source) as i32, destination, immediate);
        },
        0xF30FE6 => {
            sem::instr_F30FE6(cpu::read_xmm64s(source), destination);
        },
        _ => unreachable!("unregistered SSE FP semantic operation"),
    }
    finish(true)
}
unsafe fn memory(
    op: u32,
    offset: u32,
    segment: u32,
    destination: i32,
    immediate: i32,
) -> Result<(), ()> {
    let addr = offset.wrapping_add(cpu::get_seg(segment as i32)? as u32) as i32;
    fpu::fpu_cache_barrier();
    match op {
        0x0F2A => {
            sem::instr_0F2A(cpu::safe_read64s(addr)?, destination);
        },
        0x0F2C => {
            sem::instr_0F2C(cpu::safe_read64s(addr)?, destination);
        },
        0x0F2D => {
            sem::instr_0F2D(cpu::safe_read64s(addr)?, destination);
        },
        0x0F2E => {
            sem::instr_0F2E(cpu::safe_read_f32(addr)?, destination);
        },
        0x0F2F => {
            sem::instr_0F2F(cpu::safe_read_f32(addr)?, destination);
        },
        0x0F51 => {
            sem::instr_0F51(cpu::safe_read128s(addr)?, destination);
        },
        0x0F52 => {
            sem::instr_0F52(cpu::safe_read128s(addr)?, destination);
        },
        0x0F53 => {
            sem::instr_0F53(cpu::safe_read128s(addr)?, destination);
        },
        0x0F58 => {
            sem::instr_0F58(cpu::safe_read128s(addr)?, destination);
        },
        0x0F59 => {
            sem::instr_0F59(cpu::safe_read128s(addr)?, destination);
        },
        0x0F5A => {
            sem::instr_0F5A(cpu::safe_read64s(addr)?, destination);
        },
        0x0F5B => {
            sem::instr_0F5B(cpu::safe_read128s(addr)?, destination);
        },
        0x0F5C => {
            sem::instr_0F5C(cpu::safe_read128s(addr)?, destination);
        },
        0x0F5D => {
            sem::instr_0F5D(cpu::safe_read128s(addr)?, destination);
        },
        0x0F5E => {
            sem::instr_0F5E(cpu::safe_read128s(addr)?, destination);
        },
        0x0F5F => {
            sem::instr_0F5F(cpu::safe_read128s(addr)?, destination);
        },
        0x0FC2 => {
            sem::instr_0FC2(cpu::safe_read128s(addr)?, destination, immediate);
        },
        0x660F2A => {
            sem::instr_660F2A(cpu::safe_read64s(addr)?, destination);
        },
        0x660F2C => {
            sem::instr_660F2C(cpu::safe_read128s(addr)?, destination);
        },
        0x660F2D => {
            sem::instr_660F2D(cpu::safe_read128s(addr)?, destination);
        },
        0x660F2E => {
            sem::instr_660F2E(cpu::safe_read64s(addr)?, destination);
        },
        0x660F2F => {
            sem::instr_660F2F(cpu::safe_read64s(addr)?, destination);
        },
        0x660F51 => {
            sem::instr_660F51(cpu::safe_read128s(addr)?, destination);
        },
        0x660F58 => {
            sem::instr_660F58(cpu::safe_read128s(addr)?, destination);
        },
        0x660F59 => {
            sem::instr_660F59(cpu::safe_read128s(addr)?, destination);
        },
        0x660F5A => {
            sem::instr_660F5A(cpu::safe_read128s(addr)?, destination);
        },
        0x660F5B => {
            sem::instr_660F5B(cpu::safe_read128s(addr)?, destination);
        },
        0x660F5C => {
            sem::instr_660F5C(cpu::safe_read128s(addr)?, destination);
        },
        0x660F5D => {
            sem::instr_660F5D(cpu::safe_read128s(addr)?, destination);
        },
        0x660F5E => {
            sem::instr_660F5E(cpu::safe_read128s(addr)?, destination);
        },
        0x660F5F => {
            sem::instr_660F5F(cpu::safe_read128s(addr)?, destination);
        },
        0x660F7C => {
            sem::instr_660F7C(cpu::safe_read128s(addr)?, destination);
        },
        0x660F7D => {
            sem::instr_660F7D(cpu::safe_read128s(addr)?, destination);
        },
        0x660FC2 => {
            sem::instr_660FC2(cpu::safe_read128s(addr)?, destination, immediate);
        },
        0x660FD0 => {
            if addr & 15 != 0 {
                cpu::trigger_gp(0);
                return Err(());
            }
            sem::instr_660FD0(cpu::safe_read128s(addr)?, destination);
        },
        0x660FE6 => {
            sem::instr_660FE6(cpu::safe_read128s(addr)?, destination);
        },
        0xF20F2A => {
            sem::instr_F20F2A(cpu::safe_read32s(addr)?, destination);
        },
        0xF20F2C => {
            sem::instr_F20F2C(cpu::safe_read64s(addr)?, destination);
        },
        0xF20F2D => {
            sem::instr_F20F2D(cpu::safe_read64s(addr)?, destination);
        },
        0xF20F51 => {
            sem::instr_F20F51(cpu::safe_read64s(addr)?, destination);
        },
        0xF20F58 => {
            sem::instr_F20F58(cpu::safe_read64s(addr)?, destination);
        },
        0xF20F59 => {
            sem::instr_F20F59(cpu::safe_read64s(addr)?, destination);
        },
        0xF20F5A => {
            sem::instr_F20F5A(cpu::safe_read64s(addr)?, destination);
        },
        0xF20F5C => {
            sem::instr_F20F5C(cpu::safe_read64s(addr)?, destination);
        },
        0xF20F5D => {
            sem::instr_F20F5D(cpu::safe_read64s(addr)?, destination);
        },
        0xF20F5E => {
            sem::instr_F20F5E(cpu::safe_read64s(addr)?, destination);
        },
        0xF20F5F => {
            sem::instr_F20F5F(cpu::safe_read64s(addr)?, destination);
        },
        0xF20F7C => {
            sem::instr_F20F7C(cpu::safe_read128s(addr)?, destination);
        },
        0xF20F7D => {
            sem::instr_F20F7D(cpu::safe_read128s(addr)?, destination);
        },
        0xF20FC2 => {
            sem::instr_F20FC2(cpu::safe_read64s(addr)?, destination, immediate);
        },
        0xF20FD0 => {
            if addr & 15 != 0 {
                cpu::trigger_gp(0);
                return Err(());
            }
            sem::instr_F20FD0(cpu::safe_read128s(addr)?, destination);
        },
        0xF20FE6 => {
            sem::instr_F20FE6(cpu::safe_read128s(addr)?, destination);
        },
        0xF30F2A => {
            sem::instr_F30F2A(cpu::safe_read32s(addr)?, destination);
        },
        0xF30F2C => {
            sem::instr_F30F2C(cpu::safe_read_f32(addr)?, destination);
        },
        0xF30F2D => {
            sem::instr_F30F2D(cpu::safe_read_f32(addr)?, destination);
        },
        0xF30F51 => {
            sem::instr_F30F51(cpu::safe_read_f32(addr)?, destination);
        },
        0xF30F52 => {
            sem::instr_F30F52(cpu::safe_read_f32(addr)?, destination);
        },
        0xF30F53 => {
            sem::instr_F30F53(cpu::safe_read_f32(addr)?, destination);
        },
        0xF30F58 => {
            sem::instr_F30F58(cpu::safe_read_f32(addr)?, destination);
        },
        0xF30F59 => {
            sem::instr_F30F59(cpu::safe_read_f32(addr)?, destination);
        },
        0xF30F5A => {
            sem::instr_F30F5A(cpu::safe_read_f32(addr)?, destination);
        },
        0xF30F5B => {
            sem::instr_F30F5B(cpu::safe_read128s(addr)?, destination);
        },
        0xF30F5C => {
            sem::instr_F30F5C(cpu::safe_read_f32(addr)?, destination);
        },
        0xF30F5D => {
            sem::instr_F30F5D(cpu::safe_read_f32(addr)?, destination);
        },
        0xF30F5E => {
            sem::instr_F30F5E(cpu::safe_read_f32(addr)?, destination);
        },
        0xF30F5F => {
            sem::instr_F30F5F(cpu::safe_read_f32(addr)?, destination);
        },
        0xF30FC2 => {
            sem::instr_F30FC2(cpu::safe_read32s(addr)?, destination, immediate);
        },
        0xF30FE6 => {
            sem::instr_F30FE6(cpu::safe_read64s(addr)?, destination);
        },
        _ => unreachable!("unregistered SSE FP semantic operation"),
    }
    Ok(())
}
#[no_mangle]
pub unsafe fn ir_sse_fp_mem_continue(
    op: u32,
    offset: u32,
    segment: u32,
    destination: i32,
    immediate: i32,
) -> u32 {
    assert!(!cpu::in_jit && segment < 6 && (0..8).contains(&destination));
    if !cpu::task_switch_test_mmx() {
        return finish(false);
    }
    let before = ContinuationContext::capture();
    if memory(op, offset, segment, destination, immediate).is_err() {
        return finish(false);
    }
    if before.epoch != u64::MAX && before == ContinuationContext::capture() {
        Outcome::Normal as u32
    } else {
        // A synchronous observer can reset the VM, remap code, alter execution
        // context or invalidate a compiled dependency. The completed operation
        // retires once, and the CPU remains authoritative at the cold boundary.
        terminal(Outcome::Normal as u32)
    }
}

unsafe fn terminal(outcome: u32) -> u32 {
    if outcome == Outcome::Normal as u32 {
        *gp::instruction_counter = (*gp::instruction_counter).wrapping_add(1);
        Outcome::Invalidated as u32
    } else {
        outcome
    }
}
#[no_mangle]
pub unsafe fn ir_sse_fp_reg(op: u32, source: i32, destination: i32, immediate: i32) -> u32 {
    terminal(ir_sse_fp_reg_continue(op, source, destination, immediate))
}
#[no_mangle]
pub unsafe fn ir_sse_fp_mem(
    op: u32,
    offset: u32,
    segment: u32,
    destination: i32,
    immediate: i32,
) -> u32 {
    terminal(ir_sse_fp_mem_continue(
        op,
        offset,
        segment,
        destination,
        immediate,
    ))
}
