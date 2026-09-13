//! Terminal system-state transitions with CPU-owned results.
use super::{adapters::call, decode::DecodedInstruction, integer::IntegerBuilder, lift::snapshot};
use crate::ir::state::ResumeKind;

pub fn supports(i: &DecodedInstruction) -> bool {
    matches!(
        i.encoding.opcode,
        0xF4 | 0xFA | 0x0F06 | 0x0F09 | 0x0F34 | 0x0F35
    )
}
pub fn lift(b: &mut IntegerBuilder, i: &DecodedInstruction, count: u32) {
    let state = snapshot(b, i.instruction_pc, i.next_pc, count - 1);
    b.region.states[state.index()].resume = ResumeKind::BeforeInstruction;
    let name = match i.encoding.opcode {
        0xF4 => "ir_hlt",
        0xFA => "ir_cli",
        0x0F06 => "ir_clts",
        0x0F09 => "ir_wbinvd",
        0x0F34 => "ir_sysenter",
        0x0F35 => "ir_sysexit",
        _ => unreachable!(),
    };
    call(b, name, vec![], state, true);
}
