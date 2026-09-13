//! The compile-time assumptions of a cold CPU entry, separate from physical code dependencies.
use crate::cpu::{cpu, global_pointers as gp};
use crate::ir::frontend::decode::{GuestEip, LinearAddress};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuEntryKey {
    pub pc: GuestEip,
    pub linear: LinearAddress,
    pub default_32: bool,
}
impl CpuEntryKey {
    pub fn cs_base(self) -> u32 {
        self.linear.0.wrapping_sub(self.pc.0)
    }
}

/// A standalone artifact cannot be admitted as a CPU entry (or vice versa).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryContract {
    Standalone,
    Cpu(CpuEntryKey),
}

/// No state writes, memory translation, lazy-FLAGS evaluation or exception delivery.
/// This guard does NOT validate code versions or grant permission to call a stale slot.
#[no_mangle]
pub unsafe fn ir_entry_matches(linear: u32, cs_base: u32, default_32: u32) -> bool {
    !cpu::in_jit
        && *gp::prefixes == 0
        && !*gp::in_hlt
        && *gp::instruction_pointer as u32 == linear
        && cpu::get_seg_cs() as u32 == cs_base
        && u32::from(*gp::is_32) == default_32
}

#[cfg(feature = "ir-test-hooks")]
#[no_mangle]
pub unsafe fn ir_test_entry_in_jit(linear: u32, cs_base: u32, mode: u32) -> bool {
    let saved = cpu::in_jit;
    cpu::in_jit = true;
    let result = ir_entry_matches(linear, cs_base, mode);
    cpu::in_jit = saved;
    result
}
