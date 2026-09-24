//! Inlined fast-math x87 against the shared f64 shadow cache.
//!
//! Semantics match `cpu::fpu`'s cached paths: operands are read only from
//! full, VALID cache slots, results are written as VALID|DIRTY f64 values
//! (F80 materializes lazily), and TOP/tags/status change as the interpreter's
//! helpers change them. Every check precedes the first CPU write; any failed
//! check runs the canonical `ir_x87_op` instead, from unchanged state.
use super::Emitter;
use crate::cpu::{fpu, global_pointers as gp};
use crate::ir::ids::ValueId;
use crate::ir::mir::memory::RuntimeCall;
use crate::ir::x87::{self, Arithmetic, Native, Operand, Source, Stored};
use crate::wasmgen::wasm_builder::{Label, WasmLocal, WasmLocalF64};

const C1: i32 = 0x200;
const CONDITION_CODES: i32 = 0x4700;

/// Mutable view of the stack registers while one instruction is inlined.
struct Stack {
    top: WasmLocal,
    tags: WasmLocal,
    /// VALID & !tags at entry: slots that hold an exact cached f64.
    ready: WasmLocal,
    written: Vec<(WasmLocal, WasmLocalF64)>,
    tags_changed: bool,
    top_changed: bool,
}

impl Emitter<'_> {
    pub(super) fn x87(
        &mut self,
        opcode: u8,
        modrm: u8,
        inputs: &[ValueId],
        outputs: &[ValueId],
        call: &RuntimeCall,
    ) {
        let Some(native) = x87::native(opcode, modrm).filter(|_| self.cpu)
        else {
            self.x87_fallback(outputs, call);
            return;
        };
        let done = self.w.block_void();
        let slow = self.w.block_void();
        self.w.load_fixed_u8(gp::x87_native_policy as u32);
        self.w.eqz_i32();
        self.w.br_if(slow);
        self.x87_native(native, inputs, outputs, slow);
        self.w.br(done);
        self.w.block_end();
        self.x87_fallback(outputs, call);
        self.w.block_end();
    }

    fn x87_fallback(&mut self, outputs: &[ValueId], call: &RuntimeCall) {
        self.runtime_call(call);
        if outputs.is_empty() {
            self.w.drop_();
            return;
        }
        let packed = self.w.set_new_local_i64();
        self.w.get_local_i64(&packed);
        self.w.wrap_i64_to_i32();
        self.set(outputs[0]);
        self.w.get_local_i64(&packed);
        self.w.const_i64(32);
        self.w.shr_u_i64();
        self.w.wrap_i64_to_i32();
        self.set(outputs[1]);
        self.w.free_local_i64(packed);
    }

    fn x87_native(&mut self, native: Native, inputs: &[ValueId], outputs: &[ValueId], slow: Label) {
        let [_, valid_address, _] = fpu::x87_cache_addresses();
        self.w.load_fixed_u8(gp::fpu_stack_ptr as u32);
        let top = self.w.set_new_local();
        self.w.load_fixed_u8(gp::fpu_stack_empty as u32);
        let tags = self.w.set_new_local();
        self.w.load_fixed_i32(valid_address);
        self.w.get_local(&tags);
        self.w.const_i32(-1);
        self.w.xor_i32();
        self.w.and_i32();
        let ready = self.w.set_new_local();
        let mut s = Stack {
            top,
            tags,
            ready,
            written: vec![],
            tags_changed: false,
            top_changed: false,
        };
        // Phase 1: checks and results in locals. Phase 2 (commit) writes CPU state.
        let mut status: Option<WasmLocal> = None;
        let mut pops = 0;
        let mut clear_c1 = false;
        match native {
            Native::Push(source) => {
                let value = self.x87_source(&s, source, inputs, slow);
                let slot = self.x87_slot(&s, 7);
                // The destination must be empty: overflow sets flags in F80.
                self.w.get_local(&s.tags);
                self.w.get_local(&slot);
                self.w.shr_u_i32();
                self.w.const_i32(1);
                self.w.and_i32();
                self.w.eqz_i32();
                self.w.br_if(slow);
                self.w.get_local(&slot);
                self.w.set_local(&s.top);
                s.top_changed = true;
                self.x87_clear_tag(&mut s, &slot);
                s.written.push((slot, value));
                clear_c1 = true;
            },
            Native::Arithmetic {
                op,
                source,
                target,
                pops: n,
            } => {
                let x = self.x87_read(&s, 0, slow);
                let y = self.x87_source(&s, source, inputs, slow);
                let (a, b) = match op {
                    Arithmetic::SubR | Arithmetic::DivR => (&y, &x),
                    _ => (&x, &y),
                };
                self.w.get_local_f64(a);
                self.w.get_local_f64(b);
                self.w.arithmetic_f64(match op {
                    Arithmetic::Add => 0,
                    Arithmetic::Sub | Arithmetic::SubR => 1,
                    Arithmetic::Mul => 2,
                    Arithmetic::Div | Arithmetic::DivR => 3,
                });
                let result = self.w.set_new_local_f64();
                self.w.free_local_f64(x);
                self.w.free_local_f64(y);
                let slot = self.x87_slot(&s, target);
                s.written.push((slot, result));
                pops = n;
            },
            Native::Compare { source, pops: n } => {
                let x = self.x87_read(&s, 0, slow);
                let y = self.x87_source(&s, source, inputs, slow);
                // Unordered compares raise #IA in F80; leave NaNs to it.
                for v in [&x, &y] {
                    self.w.get_local_f64(v);
                    self.w.get_local_f64(v);
                    self.w.ne_f64();
                    self.w.br_if(slow);
                }
                self.w.load_fixed_u16(gp::fpu_status_word as u32);
                self.w.const_i32(!CONDITION_CODES);
                self.w.and_i32();
                self.w.get_local_f64(&x);
                self.w.get_local_f64(&y);
                self.w.compare_f64(1); // C0: x < y
                self.w.const_i32(8);
                self.w.shl_i32();
                self.w.or_i32();
                self.w.get_local_f64(&x);
                self.w.get_local_f64(&y);
                self.w.compare_f64(2); // C3: x == y
                self.w.const_i32(14);
                self.w.shl_i32();
                self.w.or_i32();
                status = Some(self.w.set_new_local());
                self.w.free_local_f64(x);
                self.w.free_local_f64(y);
                pops = n;
            },
            Native::Exchange(r) => {
                let a = self.x87_read(&s, 0, slow);
                let b = self.x87_read(&s, r, slow);
                let slot = self.x87_slot(&s, 0);
                s.written.push((slot, b));
                let slot = self.x87_slot(&s, r);
                s.written.push((slot, a));
            },
            Native::Copy { r, pops: n } => {
                let value = self.x87_read(&s, 0, slow);
                let slot = self.x87_slot(&s, r);
                self.x87_clear_tag(&mut s, &slot);
                s.written.push((slot, value));
                pops = n;
            },
            Native::Unary { negate } => {
                let value = self.x87_read(&s, 0, slow);
                self.w.get_local_f64(&value);
                self.w.unary_f64(negate);
                self.w.set_local_f64(&value);
                let slot = self.x87_slot(&s, 0);
                s.written.push((slot, value));
            },
            Native::Free { r, pops: n } => {
                let slot = self.x87_slot(&s, r);
                self.w.get_local(&s.tags);
                self.w.const_i32(1);
                self.w.get_local(&slot);
                self.w.shl_i32();
                self.w.or_i32();
                self.w.set_local(&s.tags);
                s.tags_changed = true;
                self.w.free_local(slot);
                pops = n;
            },
            Native::Store { to, pops: n } => {
                let value = self.x87_read(&s, 0, slow);
                self.x87_store(to, &value, outputs, slow);
                self.w.free_local_f64(value);
                pops = n;
            },
            Native::StoreControl => {
                self.w.load_fixed_u16(gp::fpu_control_word as u32);
                self.set(outputs[0]);
                self.w.const_i32(0);
                self.set(outputs[1]);
            },
            Native::StoreStatus => {
                self.w.load_fixed_u16(gp::fpu_status_word as u32);
                self.w.const_i32(!(7 << 11));
                self.w.and_i32();
                self.w.get_local(&s.top);
                self.w.const_i32(11);
                self.w.shl_i32();
                self.w.or_i32();
                self.set(outputs[0]);
                self.w.const_i32(0);
                self.set(outputs[1]);
            },
        }
        for _ in 0..pops {
            // fpu_pop: tag the old TOP empty, then TOP += 1.
            self.w.get_local(&s.tags);
            self.w.const_i32(1);
            self.w.get_local(&s.top);
            self.w.shl_i32();
            self.w.or_i32();
            self.w.set_local(&s.tags);
            self.w.get_local(&s.top);
            self.w.const_i32(1);
            self.w.add_i32();
            self.w.const_i32(7);
            self.w.and_i32();
            self.w.set_local(&s.top);
            s.tags_changed = true;
            s.top_changed = true;
        }
        self.x87_commit(s, status, clear_c1);
    }

    /// ST(r)'s physical slot: (TOP + r) & 7.
    fn x87_slot(&mut self, s: &Stack, r: u8) -> WasmLocal {
        self.w.get_local(&s.top);
        self.w.const_i32(r as i32);
        self.w.add_i32();
        self.w.const_i32(7);
        self.w.and_i32();
        self.w.set_new_local()
    }

    /// ST(r) from a full, exactly cached slot; anything else takes `slow`.
    fn x87_read(&mut self, s: &Stack, r: u8, slow: Label) -> WasmLocalF64 {
        let [values, _, _] = fpu::x87_cache_addresses();
        let slot = self.x87_slot(s, r);
        self.w.get_local(&s.ready);
        self.w.get_local(&slot);
        self.w.shr_u_i32();
        self.w.const_i32(1);
        self.w.and_i32();
        self.w.eqz_i32();
        self.w.br_if(slow);
        self.w.get_local(&slot);
        self.w.const_i32(3);
        self.w.shl_i32();
        self.w.load_aligned_f64(values);
        self.w.free_local(slot);
        self.w.set_new_local_f64()
    }

    fn x87_clear_tag(&mut self, s: &mut Stack, slot: &WasmLocal) {
        self.w.get_local(&s.tags);
        self.w.const_i32(1);
        self.w.get_local(slot);
        self.w.shl_i32();
        self.w.const_i32(-1);
        self.w.xor_i32();
        self.w.and_i32();
        self.w.set_local(&s.tags);
        s.tags_changed = true;
    }

    /// A memory operand, register or constant as an f64. NaN memory operands
    /// take `slow` (the F80 conversion may quiet them and raise #IA).
    fn x87_source(
        &mut self,
        s: &Stack,
        source: Source,
        inputs: &[ValueId],
        slow: Label,
    ) -> WasmLocalF64 {
        match source {
            Source::Register(r) => return self.x87_read(s, r, slow),
            Source::Constant(one) => {
                self.w.const_f64(if one { 1.0 } else { 0.0 });
                return self.w.set_new_local_f64();
            },
            Source::Memory(operand) => match operand {
                Operand::F32 => {
                    self.get(inputs[0]);
                    self.w.reinterpret_i32_as_f32();
                    self.w.promote_f32_to_f64();
                },
                Operand::F64 | Operand::I64 => {
                    self.get(inputs[1]);
                    self.w.extend_unsigned_i32_to_i64();
                    self.w.const_i64(32);
                    self.w.shl_i64();
                    self.get(inputs[0]);
                    self.w.extend_unsigned_i32_to_i64();
                    self.w.or_i64();
                    if operand == Operand::F64 {
                        self.w.reinterpret_i64_as_f64();
                    }
                    else {
                        // Only |v| <= 2^53 converts exactly to binary64.
                        let integer = self.w.set_new_local_i64();
                        self.w.get_local_i64(&integer);
                        self.w.const_i64(1 << 53);
                        self.w.add_i64();
                        self.w.const_i64(1 << 54);
                        self.w.gtu_i64();
                        self.w.br_if(slow);
                        self.w.get_local_i64(&integer);
                        self.w.convert_i64_to_f64();
                        self.w.free_local_i64(integer);
                    }
                },
                Operand::I32 => {
                    self.get(inputs[0]);
                    self.w.convert_i32_to_f64();
                },
                Operand::I16 => {
                    self.get(inputs[0]);
                    self.w.const_i32(16);
                    self.w.shl_i32();
                    self.w.const_i32(16);
                    self.w.shr_s_i32();
                    self.w.convert_i32_to_f64();
                },
            },
        }
        let value = self.w.set_new_local_f64();
        if matches!(source, Source::Memory(Operand::F32 | Operand::F64)) {
            self.w.get_local_f64(&value);
            self.w.get_local_f64(&value);
            self.w.ne_f64();
            self.w.br_if(slow);
        }
        value
    }

    /// FST(P) m32/m64 and FIST(P)/FISTTP bits. Results needing F80 rounding
    /// flags (tiny f32, NaN, integer overflow) or directed f32 rounding take `slow`.
    fn x87_store(&mut self, to: Stored, value: &WasmLocalF64, outputs: &[ValueId], slow: Label) {
        match to {
            Stored::F64 => {
                self.w.get_local_f64(value);
                self.w.reinterpret_f64_as_i64();
                self.x87_set_wide(outputs);
            },
            Stored::F32 => {
                self.w.load_fixed_u16(gp::fpu_control_word as u32);
                self.w.const_i32(0xC00);
                self.w.and_i32();
                self.w.br_if(slow);
                self.w.get_local_f64(value);
                self.w.demote_f64_to_f32();
                self.w.reinterpret_f32_as_i32();
                let bits = self.w.set_new_local();
                // Accept zero or a strictly normal/infinite result: no underflow.
                self.w.get_local(&bits);
                self.w.const_i32(0x7FFFFFFF);
                self.w.and_i32();
                self.w.const_i32(0x00800000);
                self.w.gtu_i32();
                self.w.get_local_f64(value);
                self.w.const_f64(0.0);
                self.w.compare_f64(2);
                self.w.or_i32();
                self.w.get_local(&bits);
                self.w.const_i32(0x7FFFFFFF);
                self.w.and_i32();
                self.w.const_i32(0x7F800000);
                self.w.leu_i32();
                self.w.and_i32();
                self.w.eqz_i32();
                self.w.br_if(slow);
                self.w.get_local(&bits);
                self.set(outputs[0]);
                self.w.const_i32(0);
                self.set(outputs[1]);
                self.w.free_local(bits);
            },
            Stored::Integer { bytes, truncate } => {
                if truncate {
                    self.w.get_local_f64(value);
                    self.w.round_f64(3);
                }
                else {
                    // RC 0..3: nearest, down, up, toward zero.
                    self.w.load_fixed_u16(gp::fpu_control_word as u32);
                    self.w.const_i32(10);
                    self.w.shr_u_i32();
                    self.w.const_i32(3);
                    self.w.and_i32();
                    let rc = self.w.set_new_local();
                    self.w.get_local_f64(value);
                    self.w.round_f64(0);
                    let rounded = self.w.set_new_local_f64();
                    for mode in 1..4 {
                        // select(a, b, c) = c ? a : b
                        self.w.get_local_f64(value);
                        self.w.round_f64(mode);
                        self.w.get_local_f64(&rounded);
                        self.w.get_local(&rc);
                        self.w.const_i32(mode as i32);
                        self.w.eq_i32();
                        self.w.select();
                        self.w.set_local_f64(&rounded);
                    }
                    self.w.get_local_f64(&rounded);
                    self.w.free_local_f64(rounded);
                    self.w.free_local(rc);
                }
                let rounded = self.w.set_new_local_f64();
                // In range after rounding; NaN fails both comparisons.
                let (low, high, below) = match bytes {
                    2 => (-32768.0, 32767.0, false),
                    4 => (-2147483648.0, 2147483647.0, false),
                    _ => (-9223372036854775808.0, 9223372036854775808.0, true),
                };
                self.w.get_local_f64(&rounded);
                self.w.const_f64(low);
                self.w.ge_f64();
                self.w.get_local_f64(&rounded);
                self.w.const_f64(high);
                if below {
                    self.w.compare_f64(1);
                }
                else {
                    self.w.le_f64();
                }
                self.w.and_i32();
                self.w.eqz_i32();
                self.w.br_if(slow);
                self.w.get_local_f64(&rounded);
                if bytes == 8 {
                    self.w.trunc_f64_to_i64();
                    self.x87_set_wide(outputs);
                }
                else {
                    self.w.trunc_f64_to_i32();
                    if bytes == 2 {
                        self.w.const_i32(0xFFFF);
                        self.w.and_i32();
                    }
                    self.set(outputs[0]);
                    self.w.const_i32(0);
                    self.set(outputs[1]);
                }
                self.w.free_local_f64(rounded);
            },
        }
    }

    fn x87_set_wide(&mut self, outputs: &[ValueId]) {
        let wide = self.w.set_new_local_i64();
        self.w.get_local_i64(&wide);
        self.w.wrap_i64_to_i32();
        self.set(outputs[0]);
        self.w.get_local_i64(&wide);
        self.w.const_i64(32);
        self.w.shr_u_i64();
        self.w.wrap_i64_to_i32();
        self.set(outputs[1]);
        self.w.free_local_i64(wide);
    }

    fn x87_commit(&mut self, s: Stack, status: Option<WasmLocal>, clear_c1: bool) {
        let [values, valid, dirty] = fpu::x87_cache_addresses();
        if !s.written.is_empty() {
            let mut mask: Option<WasmLocal> = None;
            for (slot, value) in &s.written {
                self.w.get_local(slot);
                self.w.const_i32(3);
                self.w.shl_i32();
                self.w.get_local_f64(value);
                self.w.store_aligned_f64(values);
                self.w.const_i32(1);
                self.w.get_local(slot);
                self.w.shl_i32();
                if let Some(mask) = &mask {
                    self.w.get_local(mask);
                    self.w.or_i32();
                    self.w.set_local(mask);
                }
                else {
                    mask = Some(self.w.set_new_local());
                }
            }
            let mask = mask.unwrap();
            for address in [valid, dirty] {
                self.w.const_i32(address as i32);
                self.w.load_fixed_i32(address);
                self.w.get_local(&mask);
                self.w.or_i32();
                self.w.store_aligned_i32(0);
            }
            self.w.free_local(mask);
        }
        for (slot, value) in s.written {
            self.w.free_local(slot);
            self.w.free_local_f64(value);
        }
        if s.top_changed {
            self.w.const_i32(gp::fpu_stack_ptr as i32);
            self.w.get_local(&s.top);
            self.w.store_u8(0);
        }
        if s.tags_changed {
            self.w.const_i32(gp::fpu_stack_empty as i32);
            self.w.get_local(&s.tags);
            self.w.store_u8(0);
        }
        if let Some(status) = status {
            self.w.const_i32(gp::fpu_status_word as i32);
            self.w.get_local(&status);
            self.w.store_aligned_u16(0);
            self.w.free_local(status);
        }
        else if clear_c1 {
            self.w.const_i32(gp::fpu_status_word as i32);
            self.w.load_fixed_u16(gp::fpu_status_word as u32);
            self.w.const_i32(!C1);
            self.w.and_i32();
            self.w.store_aligned_u16(0);
        }
        self.w.free_local(s.top);
        self.w.free_local(s.tags);
        self.w.free_local(s.ready);
    }
}
