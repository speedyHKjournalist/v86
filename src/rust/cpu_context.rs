use crate::cpu::memory;
use crate::prefix::{PREFIX_MASK_ADDRSIZE, PREFIX_MASK_OPSIZE};
use crate::state_flags::CachedStateFlags;

#[derive(Clone)]
pub struct CpuContext {
    pub eip: u32,
    pub prefixes: u8,
    pub cs_offset: u32,
    pub state_flags: CachedStateFlags,
}

/// A bounded copy of physical code RAM. It never crosses a page or invokes MMIO.
pub struct InstructionSnapshot {
    pub bytes: [u8; 15],
    pub length: usize,
}

impl CpuContext {
    pub fn instruction_snapshot(&self) -> Option<InstructionSnapshot> {
        let length = (0x1000 - (self.eip & 0xFFF)).min(15);
        let last = self.eip.checked_add(length - 1)?;
        if unsafe { memory::mem8.is_null() }
            || memory::in_mapped_range(self.eip)
            || memory::in_mapped_range(last)
        {
            return None;
        }
        let mut snapshot = InstructionSnapshot {
            bytes: [0; 15],
            length: length as usize,
        };
        for i in 0..length {
            snapshot.bytes[i as usize] = memory::read8_no_mmap_check(self.eip + i) as u8;
        }
        Some(snapshot)
    }

    pub fn advance16(&mut self) {
        dbg_assert!(self.eip & 0xFFF < 0xFFE);
        self.eip += 2;
    }
    pub fn advance32(&mut self) {
        dbg_assert!(self.eip & 0xFFF < 0xFFC);
        self.eip += 4;
    }
    #[allow(unused)]
    pub fn advance_moffs(&mut self) {
        if self.asize_32() {
            self.advance32()
        } else {
            self.advance16()
        }
    }

    pub fn read_imm8(&mut self) -> u8 {
        dbg_assert!(self.eip & 0xFFF < 0xFFF);
        let v = memory::read8(self.eip) as u8;
        self.eip += 1;
        v
    }
    pub fn read_imm8s(&mut self) -> i8 {
        self.read_imm8() as i8
    }
    pub fn read_imm16(&mut self) -> u16 {
        dbg_assert!(self.eip & 0xFFF < 0xFFE);
        let v = memory::read16(self.eip) as u16;
        self.eip += 2;
        v
    }
    pub fn read_imm32(&mut self) -> u32 {
        dbg_assert!(self.eip & 0xFFF < 0xFFC);
        let v = memory::read32s(self.eip) as u32;
        self.eip += 4;
        v
    }
    pub fn read_moffs(&mut self) -> u32 {
        if self.asize_32() {
            self.read_imm32()
        } else {
            self.read_imm16() as u32
        }
    }

    pub fn cpl3(&self) -> bool {
        self.state_flags.cpl3()
    }
    pub fn has_flat_segmentation(&self) -> bool {
        self.state_flags.has_flat_segmentation()
    }
    pub fn osize_32(&self) -> bool {
        self.state_flags.is_32() != (self.prefixes & PREFIX_MASK_OPSIZE != 0)
    }
    pub fn asize_32(&self) -> bool {
        self.state_flags.is_32() != (self.prefixes & PREFIX_MASK_ADDRSIZE != 0)
    }
    pub fn ssize_32(&self) -> bool {
        self.state_flags.ssize_32()
    }
}
