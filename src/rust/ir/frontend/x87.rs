//! Terminal x87 register-form lowering.
//!
//! This first IR-08 x87 slice deliberately excludes guest-memory operands.
//! The CPU helper owns the architectural F80 stack/status state and terminates
//! the current IR region, so no cached SSA x87 value can survive the call.
use super::{adapters::call, decode::DecodedInstruction, integer::IntegerBuilder, lift::snapshot};
use crate::ir::{state::ResumeKind, types::Type};

pub fn supports(i: &DecodedInstruction) -> bool {
    (0xD8..=0xDF).contains(&i.encoding.opcode) && i.ea.is_none()
}

pub fn lift(b: &mut IntegerBuilder, i: &DecodedInstruction, count: u32) {
    debug_assert!(supports(i));
    let state = snapshot(b, i.instruction_pc, i.next_pc, count - 1);
    b.region.states[state.index()].resume = ResumeKind::BeforeInstruction;

    let modrm = i.modrm.expect("x87 register form has ModRM");
    let args = [
        i.encoding.opcode,
        (modrm >> 3 & 7) as u32,
        (modrm & 7) as u32,
        i.operand_size as u32,
    ]
    .into_iter()
    .map(|value| b.constant(value, Type::I32))
    .collect();
    call(b, "ir_x87_reg", args, state, true);
}
