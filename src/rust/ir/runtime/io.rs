//! Audited port adapters. CPU owns I/O permissions, devices and fault delivery.
use crate::cpu::{cpu, global_pointers as gp};
use crate::ir::helper::Outcome;
fn valid(port: u32, bytes: u32) {
    assert!(port <= 65535 && matches!(bytes, 1 | 2 | 4));
}
unsafe fn commit() -> u32 {
    *gp::instruction_counter = (*gp::instruction_counter).wrapping_add(1);
    Outcome::Invalidated as u32
}
unsafe fn read(port: u32, bytes: u32) -> i32 {
    match bytes {
        1 => cpu::io_port_read8(port as i32),
        2 => cpu::io_port_read16(port as i32),
        4 => cpu::io_port_read32(port as i32),
        _ => unreachable!(),
    }
}
unsafe fn write(port: u32, bytes: u32, value: u32) {
    match bytes {
        1 => cpu::io_port_write8(port as i32, (value & 255) as i32),
        2 => cpu::io_port_write16(port as i32, (value & 65535) as i32),
        4 => cpu::io_port_write32(port as i32, value as i32),
        _ => unreachable!(),
    }
}
#[no_mangle]
pub unsafe fn ir_io_check(port: u32, bytes: u32) -> u32 {
    assert!(!cpu::in_jit);
    valid(port, bytes);
    if cpu::test_privileges_for_io(port as i32, bytes as i32) {
        Outcome::Normal as u32
    } else {
        Outcome::ControlTransferred as u32
    }
}
#[no_mangle]
pub unsafe fn ir_in(port: u32, bytes: u32) -> u32 {
    if ir_io_check(port, bytes) != 0 {
        return Outcome::ControlTransferred as u32;
    }
    let value = read(port, bytes);
    match bytes {
        1 => cpu::write_reg8(0, value),
        2 => cpu::write_reg16(0, value),
        4 => cpu::write_reg32(0, value),
        _ => unreachable!(),
    };
    commit()
}
#[no_mangle]
pub unsafe fn ir_out(port: u32, bytes: u32, value: u32) -> u32 {
    if ir_io_check(port, bytes) != 0 {
        return Outcome::ControlTransferred as u32;
    }
    write(port, bytes, value);
    commit()
}
/// Frontend has already checked permission before its ordered source GuestLoad.
#[no_mangle]
pub unsafe fn ir_outs(port: u32, bytes: u32, value: u32, next_si: u32) -> u32 {
    assert!(!cpu::in_jit);
    valid(port, bytes);
    write(port, bytes, value);
    cpu::write_reg32(6, next_si as i32);
    commit()
}
/// INS checks the entire destination before reading the port. The actual write
/// still translates again: a device callback may invalidate the preflight.
#[no_mangle]
pub unsafe fn ir_ins(port: u32, bytes: u32, address: u32, next_di: u32) -> u32 {
    if ir_io_check(port, bytes) != 0 {
        return Outcome::ControlTransferred as u32;
    }
    if cpu::writable_or_pagefault(address as i32, bytes as i32).is_err() {
        return Outcome::ControlTransferred as u32;
    }
    let value = read(port, bytes);
    let result = match bytes {
        1 => cpu::safe_write8(address as i32, value),
        2 => cpu::safe_write16(address as i32, value),
        4 => cpu::safe_write32(address as i32, value),
        _ => unreachable!(),
    };
    if result.is_err() {
        return Outcome::ControlTransferred as u32;
    }
    cpu::write_reg32(7, next_di as i32);
    commit()
}
