#![allow(non_snake_case)]

use crate::cpu_context::CpuContext;
use crate::gen;
use crate::modrm;
use crate::prefix::{PREFIX_66, PREFIX_67, PREFIX_F2, PREFIX_F3, PREFIX_MASK_SEGMENT};
use crate::regs::{CS, DS, ES, FS, GS, SS};

#[derive(Debug, PartialEq, Eq)]
pub enum AnalysisType {
    Normal,
    BlockBoundary,
    Jump {
        offset: i32,
        is_32: bool,
        condition: Option<u8>,
    },
    STI,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Analysis {
    pub no_next_instruction: bool,
    pub absolute_jump: bool,
    pub ty: AnalysisType,
}

/// Known complete instructions share the IR decoder. Reserved/unknown forms
/// retain the pinned legacy analyzer until their explicit invalid-decode model lands.
pub fn analyze_step(cpu: &mut CpuContext, linear_pc: u32) -> Analysis {
    use crate::decode::{decode, GuestEip, LinearAddress};
    if let Some(snapshot) = cpu.instruction_snapshot() {
        if let Ok(instruction) = decode(
            &snapshot.bytes[..snapshot.length],
            GuestEip(linear_pc.wrapping_sub(cpu.cs_offset)),
            LinearAddress(linear_pc),
            cpu.state_flags.is_32(),
        ) {
            cpu.eip = cpu.eip.wrapping_add(instruction.length as u32);
            let p = instruction.prefixes;
            cpu.prefixes = p.segment.map_or(0, |s| s + 1)
                | if p.operand { PREFIX_66 } else { 0 }
                | if p.address { PREFIX_67 } else { 0 }
                | if p.repne { PREFIX_F2 } else { 0 }
                | if p.rep { PREFIX_F3 } else { 0 };
            return analyze_decoded(&instruction);
        }
    }
    analyze_step_legacy(cpu)
}

pub fn analyze_step_legacy(mut cpu: &mut CpuContext) -> Analysis {
    let mut analysis = Analysis {
        no_next_instruction: false,
        absolute_jump: false,
        ty: AnalysisType::Normal,
    };
    cpu.prefixes = 0;
    let opcode = cpu.read_imm8() as u32 | (cpu.osize_32() as u32) << 8;
    gen::analyzer::analyzer(opcode, &mut cpu, &mut analysis);
    analysis
}

pub fn analyze_step_handle_prefix(cpu: &mut CpuContext, analysis: &mut Analysis) {
    gen::analyzer::analyzer(
        cpu.read_imm8() as u32 | (cpu.osize_32() as u32) << 8,
        cpu,
        analysis,
    )
}
pub fn analyze_step_handle_segment_prefix(
    segment: u32,
    cpu: &mut CpuContext,
    analysis: &mut Analysis,
) {
    dbg_assert!(segment <= 5);
    cpu.prefixes = cpu.prefixes & !PREFIX_MASK_SEGMENT | (segment as u8 + 1);
    analyze_step_handle_prefix(cpu, analysis)
}

pub fn instr16_0F_analyze(cpu: &mut CpuContext, analysis: &mut Analysis) {
    gen::analyzer0f::analyzer(cpu.read_imm8() as u32, cpu, analysis)
}
pub fn instr32_0F_analyze(cpu: &mut CpuContext, analysis: &mut Analysis) {
    gen::analyzer0f::analyzer(cpu.read_imm8() as u32 | 0x100, cpu, analysis)
}
pub fn instr_26_analyze(cpu: &mut CpuContext, analysis: &mut Analysis) {
    analyze_step_handle_segment_prefix(ES, cpu, analysis)
}
pub fn instr_2E_analyze(cpu: &mut CpuContext, analysis: &mut Analysis) {
    analyze_step_handle_segment_prefix(CS, cpu, analysis)
}
pub fn instr_36_analyze(cpu: &mut CpuContext, analysis: &mut Analysis) {
    analyze_step_handle_segment_prefix(SS, cpu, analysis)
}
pub fn instr_3E_analyze(cpu: &mut CpuContext, analysis: &mut Analysis) {
    analyze_step_handle_segment_prefix(DS, cpu, analysis)
}
pub fn instr_64_analyze(cpu: &mut CpuContext, analysis: &mut Analysis) {
    analyze_step_handle_segment_prefix(FS, cpu, analysis)
}
pub fn instr_65_analyze(cpu: &mut CpuContext, analysis: &mut Analysis) {
    analyze_step_handle_segment_prefix(GS, cpu, analysis)
}
pub fn instr_66_analyze(cpu: &mut CpuContext, analysis: &mut Analysis) {
    cpu.prefixes |= PREFIX_66;
    analyze_step_handle_prefix(cpu, analysis)
}
pub fn instr_67_analyze(cpu: &mut CpuContext, analysis: &mut Analysis) {
    cpu.prefixes |= PREFIX_67;
    analyze_step_handle_prefix(cpu, analysis)
}
pub fn instr_F0_analyze(cpu: &mut CpuContext, analysis: &mut Analysis) {
    // lock: Ignored
    analyze_step_handle_prefix(cpu, analysis)
}
pub fn instr_F2_analyze(cpu: &mut CpuContext, analysis: &mut Analysis) {
    cpu.prefixes |= PREFIX_F2;
    analyze_step_handle_prefix(cpu, analysis)
}
pub fn instr_F3_analyze(cpu: &mut CpuContext, analysis: &mut Analysis) {
    cpu.prefixes |= PREFIX_F3;
    analyze_step_handle_prefix(cpu, analysis)
}

pub fn modrm_analyze(ctx: &mut CpuContext, modrm_byte: u8) {
    modrm::skip(ctx, modrm_byte);
}

/// Consume decoded facts without reading memory or decoding opcode bytes again.
pub fn analyze_decoded(instruction: &crate::decode::DecodedInstruction) -> Analysis {
    use crate::decode::Flow;
    let encoding = instruction.encoding;
    Analysis {
        no_next_instruction: encoding.no_next_instruction,
        absolute_jump: encoding.absolute_jump,
        ty: match instruction.flow {
            Flow::Next => AnalysisType::Normal,
            Flow::Boundary | Flow::Stop => AnalysisType::BlockBoundary,
            Flow::Sti => AnalysisType::STI,
            Flow::Relative {
                displacement,
                conditional,
                ..
            } => AnalysisType::Jump {
                offset: displacement,
                is_32: instruction.operand_size == 32,
                condition: if conditional { Some(encoding.opcode as u8) } else { None },
            },
        },
    }
}

#[cfg(feature = "ir-test-hooks")]
#[path = "../../tests/ir/decode/legacy_oracle.rs"]
mod legacy_oracle;
