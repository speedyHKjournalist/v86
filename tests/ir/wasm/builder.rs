use super::{Signature, WasmBuilder, WasmType};
use std::fs;

fn save(builder: &mut WasmBuilder, name: &str) {
    builder.finish();
    fs::create_dir_all("build/ir-wasm").unwrap();
    fs::write(format!("build/ir-wasm/{name}.wasm"), &builder.output).unwrap();
}

#[test]
fn capacity_modules() {
    for n in [127, 128, 255, 256, 1023, 1024] {
        let mut w = WasmBuilder::new();
        let mut ints = Vec::new();
        for i in 0..n {
            w.const_i32(i);
            ints.push(w.set_new_local());
        }
        // Keep all locals live, then read and tee indices across every LEB boundary.
        for (i, local) in ints.iter().enumerate() {
            w.const_i32(i as i32 * 4);
            w.get_local(local);
            w.tee_local(local);
            w.store_aligned_i32(0);
        }
        for local in ints {
            w.free_local(local);
        }
        save(&mut w, &format!("locals-{n}"));
        w.reset();
        let mut i32s = Vec::new();
        let mut i64s = Vec::new();
        let mut f32s = Vec::new();
        let mut f64s = Vec::new();
        let mut vectors = Vec::new();
        for i in 0..n {
            w.const_i32(i);
            i32s.push(w.set_new_local());
            w.const_i64(i as i64);
            i64s.push(w.tee_new_local_i64());
            w.drop_();
            w.const_i32(0);
            w.instruction_body.extend_from_slice(&[0x2A, 2, 0]);
            f32s.push(w.set_new_local_f32());
            w.const_i32(0);
            w.load_aligned_f64(0);
            f64s.push(w.set_new_local_f64());
            w.simd_zero();
            vectors.push(w.set_new_local_v128());
        }
        for i in 0..n as usize {
            w.const_i32((i * 4) as i32);
            w.get_local(&i32s[i]);
            w.store_aligned_i32(0);
            w.get_local_i64(&i64s[i]);
            w.drop_();
            w.get_local_f32(&f32s[i]);
            w.drop_();
            w.get_local_f64(&f64s[i]);
            w.drop_();
            w.get_local_v128(&vectors[i]);
            w.drop_();
        }
        for local in i32s {
            w.free_local(local);
        }
        for local in i64s {
            w.free_local_i64(local);
        }
        for local in f32s {
            w.free_local_f32(local);
        }
        for local in f64s {
            w.free_local_f64(local);
        }
        for local in vectors {
            w.free_local_v128(local);
        }
        // Reuse by type, without allocating another local.
        let count = w.local_types.len();
        w.const_i64(12);
        let local = w.set_new_local_i64();
        w.free_local_i64(local);
        assert_eq!(count, w.local_types.len());
        save(&mut w, &format!("groups-{n}"));
        w.reset();
        for i in 0..n {
            let mut k = i;
            let types = [
                WasmType::I32,
                WasmType::I64,
                WasmType::F32,
                WasmType::F64,
                WasmType::V128,
            ];
            let params: Vec<_> = (0..5)
                .map(|_| {
                    let t = types[k as usize % 5];
                    k /= 5;
                    t
                })
                .collect();
            let signature = Signature::new(&params, &[]);
            assert_eq!(w.intern_signature(signature.clone()), i as u32);
            assert_eq!(w.intern_signature(signature), i as u32);
        }
        for i in 0..n {
            w.call_fn0(&format!("{}-{i}", "h".repeat(130)));
        }
        save(&mut w, &format!("imports-{n}"));
        w.reset();
        let outer = w.block_void();
        for _ in 0..n {
            w.block_void();
        }
        w.br(outer);
        w.unreachable();
        for _ in 0..=n {
            w.block_end();
        }
        save(&mut w, &format!("depth-{n}"));
    }
}

#[test]
#[should_panic(expected = "signature mismatch")]
fn incompatible_import_is_rejected() {
    let mut w = WasmBuilder::new();
    w.call_fn0("helper");
    w.call_fn1("helper");
}

#[test]
#[should_panic(expected = "freed twice")]
fn double_free_is_rejected_in_release_too() {
    let mut w = WasmBuilder::new();
    w.const_i32(1);
    let local = w.set_new_local();
    let duplicate = local.unsafe_clone();
    w.free_local(local);
    w.free_local(duplicate);
}

#[test]
#[should_panic(expected = "all locals must be freed")]
fn unfreed_local_is_rejected() {
    let mut w = WasmBuilder::new();
    w.const_i32(1);
    let _local = w.set_new_local();
    w.finish();
}

#[test]
fn vector_wasm_helper_and_multivalue_signature() {
    let mut w = WasmBuilder::new();
    w.const_i32(0);
    w.simd_memory(0, 4);
    w.call_signature(
        "vector_identity",
        Signature::new(&[WasmType::V128], &[WasmType::V128]),
    );
    let vector = w.set_new_local_v128();
    w.const_i32(16);
    w.get_local_v128(&vector);
    w.simd_memory(0x0B, 4);
    w.free_local_v128(vector);
    w.call_signature("pair", Signature::new(&[], &[WasmType::I32, WasmType::I64]));
    let wide = w.set_new_local_i64();
    let narrow = w.set_new_local();
    w.const_i32(32);
    w.get_local(&narrow);
    w.store_aligned_i32(0);
    w.const_i32(40);
    w.get_local_i64(&wide);
    w.store_aligned_i64(0);
    w.free_local(narrow);
    w.free_local_i64(wide);
    save(&mut w, "typed-helpers");
}
