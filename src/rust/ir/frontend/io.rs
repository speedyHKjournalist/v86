//! Terminal scalar port I/O and single-iteration INS/OUTS.
use super::{
    adapters::call,
    decode::DecodedInstruction,
    integer::IntegerBuilder,
    lift::{memory_read, segmented, snapshot},
};
use crate::ir::{
    hir::{Binary, Op},
    lowering::CompileError,
    state::ResumeKind,
    types::Type,
};
pub fn supports(i: &DecodedInstruction) -> bool {
    matches!(i.encoding.opcode,0xE4..=0xE7|0xEC..=0xEF|0x6C..=0x6F)
}
pub fn lift(
    b: &mut IntegerBuilder,
    i: &DecodedInstruction,
    count: u32,
) -> Result<(), CompileError> {
    let op = i.encoding.opcode;
    let string = op < 0x70;
    if string && (i.prefixes.rep || i.prefixes.repne) {
        return Err(CompileError::Unsupported("REP I/O must use batch frontend"));
    }
    let width = if op & 1 == 0 { 8 } else { i.operand_size };
    let bytes = b.constant((width / 8) as u32, Type::I32);
    let port = if matches!(op, 0xE4..=0xE7) {
        b.constant(i.immediate.unwrap(), Type::I32)
    } else {
        let p = b.read(2, 16);
        b.node(Op::Extend { signed: false }, vec![p], Type::I32)
    };
    let map = snapshot(b, i.instruction_pc, i.next_pc, count - 1);
    b.region.states[map.index()].resume = ResumeKind::BeforeInstruction;
    if !string {
        let input = op & 2 == 0;
        if input {
            call(b, "ir_in", vec![port, bytes], map, true);
        } else {
            let value = b.read(0, width);
            let value = if width < 32 {
                b.node(Op::Extend { signed: false }, vec![value], Type::I32)
            } else {
                value
            };
            call(b, "ir_out", vec![port, bytes, value], map, true);
        }
        return Ok(());
    }
    let input = op & 2 == 0;
    let reg = if input { 7 } else { 6 };
    let old = b.read(reg, i.address_size);
    let ty = b.ty(old);
    let off = if i.address_size == 16 {
        b.node(Op::Extend { signed: false }, vec![old], Type::I32)
    } else {
        old
    };
    let address = segmented(
        b,
        off,
        if input { 0 } else { i.prefixes.segment.unwrap_or(3) },
        map,
    );
    let df = b.extract(b.flags.system, 10, Type::I1);
    let plus = b.constant((width / 8) as u32, ty);
    let minus = b.constant(0u32.wrapping_sub((width / 8) as u32), ty);
    let delta = b.node(Op::Select, vec![df, minus, plus], ty);
    let next = b.binary(Binary::Add, old, delta);
    let next = if i.address_size == 16 {
        b.node(
            Op::Insert { lsb: 0 },
            vec![b.gpr[reg as usize], next],
            Type::I32,
        )
    } else {
        next
    };
    if input {
        call(b, "ir_ins", vec![port, bytes, address, next], map, true);
    } else {
        call(b, "ir_io_check", vec![port, bytes], map, false);
        let value = memory_read(b, address, width, map, false).0;
        let value = if width < 32 {
            b.node(Op::Extend { signed: false }, vec![value], Type::I32)
        } else {
            value
        };
        call(b, "ir_outs", vec![port, bytes, value, next], map, true);
    }
    Ok(())
}
