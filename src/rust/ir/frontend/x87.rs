//! Terminal x87 register/memory lowering with CPU-owned F80 stack/status.
//! Memory helpers resolve segments after the #NM guard and own precise faults.
use super::{
    adapters::call,
    decode::DecodedInstruction,
    integer::IntegerBuilder,
    lift::{effective_offset, snapshot},
};
use crate::ir::{state::ResumeKind, types::Type};

pub fn supports(i: &DecodedInstruction) -> bool {
    (0xD8..=0xDF).contains(&i.encoding.opcode)
}

pub fn lift(b: &mut IntegerBuilder, i: &DecodedInstruction, count: u32) {
    debug_assert!(supports(i));
    let state = snapshot(b, i.instruction_pc, i.next_pc, count - 1);
    b.region.states[state.index()].resume = ResumeKind::BeforeInstruction;

    let modrm = i.modrm.expect("x87 register form has ModRM");
    if let Some(ea) = i.ea {
        // Segment resolution belongs to the helper: #NM precedes segment faults.
        let offset = effective_offset(b, &ea);
        let opcode = b.constant(i.encoding.opcode, Type::I32);
        let group = b.constant((modrm >> 3 & 7) as u32, Type::I32);
        let segment = b.constant(ea.segment as u32, Type::I32);
        let width = b.constant(i.operand_size as u32, Type::I32);
        call(
            b,
            "ir_x87_mem",
            vec![opcode, group, offset, segment, width],
            state,
            true,
        );
        return;
    }
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
