use super::*;
use crate::ir::{
    backend::wasm::{emit, emit_cpu, StateLayout},
    frontend::{decode::GuestEip, integer::IntegerBuilder},
    lowering::lower,
    state::{ResumeKind, StateMap},
    types::Type,
};
fn finish(mut b: IntegerBuilder) -> Region {
    let state = b.region.state(StateMap {
        instruction_pc: GuestEip(0x1000),
        next_pc: GuestEip(0x1002),
        next_value: None,
        resume: ResumeKind::AfterInstruction,
        gpr: b.gpr,
        flags: b.flags,
        xmm: b.xmm,
        x87: vec![],
        committed_instructions: 1,
        count_base: None,
        rep_progress: None,
    });
    b.region.terminate(b.block, Terminator::Exit(state));
    b.region
}
fn scalar_region(bits: u8, kind: u8) -> Region {
    let mut b = IntegerBuilder::new();
    let ty = match bits {
        1 => Type::I1,
        8 => Type::I8,
        16 => Type::I16,
        32 => Type::I32,
        _ => Type::I64,
    };
    let x = if ty == Type::I64 {
        let low = b.node(Op::Extend { signed: false }, vec![b.gpr[0]], ty);
        b.node(Op::Insert { lsb: 32 }, vec![low, b.gpr[1]], ty)
    } else if ty == Type::I32 {
        b.gpr[0]
    } else {
        b.extract(b.gpr[0], 0, ty)
    };
    let zero = b.node(Op::Const(0), vec![], ty);
    let one = b.node(Op::Const(1), vec![], ty);
    let mask = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
    let max = b.node(Op::Const(mask), vec![], ty);
    let value = match kind {
        0 => b.binary(Binary::Add, x, zero),
        1 => b.binary(Binary::Sub, x, zero),
        2 => b.binary(Binary::Mul, x, one),
        3 => b.binary(Binary::And, max, x),
        4 => b.binary(Binary::Or, x, zero),
        5 => b.binary(Binary::Xor, zero, x),
        6 => b.binary(Binary::Shl, x, zero),
        7 => b.binary(Binary::Shr, x, zero),
        8 => b.binary(Binary::Sar, x, zero),
        9 => b.binary(Binary::Sub, x, x),
        10 => b.binary(Binary::Xor, x, x),
        11 => b.binary(Binary::Eq, x, x),
        12 => b.binary(Binary::Ult, x, x),
        13 => b.node(Op::Select, vec![b.flags.arithmetic[0], x, x], ty),
        14 => b.binary(Binary::Or, x, max),
        15 => b.binary(Binary::Mul, x, zero),
        16 => b.binary(Binary::And, x, x),
        _ => b.binary(Binary::Or, x, x),
    };
    let value_ty = b.ty(value);
    b.gpr[0] = match value_ty {
        Type::I32 => value,
        Type::I64 => b.extract(value, 0, Type::I32),
        _ => b.node(Op::Extend { signed: false }, vec![value], Type::I32),
    };
    b.gpr[1] = if value_ty == Type::I64 {
        b.extract(value, 32, Type::I32)
    } else {
        b.constant(0, Type::I32)
    };
    finish(b)
}
fn vector_region(kind: u8) -> Region {
    let mut b = IntegerBuilder::new();
    b.xmm = (0..8)
        .map(|i| b.node(Op::ReadXmm(i), vec![], Type::V128))
        .collect();
    let a = b.xmm[0];
    let c = b.xmm[1];
    let d = b.xmm[2];
    let reverse = std::array::from_fn(|i| (15 - i) as u8);
    let identity = std::array::from_fn(|i| i as u8);
    let swapped = std::array::from_fn(|i| (16 + i) as u8);
    b.xmm[0] = match kind {
        0 => b.node(Op::VectorShuffle(identity), vec![a, c], Type::V128),
        1 => b.node(Op::VectorShuffle(swapped), vec![a, c], Type::V128),
        2 => {
            let first = b.node(Op::VectorShuffle(reverse), vec![a, c], Type::V128);
            b.node(Op::VectorShuffle(reverse), vec![first, first], Type::V128)
        },
        3 | 4 => {
            let half = std::array::from_fn(|i| if i < 8 { i as u8 } else { (16 + i) as u8 });
            let first = b.node(Op::VectorShuffle(half), vec![a, c], Type::V128);
            if kind == 3 {
                b.node(Op::VectorShuffle(reverse), vec![first, first], Type::V128)
            } else {
                // Three source vectors: this must remain a two-shuffle chain.
                let lanes = std::array::from_fn(|i| if i < 12 { i as u8 } else { (16 + i) as u8 });
                b.node(Op::VectorShuffle(lanes), vec![first, d], Type::V128)
            }
        },
        5 => b.node(Op::VectorBinary(PackedOp::MinU8), vec![a, a], Type::V128),
        6 => b.node(Op::VectorBinary(PackedOp::And), vec![a, a], Type::V128),
        7 => b.node(Op::VectorBinary(PackedOp::AvgU16), vec![a, a], Type::V128),
        8 | 10 | 11 | 13 => {
            let bits = if kind == 10 || kind == 13 { 16 } else { 32 };
            let replaced = b.node(
                Op::VectorReplace { bits, lane: 1 },
                vec![a, b.gpr[0]],
                Type::V128,
            );
            b.gpr[1] = b.node(
                Op::VectorExtract {
                    bits: if kind == 13 { 32 } else { bits },
                    lane: if kind == 11 || kind == 13 { 0 } else { 1 },
                },
                vec![replaced],
                Type::I32,
            );
            replaced
        },
        9 => {
            let low = b.node(Op::Extend { signed: false }, vec![b.gpr[0]], Type::I64);
            let wide = b.node(Op::Insert { lsb: 32 }, vec![low, b.gpr[1]], Type::I64);
            let replaced = b.node(
                Op::VectorReplace { bits: 64, lane: 1 },
                vec![a, wide],
                Type::V128,
            );
            let value = b.node(
                Op::VectorExtract { bits: 64, lane: 1 },
                vec![replaced],
                Type::I64,
            );
            b.gpr[2] = b.extract(value, 0, Type::I32);
            b.gpr[3] = b.extract(value, 32, Type::I32);
            replaced
        },
        _ => {
            let first = b.node(
                Op::VectorReplace { bits: 32, lane: 2 },
                vec![a, b.gpr[0]],
                Type::V128,
            );
            b.node(
                Op::VectorReplace { bits: 32, lane: 2 },
                vec![first, b.gpr[1]],
                Type::V128,
            )
        },
    };
    finish(b)
}

#[test]
fn scalar_and_vector_identity_fixtures() {
    std::fs::create_dir_all("build/ir-canonicalize").unwrap();
    let layout = StateLayout {
        gpr: 0,
        flags: 32,
        eip: 36,
        committed: 40,
        flag_operand: 44,
    };
    for bits in [1, 8, 16, 32, 64] {
        for kind in 0..18 {
            let original = scalar_region(bits, kind);
            for opt in 0..2 {
                let mut r = original.clone();
                if opt != 0 {
                    assert!(
                        run(&mut r, DEFAULT_WORK_LIMIT).unwrap().aliases > 0
                            || matches!(kind, 9..=12 | 14 | 15)
                    );
                }
                std::fs::write(
                    format!("build/ir-canonicalize/scalar-{bits}-{kind}-{opt}.wasm"),
                    emit(&lower(&r).unwrap(), layout, 100).unwrap().bytes,
                )
                .unwrap();
            }
        }
    }
    for kind in 0..14 {
        let original = vector_region(kind);
        for opt in 0..2 {
            let mut r = original.clone();
            if opt != 0 {
                run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
            }
            std::fs::write(
                format!("build/ir-canonicalize/vector-{kind}-{opt}.wasm"),
                emit_cpu(&lower(&r).unwrap(), 100).unwrap().bytes,
            )
            .unwrap();
        }
    }
}

#[test]
fn sixteen_bit_lane_forwarding_does_not_alias_unmasked_i32() {
    let mut r = vector_region(10);
    run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
    let v = r.states.last().unwrap().gpr[1];
    assert!(matches!(
        definition(&r, v).unwrap().op,
        Op::VectorExtract { bits: 16, lane: 1 }
    ));
    for kind in [8, 9, 11, 12] {
        let mut r = vector_region(kind);
        assert!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().vectors > 0);
        verify(&r).unwrap();
    }
}

#[test]
fn aliases_rewrite_recovery_only_values_and_leave_effects_untouched() {
    let mut b = IntegerBuilder::new();
    let x = b.gpr[0];
    let zero = b.constant(0, Type::I32);
    let identity = b.binary(Binary::Add, x, zero);
    b.gpr[0] = identity;
    // This value is used only by the exit StateMap, not any executable instruction.
    let mut r = finish(b);
    assert!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().aliases > 0);
    assert_eq!(r.states[0].gpr[0], x);
    for block in &r.blocks {
        for id in &block.instructions {
            assert!(!r.instructions[id.index()].results.contains(&identity));
        }
    }
    verify(&r).unwrap();
    // An explicit observation on an otherwise pure node prevents its removal.
    let mut r = scalar_region(32, 0);
    let state = r.states[0].clone();
    let mut observation = state;
    for register in 0..2 {
        observation.gpr[register] = r
            .instructions
            .iter()
            .find(|i| i.op == Op::ReadGpr(register as u8))
            .unwrap()
            .results[0];
    }
    let state = r.state(observation);
    let target = r
        .instructions
        .iter()
        .position(|i| i.op == Op::Binary(Binary::Add))
        .unwrap();
    r.instructions[target].state = Some(state);
    verify(&r).unwrap();
    run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
    assert!(r.blocks[0].instructions.contains(&InstId(target as u32)));
}

#[test]
fn budget_and_validation_failure_are_atomic() {
    let original = vector_region(3);
    let mut good = original.clone();
    let cost = run(&mut good, DEFAULT_WORK_LIMIT).unwrap().work;
    for budget in [0, 1, cost / 2, cost - 1] {
        let mut r = original.clone();
        let before = format!("{r:?}");
        assert!(run(&mut r, budget).is_err());
        assert_eq!(format!("{r:?}"), before);
    }
    let mut exact = original.clone();
    assert!(run(&mut exact, cost).is_ok());
    let mut invalid = original;
    invalid.entries[0] = BlockId(u32::MAX);
    let before = format!("{invalid:?}");
    assert!(run(&mut invalid, DEFAULT_WORK_LIMIT).is_err());
    assert_eq!(format!("{invalid:?}"), before);
}
