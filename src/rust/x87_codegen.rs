// Bounded register-only regions. Native locals never cross an instruction
// with guest memory access, a helper observer, or a basic-block exit.
use crate::cpu::{memory, global_pointers};
use crate::cpu::cpu::FLAGS_ALL;
use crate::softfloat::F80;
use crate::jit::{Instruction, JitContext};
use crate::wasmgen::wasm_builder::WasmLocalF64;

#[derive(Clone, Copy)]
enum Op {
    Arithmetic { op: u8, r: usize, reverse: bool, target_r: bool, pop: bool },
    Compare { r: usize, pops: usize, integer: bool, quiet: bool },
    Store { r: usize, pop: bool },
    Constant(bool),
    Unary(bool),
    Round,
    Rotate(bool),
    Free(usize),
    Nop,
    Exchange(usize),
    Push(usize),
}

fn decode(address: u32) -> Option<Op> {
    let opcode = memory::read8(address);
    let byte = memory::read8(address + 1) as usize;
    if byte < 0xC0 { return None; }
    let r = byte & 7;
    if opcode == 0xD9 {
        return match byte {
            0xC0..=0xC7 => Some(Op::Push(r)), 0xC8..=0xCF => Some(Op::Exchange(r)),
            0xFC => Some(Op::Round),
            0xD0 => Some(Op::Nop), 0xE0 | 0xE1 => Some(Op::Unary(byte == 0xE0)),
            0xE8 | 0xEE => Some(Op::Constant(byte == 0xE8)),
            0xF6 | 0xF7 => Some(Op::Rotate(byte == 0xF7)), _ => None,
        };
    }
    if matches!(opcode, 0xDB | 0xDF) && matches!(byte & 0xF8, 0xE8 | 0xF0) {
        return Some(Op::Compare { r, pops: (opcode == 0xDF) as usize, integer: true, quiet: byte & 0xF8 == 0xE8 });
    }
    if opcode == 0xDE && byte == 0xD9 {
        return Some(Op::Compare { r: 1, pops: 2, integer: false, quiet: false });
    }
    if matches!(opcode, 0xD8 | 0xDC) && matches!(byte & 0xF8, 0xD0 | 0xD8) {
        return Some(Op::Compare { r, pops: (byte & 8 != 0) as usize, integer: false, quiet: false });
    }
    if opcode == 0xDA && byte == 0xE9 {
        return Some(Op::Compare { r: 1, pops: 2, integer: false, quiet: true });
    }
    if opcode == 0xDD {
        return match byte & 0xF8 {
            0xC0 => Some(Op::Free(r)),
            0xD0 | 0xD8 => Some(Op::Store { r, pop: byte & 8 != 0 }),
            0xE0 | 0xE8 => Some(Op::Compare { r, pops: (byte & 8 != 0) as usize, integer: false, quiet: true }),
            _ => None,
        };
    }
    if !matches!(opcode, 0xD8 | 0xDC | 0xDE) { return None; }
    let group = (byte >> 3) & 7;
    let op = match group { 0 => 0, 1 => 2, 4 | 5 => 1, 6 | 7 => 3, _ => return None };
    Some(Op::Arithmetic { op, r, reverse: group == 5 || group == 7,
        target_r: opcode != 0xD8, pop: opcode == 0xDE })
}

#[derive(Clone, Default)]
struct Plan {
    ops: Vec<Op>,
    top: usize,
    tags: [Option<bool>; 8],
    full: u32,
    empty: u32,
    dirty: u32,
    counts: [u32; 4],
    reads: u32,
    comparisons: u32,
    pushed: bool,
}
impl Plan {
    fn read(&mut self, r: usize) -> bool {
        self.reads += 1;
        match self.tags[r] {
            Some(false) => false,
            Some(true) => true,
            None => { self.full |= 1 << r; true },
        }
    }
    fn append(&mut self, op: Op) -> bool {
        let r = match op {
            Op::Arithmetic { r, .. } | Op::Compare { r, .. } | Op::Exchange(r) | Op::Push(r)
                | Op::Store { r, .. } | Op::Free(r) => (self.top + r) & 7,
            _ => self.top,
        };
        if matches!(op, Op::Arithmetic { .. } | Op::Compare { .. } | Op::Exchange(_) | Op::Push(_) | Op::Unary(_) | Op::Round)
            && !self.read(r) { return false; }
        match op {
            Op::Arithmetic { op, target_r, pop, .. } => {
                if !self.read(self.top) { return false; }
                self.dirty |= 1 << if target_r { r } else { self.top };
                self.counts[op as usize] += 1;
                if pop { self.tags[self.top] = Some(false); self.top = (self.top + 1) & 7; }
            },
            Op::Compare { pops, .. } => {
                if !self.read(self.top) { return false; }
                self.comparisons += 1;
                for _ in 0..pops {
                    self.tags[self.top] = Some(false);
                    self.top = (self.top + 1) & 7;
                }
            },
            Op::Store { pop, .. } => {
                if !self.read(self.top) { return false; }
                self.dirty |= 1 << r;
                self.tags[r] = Some(true);
                if pop { self.tags[self.top] = Some(false); self.top = (self.top + 1) & 7; }
            },
            Op::Unary(_) | Op::Round => { self.dirty |= 1 << self.top; },
            Op::Rotate(increment) => {
                self.top = (self.top + if increment { 1 } else { 7 }) & 7;
                self.pushed = true; // these instructions also clear C1
            },
            Op::Free(_) => { self.tags[r] = Some(false); },
            Op::Nop => {},
            Op::Exchange(_) => {
                if !self.read(self.top) { return false; }
                self.dirty |= (1 << self.top) | (1 << r);
            },
            Op::Push(_) | Op::Constant(_) => {
                let next = (self.top + 7) & 7;
                match self.tags[next] {
                    Some(true) => return false,
                    None => self.empty |= 1 << next,
                    Some(false) => {},
                }
                self.tags[next] = Some(true);
                self.top = next;
                self.dirty |= 1 << next;
                self.pushed = true;
            },
        }
        if self.full & self.empty != 0 { return false; }
        self.ops.push(op);
        true
    }
}

pub fn try_region(ctx: &mut JitContext) -> bool {
    if cfg!(feature = "profiler") { return false; }
    let start = ctx.cpu.eip;
    // Keep the last instruction in the ordinary path, where EIP and fault
    // bookkeeping are maintained. No region crosses its block or code page.
    if start >= ctx.last_instruction_in_block { return false; }
    if !matches!(memory::read8(start), 0xD8..=0xDF) { return false; }
    let mut plan = Plan::default();
    let mut end = start;
    while end + 2 <= ctx.last_instruction_in_block && plan.ops.len() < 32 {
        let Some(op) = decode(end) else { break; };
        let mut next = plan.clone();
        if !next.append(op) { break; }
        plan = next;
        end += 2;
    }
    if plan.ops.is_empty() { return false; }

    ctx.builder.flush_deferred_stores();
    crate::simd_codegen::flush_cache(ctx);
    crate::codegen::clear_ram_read_cache(ctx);
    crate::codegen::clear_ram_write_cache(ctx);
    ctx.start_of_current_instruction = start;
    crate::codegen::gen_task_switch_test(ctx);
    ctx.builder.const_i32(plan.full as i32);
    ctx.builder.const_i32(plan.empty as i32);
    ctx.builder.call_fn2_ret("fpu_jit_cache_begin");
    let scratch = ctx.builder.tee_new_local();
    ctx.builder.if_void();
    let mut locals: Vec<Option<WasmLocalF64>> = (0..8).map(|_| None).collect();
    for r in 0..8 {
        if (plan.full | plan.dirty) & (1 << r) != 0 {
            ctx.builder.get_local(&scratch);
            ctx.builder.load_aligned_f64(r * 8);
            locals[r as usize] = Some(ctx.builder.set_new_local_f64());
        }
    }
    let mut top = 0;
    for op in &plan.ops {
        match *op {
            Op::Arithmetic { op, r, reverse, target_r, pop } => {
                // Comparisons and ADD/DIV clear SoftFloat flags in instruction
                // order. SUB/MUL preserve them in the existing helper path.
                if plan.comparisons != 0 && (op == 0 || op == 3) {
                    ctx.builder.const_i32(F80::exception_flags_address() as i32);
                    ctx.builder.const_i32(0);
                    ctx.builder.store_u8(0);
                }
                let r = (top + r) & 7;
                let (a, b) = if reverse { (r, top) } else { (top, r) };
                ctx.builder.get_local_f64(locals[a].as_ref().unwrap());
                ctx.builder.get_local_f64(locals[b].as_ref().unwrap());
                ctx.builder.arithmetic_f64(op);
                ctx.builder.set_local_f64(locals[if target_r { r } else { top }].as_ref().unwrap());
                if pop { top = (top + 1) & 7; }
            },
            Op::Compare { r, pops, integer, quiet } => {
                emit_compare(ctx, locals[top].as_ref().unwrap(),
                    locals[(top + r) & 7].as_ref().unwrap(), integer, quiet);
                top = (top + pops) & 7;
            },
            Op::Store { r, pop } => {
                ctx.builder.get_local_f64(locals[top].as_ref().unwrap());
                ctx.builder.set_local_f64(locals[(top + r) & 7].as_ref().unwrap());
                if pop { top = (top + 1) & 7; }
            },
            Op::Round => {
                // FRNDINT follows the guest rounding control, independent of
                // the approximate arithmetic policy and precision control.
                let value = locals[top].as_ref().unwrap();
                for rc in 0..3 {
                    ctx.builder.load_fixed_u16(global_pointers::fpu_control_word as u32);
                    ctx.builder.const_i32(10); ctx.builder.shr_u_i32();
                    ctx.builder.const_i32(3); ctx.builder.and_i32();
                    ctx.builder.const_i32(rc); ctx.builder.eq_i32();
                    ctx.builder.if_void();
                    ctx.builder.get_local_f64(value);
                    ctx.builder.round_f64(rc as u8);
                    ctx.builder.set_local_f64(value);
                    ctx.builder.else_();
                }
                ctx.builder.get_local_f64(value);
                ctx.builder.round_f64(3);
                ctx.builder.set_local_f64(value);
                for _ in 0..3 { ctx.builder.block_end(); }
            },
            Op::Unary(negative) => {
                ctx.builder.get_local_f64(locals[top].as_ref().unwrap());
                ctx.builder.unary_f64(negative);
                ctx.builder.set_local_f64(locals[top].as_ref().unwrap());
            },
            Op::Constant(one) => {
                top = (top + 7) & 7;
                ctx.builder.const_i64(if one { 0x3FF0000000000000 } else { 0 });
                ctx.builder.reinterpret_i64_as_f64();
                ctx.builder.set_local_f64(locals[top].as_ref().unwrap());
            },
            Op::Rotate(increment) => { top = (top + if increment { 1 } else { 7 }) & 7; },
            Op::Free(_) | Op::Nop => {},
            Op::Exchange(r) => {
                let r = (top + r) & 7;
                ctx.builder.get_local_f64(locals[top].as_ref().unwrap());
                ctx.builder.get_local_f64(locals[r].as_ref().unwrap());
                ctx.builder.set_local_f64(locals[top].as_ref().unwrap());
                ctx.builder.set_local_f64(locals[r].as_ref().unwrap());
            },
            Op::Push(r) => {
                ctx.builder.get_local_f64(locals[(top + r) & 7].as_ref().unwrap());
                top = (top + 7) & 7;
                ctx.builder.set_local_f64(locals[top].as_ref().unwrap());
            },
        }
    }
    for r in 0..8 {
        if plan.dirty & (1 << r) != 0 {
            ctx.builder.get_local(&scratch);
            ctx.builder.get_local_f64(locals[r].as_ref().unwrap());
            ctx.builder.reinterpret_f64_as_i64();
            ctx.builder.store_aligned_i64(r as u32 * 8);
        }
    }
    let mut metadata = plan.top as u32 | if plan.pushed { 8 } else { 0 } | (plan.reads << 20);
    for (r, tag) in plan.tags.iter().enumerate() {
        if let Some(full) = tag { metadata |= 1 << (r + if *full { 12 } else { 4 }); }
    }
    let counts = plan.counts.iter().enumerate().fold(0, |bits, (op, count)| bits | (count << (op * 8)));
    ctx.builder.const_i32((plan.dirty | (plan.comparisons << 8)) as i32);
    ctx.builder.const_i32(metadata as i32);
    ctx.builder.const_i32(counts as i32);
    ctx.builder.call_fn3("fpu_jit_cache_commit");
    for local in locals.into_iter().flatten() { ctx.builder.free_local_f64(local); }
    ctx.builder.else_();
    while ctx.cpu.eip < end {
        ctx.start_of_current_instruction = ctx.cpu.eip;
        crate::jit_instructions::jit_instruction(ctx, &mut 0);
    }
    ctx.builder.block_end();
    ctx.builder.free_local(scratch);
    ctx.previous_instruction = Instruction::Other;
    ctx.current_instruction = Instruction::Other;
    true
}

// Ordered comparisons of cached values. The entry guard rejects original
// NaNs; arithmetic can produce quiet NaNs, which ordered FCOM must signal.
// All emitted stores are to emulator state, never faulting guest memory.
fn emit_compare(ctx: &mut JitContext, a: &WasmLocalF64, b: &WasmLocalF64, integer: bool, quiet: bool) {
    for operation in [0, 1, 2] {
        ctx.builder.get_local_f64(a);
        ctx.builder.get_local_f64(b);
        ctx.builder.compare_f64(operation); // greater, less, equal
        ctx.builder.if_i32();
        ctx.builder.const_i32([0, 1, 0x40][operation as usize]);
        ctx.builder.else_();
    }
    ctx.builder.const_i32(0x45); // unordered: ZF/PF/CF
    for _ in 0..3 { ctx.builder.block_end(); }
    let result = ctx.builder.set_new_local();
    if integer {
        ctx.builder.const_i32(global_pointers::flags as i32);
        ctx.builder.load_fixed_i32(global_pointers::flags as u32);
        ctx.builder.const_i32(!FLAGS_ALL);
        ctx.builder.and_i32();
        ctx.builder.get_local(&result);
        ctx.builder.or_i32();
        ctx.builder.store_aligned_i32(0);
        ctx.builder.const_i32(global_pointers::flags_changed as i32);
        ctx.builder.const_i32(0);
        ctx.builder.store_aligned_i32(0);
    }
    ctx.builder.const_i32(global_pointers::fpu_status_word as i32);
    ctx.builder.load_fixed_u16(global_pointers::fpu_status_word as u32);
    if !integer {
        ctx.builder.const_i32(!0x4700); // clear C0/C1/C2/C3
        ctx.builder.and_i32();
        ctx.builder.get_local(&result);
        ctx.builder.const_i32(8);
        ctx.builder.shl_i32(); // CF/PF/ZF -> C0/C2/C3
        ctx.builder.or_i32();
    }
    if !quiet {
        ctx.builder.get_local(&result);
        ctx.builder.const_i32(4);
        ctx.builder.and_i32();
        ctx.builder.const_i32(2);
        ctx.builder.shr_u_i32(); // ordered NaN sets sticky x87 invalid
        ctx.builder.or_i32();
    }
    ctx.builder.store_aligned_u16(0);
    ctx.builder.const_i32(F80::exception_flags_address() as i32);
    if quiet { ctx.builder.const_i32(0); } else {
        ctx.builder.get_local(&result);
        ctx.builder.const_i32(4);
        ctx.builder.and_i32();
        ctx.builder.const_i32(2);
        ctx.builder.shl_i32(); // SoftFloat invalid = 16; clears previous flags
    }
    ctx.builder.store_u8(0);
    ctx.builder.free_local(result);
}

// MMX aliases the physical F80 slots. Cover direct generated MMX accesses,
// including the SSE conversion opcodes with MMX operands and EMMS. Ordinary
// integer branches and memory instructions do not observe the FPU storage.
pub fn prepare_instruction(ctx: &mut JitContext) {
    let start = ctx.cpu.eip;
    let mut a = start;
    let mut simd_prefix = 0;
    while matches!(memory::read8(a), 0x66 | 0x67 | 0xF2 | 0xF3 | 0x26 | 0x2E | 0x36 | 0x3E | 0x64 | 0x65) {
        let byte = memory::read8(a);
        if matches!(byte, 0x66 | 0xF2 | 0xF3) { simd_prefix = byte; }
        a += 1;
        if a - start >= 15 { return; }
    }
    if memory::read8(a) != 0x0F { return; }
    let opcode = memory::read8(a + 1);
    let observes_mmx = match opcode {
        0x77 => true, // EMMS
        0x2A | 0x2C | 0x2D => !matches!(simd_prefix, 0xF2 | 0xF3), // PI conversions
        0xD6 if matches!(simd_prefix, 0xF2 | 0xF3) => true, // MOVDQ2Q / MOVQ2DQ
        0x60..=0x7F | 0xC4 | 0xC5 | 0xD0..=0xFF => simd_prefix == 0,
        _ => false,
    };
    if observes_mmx { ctx.builder.call_fn0("fpu_cache_barrier"); }
}
