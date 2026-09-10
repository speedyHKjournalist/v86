//! Direct packed operations. Compile-time SIMD gating keeps fallback cores
//! usable on hosts without SIMD; safe guest accesses precede register writes.
use crate::codegen;
use crate::cpu::global_pointers as gp;
use crate::jit::JitContext;
use crate::modrm::ModrmByte;

#[derive(Clone, Copy)]
enum Op {
    Shuffle([u8; 16]),
    CompareFlags(bool),
    Convert,
    Float(bool, bool, u8),
    MulHigh(bool),
    MulDwords,
    Sad,
    Binary(u32),
    AndNot,
    Unpack(u8, bool),
    Pack(u32),
    Shift(u32, u32, bool),
}

fn operation(name: &str, mmx: bool) -> Option<Op> {
    if !mmx {
        if let Some((prefix, code)) = name
            .strip_prefix("instr_")
            .and_then(|n| n.rsplit_once("0F"))
        {
            if matches!(prefix, "66" | "F2") && matches!(code, "7C" | "7D" | "D0") {
                return Some(Op::Float(
                    prefix == "66",
                    false,
                    u8::from_str_radix(code, 16).unwrap(),
                ));
            }
            if matches!(prefix, "" | "66" | "F2" | "F3")
                && matches!(code, "51" | "58" | "59" | "5C" | "5D" | "5E" | "5F")
                || matches!(prefix, "" | "F3") && matches!(code, "52" | "53")
            {
                let opcode = u8::from_str_radix(code, 16).unwrap();
                return Some(Op::Float(
                    matches!(prefix, "66" | "F2"),
                    matches!(prefix, "F2" | "F3"),
                    opcode,
                ));
            }
        }
        match name {
            "instr_0F16" => {
                return Some(Op::Shuffle([
                    0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23,
                ]))
            },
            "instr_F20F12" => {
                return Some(Op::Shuffle([
                    16, 17, 18, 19, 20, 21, 22, 23, 16, 17, 18, 19, 20, 21, 22, 23,
                ]))
            },
            "instr_F30F12" => {
                return Some(Op::Shuffle([
                    16, 17, 18, 19, 16, 17, 18, 19, 24, 25, 26, 27, 24, 25, 26, 27,
                ]))
            },
            "instr_F30F16" => {
                return Some(Op::Shuffle([
                    20, 21, 22, 23, 20, 21, 22, 23, 28, 29, 30, 31, 28, 29, 30, 31,
                ]))
            },
            "instr_0F2E" | "instr_0F2F" => return Some(Op::CompareFlags(false)),
            "instr_660F2E" | "instr_660F2F" => return Some(Op::CompareFlags(true)),
            "instr_0F5A" | "instr_660F5A" | "instr_F20F5A" | "instr_F30F5A" | "instr_0F5B"
            | "instr_660F5B" | "instr_F30F5B" | "instr_660FE6" | "instr_F20FE6"
            | "instr_F30FE6" => return Some(Op::Convert),
            "instr_0F14" => return Some(Op::Unpack(4, false)),
            "instr_0F15" => return Some(Op::Unpack(4, true)),
            "instr_660F14" => return Some(Op::Unpack(8, false)),
            "instr_660F15" => return Some(Op::Unpack(8, true)),
            "instr_0F54" | "instr_660F54" => return Some(Op::Binary(0x4E)),
            "instr_0F55" | "instr_660F55" => return Some(Op::AndNot),
            "instr_0F56" | "instr_660F56" => return Some(Op::Binary(0x50)),
            "instr_0F57" | "instr_660F57" => return Some(Op::Binary(0x51)),
            _ => {},
        }
    }
    let code = name.strip_prefix(if mmx { "instr_0F" } else { "instr_660F" })?;
    Some(match code {
        "FC" => Op::Binary(0x6E),
        "FD" => Op::Binary(0x8E),
        "FE" => Op::Binary(0xAE),
        "D4" => Op::Binary(0xCE),
        "F8" => Op::Binary(0x71),
        "F9" => Op::Binary(0x91),
        "FA" => Op::Binary(0xB1),
        "FB" => Op::Binary(0xD1),
        "EC" => Op::Binary(0x6F),
        "ED" => Op::Binary(0x8F),
        "DC" => Op::Binary(0x70),
        "DD" => Op::Binary(0x90),
        "E8" => Op::Binary(0x72),
        "E9" => Op::Binary(0x92),
        "D8" => Op::Binary(0x73),
        "D9" => Op::Binary(0x93),
        "64" => Op::Binary(0x27),
        "65" => Op::Binary(0x31),
        "66" => Op::Binary(0x3B),
        "74" => Op::Binary(0x23),
        "75" => Op::Binary(0x2D),
        "76" => Op::Binary(0x37),
        "DA" => Op::Binary(0x77),
        "DE" => Op::Binary(0x79),
        "EA" => Op::Binary(0x96),
        "EE" => Op::Binary(0x98),
        "E0" => Op::Binary(0x7B),
        "E3" => Op::Binary(0x9B),
        "E4" => Op::MulHigh(false),
        "E5" => Op::MulHigh(true),
        "F4" => Op::MulDwords,
        "F6" => Op::Sad,
        "D5" => Op::Binary(0x95),
        "F5" => Op::Binary(0xBA),
        "DB" => Op::Binary(0x4E),
        "DF" => Op::AndNot,
        "EB" => Op::Binary(0x50),
        "EF" => Op::Binary(0x51),
        "60" => Op::Unpack(1, false),
        "61" => Op::Unpack(2, false),
        "62" => Op::Unpack(4, false),
        "68" => Op::Unpack(1, true),
        "69" => Op::Unpack(2, true),
        "6A" => Op::Unpack(4, true),
        "6C" if !mmx => Op::Unpack(8, false),
        "6D" if !mmx => Op::Unpack(8, true),
        "63" => Op::Pack(0x65),
        "67" => Op::Pack(0x66),
        "6B" => Op::Pack(0x85),
        "D1" => Op::Shift(0x8D, 16, false),
        "D2" => Op::Shift(0xAD, 32, false),
        "D3" => Op::Shift(0xCD, 64, false),
        "E1" => Op::Shift(0x8C, 16, true),
        "E2" => Op::Shift(0xAC, 32, true),
        "F1" => Op::Shift(0x8B, 16, false),
        "F2" => Op::Shift(0xAB, 32, false),
        "F3" => Op::Shift(0xCB, 64, false),
        _ => return None,
    })
}
pub fn supported(name: &str, mmx: bool) -> bool {
    cfg!(target_feature = "simd128") && operation(name, mmx).is_some()
}
fn address(r: u32, mmx: bool) -> u32 {
    if mmx {
        gp::get_reg_mmx_offset(r)
    } else {
        gp::get_reg_xmm_offset(r)
    }
}
/// Cache uninterrupted register-only arithmetic. No guest memory or control
/// changes occur between writes. Float fallback helpers get a materialized
/// snapshot and their destination is reloaded before continuing.
/// Flush before every other instruction and at every basic-block boundary.
pub fn prepare_instruction(ctx: &mut JitContext) {
    use crate::cpu::memory::read8;
    let mut a = ctx.cpu.eip;
    let mut kind = None;
    if cfg!(target_feature = "simd128") && !cfg!(feature = "profiler") && unsafe { crate::jit::JIT_SIMD_CACHE } {
        let prefix = match read8(a) {
            p @ (0x66 | 0xF2 | 0xF3) => {
                a += 1;
                p
            },
            _ => 0,
        };
        if read8(a) == 0x0F && read8(a + 2) >= 0xC0 {
            let opcode = read8(a + 1);
            let mmx = prefix == 0 && opcode >= 0x60;
            let name = if prefix == 0 {
                format!("instr_0F{:02X}", opcode)
            } else {
                format!("instr_{:02X}0F{:02X}", prefix, opcode)
            };
            let op = operation(&name, mmx);
            // Scalar stores are cheaper than reconstructing the untouched
            // upper lanes on every operation; measured caching regressed SS/SD.
            // Mixed-precision ADDSUB chains quickly reach NaNs; materializing
            // the entire cache at each helper call was slower than direct stores.
            if !matches!(op, Some(Op::Float(_, true, _) | Op::Float(_, _, 0xD0))) && matches!(
                op,
                Some(
                    Op::Binary(_)
                        | Op::AndNot
                        | Op::Pack(_)
                        | Op::Unpack(..)
                        | Op::Shift(..)
                        | Op::MulHigh(_)
                        | Op::MulDwords
                        | Op::Sad
                        | Op::Float(..)
                )
            ) && (!mmx || !matches!(opcode, 0xFE | 0xF5))
                || matches!(name.as_str(), "instr_F20F12" | "instr_F30F12" | "instr_F30F16")
            {
                kind = Some(mmx);
            }
        }
    }
    if kind.is_none() || kind != ctx.simd_cache_kind {
        flush_cache(ctx);
    }
    ctx.simd_cache_kind = kind;
}
fn write_cache(ctx: &mut JitContext) {
    for (addr, local, mmx) in &ctx.simd_cache {
        ctx.builder.const_i32(*addr as i32);
        ctx.builder.get_local_v128(local);
        if *mmx {
            ctx.builder.simd_lane(0x1D, 0);
            ctx.builder.store_aligned_i64(0);
        } else {
            ctx.builder.simd_memory(0x0B, 2);
        }
    }
}
pub fn flush_cache(ctx: &mut JitContext) {
    write_cache(ctx);
    for (_, local, _) in ctx.simd_cache.drain(..) {
        ctx.builder.free_local_v128(local);
    }
    ctx.simd_cache_kind = None;
}
fn cache_value(ctx: &mut JitContext, dst: u32, mmx: bool) {
    let value = ctx.builder.set_new_local_v128();
    if let Some(index) = ctx.simd_cache.iter().position(|(a, _, _)| *a == dst) {
        let (_, old, _) = ctx.simd_cache.swap_remove(index);
        ctx.builder.free_local_v128(old);
    }
    ctx.simd_cache.push((dst, value, mmx));
}
fn load(ctx: &mut JitContext, address: u32, bytes: u32) {
    if let Some((_, local, _)) = ctx.simd_cache.iter().find(|(a, _, _)| *a == address) {
        ctx.builder.get_local_v128(local);
        return;
    }
    ctx.builder.const_i32(address as i32);
    ctx.builder.simd_memory(
        match bytes {
            4 => 0x5C,
            8 => 0x5D,
            _ => 0,
        },
        2,
    );
}
fn emit(ctx: &mut JitContext, op: Op, src: u32, dst: u32, bytes: u32, source_bytes: u32) {
    if ctx.simd_cache_kind.is_none() {
        ctx.builder.const_i32(dst as i32);
    }
    if let Op::MulHigh(signed) = op {
        load(ctx, dst, bytes);
        load(ctx, src, source_bytes);
        ctx.builder.simd(if signed { 0xBC } else { 0xBE });
        if bytes == 16 {
            load(ctx, dst, bytes);
            load(ctx, src, source_bytes);
            ctx.builder.simd(if signed { 0xBD } else { 0xBF });
        } else {
            ctx.builder.simd_zero();
        }
        ctx.builder
            .simd_shuffle([2, 3, 6, 7, 10, 11, 14, 15, 18, 19, 22, 23, 26, 27, 30, 31]);
    } else if let Op::MulDwords = op {
        for addr in [dst, src] {
            load(ctx, addr, bytes);
            ctx.builder.simd_zero();
            ctx.builder
                .simd_shuffle([0, 1, 2, 3, 8, 9, 10, 11, 16, 17, 18, 19, 20, 21, 22, 23]);
        }
        ctx.builder.simd(0xDE);
    } else if let Op::Sad = op {
        load(ctx, dst, bytes);
        load(ctx, src, source_bytes);
        ctx.builder.simd(0x79);
        load(ctx, dst, bytes);
        load(ctx, src, source_bytes);
        ctx.builder.simd(0x77);
        ctx.builder.simd(0x71);
        ctx.builder.simd(0x7D);
        ctx.builder.simd(0x7F);
        let sums = ctx.builder.set_new_local_v128();
        ctx.builder.get_local_v128(&sums);
        ctx.builder.get_local_v128(&sums);
        ctx.builder.get_local_v128(&sums);
        ctx.builder
            .simd_shuffle([4, 5, 6, 7, 0, 1, 2, 3, 12, 13, 14, 15, 8, 9, 10, 11]);
        ctx.builder.simd(0xAE);
        ctx.builder.simd_zero();
        ctx.builder
            .simd_shuffle([0, 1, 2, 3, 16, 17, 18, 19, 8, 9, 10, 11, 16, 17, 18, 19]);
        ctx.builder.free_local_v128(sums);
    } else if let Op::Shift(opcode, bits, arithmetic) = op {
        load(ctx, src, source_bytes);
        ctx.builder.simd_lane(0x1D, 0);
        let count = ctx.builder.set_new_local_i64();
        ctx.builder.get_local_i64(&count);
        ctx.builder.const_i64((bits - 1) as i64);
        ctx.builder.gtu_i64();
        ctx.builder.if_v128();
        if arithmetic {
            load(ctx, dst, bytes);
            ctx.builder.const_i32((bits - 1) as i32);
            ctx.builder.simd(opcode);
        } else {
            ctx.builder.simd_zero();
        }
        ctx.builder.else_();
        load(ctx, dst, bytes);
        ctx.builder.get_local_i64(&count);
        ctx.builder.wrap_i64_to_i32();
        ctx.builder.simd(opcode);
        ctx.builder.block_end();
        ctx.builder.free_local_i64(count);
    } else {
        if let Op::AndNot = op {
            load(ctx, src, source_bytes);
            load(ctx, dst, bytes);
        } else {
            load(ctx, dst, bytes);
            load(ctx, src, source_bytes);
        }
        match op {
            Op::Shuffle(lanes) => ctx.builder.simd_shuffle(lanes),
            Op::Binary(opcode) => ctx.builder.simd(opcode),
            Op::AndNot => ctx.builder.simd(0x4F),
            Op::Pack(opcode) => {
                ctx.builder.simd(opcode);
                if bytes == 8 {
                    ctx.builder.simd_zero();
                    ctx.builder
                        .simd_shuffle([0, 1, 2, 3, 8, 9, 10, 11, 16, 17, 18, 19, 20, 21, 22, 23]);
                }
            },
            Op::Unpack(width, high) => {
                let mut lanes = [0; 16];
                let half = bytes as u8 / 2;
                for i in 0..bytes as u8 {
                    let element = i / (width * 2);
                    let side = i / width % 2;
                    lanes[i as usize] =
                        (if high { half } else { 0 }) + element * width + i % width + side * 16;
                }
                ctx.builder.simd_shuffle(lanes);
            },
            Op::CompareFlags(..)
            | Op::Convert
            | Op::Float(..)
            | Op::Shift(..)
            | Op::MulHigh(..)
            | Op::MulDwords
            | Op::Sad => unreachable!(),
        }
    }
    if let Some(mmx) = ctx.simd_cache_kind {
        cache_value(ctx, dst, mmx);
    } else if bytes == 8 {
        ctx.builder.simd_lane(0x1D, 0);
        ctx.builder.store_aligned_i64(0);
    } else {
        ctx.builder.simd_memory(0x0B, 2);
    }
}
pub fn register(ctx: &mut JitContext, name: &str, src: u32, dst: u32, mmx: bool) -> bool {
    if !cfg!(target_feature = "simd128") {
        return false;
    }
    let Some(op) = operation(name, mmx) else {
        return false;
    };
    let bytes = if mmx { 8 } else { 16 };
    if let Op::CompareFlags(double) = op {
        compare_flags(ctx, address(src, false), dst, double);
    } else if let Op::Convert = op {
        convert(ctx, name, address(src, false), dst);
    } else if let Op::Float(double, scalar, opcode) = op {
        float_emit(ctx, name, address(src, false), dst, double, scalar, opcode);
    } else {
        emit(ctx, op, address(src, mmx), address(dst, mmx), bytes, bytes);
    }
    if mmx {
        crate::jit_instructions::mmx_finish(ctx, Some(dst));
    }
    true
}
pub fn memory(
    ctx: &mut JitContext,
    name: &str,
    modrm: ModrmByte,
    dst: u32,
    mmx: bool,
    source_bytes: u32,
) -> bool {
    if !cfg!(target_feature = "simd128") {
        return false;
    }
    let Some(op) = operation(name, mmx) else {
        return false;
    };
    let scratch = gp::sse_scratch_register as u32;
    match source_bytes {
        4 => {
            ctx.builder.const_i32(scratch as i32);
            codegen::gen_modrm_resolve_safe_read32(ctx, modrm);
            ctx.builder.store_aligned_i32(0);
        },
        8 => {
            ctx.builder.const_i32(scratch as i32);
            codegen::gen_modrm_resolve_safe_read64(ctx, modrm);
            ctx.builder.store_aligned_i64(0);
        },
        _ if matches!(name, "instr_660FD0" | "instr_F20FD0") =>
            codegen::gen_modrm_resolve_safe_read128_aligned(ctx, modrm, scratch),
        _ => codegen::gen_modrm_resolve_safe_read128(ctx, modrm, scratch),
    }
    if let Op::CompareFlags(double) = op {
        compare_flags(ctx, scratch, dst, double);
    } else if let Op::Convert = op {
        convert(ctx, name, scratch, dst);
    } else if let Op::Float(double, scalar, opcode) = op {
        float_emit(ctx, name, scratch, dst, double, scalar, opcode);
    } else {
        emit(
            ctx,
            op,
            scratch,
            address(dst, mmx),
            if mmx { 8 } else { 16 },
            source_bytes,
        );
    }
    if mmx {
        crate::jit_instructions::mmx_finish(ctx, Some(dst));
    }
    true
}

/// Immediate counts are guest constants, so out-of-range handling is decided
/// while compiling, rather than relying on Wasm's modulo-count shifts.
pub fn shift_immediate(
    ctx: &mut JitContext,
    r: u32,
    count: u32,
    mmx: bool,
    bits: u32,
    kind: u32,
) -> bool {
    if !cfg!(target_feature = "simd128") {
        return false;
    }
    let dst = address(r, mmx);
    ctx.builder.const_i32(dst as i32);
    if bits == 128 {
        load(ctx, dst, 16);
        ctx.builder.simd_zero();
        let mut lanes = [16; 16];
        for i in 0..16 {
            let index = if kind == 3 { i as i32 + count as i32 } else { i as i32 - count as i32 };
            if (0..16).contains(&index) {
                lanes[i] = index as u8;
            }
        }
        ctx.builder.simd_shuffle(lanes);
    } else if count >= bits && kind != 4 {
        ctx.builder.simd_zero();
    } else {
        load(ctx, dst, if mmx { 8 } else { 16 });
        ctx.builder.const_i32(count.min(bits - 1) as i32);
        let base = match bits {
            16 => 0x8B,
            32 => 0xAB,
            64 => 0xCB,
            _ => unreachable!(),
        };
        ctx.builder.simd(
            base + match kind {
                6 => 0,
                4 => 1,
                2 => 2,
                _ => unreachable!(),
            },
        );
    }
    if mmx {
        ctx.builder.simd_lane(0x1D, 0);
        ctx.builder.store_aligned_i64(0);
    } else {
        ctx.builder.simd_memory(0x0B, 2);
    }
    if mmx {
        crate::jit_instructions::mmx_finish(ctx, Some(r));
    }
    true
}

pub fn shuffle_supported(name: &str) -> bool {
    cfg!(target_feature = "simd128")
        && matches!(
            name,
            "instr_0F70"
                | "instr_660F70"
                | "instr_F20F70"
                | "instr_F30F70"
                | "instr_0FC6"
                | "instr_660FC6"
                | "instr_0FC2"
                | "instr_660FC2"
                | "instr_F20FC2"
                | "instr_F30FC2"
        )
}
fn shuffle(ctx: &mut JitContext, name: &str, src: u32, dst: u32, imm: u32) {
    if name.ends_with("C2") {
        compare(ctx, name, src, dst, imm);
        return;
    }
    let mmx = name == "instr_0F70";
    let two = name.ends_with("C6");
    let bytes = if mmx { 8 } else { 16 };
    ctx.builder.const_i32(dst as i32);
    load(ctx, if two { dst } else { src }, bytes);
    load(ctx, src, bytes);
    let mut lanes = [0; 16];
    for i in 0..16u32 {
        lanes[i as usize] = match name {
            "instr_0F70" => ((imm >> (2 * (i / 2 % 4)) & 3) * 2 + i % 2) as u8,
            "instr_660F70" => ((imm >> (2 * (i / 4)) & 3) * 4 + i % 4) as u8,
            "instr_F20F70" if i < 8 => ((imm >> (2 * (i / 2)) & 3) * 2 + i % 2) as u8,
            "instr_F30F70" if i >= 8 => (8 + (imm >> (2 * ((i - 8) / 2)) & 3) * 2 + i % 2) as u8,
            "instr_0FC6" => {
                ((imm >> (2 * (i / 4)) & 3) * 4 + i % 4 + if i >= 8 { 16 } else { 0 }) as u8
            },
            "instr_660FC6" => {
                ((imm >> (i / 8) & 1) * 8 + i % 8 + if i >= 8 { 16 } else { 0 }) as u8
            },
            _ => i as u8,
        };
    }
    ctx.builder.simd_shuffle(lanes);
    if mmx {
        ctx.builder.simd_lane(0x1D, 0);
        ctx.builder.store_aligned_i64(0);
    } else {
        ctx.builder.simd_memory(0x0B, 2);
    }
}
pub fn shuffle_register(ctx: &mut JitContext, name: &str, src: u32, dst: u32, imm: u32) -> bool {
    if !shuffle_supported(name) {
        return false;
    }
    let mmx = name == "instr_0F70";
    shuffle(ctx, name, address(src, mmx), address(dst, mmx), imm);
    if mmx {
        crate::jit_instructions::mmx_finish(ctx, Some(dst));
    }
    true
}
pub fn shuffle_memory(ctx: &mut JitContext, name: &str, modrm: ModrmByte, dst: u32, imm: u32) {
    let mmx = name == "instr_0F70";
    let scratch = gp::sse_scratch_register as u32;
    if name == "instr_F30FC2" {
        ctx.builder.const_i32(scratch as i32);
        codegen::gen_modrm_resolve_safe_read32(ctx, modrm);
        ctx.builder.store_aligned_i32(0);
    } else if mmx || name == "instr_F20FC2" {
        ctx.builder.const_i32(scratch as i32);
        codegen::gen_modrm_resolve_safe_read64(ctx, modrm);
        ctx.builder.store_aligned_i64(0);
    } else {
        codegen::gen_modrm_resolve_safe_read128(ctx, modrm, scratch);
    }
    shuffle(ctx, name, scratch, address(dst, mmx), imm);
    if mmx {
        crate::jit_instructions::mmx_finish(ctx, Some(dst));
    }
}

// Keep the existing core's floating-point behavior. In particular, NaN payload
// selection is implementation-dependent in Wasm SIMD, so NaN results use the
// original helper. Scalar instructions write only their low lane.
fn float_emit(
    ctx: &mut JitContext,
    name: &str,
    src: u32,
    r: u32,
    double: bool,
    scalar: bool,
    opcode: u8,
) {
    let dst = address(r, false);
    let bytes = if scalar {
        if double {
            8
        } else {
            4
        }
    } else {
        16
    };
    let base = if double { 0xF0 } else { 0xE4 };
    match opcode {
        0xD0 => {
            load(ctx, dst, 16);
            load(ctx, src, 16);
            ctx.builder.simd(base + 1);
            load(ctx, dst, 16);
            load(ctx, src, 16);
            ctx.builder.simd(base);
            ctx.builder.simd_shuffle(if double {
                [0, 1, 2, 3, 4, 5, 6, 7, 24, 25, 26, 27, 28, 29, 30, 31]
            } else {
                [0, 1, 2, 3, 20, 21, 22, 23, 8, 9, 10, 11, 28, 29, 30, 31]
            });
        },
        0x7C | 0x7D => {
            for high in [false, true] {
                load(ctx, dst, 16);
                load(ctx, src, 16);
                let lanes = if double {
                    if high {
                        [8, 9, 10, 11, 12, 13, 14, 15, 24, 25, 26, 27, 28, 29, 30, 31]
                    } else {
                        [0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23]
                    }
                } else if high {
                    [4, 5, 6, 7, 12, 13, 14, 15, 20, 21, 22, 23, 28, 29, 30, 31]
                } else {
                    [0, 1, 2, 3, 8, 9, 10, 11, 16, 17, 18, 19, 24, 25, 26, 27]
                };
                ctx.builder.simd_shuffle(lanes);
            }
            ctx.builder.simd(base + if opcode == 0x7D { 1 } else { 0 });
        },
        0x51 => {
            load(ctx, src, bytes);
            ctx.builder.simd(base - 1);
        },
        0x52 | 0x53 => {
            ctx.builder.const_i32(0x3F800000);
            ctx.builder.simd(0x11); // i32x4.splat, float 1
            load(ctx, src, bytes);
            if opcode == 0x52 {
                ctx.builder.simd(0xE3);
            }
            ctx.builder.simd(0xE7);
        },
        0x5D | 0x5F => {
            load(ctx, dst, bytes);
            load(ctx, src, bytes);
            load(ctx, dst, bytes);
            load(ctx, src, bytes);
            ctx.builder
                .simd(if double { 0x49 } else { 0x43 } + if opcode == 0x5F { 1 } else { 0 });
            ctx.builder.simd(0x52); // select destination only on strict less/greater
        },
        _ => {
            if matches!(opcode, 0x58 | 0x59) {
                load(ctx, src, bytes);
                load(ctx, dst, bytes);
            } else {
                load(ctx, dst, bytes);
                load(ctx, src, bytes);
            }
            ctx.builder.simd(
                base + match opcode {
                    0x58 => 0,
                    0x5C => 1,
                    0x59 => 2,
                    0x5E => 3,
                    _ => unreachable!(),
                },
            );
        },
    }
    finish_float(ctx, name, src, r, bytes, bytes, double);
}
fn finish_float(
    ctx: &mut JitContext,
    name: &str,
    src: u32,
    r: u32,
    source_bytes: u32,
    bytes: u32,
    double: bool,
) {
    let dst = address(r, false);
    let result = ctx.builder.set_new_local_v128();
    ctx.builder.get_local_v128(&result);
    ctx.builder.get_local_v128(&result);
    ctx.builder.simd(if double { 0x48 } else { 0x42 });
    ctx.builder.simd(if double { 0xC4 } else { 0xA4 });
    if bytes < 16 {
        ctx.builder.const_i32(1);
        ctx.builder.and_i32();
    }
    if ctx.simd_cache_kind.is_some() {
        ctx.builder.if_v128();
        // Only the exceptional branch materializes registers for the original
        // pure arithmetic helper. It cannot change mapping or other XMMs.
        write_cache(ctx);
    } else {
        ctx.builder.if_void();
    }
    match source_bytes {
        4 => {
            ctx.builder.const_i32(src as i32);
            ctx.builder.load_aligned_f32(0);
            ctx.builder.const_i32(r as i32);
            ctx.builder.call_fn2_f32_i32(name);
        },
        8 => {
            ctx.builder.load_fixed_i64(src);
            ctx.builder.const_i32(r as i32);
            ctx.builder.call_fn2_i64_i32(name);
        },
        _ => {
            let scratch = gp::sse_scratch_register as u32;
            if src != scratch {
                for offset in [0, 8] {
                    ctx.builder.const_i32((scratch + offset) as i32);
                    ctx.builder.load_fixed_i64(src + offset);
                    ctx.builder.store_aligned_i64(0);
                }
            }
            ctx.builder.const_i32(scratch as i32);
            ctx.builder.const_i32(r as i32);
            ctx.builder.call_fn2(name);
        },
    }
    if ctx.simd_cache_kind.is_some() {
        // Bypass the compile-time cache after the helper changed the target.
        ctx.builder.const_i32(dst as i32);
        ctx.builder.simd_memory(0, 2);
        ctx.builder.else_();
        ctx.builder.get_local_v128(&result);
        if bytes < 16 {
            load(ctx, dst, 16);
            let mut lanes = [0; 16];
            for i in 0..16 {
                lanes[i] = if i < bytes as usize { i as u8 } else { (16 + i) as u8 };
            }
            ctx.builder.simd_shuffle(lanes);
        }
        ctx.builder.block_end();
        cache_value(ctx, dst, false);
    } else {
        ctx.builder.else_();
        ctx.builder.const_i32(dst as i32);
        ctx.builder.get_local_v128(&result);
        match bytes {
            4 => {
                ctx.builder.simd_lane(0x1B, 0);
                ctx.builder.store_aligned_i32(0);
            },
            8 => {
                ctx.builder.simd_lane(0x1D, 0);
                ctx.builder.store_aligned_i64(0);
            },
            _ => ctx.builder.simd_memory(0x0B, 2),
        }
        ctx.builder.block_end();
    }
    ctx.builder.free_local_v128(result);
}

fn compare(ctx: &mut JitContext, name: &str, src: u32, dst: u32, imm: u32) {
    let double = name.starts_with("instr_66") || name.starts_with("instr_F2");
    let scalar = name.starts_with("instr_F");
    let bytes = if scalar {
        if double {
            8
        } else {
            4
        }
    } else {
        16
    };
    let op = imm & 7;
    let base = if double { 0x47 } else { 0x41 };
    ctx.builder.const_i32(dst as i32);
    if op == 3 || op == 7 {
        load(ctx, dst, bytes);
        load(ctx, dst, bytes);
        ctx.builder.simd(base + 1);
        load(ctx, src, bytes);
        load(ctx, src, bytes);
        ctx.builder.simd(base + 1);
        ctx.builder.simd(0x50);
        if op == 7 {
            ctx.builder.simd(0x4D);
        }
    } else {
        load(ctx, dst, bytes);
        load(ctx, src, bytes);
        ctx.builder.simd(
            base + match op {
                0 => 0,
                1 | 5 => 2,
                2 | 6 => 4,
                4 => 1,
                _ => unreachable!(),
            },
        );
        if op == 5 || op == 6 {
            ctx.builder.simd(0x4D);
        }
    }
    match bytes {
        4 => {
            ctx.builder.simd_lane(0x1B, 0);
            ctx.builder.store_aligned_i32(0);
        },
        8 => {
            ctx.builder.simd_lane(0x1D, 0);
            ctx.builder.store_aligned_i64(0);
        },
        _ => ctx.builder.simd_memory(0x0B, 2),
    }
}

fn convert(ctx: &mut JitContext, name: &str, src: u32, r: u32) {
    let (opcode, source_bytes, result_bytes, double) = match name {
        "instr_0F5A" => (0x5F, 8, 16, true),
        "instr_660F5A" => (0x5E, 16, 16, false),
        "instr_F20F5A" => (0x5E, 8, 4, false),
        "instr_F30F5A" => (0x5F, 4, 8, true),
        "instr_0F5B" => (0xFA, 16, 16, false),
        "instr_F30FE6" => (0xFE, 8, 16, true),
        _ => {
            let double = name.ends_with("E6");
            let trunc = name == "instr_F30F5B" || name == "instr_660FE6";
            load(ctx, src, 16);
            convert_integer(ctx, double, trunc);
            ctx.builder.const_i32(address(r, false) as i32);
            let dst = ctx.builder.set_new_local();
            let value = ctx.builder.set_new_local_v128();
            ctx.builder.get_local(&dst);
            ctx.builder.get_local_v128(&value);
            ctx.builder.simd_memory(0x0B, 2);
            ctx.builder.free_local(dst);
            ctx.builder.free_local_v128(value);
            return;
        },
    };
    load(ctx, src, source_bytes);
    ctx.builder.simd(opcode);
    finish_float(ctx, name, src, r, source_bytes, result_bytes, double);
}
// SSE's invalid conversion result is 0x80000000, unlike Wasm trunc_sat.
// Apply MXCSR round control before the range check, then select that sentinel
// explicitly for NaNs and either overflow direction.
fn convert_integer(ctx: &mut JitContext, double: bool, truncate: bool) {
    let input = ctx.builder.set_new_local_v128();
    if truncate {
        ctx.builder.get_local_v128(&input);
        ctx.builder.simd(if double { 0x7A } else { 0x69 });
    } else {
        ctx.builder.load_fixed_i32(gp::mxcsr as u32);
        ctx.builder.const_i32(13);
        ctx.builder.shr_u_i32();
        ctx.builder.const_i32(3);
        ctx.builder.and_i32();
        let mode = ctx.builder.set_new_local();
        for (i, op) in (if double { [0x94, 0x75, 0x74, 0x7A] } else { [0x6A, 0x68, 0x67, 0x69] })
            .iter()
            .enumerate()
        {
            if i < 3 {
                ctx.builder.get_local(&mode);
                ctx.builder.const_i32(i as i32);
                ctx.builder.eq_i32();
                ctx.builder.if_v128();
            }
            ctx.builder.get_local_v128(&input);
            ctx.builder.simd(*op);
            if i < 3 {
                ctx.builder.else_();
            }
        }
        for _ in 0..3 {
            ctx.builder.block_end();
        }
        ctx.builder.free_local(mode);
    }
    let rounded = ctx.builder.set_new_local_v128();
    // Rounded >= INT_MIN && rounded < INT_MAX + 1 (NaNs fail both comparisons).
    ctx.builder.get_local_v128(&rounded);
    if double {
        ctx.builder.const_i64(0xC1E0000000000000u64 as i64);
        ctx.builder.simd(0x12);
    } else {
        ctx.builder.const_i32(0xCF000000u32 as i32);
        ctx.builder.simd(0x11);
    }
    ctx.builder.simd(if double { 0x4C } else { 0x46 });
    ctx.builder.get_local_v128(&rounded);
    if double {
        ctx.builder.const_i64(0x41E0000000000000);
        ctx.builder.simd(0x12);
    } else {
        ctx.builder.const_i32(0x4F000000);
        ctx.builder.simd(0x11);
    }
    ctx.builder.simd(if double { 0x49 } else { 0x43 });
    ctx.builder.simd(0x4E);
    if double {
        ctx.builder.simd_zero();
        ctx.builder
            .simd_shuffle([0, 1, 2, 3, 8, 9, 10, 11, 16, 17, 18, 19, 20, 21, 22, 23]);
    }
    let valid = ctx.builder.set_new_local_v128();
    ctx.builder.get_local_v128(&rounded);
    ctx.builder.simd(if double { 0xFC } else { 0xF8 });
    ctx.builder.const_i32(i32::MIN);
    ctx.builder.simd(0x11);
    ctx.builder.get_local_v128(&valid);
    ctx.builder.simd(0x52);
    if double {
        ctx.builder.simd_zero();
        ctx.builder
            .simd_shuffle([0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23]);
    }
    ctx.builder.free_local_v128(input);
    ctx.builder.free_local_v128(rounded);
    ctx.builder.free_local_v128(valid);
}

pub fn cross_convert(
    ctx: &mut JitContext,
    src: u32,
    dst: u32,
    double: bool,
    to_mmx: bool,
    truncate: bool,
    memory: bool,
) {
    if to_mmx {
        load(ctx, src, if double { 16 } else { 8 });
        convert_integer(ctx, double, truncate);
        let result = ctx.builder.set_new_local_v128();
        ctx.builder.const_i32(address(dst, true) as i32);
        ctx.builder.get_local_v128(&result);
        ctx.builder.simd_lane(0x1D, 0);
        ctx.builder.store_aligned_i64(0);
        ctx.builder.free_local_v128(result);
        crate::jit_instructions::mmx_finish(ctx, Some(dst));
    } else {
        ctx.builder.const_i32(address(dst, false) as i32);
        load(ctx, src, 8);
        ctx.builder.simd(if double { 0xFE } else { 0xFA });
        if double {
            ctx.builder.simd_memory(0x0B, 2);
        } else {
            ctx.builder.simd_lane(0x1D, 0);
            ctx.builder.store_aligned_i64(0);
        }
        // Preserve the existing CVTPI2PD memory form's x87 state behavior.
        if !double || !memory {
            crate::jit_instructions::mmx_finish(ctx, None);
        }
    }
}
pub fn integer_to_scalar(ctx: &mut JitContext, dst: u32, double: bool) {
    ctx.builder.simd(0x11);
    ctx.builder.simd(if double { 0xFE } else { 0xFA });
    let result = ctx.builder.set_new_local_v128();
    ctx.builder.const_i32(address(dst, false) as i32);
    ctx.builder.get_local_v128(&result);
    if double {
        ctx.builder.simd_lane(0x1D, 0);
        ctx.builder.store_aligned_i64(0);
    } else {
        ctx.builder.simd_lane(0x1B, 0);
        ctx.builder.store_aligned_i32(0);
    }
    ctx.builder.free_local_v128(result);
}
pub fn scalar_to_integer(ctx: &mut JitContext, src: u32, dst: u32, double: bool, truncate: bool) {
    load(ctx, src, if double { 8 } else { 4 });
    convert_integer(ctx, double, truncate);
    ctx.builder.simd_lane(0x1B, 0);
    codegen::gen_set_reg32(ctx, dst);
}

fn compare_flags(ctx: &mut JitContext, src: u32, r: u32, double: bool) {
    let dst = address(r, false);
    let bytes = if double { 8 } else { 4 };
    let base = if double { 0x47 } else { 0x41 };
    ctx.builder.const_i32(gp::flags as i32);
    ctx.builder.load_fixed_i32(gp::flags as u32);
    ctx.builder.const_i32(!crate::cpu::cpu::FLAGS_ALL);
    ctx.builder.and_i32();
    for (op, flag) in [(0, 0x40), (2, 1)] {
        load(ctx, dst, bytes);
        load(ctx, src, bytes);
        ctx.builder.simd(base + op);
        ctx.builder.simd_lane(0x1B, 0);
        ctx.builder.const_i32(flag);
        ctx.builder.and_i32();
        ctx.builder.or_i32();
    }
    load(ctx, dst, bytes);
    load(ctx, dst, bytes);
    ctx.builder.simd(base + 1);
    load(ctx, src, bytes);
    load(ctx, src, bytes);
    ctx.builder.simd(base + 1);
    ctx.builder.simd(0x50);
    ctx.builder.simd_lane(0x1B, 0);
    ctx.builder.const_i32(0x45);
    ctx.builder.and_i32();
    ctx.builder.or_i32();
    ctx.builder.store_aligned_i32(0);
    ctx.builder.const_i32(gp::flags_changed as i32);
    ctx.builder.const_i32(0);
    ctx.builder.store_aligned_i32(0);
}
