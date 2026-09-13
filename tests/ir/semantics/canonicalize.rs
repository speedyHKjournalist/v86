use crate::ir::{
    backend::wasm::{emit, emit_cpu, StateLayout},
    frontend::{
        decode::{GuestEip, LinearAddress},
        integer::IntegerBuilder,
        lift::lift_cpu,
    },
    hir::*,
    lowering::lower,
    passes::{run, PassConfig},
    simd::PackedOp,
    state::{ResumeKind, StateMap},
    types::Type,
    verify::verify,
};
fn only() -> PassConfig {
    PassConfig {
        prune: false,
        merge: false,
        phis: false,
        fold: false,
        canonicalize: true,
        gvn: false,
        licm: false,
        dce: false,
        rounds: 2,
    }
}
fn finish(mut b: IntegerBuilder) -> Region {
    // Deliberately live only through a recovery field, not an ordinary use.
    let zero = b.constant(0, Type::I32);
    b.flags.last_op1 = Some(b.binary(Binary::Add, b.gpr[7], zero));
    let state = b.region.state(StateMap {
        instruction_pc: GuestEip(0x1000),
        next_pc: GuestEip(0x1002),
        next_value: None,
        resume: ResumeKind::AfterInstruction,
        gpr: b.gpr,
        flags: b.flags,
        xmm: vec![],
        x87: vec![],
        committed_instructions: 1,
        count_base: None,
        rep_progress: None,
    });
    b.region.terminate(b.block, Terminator::Exit(state));
    b.region
}
fn scalar(ty: Type, kind: &str) -> Region {
    let mut b = IntegerBuilder::new();
    let bits = ty.bits().unwrap();
    let mut inputs = Vec::new();
    for reg in [0, 2] {
        let x = b.gpr[reg];
        let value = if ty == Type::I64 {
            let low = b.node(Op::Extend { signed: false }, vec![x], Type::I64);
            b.node(Op::Insert { lsb: 32 }, vec![low, b.gpr[reg + 1]], Type::I64)
        } else if ty == Type::I32 {
            x
        } else {
            b.extract(x, 0, ty)
        };
        inputs.push(value);
    }
    let x = inputs[0];
    let y = inputs[1];
    let zero = b.node(Op::Const(0), vec![], ty);
    let one = b.node(Op::Const(1), vec![], ty);
    let result = match kind {
        "add_zero" => b.binary(Binary::Add, x, zero),
        "sub_zero" => b.binary(Binary::Sub, x, zero),
        "or_zero" => b.binary(Binary::Or, zero, x),
        "xor_zero" => b.binary(Binary::Xor, x, zero),
        "mul_one" => b.binary(Binary::Mul, one, x),
        "and_mask" => {
            let mask = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
            let mask = b.node(Op::Const(mask), vec![], ty);
            b.binary(Binary::And, x, mask)
        },
        "self_and" => b.binary(Binary::And, x, x),
        "self_or" => b.binary(Binary::Or, x, x),
        "self_sub" => b.binary(Binary::Sub, x, x),
        "self_xor" => b.binary(Binary::Xor, x, x),
        "self_eq" => b.binary(Binary::Eq, x, x),
        "self_ult" => b.binary(Binary::Ult, x, x),
        "self_slt" => b.binary(Binary::Slt, x, x),
        "mul_zero" => b.binary(Binary::Mul, x, zero),
        "and_zero" => b.binary(Binary::And, zero, x),
        "zero_shl" => b.binary(Binary::Shl, zero, y),
        "masked_shl" | "masked_shr" | "masked_sar" => {
            let amount = b.node(
                Op::Const(if bits == 64 {
                    64
                } else if bits == 1 {
                    0
                } else {
                    32
                }),
                vec![],
                ty,
            );
            let op = match kind {
                "masked_shl" => Binary::Shl,
                "masked_shr" => Binary::Shr,
                _ => Binary::Sar,
            };
            b.binary(op, x, amount)
        },
        "select_same" => {
            let c = b.extract(b.gpr[2], 0, Type::I1);
            b.node(Op::Select, vec![c, x, x], ty)
        },
        "select_true" | "select_false" => {
            let yes = kind == "select_true";
            let c = b.constant(yes as u32, Type::I1);
            b.node(
                Op::Select,
                if yes { vec![c, x, y] } else { vec![c, y, x] },
                ty,
            )
        },
        "roundtrip_signed" | "roundtrip_unsigned" => {
            let wide = b.node(
                Op::Extend {
                    signed: kind == "roundtrip_signed",
                },
                vec![x],
                Type::I64,
            );
            b.node(Op::Truncate, vec![wide], ty)
        },
        "insert_extract" => {
            let (lsb, part_ty) = match ty {
                Type::I1 => (0, Type::I1),
                Type::I8 => (3, Type::I1),
                Type::I16 => (4, Type::I8),
                Type::I32 => (8, Type::I16),
                Type::I64 => (16, Type::I32),
                _ => unreachable!(),
            };
            let part = b.extract(x, lsb, part_ty);
            b.node(Op::Insert { lsb }, vec![x, part], ty)
        },
        "extract_insert" => {
            let lsb = if bits == 64 { 0 } else { 8 };
            let base = if ty == Type::I64 {
                y
            } else {
                b.node(Op::Extend { signed: false }, vec![y], Type::I64)
            };
            let inserted = b.node(Op::Insert { lsb }, vec![base, x], Type::I64);
            b.extract(inserted, lsb, ty)
        },
        "extract_other" => {
            let base = b.node(Op::Extend { signed: false }, vec![x], Type::I64);
            let part = b.extract(b.gpr[2], 0, Type::I8);
            let inserted = b.node(Op::Insert { lsb: bits }, vec![base, part], Type::I64);
            b.extract(inserted, 0, ty)
        },
        _ => panic!("unknown scalar fixture"),
    };
    b.gpr[0] = match b.ty(result) {
        Type::I32 => result,
        Type::I64 => b.extract(result, 0, Type::I32),
        _ => b.node(Op::Extend { signed: false }, vec![result], Type::I32),
    };
    b.gpr[1] = if b.ty(result) == Type::I64 {
        b.extract(result, 32, Type::I32)
    } else {
        b.constant(0, Type::I32)
    };
    finish(b)
}
#[test]
fn width_aware_identities_generate_independent_oracles() {
    std::fs::create_dir_all("build/ir-canonical").unwrap();
    let layout = StateLayout {
        gpr: 0,
        flags: 32,
        eip: 36,
        committed: 40,
        flag_operand: 44,
    };
    let mut cases = Vec::new();
    for ty in [Type::I1, Type::I8, Type::I16, Type::I32, Type::I64] {
        for kind in [
            "add_zero",
            "sub_zero",
            "or_zero",
            "xor_zero",
            "mul_one",
            "and_mask",
            "self_and",
            "self_or",
            "self_sub",
            "self_xor",
            "self_eq",
            "self_ult",
            "self_slt",
            "mul_zero",
            "and_zero",
            "zero_shl",
            "masked_shl",
            "masked_shr",
            "masked_sar",
            "select_same",
            "select_true",
            "select_false",
            "roundtrip_signed",
            "roundtrip_unsigned",
            "insert_extract",
            "extract_insert",
            "extract_other",
        ] {
            if ty == Type::I64
                && matches!(
                    kind,
                    "roundtrip_signed" | "roundtrip_unsigned" | "extract_other"
                )
            {
                continue;
            }
            let original = scalar(ty, kind);
            let index = cases.len();
            cases.push(format!("[{},\"{kind}\"]", ty.bits().unwrap()));
            for mode in 0..3 {
                let mut r = original.clone();
                if mode != 0 {
                    let stats = run(
                        &mut r,
                        if mode == 1 { only() } else { PassConfig::default() },
                    )
                    .unwrap();
                    assert!(stats.canonicalized > 0, "{ty:?}, {kind}, {mode}");
                }
                std::fs::write(
                    format!("build/ir-canonical/scalar-{index}-{mode}.wasm"),
                    emit(&lower(&r).unwrap(), layout, 100).unwrap().bytes,
                )
                .unwrap();
            }
        }
    }
    std::fs::write(
        "build/ir-canonical/scalar.json",
        format!("[{}]", cases.join(",")),
    )
    .unwrap();
}

fn vector(kind: &str, bits: u8, lane: u8) -> Region {
    let mut b = IntegerBuilder::new();
    let a = b.node(Op::ReadXmm(0), vec![], Type::V128);
    let c = b.node(Op::ReadXmm(1), vec![], Type::V128);
    let d = b.node(Op::ReadXmm(2), vec![], Type::V128);
    let scalar_ty = if bits == 64 { Type::I64 } else { Type::I32 };
    let scalar = if bits == 64 {
        let low = b.node(Op::Extend { signed: false }, vec![b.gpr[4]], Type::I64);
        b.node(Op::Insert { lsb: 32 }, vec![low, b.gpr[5]], Type::I64)
    } else {
        b.gpr[4]
    };
    let result = match kind {
        "identity_left" => b.node(
            Op::VectorShuffle(std::array::from_fn(|i| i as u8)),
            vec![a, c],
            Type::V128,
        ),
        "identity_right" => b.node(
            Op::VectorShuffle(std::array::from_fn(|i| i as u8 + 16)),
            vec![a, c],
            Type::V128,
        ),
        "identity_same" => b.node(
            Op::VectorShuffle(std::array::from_fn(|i| {
                i as u8 + if i % 2 == 0 { 0 } else { 16 }
            })),
            vec![a, a],
            Type::V128,
        ),
        "nested" | "three_sources" => {
            let inner = b.node(
                Op::VectorShuffle(std::array::from_fn(|i| (i as u8 * 3) % 32)),
                vec![a, c],
                Type::V128,
            );
            let other = if kind == "nested" { inner } else { d };
            b.node(
                Op::VectorShuffle(std::array::from_fn(|i| (i as u8 * 7 + 3) % 32)),
                vec![inner, other],
                Type::V128,
            )
        },
        "and_same" => b.node(Op::VectorBinary(PackedOp::And), vec![a, a], Type::V128),
        "or_same" => b.node(Op::VectorBinary(PackedOp::Or), vec![a, a], Type::V128),
        "restore" => {
            let old = b.node(Op::VectorExtract { bits, lane }, vec![a], scalar_ty);
            b.node(Op::VectorReplace { bits, lane }, vec![a, old], Type::V128)
        },
        "overwrite" => {
            let x = b.node(
                Op::VectorReplace { bits, lane },
                vec![a, scalar],
                Type::V128,
            );
            let old = b.node(Op::VectorExtract { bits, lane }, vec![c], scalar_ty);
            b.node(Op::VectorReplace { bits, lane }, vec![x, old], Type::V128)
        },
        "read_same" | "read_other" => {
            let x = b.node(
                Op::VectorReplace { bits, lane },
                vec![a, scalar],
                Type::V128,
            );
            let at = if kind == "read_same" { lane } else { (lane + 1) % (128 / bits) };
            let read = b.node(Op::VectorExtract { bits, lane: at }, vec![x], scalar_ty);
            b.gpr[0] = if bits == 64 { b.extract(read, 0, Type::I32) } else { read };
            b.gpr[1] =
                if bits == 64 { b.extract(read, 32, Type::I32) } else { b.constant(0, Type::I32) };
            return finish(b);
        },
        "extract_shuffle" | "extract_unaligned" => {
            let lanes = if kind == "extract_shuffle" {
                std::array::from_fn(|i| (i as u8 + 16) % 32)
            } else {
                std::array::from_fn(|i| (i as u8 * 3 + 1) % 32)
            };
            let x = b.node(Op::VectorShuffle(lanes), vec![a, c], Type::V128);
            let read = b.node(Op::VectorExtract { bits, lane }, vec![x], scalar_ty);
            b.gpr[0] = if bits == 64 { b.extract(read, 0, Type::I32) } else { read };
            b.gpr[1] =
                if bits == 64 { b.extract(read, 32, Type::I32) } else { b.constant(0, Type::I32) };
            return finish(b);
        },
        _ => panic!("unknown vector fixture"),
    };
    for lane in 0..4 {
        b.gpr[lane] = b.node(
            Op::VectorExtract {
                bits: 32,
                lane: lane as u8,
            },
            vec![result],
            Type::I32,
        );
    }
    finish(b)
}
#[test]
fn simd_lane_and_shuffle_oracles_cover_narrow_readback() {
    std::fs::create_dir_all("build/ir-canonical").unwrap();
    let mut cases = Vec::new();
    for kind in [
        "identity_left",
        "identity_right",
        "identity_same",
        "nested",
        "three_sources",
        "and_same",
        "or_same",
        "restore",
        "overwrite",
        "read_same",
        "read_other",
        "extract_shuffle",
        "extract_unaligned",
    ] {
        for bits in [16, 32, 64] {
            for lane in 0..128 / bits {
                let original = vector(kind, bits, lane);
                let index = cases.len();
                cases.push(format!("[\"{kind}\",{bits},{lane}]"));
                for mode in 0..3 {
                    let mut r = original.clone();
                    if mode != 0 {
                        run(
                            &mut r,
                            if mode == 1 { only() } else { PassConfig::default() },
                        )
                        .unwrap();
                    }
                    std::fs::write(
                        format!("build/ir-canonical/vector-{index}-{mode}.wasm"),
                        emit_cpu(&lower(&r).unwrap(), 100).unwrap().bytes,
                    )
                    .unwrap();
                }
            }
        }
    }
    std::fs::write(
        "build/ir-canonical/vector.json",
        format!("[{}]", cases.join(",")),
    )
    .unwrap();
}
#[test]
fn three_source_shuffles_and_narrow_insert_high_bits_are_not_aliased() {
    for (kind, bits) in [("three_sources", 32), ("read_same", 16)] {
        let mut r = vector(kind, bits, 0);
        let stats = run(&mut r, only()).unwrap();
        assert_eq!(stats.simd_simplified, 0, "{kind}, {bits}");
        verify(&r).unwrap();
    }
}
#[test]
fn dead_values_do_not_delete_faulting_memory_or_observation_points() {
    let mut r = lift_cpu(
        &[0x8B, 0x06, 0x31, 0xC0],
        GuestEip(0x1000),
        LinearAddress(0x1000),
        true,
    )
    .unwrap();
    let before: Vec<_> = r
        .instructions
        .iter()
        .filter(|i| i.op.ordered())
        .map(|i| i.op.clone())
        .collect();
    run(&mut r, PassConfig::default()).unwrap();
    let after: Vec<_> = r
        .blocks
        .iter()
        .flat_map(|b| b.instructions.iter().map(|id| &r.instructions[id.index()]))
        .filter(|i| i.op.ordered())
        .map(|i| i.op.clone())
        .collect();
    assert_eq!(before, after);
    assert!(after.iter().any(|op| matches!(op, Op::GuestLoad { .. })));
    let mut disabled = scalar(Type::I32, "mul_one");
    assert_eq!(
        run(
            &mut disabled,
            PassConfig {
                canonicalize: false,
                ..only()
            }
        )
        .unwrap()
        .canonicalized,
        0
    );
}
#[test]
fn commutative_normalization_exposes_dominator_gvn() {
    let mut b = IntegerBuilder::new();
    let x = b.gpr[0];
    let y = b.gpr[1];
    b.gpr[2] = b.binary(Binary::Mul, y, x);
    b.gpr[3] = b.binary(Binary::Mul, x, y);
    let mut r = finish(b);
    let stats = run(
        &mut r,
        PassConfig {
            gvn: true,
            ..only()
        },
    )
    .unwrap();
    assert!(stats.commoned > 0);
    let Terminator::Exit(state) = r.blocks[0].terminator.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(
        r.states[state.index()].gpr[2],
        r.states[state.index()].gpr[3]
    );
}
