use crate::cpu::global_pointers as gp;
use crate::ir::mir::arithmetic::{
    ArithmeticPlan, CompareExchange, Division, QuotientRange, RegisterPair,
};
use crate::ir::mir::call::{CallPlan, Observation};
use crate::ir::mir::control::{Copy, Edge as MirEdge, Source, Terminator as MirTerminator};
use crate::ir::mir::effect::EffectPlan;
use crate::ir::mir::forwarding::Forwarding;
use crate::ir::mir::materialize::{Count, CountMode, Store, Write};
use crate::ir::mir::memory::{
    Argument, MemoryPlan, NativeMemory, RamGuard, RuntimeCall, SlowResult, VectorCombine,
};
use crate::ir::mir::value::{Address, Load, Reading, Step, ValuePlan};
use crate::ir::runtime::entry::CpuEntryKey;
use crate::ir::{ids::*, lowering::CompileError, mir::MirRegion, types::Type};
use crate::wasmgen::wasm_builder::{WasmBuilder, WasmLocal, WasmLocalI64, WasmLocalV128};
#[derive(Clone, Copy)]
pub struct StateLayout {
    pub gpr: u32,
    pub flags: u32,
    pub eip: u32,
    pub committed: u32,
    pub flag_operand: u32,
}
pub struct Artifact {
    pub bytes: Vec<u8>,
    pub locals: usize,
}
enum Local {
    I32(WasmLocal),
    I64(WasmLocalI64),
    V128(WasmLocalV128),
}
struct Emitter<'a> {
    w: WasmBuilder,
    mir: &'a MirRegion,
    locals: Vec<Local>,
    layout: StateLayout,
    cpu: bool,
    accounted: Option<WasmLocal>,
    tlb: Option<WasmLocal>,
    read_cache: Option<(WasmLocal, WasmLocal)>,
    code_pages: &'a [u32],
}
impl Emitter<'_> {
    fn get(&mut self, value: ValueId) {
        self.get_local(self.mir.allocation.value_local[value.index()].unwrap());
    }
    fn get_local(&mut self, slot: usize) {
        match &self.locals[slot] {
            Local::I32(local) => self.w.get_local(local),
            Local::I64(local) => self.w.get_local_i64(local),
            Local::V128(local) => self.w.get_local_v128(local),
        }
    }
    fn set(&mut self, value: ValueId) {
        self.set_local(self.mir.allocation.value_local[value.index()].unwrap());
    }
    fn set_local(&mut self, slot: usize) {
        match &self.locals[slot] {
            Local::I32(local) => self.w.set_local(local),
            Local::I64(local) => self.w.set_local_i64(local),
            Local::V128(local) => self.w.set_local_v128(local),
        }
    }
    fn temporary(&mut self, ty: Type) -> Local {
        if ty == Type::V128 {
            Local::V128(self.w.set_new_local_v128())
        } else if matches!(ty, Type::I64 | Type::RmwTicket) {
            Local::I64(self.w.set_new_local_i64())
        } else {
            Local::I32(self.w.set_new_local())
        }
    }
    fn get_temporary(&mut self, local: &Local) {
        match local {
            Local::I32(l) => self.w.get_local(l),
            Local::I64(l) => self.w.get_local_i64(l),
            Local::V128(l) => self.w.get_local_v128(l),
        }
    }
    fn free_temporary(&mut self, local: Local) {
        match local {
            Local::I32(l) => self.w.free_local(l),
            Local::I64(l) => self.w.free_local_i64(l),
            Local::V128(l) => self.w.free_local_v128(l),
        }
    }
    fn mask(&mut self, ty: Type) {
        if ty == Type::V128 {
            return;
        }
        let bits = if ty == Type::LinearAddress { 32 } else { ty.bits().unwrap() };
        if bits < 32 {
            self.w.const_i32((1i32 << bits) - 1);
            self.w.and_i32();
        }
    }
    fn state_write(&mut self, write: &Write) {
        self.w.const_i32(self.address(write.address) as i32);
        self.value_steps(&write.expression);
        match write.store {
            Store::I32 => self.w.store_aligned_i32(0),
            Store::V128 => self.w.simd_memory(0x0B, 2),
        }
    }
    fn count_value(&mut self, count: &Count) {
        self.w.const_i32(count.snapshot as i32);
        if let Some(base) = count.base {
            self.get(base);
            self.w.add_i32();
        }
    }
    fn count(&mut self, count: &Count) {
        let destination = self.address(count.destination);
        self.w.const_i32(destination as i32);
        self.count_value(count);
        if count.mode == CountMode::Delta {
            self.w.get_local(self.accounted.as_ref().unwrap());
            self.w.sub_i32();
            self.w.load_fixed_i32(destination);
            self.w.add_i32();
        }
        self.w.store_aligned_i32(0);
        if count.mode == CountMode::Delta {
            self.count_value(count);
            self.w.set_local(self.accounted.as_ref().unwrap());
        }
    }
    fn observe_state(&mut self, values: StateId, count: StateId, decoded_next: bool) {
        let states = &self.mir.states;
        let plan = &states[values.index()];
        let materialization = if self.cpu { &plan.cpu } else { &plan.standalone };
        for write in &materialization.writes {
            self.state_write(write);
        }
        let count = &states[count.index()];
        self.count(if self.cpu { &count.cpu.count } else { &count.standalone.count });
        if decoded_next {
            self.state_write(&plan.decoded_next);
        }
    }
    fn state(&mut self, state: StateId) {
        self.observe_state(state, state, false);
    }
    fn copy_edge(&mut self, edge: &MirEdge, pc: &WasmLocal) {
        let mut scratch = Vec::new();
        for copy in &edge.copies {
            match *copy {
                Copy::Save {
                    local,
                    scratch: slot,
                } => {
                    self.get_local(local);
                    scratch.push(self.temporary(edge.scratch[slot]));
                },
                Copy::Move {
                    source,
                    destination,
                } => {
                    match source {
                        Source::Local(slot) => self.get_local(slot),
                        Source::Scratch(slot) => self.get_temporary(&scratch[slot]),
                    }
                    self.set_local(destination);
                },
            }
        }
        for temp in scratch {
            self.free_temporary(temp);
        }
        self.w.const_i32(edge.target.0 as i32);
        self.w.set_local(pc);
    }
    /// Decode the packed CPU adapter result without touching any SSA result slot.
    fn packed_cpu_result(&mut self, result: ValueId, trap_after_fault: bool) {
        let packed = self.w.set_new_local_i64();
        self.w.get_local_i64(&packed);
        self.w.const_i64(32);
        self.w.shr_u_i64();
        self.w.wrap_i64_to_i32();
        let outcome = self.w.set_new_local();
        self.w.get_local(&outcome);
        self.w.const_i32(2);
        self.w.eq_i32();
        self.w.if_void();
        if trap_after_fault {
            self.w.unreachable();
        } else {
            self.w.return_();
        }
        self.w.block_end();
        self.w.get_local(&outcome);
        self.w.if_void();
        self.w.unreachable();
        self.w.block_end();
        self.w.get_local_i64(&packed);
        self.w.wrap_i64_to_i32();
        self.mask(self.mir.value_types[result.index()]);
        self.set(result);
        self.w.free_local(outcome);
        self.w.free_local_i64(packed);
    }
    fn prepare_memory_call(&mut self, state: StateId) {
        self.observe_state(state, state, true);
    }
    fn planned_ram_guard(&mut self, address: ValueId, guard: &RamGuard) -> WasmLocal {
        self.w.get_local(self.tlb.as_ref().unwrap());
        self.get(address);
        self.w.const_i32(12);
        self.w.shr_u_i32();
        self.w.const_i32(2);
        self.w.shl_i32();
        self.w.add_i32();
        self.w.load_aligned_i32(0);
        let entry = self.w.set_new_local();
        self.w.get_local(&entry);
        self.w.const_i32(guard.flags_mask);
        self.w.load_fixed_u8(gp::cpl as u32); // CPL
        self.w.const_i32(3);
        self.w.eq_i32();
        self.w.const_i32(guard.user_mask);
        self.w.mul_i32();
        self.w.or_i32();
        self.w.and_i32();
        self.w.const_i32(guard.required_flags);
        self.w.eq_i32();
        self.get(address);
        self.w.const_i32(4095);
        self.w.and_i32();
        self.w.const_i32(guard.page_offset_limit);
        self.w.ltu_i32();
        self.w.and_i32();
        entry
    }
    fn finish_scalar_store(&mut self, commit: StateId, pointer: &WasmLocal) {
        // Materialize the completed instruction before either returning or
        // continuing. A later fault therefore observes the store as retired.
        self.state(commit);
        if self.code_pages.is_empty() {
            // Standalone/test emitters without an immutable code snapshot keep
            // the historical conservative boundary.
            self.w.return_();
            return;
        }
        // TLB_HAS_CODE only describes this virtual translation. A second virtual
        // address can alias a physical page backing the currently executing IR
        // region, so compare the translated physical pointer against every page
        // captured by the immutable compilation snapshot before continuing.
        for (index, page) in self.code_pages.iter().enumerate() {
            self.w.get_local(pointer);
            self.w.const_i32(!4095);
            self.w.and_i32();
            self.w.const_i32(*page as i32);
            self.w.eq_i32();
            if index != 0 {
                self.w.or_i32();
            }
        }
        self.w.if_void();
        self.w.return_();
        self.w.block_end();
    }
    fn compare_exchange8b(&mut self, plan: &CompareExchange) {
        self.prepare_memory_call(plan.before);
        let entry = self.planned_ram_guard(plan.address, &plan.guard);
        self.w.if_void();
        self.w.get_local(&entry);
        self.w.const_i32(!4095);
        self.w.and_i32();
        self.get(plan.address);
        self.w.xor_i32();
        let pointer = self.w.set_new_local();
        self.w.get_local(&pointer);
        self.w.load_unaligned_i64(0);
        let value = self.w.set_new_local_i64();
        self.w.get_local_i64(&value);
        self.cpu_register_pair(&plan.expected); // EDX:EAX, read after the memory-read point.
        self.w.eq_i64();
        let equal = self.w.set_new_local();
        self.w.get_local(&equal);
        self.w.if_void();
        self.w.get_local(&pointer);
        self.cpu_register_pair(&plan.replacement); // ECX:EBX
        self.w.store_unaligned_i64(0);
        self.w.else_();
        self.w.const_i32(plan.expected.low as i32);
        self.w.get_local_i64(&value);
        self.w.wrap_i64_to_i32();
        self.w.store_aligned_i32(0);
        self.w.const_i32(plan.expected.high as i32);
        self.w.get_local_i64(&value);
        self.w.const_i64(32);
        self.w.shr_u_i64();
        self.w.wrap_i64_to_i32();
        self.w.store_aligned_i32(0);
        self.w.block_end();
        // No observer/fault can occur between the checked RAM access and these
        // updates. Slow paths retain the baseline's earlier raw-ZF write timing.
        self.w.const_i32(plan.flags as i32);
        self.w.load_fixed_i32(plan.flags);
        self.w.const_i32(!plan.zero_mask);
        self.w.and_i32();
        self.w.get_local(&equal);
        self.w.const_i32(plan.zero_shift);
        self.w.shl_i32();
        self.w.or_i32();
        self.w.store_aligned_i32(0);
        self.w.const_i32(plan.flags_changed as i32);
        self.w.load_fixed_i32(plan.flags_changed);
        self.w.const_i32(!plan.zero_mask);
        self.w.and_i32();
        self.w.store_aligned_i32(0);
        self.w.const_i32(plan.counter as i32);
        self.w.load_fixed_i32(plan.counter);
        self.w.const_i32(plan.increment);
        self.w.add_i32();
        self.w.store_aligned_i32(0);
        self.w.free_local(equal);
        self.w.free_local_i64(value);
        self.w.free_local(pointer);
        self.w.else_();
        self.runtime_call(&plan.call);
        let outcome = self.w.set_new_local();
        self.w.get_local(&outcome);
        self.w.const_i32(plan.outcomes[0]);
        self.w.ne_i32();
        self.w.get_local(&outcome);
        self.w.const_i32(plan.outcomes[1]);
        self.w.ne_i32();
        self.w.and_i32();
        self.w.if_void();
        self.w.unreachable();
        self.w.block_end();
        self.w.free_local(outcome);
        self.w.block_end();
        self.w.free_local(entry);
        self.w.return_();
    }
    fn cpu_register_pair(&mut self, pair: &RegisterPair) {
        self.w.load_fixed_i32(pair.low);
        self.w.extend_unsigned_i32_to_i64();
        self.w.load_fixed_i32(pair.high);
        self.w.extend_unsigned_i32_to_i64();
        self.w.const_i64(32);
        self.w.shl_i64();
        self.w.or_i64();
    }
    fn divide_fault_if(&mut self, plan: &Division) {
        self.w.if_void();
        self.prepare_memory_call(plan.before);
        self.runtime_call(&plan.fault);
        self.w.return_();
        self.w.block_end();
    }
    fn divide(&mut self, plan: &Division) {
        self.get(plan.divisor);
        self.w.const_i64(0);
        self.w.eq_i64();
        if let Some((dividend, divisor)) = plan.overflow_pair {
            self.get(plan.dividend);
            self.w.const_i64(dividend);
            self.w.eq_i64();
            self.get(plan.divisor);
            self.w.const_i64(divisor);
            self.w.eq_i64();
            self.w.and_i32();
            self.w.or_i32();
        }
        self.divide_fault_if(plan);
        self.get(plan.dividend);
        self.get(plan.divisor);
        if plan.signed {
            self.w.div_s_i64();
        } else {
            self.w.div_i64();
        }
        let quotient = self.w.set_new_local_i64();
        self.w.get_local_i64(&quotient);
        match plan.range {
            QuotientRange::Signed { minimum, maximum } => {
                self.w.const_i64(minimum);
                self.w.lt_i64();
                self.w.const_i64(maximum);
                self.w.get_local_i64(&quotient);
                self.w.lt_i64();
                self.w.or_i32();
            },
            QuotientRange::Unsigned { maximum } => {
                self.w.const_i64(maximum);
                self.w.gtu_i64();
            },
        }
        self.divide_fault_if(plan);
        // Stage both outputs after the last possible fault, before assigning SSA
        // slots. The dividend/divisor may share result slots after legalization.
        self.get(plan.dividend);
        self.get(plan.divisor);
        if plan.signed {
            self.w.rem_s_i64();
        } else {
            self.w.rem_i64();
        }
        let remainder = self.w.set_new_local_i64();
        for (value, local) in [(plan.quotient, quotient), (plan.remainder, remainder)] {
            self.w.get_local_i64(&local);
            self.w.wrap_i64_to_i32();
            self.mask(self.mir.value_types[value.index()]);
            self.set(value);
            self.w.free_local_i64(local);
        }
    }
    fn runtime_call(&mut self, call: &RuntimeCall) {
        for arg in &call.args {
            match *arg {
                Argument::Value(value) => self.get(value),
                Argument::I32(value) => self.w.const_i32(value),
            }
        }
        self.w.call_signature(call.name, call.signature.clone());
    }
    fn memory_with_forwarding(&mut self, plan: &MemoryPlan, proof: Option<Forwarding>) {
        match proof {
            Some(Forwarding::Begin) => {
                self.w.const_i32(0);
                self.w.set_local(&self.read_cache.as_ref().unwrap().0);
                self.planned_memory(plan, true);
            },
            Some(Forwarding::Reuse { .. }) => {
                self.w.get_local(&self.read_cache.as_ref().unwrap().0);
                self.w.if_void();
                self.w.get_local(&self.read_cache.as_ref().unwrap().1);
                let NativeMemory::ScalarLoad {
                    result,
                    ticket: None,
                } = plan.native
                else {
                    unreachable!("verified scalar forwarding certificate")
                };
                self.set(result);
                self.w.else_();
                self.planned_memory(plan, true);
                self.w.block_end();
            },
            None => self.planned_memory(plan, false),
        }
    }
    fn planned_memory(&mut self, plan: &MemoryPlan, cache: bool) {
        let bytes = plan.guard.bytes;
        let entry = self.planned_ram_guard(plan.address, &plan.guard);
        self.w.if_void();
        self.w.get_local(&entry);
        self.w.const_i32(!4095);
        self.w.and_i32();
        self.get(plan.address);
        self.w.xor_i32();
        match &plan.native {
            NativeMemory::ScalarLoad { result, ticket } => {
                if let Some(ticket) = ticket {
                    let pointer = self.w.set_new_local();
                    self.w.get_local(&pointer);
                    self.w.extend_unsigned_i32_to_i64();
                    self.set(*ticket);
                    self.w.get_local(&pointer);
                    self.w.free_local(pointer);
                }
                match bytes {
                    1 => self.w.load_u8(0),
                    2 => self.w.load_unaligned_u16(0),
                    4 => self.w.load_unaligned_i32(0),
                    _ => unreachable!(),
                }
                if cache {
                    // The load is guarded as ordinary, same-page readable RAM;
                    // this is the only path allowed to make the cache valid.
                    let (valid, value) = self.read_cache.as_ref().unwrap();
                    self.w.set_local(value);
                    self.w.const_i32(1);
                    self.w.set_local(valid);
                    self.w.get_local(value);
                }
                self.set(*result);
            },
            NativeMemory::ScalarStore { value, commit } => {
                let pointer = commit.map(|_| {
                    let pointer = self.w.set_new_local();
                    self.w.get_local(&pointer);
                    pointer
                });
                self.get(*value);
                match bytes {
                    1 => self.w.store_u8(0),
                    2 => self.w.store_aligned_u16(0),
                    4 => self.w.store_unaligned_i32(0),
                    _ => unreachable!(),
                }
                if let Some(commit) = commit {
                    let pointer = pointer.unwrap();
                    self.finish_scalar_store(*commit, &pointer);
                    self.w.free_local(pointer);
                }
            },
            NativeMemory::VectorStore {
                value,
                lane,
                mask,
                commit,
            } => {
                if let Some(mask) = mask {
                    let address = self.w.set_new_local();
                    self.get(*mask);
                    self.w.simd(0x64);
                    let mask = self.w.set_new_local();
                    for lane in 0..16 {
                        self.w.get_local(&mask);
                        self.w.const_i32(1 << lane);
                        self.w.and_i32();
                        self.w.if_void();
                        self.w.get_local(&address);
                        self.get(*value);
                        self.w.simd_lane(0x16, lane);
                        self.w.store_u8(lane as u32);
                        self.w.block_end();
                    }
                    self.w.free_local(mask);
                    self.w.free_local(address);
                } else {
                    self.get(*value);
                    match bytes {
                        4 => {
                            self.w.simd_lane(0x1B, 0);
                            self.w.store_unaligned_i32(0);
                        },
                        8 => {
                            self.w.simd_lane(0x1D, *lane);
                            self.w.store_unaligned_i64(0);
                        },
                        16 => self.w.simd_memory(0x0B, 0),
                        _ => unreachable!(),
                    }
                }
                self.state(*commit);
                self.w.return_();
            },
            NativeMemory::VectorLoad { result, combine } => {
                if let VectorCombine::ReplaceWord { old, lane } = combine {
                    self.w.load_unaligned_u16(0);
                    let word = self.w.set_new_local();
                    self.get(*old);
                    self.w.get_local(&word);
                    self.w.simd_lane(0x1A, *lane);
                    self.set(*result);
                    self.w.free_local(word);
                } else {
                    self.w.simd_memory(
                        match bytes {
                            4 => 0x5C,
                            8 => 0x5D,
                            16 => 0,
                            _ => unreachable!(),
                        },
                        0,
                    );
                    match combine {
                        VectorCombine::None => (),
                        VectorCombine::Shuffle { old, .. } | VectorCombine::Binary { old, .. } => {
                            let source = self.w.set_new_local_v128();
                            self.get(*old);
                            let destination = self.w.set_new_local_v128();
                            match combine {
                                VectorCombine::Shuffle { lanes, .. } => {
                                    self.w.get_local_v128(&destination);
                                    self.w.get_local_v128(&source);
                                    self.w.simd_shuffle(*lanes);
                                },
                                VectorCombine::Binary { plan, .. } => {
                                    super::simd::binary(&mut self.w, plan, &destination, &source)
                                },
                                _ => unreachable!(),
                            }
                            self.w.free_local_v128(destination);
                            self.w.free_local_v128(source);
                        },
                        _ => unreachable!(),
                    }
                    self.set(*result);
                }
            },
        }
        self.w.else_();
        if cache {
            // Clear BEFORE entering a callback or a page walk. Slow success is
            // not evidence that RAM or its mapping remained stable.
            self.w.const_i32(0);
            self.w.set_local(&self.read_cache.as_ref().unwrap().0);
        }
        self.prepare_memory_call(plan.before);
        self.runtime_call(&plan.call);
        match &plan.result {
            SlowResult::Packed {
                result,
                trap_after_fault,
            } => self.packed_cpu_result(*result, *trap_after_fault),
            SlowResult::Rmw {
                result,
                ticket,
                read_value,
            } => {
                let staged = self.w.set_new_local_i64();
                self.w.get_local_i64(&staged);
                self.w.const_i64(-1);
                self.w.eq_i64();
                self.w.if_void();
                self.w.return_();
                self.w.block_end();
                self.w.get_local_i64(&staged);
                self.set(*ticket);
                self.w.free_local_i64(staged);
                self.runtime_call(read_value);
                self.mask(self.mir.value_types[result.index()]);
                self.set(*result);
            },
            SlowResult::Store {
                commit,
                trap_after_fault,
            } => {
                let outcome = self.w.set_new_local();
                self.w.get_local(&outcome);
                self.w.const_i32(4);
                self.w.eq_i32();
                self.w.if_void();
                if let Some(commit) = commit {
                    self.state(*commit);
                    self.w.return_();
                }
                self.w.else_();
                self.w.get_local(&outcome);
                self.w.const_i32(2);
                self.w.ne_i32();
                self.w.if_void();
                self.w.unreachable();
                self.w.block_end();
                if *trap_after_fault {
                    self.w.unreachable();
                } else {
                    self.w.return_();
                }
                self.w.block_end();
                self.w.free_local(outcome);
            },
            SlowResult::CpuExit { accepted } => {
                let outcome = self.w.set_new_local();
                self.w.get_local(&outcome);
                self.w.const_i32(accepted[0]);
                self.w.ne_i32();
                self.w.get_local(&outcome);
                self.w.const_i32(accepted[1]);
                self.w.ne_i32();
                self.w.and_i32();
                self.w.if_void();
                self.w.unreachable();
                self.w.block_end();
                self.w.free_local(outcome);
                self.w.return_();
            },
        }
        self.w.block_end();
        self.w.free_local(entry);
    }
    fn planned_effect(&mut self, plan: &EffectPlan) {
        match plan {
            EffectPlan::Arithmetic(plan) => match plan {
                ArithmeticPlan::Division(p) => self.divide(p),
                ArithmeticPlan::CompareExchange(p) => self.compare_exchange8b(p),
            },
            EffectPlan::Address {
                null_byte,
                base,
                offset,
                result,
                before,
                call,
                trap_after_fault,
            } => {
                self.w.load_fixed_u8(*null_byte);
                self.w.if_void();
                self.prepare_memory_call(*before);
                self.runtime_call(call);
                self.packed_cpu_result(*result, *trap_after_fault);
                self.w.else_();
                self.get(*offset);
                self.w.load_fixed_i32(*base);
                self.w.add_i32();
                self.set(*result);
                self.w.block_end();
            },
            EffectPlan::Check {
                guard,
                before,
                call,
                success,
                fault,
            } => {
                if let Some(guard) = guard {
                    self.w.load_fixed_i32(guard.address);
                    self.w.const_i32(guard.mask);
                    self.w.and_i32();
                    self.w.if_void();
                }
                self.prepare_memory_call(*before);
                self.runtime_call(call);
                let outcome = self.w.set_new_local();
                if let Some(success) = success {
                    self.w.get_local(&outcome);
                    self.w.const_i32(*success);
                    self.w.ne_i32();
                    self.w.if_void();
                }
                self.w.get_local(&outcome);
                self.w.const_i32(*fault);
                self.w.ne_i32();
                self.w.if_void();
                self.w.unreachable();
                self.w.block_end();
                self.w.return_();
                if success.is_some() {
                    self.w.block_end();
                }
                self.w.free_local(outcome);
                if guard.is_some() {
                    self.w.block_end();
                }
            },
            EffectPlan::RmwCommit {
                bytes,
                ticket,
                value,
                observe,
                commit,
                call,
            } => {
                self.get(*ticket);
                self.w.const_i64(32);
                self.w.shr_u_i64();
                self.w.wrap_i64_to_i32();
                self.w.if_void();
                self.observe_state(observe.values, observe.count, true);
                self.runtime_call(call);
                self.w.else_();
                self.get(*ticket);
                self.w.wrap_i64_to_i32();
                self.get(*value);
                match bytes {
                    1 => self.w.store_u8(0),
                    2 => self.w.store_aligned_u16(0),
                    4 => self.w.store_unaligned_i32(0),
                    _ => unreachable!(),
                }
                self.w.block_end();
                self.state(*commit);
                self.w.return_();
            },
        }
    }
    fn planned_call(&mut self, plan: &CallPlan) {
        let call = self.mir.helpers[plan.helper.index()].as_ref().unwrap();
        let observation = if self.cpu { plan.cpu_observation } else { plan.standalone_observation };
        match observation {
            Observation::CapturedState => self.state(plan.state),
            Observation::DecodedNextPc => self.prepare_memory_call(plan.state),
        }
        for &arg in &plan.args {
            self.get(arg);
        }
        self.w.call_signature(&call.name, call.signature.clone());
        // Do not assign SSA result slots until the outcome is checked: the
        // allocator may reuse pre-call snapshot slots for normal results.
        let mut staged = Vec::new();
        for slot in &plan.staged {
            staged.push((slot, self.temporary(slot.ty)));
        }
        let outcome = self.w.set_new_local();
        if let Some(delivery) = &plan.delivery {
            self.w.get_local(&outcome);
            self.w.const_i32(delivery.outcome as i32);
            self.w.eq_i32();
            self.w.if_void();
            self.state(delivery.restore);
            self.w
                .call_signature(&delivery.name, delivery.signature.clone());
            self.w.return_();
            self.w.block_end();
        }
        for &exit in &plan.exits {
            self.w.get_local(&outcome);
            self.w.const_i32(exit as i32);
            self.w.eq_i32();
            self.w.if_void();
            self.w.return_();
            self.w.block_end();
        }
        if let Some(normal) = plan.normal {
            self.w.get_local(&outcome);
            self.w.const_i32(normal as i32);
            self.w.ne_i32();
            self.w.if_void();
            self.w.unreachable();
            self.w.block_end();
        } else {
            self.w.unreachable();
        }
        for (slot, temp) in staged {
            self.get_temporary(&temp);
            self.mask(slot.ty);
            self.set(slot.value);
            self.free_temporary(temp);
        }
        self.w.free_local(outcome);
    }
    fn address(&self, address: Address) -> u32 {
        match address {
            Address::Absolute(n) => n,
            Address::Gpr(reg) => self.layout.gpr + reg as u32 * 4,
            Address::Flags => self.layout.flags,
            Address::FlagOperand => self.layout.flag_operand,
            Address::Eip => self.layout.eip,
            Address::Committed => self.layout.committed,
        }
    }
    fn read_value(&mut self, reading: &Reading) {
        match reading {
            Reading::Constant(n) => self.w.const_i32(*n),
            Reading::Call { name, signature } => self.w.call_signature(name, signature.clone()),
            Reading::Memory { address, load } => {
                let address = self.address(*address);
                match load {
                    Load::U8 => self.w.load_fixed_u8(address),
                    Load::U16 => self.w.load_fixed_u16(address),
                    Load::I32 => self.w.load_fixed_i32(address),
                    Load::V128 => {
                        self.w.const_i32(address as i32);
                        self.w.simd_memory(0, 2);
                    },
                }
            },
        }
    }
    fn value_steps(&mut self, steps: &[Step]) {
        for step in steps {
            match step {
                Step::Value(value) => self.get(*value),
                Step::I32(n) => self.w.const_i32(*n),
                Step::I64(n) => self.w.const_i64(*n),
                Step::Scalar(op) => super::scalar::emit(&mut self.w, *op),
                Step::Read { cpu, standalone } => {
                    self.read_value(if self.cpu { cpu } else { standalone })
                },
                Step::Simd(opcode) => self.w.simd(*opcode),
                Step::Lane { opcode, lane } => self.w.simd_lane(*opcode, *lane),
                Step::Shuffle(lanes) => self.w.simd_shuffle(*lanes),
                Step::Packed {
                    destination,
                    source,
                    plan,
                } => {
                    self.get(*destination);
                    let destination = self.w.set_new_local_v128();
                    self.get(*source);
                    let source = self.w.set_new_local_v128();
                    super::simd::binary(&mut self.w, plan, &destination, &source);
                    self.w.free_local_v128(destination);
                    self.w.free_local_v128(source);
                },
            }
        }
    }
    fn planned_value(&mut self, plan: &ValuePlan) {
        self.value_steps(&plan.steps);
        self.set(plan.result);
    }
    fn poll(&mut self, state: Option<StateId>, cost: u32, remaining: &WasmLocal) {
        if let Some(state) = state {
            self.w.get_local(remaining);
            self.w.eqz_i32();
            self.w.if_void();
            self.state(state);
            self.w.return_();
            self.w.block_end();
        }
        self.w.get_local(remaining);
        self.w.const_i32(cost as i32);
        self.w.sub_i32();
        self.w.set_local(remaining);
    }
    fn instruction(&mut self, id: InstId, remaining: &WasmLocal) {
        let mir = self.mir;
        if let Some(plan) = &mir.control.polls[id.index()] {
            self.poll(Some(plan.recovery), plan.cost, remaining);
        } else if let Some(plan) = &mir.memory[id.index()] {
            self.memory_with_forwarding(plan, mir.ram_forwarding(id));
        } else if let Some(plan) = &mir.effects[id.index()] {
            self.planned_effect(plan);
        } else if let Some(plan) = &mir.calls[id.index()] {
            self.planned_call(plan);
        } else {
            self.planned_value(mir.values[id.index()].as_ref().unwrap());
        }
    }
}

pub fn emit(mir: &MirRegion, layout: StateLayout, budget: u32) -> Result<Artifact, CompileError> {
    emit_inner(mir, layout, budget, false, None, &[])
}
/// Cold CPU entry, outside the legacy JIT frame. Uses actual CPU globals and MMU.
pub fn emit_cpu(mir: &MirRegion, budget: u32) -> Result<Artifact, CompileError> {
    emit_cpu_inner(mir, budget, None, &[])
}
/// CPU ABI fixture with an explicit immutable physical code dependency set.
pub(crate) fn emit_cpu_with_code_pages(
    mir: &MirRegion,
    budget: u32,
    code_pages: &[u32],
) -> Result<Artifact, CompileError> {
    emit_cpu_inner(mir, budget, None, code_pages)
}
/// CompileRequest owns the association between this key and the lifted guest bytes.
pub(crate) fn emit_cpu_entry(
    mir: &MirRegion,
    budget: u32,
    entry: CpuEntryKey,
    code_pages: &[u32],
) -> Result<Artifact, CompileError> {
    if mir.control.entries.len() != 1 {
        return Err(CompileError::Unsupported(
            "CPU entry key requires a single external entry",
        ));
    }
    emit_cpu_inner(mir, budget, Some(entry), code_pages)
}
fn emit_cpu_inner(
    mir: &MirRegion,
    budget: u32,
    entry: Option<CpuEntryKey>,
    code_pages: &[u32],
) -> Result<Artifact, CompileError> {
    emit_inner(
        mir,
        StateLayout {
            gpr: gp::reg32 as u32,
            flags: gp::flags as u32,
            eip: gp::instruction_pointer as u32,
            committed: gp::instruction_counter as u32,
            flag_operand: gp::last_op1 as u32,
        },
        budget,
        true,
        entry,
        code_pages,
    )
}
fn emit_inner(
    mir: &MirRegion,
    layout: StateLayout,
    budget: u32,
    cpu: bool,
    entry: Option<CpuEntryKey>,
    code_pages: &[u32],
) -> Result<Artifact, CompileError> {
    // MirRegion can only be constructed by the checked lowering transaction.
    // Its machine plans and allocation are immutable across this boundary.
    mir.control.check_target(cpu)?;
    if !cpu && !code_pages.is_empty() {
        return Err(CompileError::InvalidIr(
            "standalone emitter cannot own CPU code dependencies".into(),
        ));
    }
    if code_pages.len() > 8 {
        return Err(CompileError::Budget("code dependency pages"));
    }
    for (index, page) in code_pages.iter().enumerate() {
        if page & 4095 != 0 || code_pages[..index].contains(page) {
            return Err(CompileError::InvalidIr(
                "invalid or duplicate physical code page".into(),
            ));
        }
    }
    if !cpu && mir.states.iter().any(|state| state.requires_cpu) {
        return Err(CompileError::Unsupported("XMM state requires CPU ABI"));
    }
    if !cpu && mir.helpers.iter().flatten().any(|call| call.cpu_exit) {
        return Err(CompileError::Unsupported(
            "terminal helper requires CPU ABI",
        ));
    }
    if !cpu
        && (mir.memory.iter().any(Option::is_some)
            || mir.effects.iter().any(Option::is_some)
            || mir.values.iter().flatten().any(|plan| plan.requires_cpu))
    {
        return Err(CompileError::Unsupported("guest memory requires CPU ABI"));
    }
    if budget == 0 || budget > i32::MAX as u32 {
        return Err(CompileError::Budget("invalid execution budget"));
    }
    if [
        layout.gpr,
        layout.flags,
        layout.eip,
        layout.committed,
        layout.flag_operand,
    ]
    .iter()
    .any(|a| a % 4 != 0 || *a > 64 * 65536 - 32)
    {
        return Err(CompileError::Unsupported(
            "state layout outside imported memory minimum",
        ));
    }
    let fields = [
        (layout.gpr, 32),
        (layout.flags, 4),
        (layout.eip, 4),
        (layout.committed, 4),
        (layout.flag_operand, 4),
    ];
    for (i, &(start, len)) in fields.iter().enumerate() {
        if fields[..i]
            .iter()
            .any(|&(other, size)| start < other + size && other < start + len)
        {
            return Err(CompileError::Unsupported("overlapping state layout"));
        }
    }
    if cpu {
        let reserved = crate::ir::mir::CPU_IMPORTS;
        for helper in mir.helpers.iter().flatten() {
            if reserved.contains(&helper.name.as_str())
                || helper
                    .fault_delivery
                    .as_ref()
                    .is_some_and(|n| reserved.contains(&n.as_str()))
            {
                return Err(CompileError::InvalidIr(
                    "helper shadows CPU ABI import".into(),
                ));
            }
        }
    }
    let mut e = Emitter {
        w: WasmBuilder::new(),
        mir,
        locals: vec![],
        layout,
        cpu,
        accounted: None,
        tlb: None,
        read_cache: None,
        code_pages,
    };
    if let Some(entry) = entry {
        // Reject before ir_enter (which writes previous_ip and clears REP results),
        // before any state loads/materialization, and before any guest access/helper.
        e.w.get_local(&e.w.arg_local_initial_state.unsafe_clone());
        e.w.if_void();
        e.w.return_();
        e.w.block_end();
        e.w.const_i32(entry.linear.0 as i32);
        e.w.const_i32(entry.cs_base() as i32);
        e.w.const_i32(i32::from(entry.default_32));
        e.w.call_fn3_ret("ir_entry_matches");
        e.w.eqz_i32();
        e.w.if_void();
        e.w.return_();
        e.w.block_end();
    }
    if cpu {
        e.w.call_fn0("ir_enter");
        e.w.const_i32(0);
        e.accounted = Some(e.w.set_new_local());
        e.w.call_fn0_ret("ir_tlb_base");
        e.tlb = Some(e.w.set_new_local());
    }
    if mir.has_ram_forwarding() {
        e.w.const_i32(0);
        let valid = e.w.set_new_local();
        e.w.const_i32(0);
        let value = e.w.set_new_local();
        e.read_cache = Some((valid, value));
    }
    for ty in &mir.allocation.local_types {
        if *ty == Type::V128 {
            e.w.simd_zero();
            e.locals.push(Local::V128(e.w.set_new_local_v128()));
        } else if matches!(*ty, Type::I64 | Type::RmwTicket) {
            e.w.const_i64(0);
            e.locals.push(Local::I64(e.w.set_new_local_i64()));
        } else {
            e.w.const_i32(0);
            e.locals.push(Local::I32(e.w.set_new_local()));
        }
    }
    e.w.const_i32(-1);
    let pc = e.w.set_new_local();
    for (i, entry) in mir.control.entries.iter().enumerate() {
        e.w.get_local(&e.w.arg_local_initial_state.unsafe_clone());
        e.w.const_i32(i as i32);
        e.w.eq_i32();
        e.w.if_void();
        e.w.const_i32(entry.0 as i32);
        e.w.set_local(&pc);
        e.w.block_end();
    }
    e.w.get_local(&pc);
    e.w.const_i32(-1);
    e.w.eq_i32();
    e.w.if_void();
    e.w.return_();
    e.w.block_end();
    e.w.const_i32(budget as i32);
    let remaining = e.w.set_new_local();
    let dispatch = e.w.loop_void();
    for (b, block) in mir.control.blocks.iter().enumerate() {
        e.w.get_local(&pc);
        e.w.const_i32(b as i32);
        e.w.eq_i32();
        e.w.if_void();
        e.poll(block.recovery, block.budget_cost, &remaining);
        for id in &block.instructions {
            e.instruction(*id, &remaining);
        }
        match &block.terminator {
            MirTerminator::Exit(state) => {
                e.state(*state);
                e.w.return_();
            },
            MirTerminator::Jump(edge) => {
                e.copy_edge(edge, &pc);
                e.w.br(dispatch);
            },
            MirTerminator::Branch {
                condition,
                taken,
                not_taken,
            } => {
                e.get_local(*condition);
                e.w.if_void();
                e.copy_edge(taken, &pc);
                e.w.else_();
                e.copy_edge(not_taken, &pc);
                e.w.block_end();
                e.w.br(dispatch);
            },
        }
        e.w.block_end();
    }
    e.w.unreachable();
    e.w.block_end();
    let locals = e.w.declared_local_count();
    for local in e.locals {
        match local {
            Local::I32(local) => e.w.free_local(local),
            Local::I64(local) => e.w.free_local_i64(local),
            Local::V128(local) => e.w.free_local_v128(local),
        }
    }
    e.w.free_local(pc);
    e.w.free_local(remaining);
    if let Some(local) = e.accounted {
        e.w.free_local(local);
    }
    if let Some(local) = e.tlb {
        e.w.free_local(local);
    }
    if let Some((valid, value)) = e.read_cache {
        e.w.free_local(valid);
        e.w.free_local(value);
    }
    e.w.finish();
    let bytes = e.w.output().to_vec();
    if bytes.len() > 256 * 1024 {
        return Err(CompileError::Budget("Wasm bytes"));
    }
    Ok(Artifact { bytes, locals })
}
