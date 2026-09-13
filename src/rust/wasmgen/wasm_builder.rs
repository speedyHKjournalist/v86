use crate::leb::{write_leb_i32, write_leb_i64, write_leb_u32};
use crate::wasmgen::wasm_opcodes as op;
use std::collections::HashMap;

// Wasm indices and vector/section lengths are unsigned 32-bit LEBs.
fn wasm_len(n: usize) -> u32 { u32::try_from(n).expect("Wasm length exceeds u32") }
fn section(output: &mut Vec<u8>, kind: u8, contents: &[u8]) {
    output.push(kind);
    write_leb_u32(output, wasm_len(contents.len()));
    output.extend_from_slice(contents);
}
fn name(output: &mut Vec<u8>, text: &str) {
    write_leb_u32(output, wasm_len(text.len()));
    output.extend_from_slice(text.as_bytes());
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum WasmType {
    I32 = 0x7F,
    I64 = 0x7E,
    F32 = 0x7D,
    F64 = 0x7C,
    V128 = 0x7B,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Signature {
    pub params: Vec<WasmType>,
    pub results: Vec<WasmType>,
}
impl Signature {
    pub fn new(params: &[WasmType], results: &[WasmType]) -> Self {
        Self {
            params: params.to_vec(),
            results: results.to_vec(),
        }
    }
}
#[derive(Copy, Clone, PartialEq)]
#[allow(non_camel_case_types)]
enum FunctionType {
    FN0,
    FN1,
    FN1_I64,
    FN2,
    FN3,

    FN0_RET,
    FN0_RET_I64,
    FN1_RET,
    FN2_RET,

    FN1_F32_RET,
    FN1_F64_RET,

    FN2_I32_I64,
    FN2_I64_I32,
    FN2_I64_I32_RET,
    FN2_I64_I32_RET_I64,
    FN2_F32_I32,

    FN3_RET,

    FN3_I64_I32_I32,
    FN3_I32_I64_I32,
    FN3_I32_I64_I32_RET,
    FN4_I32_I64_I64_I32_RET,
}

impl FunctionType {
    fn signature(self) -> Signature {
        use WasmType::*;
        let (params, results): (&[WasmType], &[WasmType]) = match self {
            Self::FN0 => (&[], &[]),
            Self::FN1 => (&[I32], &[]),
            Self::FN1_I64 => (&[I64], &[]),
            Self::FN2 => (&[I32, I32], &[]),
            Self::FN3 => (&[I32, I32, I32], &[]),
            Self::FN0_RET => (&[], &[I32]),
            Self::FN0_RET_I64 => (&[], &[I64]),
            Self::FN1_RET => (&[I32], &[I32]),
            Self::FN2_RET => (&[I32, I32], &[I32]),
            Self::FN1_F32_RET => (&[F32], &[I32]),
            Self::FN1_F64_RET => (&[F64], &[I32]),
            Self::FN2_I32_I64 => (&[I32, I64], &[]),
            Self::FN2_I64_I32 => (&[I64, I32], &[]),
            Self::FN2_I64_I32_RET => (&[I64, I32], &[I32]),
            Self::FN2_I64_I32_RET_I64 => (&[I64, I32], &[I64]),
            Self::FN2_F32_I32 => (&[F32, I32], &[]),
            Self::FN3_RET => (&[I32, I32, I32], &[I32]),
            Self::FN3_I64_I32_I32 => (&[I64, I32, I32], &[]),
            Self::FN3_I32_I64_I32 => (&[I32, I64, I32], &[]),
            Self::FN3_I32_I64_I32_RET => (&[I32, I64, I32], &[I32]),
            Self::FN4_I32_I64_I64_I32_RET => (&[I32, I64, I64, I32], &[I32]),
        };
        Signature::new(params, results)
    }
}

pub const WASM_MODULE_ARGUMENT_COUNT: u32 = 1;

pub struct WasmBuilder {
    output: Vec<u8>,
    instruction_body: Vec<u8>,
    signatures: Vec<Signature>,
    signature_indices: HashMap<Signature, u32>,
    imports: Vec<(String, u32)>,
    import_indices: HashMap<String, u32>,
    finished: bool,
    next_label: Label,
    label_stack: Vec<Label>,
    label_to_depth: HashMap<Label, usize>,
    free_locals_i32: Vec<WasmLocal>,
    free_locals_i64: Vec<WasmLocalI64>,
    free_locals_f32: Vec<WasmLocalF32>,
    free_locals_f64: Vec<WasmLocalF64>,
    free_locals_v128: Vec<WasmLocalV128>,
    local_types: Vec<WasmType>,
    local_live: Vec<bool>,
    pub arg_local_initial_state: WasmLocal,
    pub defer_flags: bool,
    deferred_stores: Vec<(u32, WasmLocal)>,
}

#[derive(Eq, PartialEq)]
pub struct WasmLocal(u32);
impl WasmLocal {
    pub fn idx(&self) -> u32 { self.0 }
    /// Only clone locals whose owner keeps them live for the whole use interval.
    pub fn unsafe_clone(&self) -> Self { Self(self.0) }
}
pub struct WasmLocalI64(u32);
impl WasmLocalI64 {
    pub fn unsafe_clone(&self) -> Self { Self(self.0) }
    pub fn idx(&self) -> u32 { self.0 }
}
// F32 locals are exercised by the new backend tests; legacy SIMD uses V128.
#[allow(dead_code)]
pub struct WasmLocalF32(u32);
pub struct WasmLocalF64(u32);
pub struct WasmLocalV128(u32);

#[derive(Copy, Clone, Eq, Hash, PartialEq)]
pub struct Label(u32);
impl Label {
    const ZERO: Label = Label(0);
    fn next(&self) -> Label { Label(self.0.checked_add(1).expect("Wasm label overflow")) }
}

impl WasmBuilder {
    pub fn new() -> Self {
        Self {
            output: Vec::with_capacity(256),
            instruction_body: Vec::with_capacity(256),
            signatures: Vec::new(),
            signature_indices: HashMap::new(),
            imports: Vec::new(),
            import_indices: HashMap::new(),
            finished: false,
            next_label: Label::ZERO,
            label_stack: Vec::new(),
            label_to_depth: HashMap::new(),
            free_locals_i32: Vec::new(),
            free_locals_i64: Vec::new(),
            free_locals_f32: Vec::new(),
            free_locals_f64: Vec::new(),
            free_locals_v128: Vec::new(),
            local_types: Vec::new(),
            local_live: Vec::new(),
            arg_local_initial_state: WasmLocal(0),
            defer_flags: false,
            deferred_stores: Vec::new(),
        }
    }
    pub fn defer_fixed_i32(&mut self, address: u32) {
        if let Some((_, local)) = self.deferred_stores.iter().find(|(a, _)| *a == address) {
            let local = local.unsafe_clone();
            self.set_local(&local);
        } else {
            let local = self.set_new_local();
            self.deferred_stores.push((address, local));
        }
    }

    pub fn load_deferred_i32(&mut self, address: u32) -> bool {
        if let Some((_, local)) = self.deferred_stores.iter().find(|(a, _)| *a == address) {
            let local = local.unsafe_clone();
            self.get_local(&local);
            true
        } else {
            false
        }
    }

    // Does not clear compile-time state: a cold branch may materialize state
    // while the hot branch keeps it exclusively in locals.
    pub fn materialize_deferred_stores(&mut self) {
        for i in 0..self.deferred_stores.len() {
            let (address, local) = &self.deferred_stores[i];
            let address = *address;
            let local = local.unsafe_clone();
            self.const_i32(address as i32);
            self.get_local(&local);
            self.store_aligned_i32(0);
        }
    }

    pub fn flush_deferred_stores(&mut self) {
        self.materialize_deferred_stores();
        let stores = std::mem::take(&mut self.deferred_stores);
        for (_, local) in stores {
            self.free_local(local);
        }
        self.defer_flags = false;
    }

    pub fn reset(&mut self) {
        assert!(
            self.deferred_stores.is_empty(),
            "unmaterialized deferred stores"
        );
        assert!(self.label_stack.is_empty(), "unclosed Wasm blocks");
        self.output.clear();
        self.instruction_body.clear();
        self.signatures.clear();
        self.signature_indices.clear();
        self.imports.clear();
        self.import_indices.clear();
        self.free_locals_i32.clear();
        self.free_locals_i64.clear();
        self.free_locals_f32.clear();
        self.free_locals_f64.clear();
        self.free_locals_v128.clear();
        self.local_types.clear();
        self.local_live.clear();
        self.next_label = Label::ZERO;
        self.label_to_depth.clear();
        self.defer_flags = false;
        self.finished = false;
    }

    pub fn intern_signature(&mut self, signature: Signature) -> u32 {
        assert!(!self.finished, "reset builder before reuse");
        if let Some(&index) = self.signature_indices.get(&signature) {
            return index;
        }
        let index = wasm_len(self.signatures.len());
        self.signatures.push(signature.clone());
        self.signature_indices.insert(signature, index);
        index
    }

    fn get_fn_idx_signature(&mut self, fn_name: &str, signature: Signature) -> u32 {
        let ty = self.intern_signature(signature);
        if let Some(&index) = self.import_indices.get(fn_name) {
            assert_eq!(
                self.imports[index as usize].1, ty,
                "helper signature mismatch: {}",
                fn_name
            );
            return index;
        }
        let index = wasm_len(self.imports.len());
        self.imports.push((fn_name.to_owned(), ty));
        self.import_indices.insert(fn_name.to_owned(), index);
        index
    }

    #[cfg(test)]
    fn get_fn_idx(&mut self, fn_name: &str, function: FunctionType) -> u32 {
        self.get_fn_idx_signature(fn_name, function.signature())
    }

    /// Vector signatures are for Wasm-to-Wasm calls; JS helpers need a scratch-memory ABI.
    pub fn call_signature(&mut self, fn_name: &str, signature: Signature) {
        let index = self.get_fn_idx_signature(fn_name, signature);
        self.instruction_body.push(op::OP_CALL);
        write_leb_u32(&mut self.instruction_body, index);
    }

    pub fn finish(&mut self) -> usize {
        assert!(!self.finished, "reset builder before reuse");
        assert!(
            self.label_stack.is_empty() && self.label_to_depth.is_empty(),
            "unclosed Wasm blocks"
        );
        assert!(
            self.local_live.iter().all(|&live| !live),
            "all locals must be freed"
        );
        assert!(
            self.deferred_stores.is_empty(),
            "unmaterialized deferred stores"
        );
        let entry_type = self.intern_signature(FunctionType::FN1.signature());
        self.output.extend_from_slice(b"\0asm\x01\0\0\0");
        let mut body = Vec::new();
        write_leb_u32(&mut body, wasm_len(self.signatures.len()));
        for signature in &self.signatures {
            body.push(op::TYPE_FUNC);
            write_leb_u32(&mut body, wasm_len(signature.params.len()));
            body.extend(signature.params.iter().map(|t| *t as u8));
            write_leb_u32(&mut body, wasm_len(signature.results.len()));
            body.extend(signature.results.iter().map(|t| *t as u8));
        }
        section(&mut self.output, op::SC_TYPE, &body);
        body.clear();
        write_leb_u32(
            &mut body,
            wasm_len(self.imports.len())
                .checked_add(1)
                .expect("import overflow"),
        );
        for (fn_name, ty) in &self.imports {
            name(&mut body, "e");
            name(&mut body, fn_name);
            body.push(op::EXT_FUNCTION);
            write_leb_u32(&mut body, *ty);
        }
        name(&mut body, "e");
        name(&mut body, "m");
        body.extend_from_slice(&[op::EXT_MEMORY, 0]);
        write_leb_u32(&mut body, 64);
        section(&mut self.output, op::SC_IMPORT, &body);
        body.clear();
        body.push(1);
        write_leb_u32(&mut body, entry_type);
        section(&mut self.output, op::SC_FUNCTION, &body);
        body.clear();
        body.push(1);
        name(&mut body, "f");
        body.push(op::EXT_FUNCTION);
        write_leb_u32(&mut body, wasm_len(self.imports.len()));
        section(&mut self.output, op::SC_EXPORT, &body);
        let mut groups: Vec<(WasmType, u32)> = Vec::new();
        for &ty in &self.local_types {
            if let Some((last, count)) = groups.last_mut() {
                if *last == ty {
                    *count = count.checked_add(1).expect("local group overflow");
                    continue;
                }
            }
            groups.push((ty, 1));
        }
        body.clear();
        write_leb_u32(&mut body, wasm_len(groups.len()));
        for (ty, count) in groups {
            write_leb_u32(&mut body, count);
            body.push(ty as u8);
        }
        body.extend_from_slice(&self.instruction_body);
        body.push(op::OP_END);
        let mut code = vec![1];
        write_leb_u32(&mut code, wasm_len(body.len()));
        code.extend_from_slice(&body);
        section(&mut self.output, op::SC_CODE, &code);
        self.finished = true;
        self.output.len()
    }

    #[cfg(any(test, feature = "ir-experimental"))]
    pub fn output(&self) -> &[u8] { &self.output }

    /// Declared locals excluding parameters, including temporary staging slots.
    #[cfg(any(test, feature = "ir-experimental"))]
    pub fn declared_local_count(&self) -> usize { self.local_types.len() }

    pub fn get_output_ptr(&self) -> *const u8 { self.output.as_ptr() }
    pub fn get_output_len(&self) -> u32 { wasm_len(self.output.len()) }

    fn new_local_index(&mut self, ty: WasmType) -> u32 {
        assert!(!self.finished, "reset builder before reuse");
        let index = wasm_len(self.local_types.len())
            .checked_add(WASM_MODULE_ARGUMENT_COUNT)
            .expect("local overflow");
        self.local_types.push(ty);
        self.local_live.push(true);
        index
    }
    fn reuse_local_index(&mut self, index: u32, ty: WasmType) {
        let i = (index - WASM_MODULE_ARGUMENT_COUNT) as usize;
        assert_eq!(self.local_types[i], ty);
        assert!(!self.local_live[i], "reusing live local");
        self.local_live[i] = true;
    }
    fn release_local_index(&mut self, index: u32, ty: WasmType) {
        let i = index
            .checked_sub(WASM_MODULE_ARGUMENT_COUNT)
            .expect("cannot free argument") as usize;
        assert_eq!(self.local_types[i], ty);
        assert!(self.local_live[i], "local freed twice");
        self.local_live[i] = false;
    }
    fn open_block(&mut self) -> Label {
        let label = self.next_label;
        self.next_label = self.next_label.next();
        self.label_to_depth
            .insert(label, self.label_stack.len() + 1);
        self.label_stack.push(label);
        label
    }
    fn close_block(&mut self) {
        let label = self.label_stack.pop().unwrap();
        let old_depth = self.label_to_depth.remove(&label).unwrap();
        dbg_assert!(self.label_to_depth.len() + 1 == old_depth);
    }

    fn alloc_local(&mut self) -> WasmLocal {
        if let Some(local) = self.free_locals_i32.pop() {
            self.reuse_local_index(local.0, WasmType::I32);
            local
        } else {
            WasmLocal(self.new_local_index(WasmType::I32))
        }
    }
    pub fn free_local(&mut self, local: WasmLocal) {
        self.release_local_index(local.0, WasmType::I32);
        self.free_locals_i32.push(local);
    }
    #[must_use]
    pub fn set_new_local(&mut self) -> WasmLocal {
        let local = self.alloc_local();
        self.set_local(&local);
        local
    }
    #[must_use]
    pub fn tee_new_local(&mut self) -> WasmLocal {
        let local = self.alloc_local();
        self.instruction_body.push(op::OP_TEELOCAL);
        write_leb_u32(&mut self.instruction_body, local.0);
        local
    }
    pub fn set_local(&mut self, local: &WasmLocal) {
        self.instruction_body.push(op::OP_SETLOCAL);
        write_leb_u32(&mut self.instruction_body, local.0);
    }
    pub fn get_local(&mut self, local: &WasmLocal) {
        self.instruction_body.push(op::OP_GETLOCAL);
        write_leb_u32(&mut self.instruction_body, local.idx());
    }
    pub fn tee_local(&mut self, local: &WasmLocal) {
        self.instruction_body.push(op::OP_TEELOCAL);
        write_leb_u32(&mut self.instruction_body, local.0);
    }
    fn alloc_local_i64(&mut self) -> WasmLocalI64 {
        if let Some(local) = self.free_locals_i64.pop() {
            self.reuse_local_index(local.0, WasmType::I64);
            local
        } else {
            WasmLocalI64(self.new_local_index(WasmType::I64))
        }
    }
    pub fn free_local_i64(&mut self, local: WasmLocalI64) {
        self.release_local_index(local.0, WasmType::I64);
        self.free_locals_i64.push(local);
    }
    #[must_use]
    pub fn set_new_local_i64(&mut self) -> WasmLocalI64 {
        let local = self.alloc_local_i64();
        self.set_local_i64(&local);
        local
    }
    #[must_use]
    pub fn tee_new_local_i64(&mut self) -> WasmLocalI64 {
        let local = self.alloc_local_i64();
        self.instruction_body.push(op::OP_TEELOCAL);
        write_leb_u32(&mut self.instruction_body, local.0);
        local
    }
    pub fn set_local_i64(&mut self, local: &WasmLocalI64) {
        self.instruction_body.push(op::OP_SETLOCAL);
        write_leb_u32(&mut self.instruction_body, local.0);
    }
    pub fn get_local_i64(&mut self, local: &WasmLocalI64) {
        self.instruction_body.push(op::OP_GETLOCAL);
        write_leb_u32(&mut self.instruction_body, local.idx());
    }
    #[allow(dead_code)]
    fn alloc_local_f32(&mut self) -> WasmLocalF32 {
        if let Some(local) = self.free_locals_f32.pop() {
            self.reuse_local_index(local.0, WasmType::F32);
            local
        } else {
            WasmLocalF32(self.new_local_index(WasmType::F32))
        }
    }
    #[allow(dead_code)]
    pub fn free_local_f32(&mut self, local: WasmLocalF32) {
        self.release_local_index(local.0, WasmType::F32);
        self.free_locals_f32.push(local);
    }
    #[must_use]
    #[allow(dead_code)]
    pub fn set_new_local_f32(&mut self) -> WasmLocalF32 {
        let local = self.alloc_local_f32();
        self.set_local_f32(&local);
        local
    }
    #[allow(dead_code)]
    pub fn set_local_f32(&mut self, local: &WasmLocalF32) {
        self.instruction_body.push(op::OP_SETLOCAL);
        write_leb_u32(&mut self.instruction_body, local.0);
    }
    #[allow(dead_code)]
    pub fn get_local_f32(&mut self, local: &WasmLocalF32) {
        self.instruction_body.push(op::OP_GETLOCAL);
        write_leb_u32(&mut self.instruction_body, local.0);
    }
    fn alloc_local_f64(&mut self) -> WasmLocalF64 {
        if let Some(local) = self.free_locals_f64.pop() {
            self.reuse_local_index(local.0, WasmType::F64);
            local
        } else {
            WasmLocalF64(self.new_local_index(WasmType::F64))
        }
    }
    pub fn free_local_f64(&mut self, local: WasmLocalF64) {
        self.release_local_index(local.0, WasmType::F64);
        self.free_locals_f64.push(local);
    }
    #[must_use]
    pub fn set_new_local_f64(&mut self) -> WasmLocalF64 {
        let local = self.alloc_local_f64();
        self.set_local_f64(&local);
        local
    }
    pub fn set_local_f64(&mut self, local: &WasmLocalF64) {
        self.instruction_body.push(op::OP_SETLOCAL);
        write_leb_u32(&mut self.instruction_body, local.0);
    }
    pub fn get_local_f64(&mut self, local: &WasmLocalF64) {
        self.instruction_body.push(op::OP_GETLOCAL);
        write_leb_u32(&mut self.instruction_body, local.0);
    }
    fn alloc_local_v128(&mut self) -> WasmLocalV128 {
        if let Some(local) = self.free_locals_v128.pop() {
            self.reuse_local_index(local.0, WasmType::V128);
            local
        } else {
            WasmLocalV128(self.new_local_index(WasmType::V128))
        }
    }
    pub fn free_local_v128(&mut self, local: WasmLocalV128) {
        self.release_local_index(local.0, WasmType::V128);
        self.free_locals_v128.push(local);
    }
    #[must_use]
    pub fn set_new_local_v128(&mut self) -> WasmLocalV128 {
        let local = self.alloc_local_v128();
        self.set_local_v128(&local);
        local
    }
    pub fn set_local_v128(&mut self, local: &WasmLocalV128) {
        self.instruction_body.push(op::OP_SETLOCAL);
        write_leb_u32(&mut self.instruction_body, local.0);
    }
    pub fn get_local_v128(&mut self, local: &WasmLocalV128) {
        self.instruction_body.push(op::OP_GETLOCAL);
        write_leb_u32(&mut self.instruction_body, local.0);
    }
    pub fn unary_f64(&mut self, negative: bool) {
        self.instruction_body
            .push(if negative { op::OP_F64NEG } else { op::OP_F64ABS });
    }

    pub fn round_f64(&mut self, rc: u8) {
        self.instruction_body.push(match rc {
            0 => op::OP_F64NEAREST,
            1 => op::OP_F64FLOOR,
            2 => op::OP_F64CEIL,
            3 => op::OP_F64TRUNC,
            _ => unreachable!(),
        });
    }

    pub fn compare_f64(&mut self, operation: u8) {
        self.instruction_body.push(match operation {
            0 => op::OP_F64GT,
            1 => op::OP_F64LT,
            2 => op::OP_F64EQ,
            _ => unreachable!(),
        });
    }

    pub fn arithmetic_f64(&mut self, operation: u8) {
        self.instruction_body.push(match operation {
            0 => op::OP_F64ADD,
            1 => op::OP_F64SUB,
            2 => op::OP_F64MUL,
            3 => op::OP_F64DIV,
            _ => unreachable!(),
        });
    }
    pub fn reinterpret_f64_as_i64(&mut self) {
        self.instruction_body.push(op::OP_I64REINTERPRETF64);
    }

    // Standard SIMD binary encoding: 0xfd, unsigned LEB opcode, immediates.
    pub fn simd(&mut self, opcode: u32) {
        self.instruction_body.push(0xFD);
        write_leb_u32(&mut self.instruction_body, opcode);
    }
    pub fn simd_lane(&mut self, opcode: u32, lane: u8) {
        self.simd(opcode);
        self.instruction_body.push(lane);
    }
    pub fn simd_memory(&mut self, opcode: u32, alignment: u8) {
        self.simd(opcode);
        self.instruction_body.push(alignment);
        self.instruction_body.push(0);
    }
    pub fn simd_shuffle(&mut self, lanes: [u8; 16]) {
        dbg_assert!(lanes.iter().all(|&x| x < 32));
        self.simd(0x0D);
        self.instruction_body.extend_from_slice(&lanes);
    }
    pub fn simd_const(&mut self, bytes: [u8; 16]) {
        self.simd(0x0C);
        self.instruction_body.extend_from_slice(&bytes);
    }
    pub fn simd_zero(&mut self) {
        self.simd_const([0; 16]);
    }
    pub fn if_v128(&mut self) {
        self.open_block();
        self.instruction_body.push(op::OP_IF);
        self.instruction_body.push(0x7B);
    }

    pub fn const_i32(&mut self, v: i32) {
        self.instruction_body.push(op::OP_I32CONST);
        write_leb_i32(&mut self.instruction_body, v);
    }
    pub fn const_i64(&mut self, v: i64) {
        self.instruction_body.push(op::OP_I64CONST);
        write_leb_i64(&mut self.instruction_body, v);
    }

    pub fn load_fixed_u8(&mut self, addr: u32) {
        self.const_i32(addr as i32);
        self.load_u8(0);
    }
    pub fn load_fixed_u16(&mut self, addr: u32) {
        // doesn't cause a failure in the generated code, but it will be much slower
        dbg_assert!((addr & 1) == 0);

        self.const_i32(addr as i32);
        self.instruction_body.push(op::OP_I32LOAD16U);
        self.instruction_body.push(op::MEM_ALIGN16);
        self.instruction_body.push(0); // immediate offset
    }
    pub fn load_fixed_i32(&mut self, addr: u32) {
        // doesn't cause a failure in the generated code, but it will be much slower
        dbg_assert!((addr & 3) == 0);

        self.const_i32(addr as i32);
        self.load_aligned_i32(0);
    }
    pub fn load_fixed_i64(&mut self, addr: u32) {
        // doesn't cause a failure in the generated code, but it will be much slower
        dbg_assert!((addr & 7) == 0);

        self.const_i32(addr as i32);
        self.load_aligned_i64(0);
    }

    pub fn load_u8(&mut self, byte_offset: u32) {
        self.instruction_body.push(op::OP_I32LOAD8U);
        self.instruction_body.push(op::MEM_NO_ALIGN);
        write_leb_u32(&mut self.instruction_body, byte_offset);
    }

    pub fn load_unaligned_i64(&mut self, byte_offset: u32) {
        self.instruction_body.push(op::OP_I64LOAD);
        self.instruction_body.push(op::MEM_NO_ALIGN);
        write_leb_u32(&mut self.instruction_body, byte_offset);
    }

    pub fn load_unaligned_i32(&mut self, byte_offset: u32) {
        self.instruction_body.push(op::OP_I32LOAD);
        self.instruction_body.push(op::MEM_NO_ALIGN);
        write_leb_u32(&mut self.instruction_body, byte_offset);
    }

    pub fn load_unaligned_u16(&mut self, byte_offset: u32) {
        self.instruction_body.push(op::OP_I32LOAD16U);
        self.instruction_body.push(op::MEM_NO_ALIGN);
        write_leb_u32(&mut self.instruction_body, byte_offset);
    }

    pub fn load_aligned_f64(&mut self, byte_offset: u32) {
        self.instruction_body.push(op::OP_F64LOAD);
        self.instruction_body.push(op::MEM_ALIGN64);
        write_leb_u32(&mut self.instruction_body, byte_offset);
    }

    pub fn load_aligned_i64(&mut self, byte_offset: u32) {
        self.instruction_body.push(op::OP_I64LOAD);
        self.instruction_body.push(op::MEM_ALIGN64);
        write_leb_u32(&mut self.instruction_body, byte_offset);
    }

    pub fn load_aligned_f32(&mut self, byte_offset: u32) {
        self.instruction_body.push(op::OP_F32LOAD);
        self.instruction_body.push(op::MEM_ALIGN32);
        write_leb_u32(&mut self.instruction_body, byte_offset);
    }

    pub fn load_aligned_i32(&mut self, byte_offset: u32) {
        self.instruction_body.push(op::OP_I32LOAD);
        self.instruction_body.push(op::MEM_ALIGN32);
        write_leb_u32(&mut self.instruction_body, byte_offset);
    }

    pub fn load_aligned_u16(&mut self, byte_offset: u32) {
        self.instruction_body.push(op::OP_I32LOAD16U);
        self.instruction_body.push(op::MEM_ALIGN16);
        write_leb_u32(&mut self.instruction_body, byte_offset);
    }

    pub fn store_u8(&mut self, byte_offset: u32) {
        self.instruction_body.push(op::OP_I32STORE8);
        self.instruction_body.push(op::MEM_NO_ALIGN);
        write_leb_u32(&mut self.instruction_body, byte_offset);
    }

    pub fn store_aligned_u16(&mut self, byte_offset: u32) {
        self.instruction_body.push(op::OP_I32STORE16);
        self.instruction_body.push(op::MEM_ALIGN16);
        write_leb_u32(&mut self.instruction_body, byte_offset);
    }

    pub fn store_aligned_i32(&mut self, byte_offset: u32) {
        self.instruction_body.push(op::OP_I32STORE);
        self.instruction_body.push(op::MEM_ALIGN32);
        write_leb_u32(&mut self.instruction_body, byte_offset);
    }

    pub fn store_aligned_i64(&mut self, byte_offset: u32) {
        self.instruction_body.push(op::OP_I64STORE);
        self.instruction_body.push(op::MEM_ALIGN64);
        write_leb_u32(&mut self.instruction_body, byte_offset);
    }

    pub fn store_unaligned_u16(&mut self, byte_offset: u32) {
        self.instruction_body.push(op::OP_I32STORE16);
        self.instruction_body.push(op::MEM_NO_ALIGN);
        write_leb_u32(&mut self.instruction_body, byte_offset);
    }

    pub fn store_unaligned_i32(&mut self, byte_offset: u32) {
        self.instruction_body.push(op::OP_I32STORE);
        self.instruction_body.push(op::MEM_NO_ALIGN);
        write_leb_u32(&mut self.instruction_body, byte_offset);
    }

    pub fn store_unaligned_i64(&mut self, byte_offset: u32) {
        self.instruction_body.push(op::OP_I64STORE);
        self.instruction_body.push(op::MEM_NO_ALIGN);
        write_leb_u32(&mut self.instruction_body, byte_offset);
    }

    pub fn increment_fixed_i64(&mut self, byte_offset: u32, n: i64) {
        self.const_i32(byte_offset as i32);
        self.load_fixed_i64(byte_offset);
        self.const_i64(n);
        self.add_i64();
        self.store_aligned_i64(0);
    }

    pub fn add_i32(&mut self) { self.instruction_body.push(op::OP_I32ADD); }
    pub fn clz_i32(&mut self) { self.instruction_body.push(op::OP_I32CLZ); }
    pub fn ctz_i32(&mut self) { self.instruction_body.push(op::OP_I32CTZ); }
    pub fn popcnt_i32(&mut self) { self.instruction_body.push(op::OP_I32POPCNT); }
    pub fn clz_i64(&mut self) { self.instruction_body.push(op::OP_I64CLZ); }
    pub fn ctz_i64(&mut self) { self.instruction_body.push(op::OP_I64CTZ); }
    pub fn popcnt_i64(&mut self) { self.instruction_body.push(op::OP_I64POPCNT); }
    pub fn sub_i64(&mut self) { self.instruction_body.push(op::OP_I64SUB); }
    pub fn div_s_i64(&mut self) { self.instruction_body.push(op::OP_I64DIVS); }
    pub fn rem_s_i64(&mut self) { self.instruction_body.push(op::OP_I64REMS); }
    pub fn shr_s_i64(&mut self) { self.instruction_body.push(op::OP_I64SHRS); }
    pub fn lt_i64(&mut self) { self.instruction_body.push(op::OP_I64LTS); }
    pub fn ltu_i64(&mut self) { self.instruction_body.push(op::OP_I64LTU); }
    pub fn add_i64(&mut self) { self.instruction_body.push(op::OP_I64ADD); }
    pub fn sub_i32(&mut self) { self.instruction_body.push(op::OP_I32SUB); }
    pub fn and_i32(&mut self) { self.instruction_body.push(op::OP_I32AND); }
    pub fn and_i64(&mut self) { self.instruction_body.push(op::OP_I64AND); }
    pub fn or_i32(&mut self) { self.instruction_body.push(op::OP_I32OR); }
    pub fn or_i64(&mut self) { self.instruction_body.push(op::OP_I64OR); }
    pub fn xor_i32(&mut self) { self.instruction_body.push(op::OP_I32XOR); }
    pub fn xor_i64(&mut self) { self.instruction_body.push(op::OP_I64XOR); }
    pub fn mul_i32(&mut self) { self.instruction_body.push(op::OP_I32MUL); }
    pub fn mul_i64(&mut self) { self.instruction_body.push(op::OP_I64MUL); }
    pub fn div_i64(&mut self) { self.instruction_body.push(op::OP_I64DIVU); }
    pub fn rem_i64(&mut self) { self.instruction_body.push(op::OP_I64REMU); }

    pub fn rotl_i32(&mut self) { self.instruction_body.push(op::OP_I32ROTL); }

    pub fn shl_i32(&mut self) { self.instruction_body.push(op::OP_I32SHL); }
    pub fn shl_i64(&mut self) { self.instruction_body.push(op::OP_I64SHL); }
    pub fn shr_u_i32(&mut self) { self.instruction_body.push(op::OP_I32SHRU); }
    pub fn shr_u_i64(&mut self) { self.instruction_body.push(op::OP_I64SHRU); }
    pub fn shr_s_i32(&mut self) { self.instruction_body.push(op::OP_I32SHRS); }

    pub fn eq_i32(&mut self) { self.instruction_body.push(op::OP_I32EQ); }
    pub fn eq_i64(&mut self) { self.instruction_body.push(op::OP_I64EQ); }
    pub fn ne_i32(&mut self) { self.instruction_body.push(op::OP_I32NE); }
    pub fn ne_i64(&mut self) { self.instruction_body.push(op::OP_I64NE); }

    pub fn le_i32(&mut self) { self.instruction_body.push(op::OP_I32LES); }
    pub fn lt_i32(&mut self) { self.instruction_body.push(op::OP_I32LTS); }
    pub fn ge_i32(&mut self) { self.instruction_body.push(op::OP_I32GES); }
    pub fn gt_i32(&mut self) { self.instruction_body.push(op::OP_I32GTS); }

    pub fn gtu_i32(&mut self) { self.instruction_body.push(op::OP_I32GTU); }
    pub fn geu_i32(&mut self) { self.instruction_body.push(op::OP_I32GEU); }
    pub fn ltu_i32(&mut self) { self.instruction_body.push(op::OP_I32LTU); }
    pub fn leu_i32(&mut self) { self.instruction_body.push(op::OP_I32LEU); }

    pub fn gtu_i64(&mut self) { self.instruction_body.push(op::OP_I64GTU); }

    pub fn reinterpret_i32_as_f32(&mut self) {
        self.instruction_body.push(op::OP_F32REINTERPRETI32);
    }
    //pub fn reinterpret_f32_as_i32(&mut self) {
    //    self.instruction_body.push(op::OP_I32REINTERPRETF32);
    //}
    pub fn reinterpret_i64_as_f64(&mut self) {
        self.instruction_body.push(op::OP_F64REINTERPRETI64);
    }
    //pub fn reinterpret_f64_as_i64(&mut self) {
    //    self.instruction_body.push(op::OP_I64REINTERPRETF64);
    //}
    //pub fn promote_f32_to_f64(&mut self) { self.instruction_body.push(op::OP_F64PROMOTEF32); }
    //pub fn demote_f64_to_f32(&mut self) { self.instruction_body.push(op::OP_F32DEMOTEF64); }
    //pub fn convert_i32_to_f64(&mut self) { self.instruction_body.push(op::OP_F64CONVERTSI32); }
    //pub fn convert_i64_to_f64(&mut self) { self.instruction_body.push(op::OP_F64CONVERTSI64); }
    pub fn extend_unsigned_i32_to_i64(&mut self) {
        self.instruction_body.push(op::OP_I64EXTENDUI32);
    }
    pub fn extend_signed_i32_to_i64(&mut self) { self.instruction_body.push(op::OP_I64EXTENDSI32); }
    pub fn wrap_i64_to_i32(&mut self) { self.instruction_body.push(op::OP_I32WRAPI64); }

    pub fn eqz_i32(&mut self) { self.instruction_body.push(op::OP_I32EQZ); }

    pub fn select(&mut self) { self.instruction_body.push(op::OP_SELECT); }

    pub fn if_i32(&mut self) {
        self.open_block();
        self.instruction_body.push(op::OP_IF);
        self.instruction_body.push(op::TYPE_I32);
    }
    #[allow(dead_code)]
    pub fn if_i64(&mut self) {
        self.open_block();
        self.instruction_body.push(op::OP_IF);
        self.instruction_body.push(op::TYPE_I64);
    }
    #[allow(dead_code)]
    pub fn block_i32(&mut self) {
        self.open_block();
        self.instruction_body.push(op::OP_BLOCK);
        self.instruction_body.push(op::TYPE_I32);
    }

    pub fn if_void(&mut self) {
        self.open_block();
        self.instruction_body.push(op::OP_IF);
        self.instruction_body.push(op::TYPE_VOID_BLOCK);
    }

    pub fn else_(&mut self) {
        dbg_assert!(!self.label_stack.is_empty());
        self.instruction_body.push(op::OP_ELSE);
    }

    pub fn loop_void(&mut self) -> Label {
        self.instruction_body.push(op::OP_LOOP);
        self.instruction_body.push(op::TYPE_VOID_BLOCK);
        self.open_block()
    }

    pub fn block_void(&mut self) -> Label {
        self.instruction_body.push(op::OP_BLOCK);
        self.instruction_body.push(op::TYPE_VOID_BLOCK);
        self.open_block()
    }

    pub fn block_end(&mut self) {
        self.close_block();
        self.instruction_body.push(op::OP_END);
    }

    pub fn return_(&mut self) { self.instruction_body.push(op::OP_RETURN); }

    #[allow(dead_code)]
    pub fn drop_(&mut self) { self.instruction_body.push(op::OP_DROP); }

    pub fn brtable(
        &mut self,
        default_case: Label,
        cases: &mut dyn std::iter::ExactSizeIterator<Item = &Label>,
    ) {
        self.instruction_body.push(op::OP_BRTABLE);
        write_leb_u32(&mut self.instruction_body, wasm_len(cases.len()));
        for case in cases {
            self.write_label(*case);
        }
        self.write_label(default_case);
    }

    pub fn br(&mut self, label: Label) {
        self.instruction_body.push(op::OP_BR);
        self.write_label(label);
    }
    pub fn br_if(&mut self, label: Label) {
        self.instruction_body.push(op::OP_BRIF);
        self.write_label(label);
    }

    fn write_label(&mut self, label: Label) {
        let depth = *self.label_to_depth.get(&label).unwrap();
        dbg_assert!(depth <= self.label_stack.len());
        write_leb_u32(
            &mut self.instruction_body,
            wasm_len(self.label_stack.len() - depth),
        );
    }

    fn call_fn(&mut self, name: &str, function: FunctionType) {
        self.call_signature(name, function.signature());
    }

    pub fn call_fn0(&mut self, name: &str) { self.call_fn(name, FunctionType::FN0) }
    pub fn call_fn0_ret(&mut self, name: &str) { self.call_fn(name, FunctionType::FN0_RET) }
    pub fn call_fn0_ret_i64(&mut self, name: &str) { self.call_fn(name, FunctionType::FN0_RET_I64) }
    pub fn call_fn1(&mut self, name: &str) { self.call_fn(name, FunctionType::FN1) }
    pub fn call_fn1_i64(&mut self, name: &str) { self.call_fn(name, FunctionType::FN1_I64) }
    pub fn call_fn1_ret(&mut self, name: &str) { self.call_fn(name, FunctionType::FN1_RET) }
    pub fn call_fn1_f32_ret(&mut self, name: &str) { self.call_fn(name, FunctionType::FN1_F32_RET) }
    pub fn call_fn1_f64_ret(&mut self, name: &str) { self.call_fn(name, FunctionType::FN1_F64_RET) }
    pub fn call_fn2(&mut self, name: &str) { self.call_fn(name, FunctionType::FN2) }
    pub fn call_fn2_i32_i64(&mut self, name: &str) { self.call_fn(name, FunctionType::FN2_I32_I64) }
    pub fn call_fn2_i64_i32(&mut self, name: &str) { self.call_fn(name, FunctionType::FN2_I64_I32) }
    pub fn call_fn2_i64_i32_ret(&mut self, name: &str) {
        self.call_fn(name, FunctionType::FN2_I64_I32_RET)
    }
    pub fn call_fn2_i64_i32_ret_i64(&mut self, name: &str) {
        self.call_fn(name, FunctionType::FN2_I64_I32_RET_I64)
    }
    pub fn call_fn2_f32_i32(&mut self, name: &str) { self.call_fn(name, FunctionType::FN2_F32_I32) }
    pub fn call_fn2_ret(&mut self, name: &str) { self.call_fn(name, FunctionType::FN2_RET) }
    pub fn call_fn3(&mut self, name: &str) { self.call_fn(name, FunctionType::FN3) }
    pub fn call_fn3_ret(&mut self, name: &str) { self.call_fn(name, FunctionType::FN3_RET) }
    pub fn call_fn3_i64_i32_i32(&mut self, name: &str) {
        self.call_fn(name, FunctionType::FN3_I64_I32_I32)
    }
    pub fn call_fn3_i32_i64_i32(&mut self, name: &str) {
        self.call_fn(name, FunctionType::FN3_I32_I64_I32)
    }
    pub fn call_fn3_i32_i64_i32_ret(&mut self, name: &str) {
        self.call_fn(name, FunctionType::FN3_I32_I64_I32_RET)
    }
    pub fn call_fn4_i32_i64_i64_i32_ret(&mut self, name: &str) {
        self.call_fn(name, FunctionType::FN4_I32_I64_I64_I32_RET)
    }

    pub fn unreachable(&mut self) { self.instruction_body.push(op::OP_UNREACHABLE) }

    pub fn instruction_body_length(&self) -> u32 { wasm_len(self.instruction_body.len()) }
}

#[cfg(test)]
mod tests {
    use super::{FunctionType, WasmBuilder, WASM_MODULE_ARGUMENT_COUNT};
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn import_table_management() {
        let mut w = WasmBuilder::new();

        assert_eq!(0, w.get_fn_idx("foo", FunctionType::FN0));
        assert_eq!(1, w.get_fn_idx("bar", FunctionType::FN1));
        assert_eq!(0, w.get_fn_idx("foo", FunctionType::FN0));
        assert_eq!(2, w.get_fn_idx("baz", FunctionType::FN2));
    }

    #[test]
    fn builder_test() {
        let mut m = WasmBuilder::new();

        m.call_fn("foo", FunctionType::FN0);
        m.call_fn("bar", FunctionType::FN0);

        let local0 = m.alloc_local(); // for ensuring that reset clears previous locals
        m.free_local(local0);

        m.finish();
        m.reset();

        m.const_i32(2);

        m.call_fn("baz", FunctionType::FN1_RET);
        m.call_fn("foo", FunctionType::FN1);

        m.const_i32(10);
        let local1 = m.alloc_local();
        m.tee_local(&local1); // local1 = 10

        m.const_i32(20);
        m.add_i32();
        let local2 = m.alloc_local();
        m.tee_local(&local2); // local2 = 30

        m.free_local(local1);

        let local3 = m.alloc_local();
        assert_eq!(local3.idx(), WASM_MODULE_ARGUMENT_COUNT);

        m.free_local(local2);
        m.free_local(local3);

        m.const_i32(30);
        m.ne_i32();
        m.if_void();
        m.unreachable();
        m.block_end();

        m.finish();

        let op_ptr = m.get_output_ptr();
        let op_len = m.get_output_len();
        dbg_log!("op_ptr: {:?}, op_len: {:?}", op_ptr, op_len);

        let mut f = File::create("build/dummy_output.wasm").expect("creating dummy_output.wasm");
        f.write_all(&m.output).expect("write dummy_output.wasm");
    }
}

#[cfg(test)]
#[path = "../../../tests/ir/wasm/builder.rs"]
mod capacity_tests;
