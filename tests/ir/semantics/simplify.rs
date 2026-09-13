use super::{run, DEFAULT_WORK_LIMIT};
use crate::ir::{
    backend::wasm::{emit, emit_cpu, StateLayout},
    frontend::{decode::GuestEip, integer::IntegerBuilder},
    hir::*,
    ids::*,
    lowering::lower,
    passes::{self, PassConfig},
    simd::PackedOp,
    state::{ResumeKind, StateMap},
    types::Type,
    verify::verify,
};
fn snapshot(b: &mut IntegerBuilder, after: bool) -> StateId {
    b.region.state(StateMap {
        instruction_pc: GuestEip(0x9000),
        next_pc: GuestEip(0x9001),
        next_value: None,
        resume: if after { ResumeKind::AfterInstruction } else { ResumeKind::BeforeInstruction },
        gpr: b.gpr,
        flags: b.flags.clone(),
        xmm: b.xmm.clone(),
        x87: vec![],
        committed_instructions: u32::from(after),
        count_base: None,
        rep_progress: None,
    })
}
fn initial_vector() -> IntegerBuilder {
    let mut b = IntegerBuilder::new();
    b.xmm = (0..8)
        .map(|i| b.node(Op::ReadXmm(i), vec![], Type::V128))
        .collect();
    let before = snapshot(&mut b, false);
    b.effect = b.region.append(
        b.block,
        Op::SseCheck,
        vec![b.effect],
        &[Type::Effect],
        Some(before),
    )[0];
    b
}
fn finish(mut b: IntegerBuilder) -> Region {
    let exit = snapshot(&mut b, true);
    b.region.terminate(b.block, Terminator::Exit(exit));
    verify(&b.region).unwrap();
    b.region
}
fn vector_case(case: usize) -> Region {
    let mut b = initial_vector();
    let [a, c, d] = [b.xmm[1], b.xmm[2], b.xmm[3]];
    b.xmm[0] = match case {
        0 => b.node(
            Op::VectorShuffle(std::array::from_fn(|i| i as u8)),
            vec![a, c],
            Type::V128,
        ),
        1 => {
            let first = b.node(
                Op::VectorShuffle(std::array::from_fn(|i| {
                    if i < 8 {
                        i as u8
                    } else {
                        i as u8 + 8
                    }
                })),
                vec![a, c],
                Type::V128,
            );
            b.node(
                Op::VectorShuffle(std::array::from_fn(|i| (15 - i) as u8)),
                vec![first, first],
                Type::V128,
            )
        },
        2 => {
            let first = b.node(
                Op::VectorShuffle(std::array::from_fn(|i| {
                    if i < 8 {
                        i as u8
                    } else {
                        i as u8 + 8
                    }
                })),
                vec![a, c],
                Type::V128,
            );
            b.node(
                Op::VectorShuffle(std::array::from_fn(|i| {
                    if i < 12 {
                        i as u8
                    } else {
                        (i + 4) as u8
                    }
                })),
                vec![first, d],
                Type::V128,
            )
        },
        3 => {
            let v = b.node(
                Op::VectorReplace { bits: 16, lane: 3 },
                vec![a, b.gpr[0]],
                Type::V128,
            );
            b.gpr[0] = b.node(Op::VectorExtract { bits: 16, lane: 3 }, vec![v], Type::I32);
            v
        },
        4 => {
            let v = b.node(
                Op::VectorReplace { bits: 32, lane: 1 },
                vec![a, b.gpr[0]],
                Type::V128,
            );
            let v = b.node(
                Op::VectorReplace { bits: 32, lane: 1 },
                vec![v, b.gpr[2]],
                Type::V128,
            );
            b.gpr[0] = b.node(Op::VectorExtract { bits: 32, lane: 1 }, vec![v], Type::I32);
            v
        },
        5 => {
            let value = b.node(Op::Extend { signed: true }, vec![b.gpr[0]], Type::I64);
            let v = b.node(
                Op::VectorReplace { bits: 64, lane: 0 },
                vec![a, value],
                Type::V128,
            );
            b.gpr[0] = b.node(Op::VectorExtract { bits: 32, lane: 3 }, vec![v], Type::I32);
            v
        },
        6 => {
            let v = b.node(
                Op::VectorReplace { bits: 32, lane: 1 },
                vec![a, b.gpr[0]],
                Type::V128,
            );
            let wide = b.node(Op::VectorExtract { bits: 64, lane: 0 }, vec![v], Type::I64);
            let shift = b.node(Op::Const(32), vec![], Type::I64);
            let hi = b.binary(Binary::Shr, wide, shift);
            b.gpr[0] = b.node(Op::Truncate, vec![hi], Type::I32);
            v
        },
        7 => b.node(Op::VectorBinary(PackedOp::Xor), vec![a, a], Type::V128),
        8 => b.node(Op::VectorBinary(PackedOp::Eq16), vec![a, a], Type::V128),
        9 => {
            let zero = b.node(
                Op::VectorBinary(PackedOp::SubSatS16),
                vec![c, c],
                Type::V128,
            );
            b.node(Op::VectorBinary(PackedOp::Shr32), vec![a, zero], Type::V128)
        },
        10 => {
            let a = b.node(
                Op::VectorConst(std::array::from_fn(|i| (i * 17) as u8)),
                vec![],
                Type::V128,
            );
            let c = b.node(
                Op::VectorConst(std::array::from_fn(|i| (255 - i * 7) as u8)),
                vec![],
                Type::V128,
            );
            b.node(Op::VectorBinary(PackedOp::AndNot), vec![a, c], Type::V128)
        },
        11 => {
            let v = b.node(
                Op::VectorShuffle(std::array::from_fn(|i| ((i + 8) % 16) as u8)),
                vec![a, a],
                Type::V128,
            );
            b.gpr[0] = b.node(Op::VectorExtract { bits: 32, lane: 2 }, vec![v], Type::I32);
            v
        },
        12 => {
            let v = b.node(
                Op::VectorConst(std::array::from_fn(|i| (i * 17) as u8)),
                vec![],
                Type::V128,
            );
            let value = b.node(Op::Const(0xFFFF1234), vec![], Type::I32);
            let v = b.node(
                Op::VectorReplace { bits: 16, lane: 7 },
                vec![v, value],
                Type::V128,
            );
            b.gpr[0] = b.node(Op::VectorExtract { bits: 16, lane: 7 }, vec![v], Type::I32);
            v
        },
        _ => unreachable!(),
    };
    finish(b)
}
fn scalar_case(bits: u8, case: usize) -> Region {
    let mut b = IntegerBuilder::new();
    let ty = match bits {
        1 => Type::I1,
        8 => Type::I8,
        16 => Type::I16,
        32 => Type::I32,
        64 => Type::I64,
        _ => panic!(),
    };
    let input = b.gpr[0];
    let a = if bits < 32 {
        b.node(Op::Extract { lsb: 0 }, vec![input], ty)
    } else if bits == 64 {
        b.node(Op::Extend { signed: true }, vec![input], ty)
    } else {
        input
    };
    let value = match case {
        0 => {
            let zero = b.node(Op::Const(0), vec![], ty);
            b.binary(Binary::Add, a, zero)
        },
        1 => b.binary(Binary::Xor, a, a),
        2 => b.binary(Binary::Eq, a, a),
        3 => b.binary(Binary::Ult, a, a),
        4 => {
            let all = b.node(
                Op::Const(if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 }),
                vec![],
                ty,
            );
            b.binary(Binary::And, a, all)
        },
        5 => {
            let one = b.node(Op::Const(1), vec![], ty);
            b.binary(Binary::Mul, a, one)
        },
        6 => {
            let count = b.node(
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
            b.binary(Binary::Shl, a, count)
        },
        7 => {
            let zero = b.node(Op::Const(0), vec![], ty);
            b.binary(Binary::Sar, zero, a)
        },
        8 => {
            let mask = b.node(
                Op::Const(if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 }),
                vec![],
                ty,
            );
            b.binary(Binary::Or, a, mask)
        },
        9 => b.node(Op::Select, vec![b.flags.arithmetic[0], a, a], ty),
        _ => unreachable!(),
    };
    let result_ty = b.ty(value);
    b.gpr[0] = if result_ty == Type::I64 {
        b.node(Op::Truncate, vec![value], Type::I32)
    } else if result_ty == Type::I32 {
        value
    } else {
        b.node(Op::Extend { signed: false }, vec![value], Type::I32)
    };
    finish(b)
}
#[test]
fn byte_and_scalar_fixtures_survive_owned_mir_and_execute() {
    std::fs::create_dir_all("build/ir-simplify").unwrap();
    let mut total = 0;
    for case in 0..13 {
        for optimized in [false, true] {
            let mut r = vector_case(case);
            if optimized {
                let stats = run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
                total += stats.vector;
            }
            let mir = lower(&r).unwrap();
            drop(r);
            std::fs::write(
                format!("build/ir-simplify/vector-{case}-{optimized}.wasm"),
                emit_cpu(&mir, 100).unwrap().bytes,
            )
            .unwrap();
        }
    }
    assert!(total >= 12);
    for bits in [1, 8, 16, 32, 64] {
        for case in 0..10 {
            for optimized in [false, true] {
                let mut r = scalar_case(bits, case);
                if optimized {
                    assert!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().scalar > 0);
                }
                let mir = lower(&r).unwrap();
                drop(r);
                let layout = StateLayout {
                    gpr: 0,
                    flags: 32,
                    eip: 36,
                    committed: 40,
                    flag_operand: 44,
                };
                std::fs::write(
                    format!("build/ir-simplify/scalar-{bits}-{case}-{optimized}.wasm"),
                    emit(&mir, layout, 100).unwrap().bytes,
                )
                .unwrap();
            }
        }
    }
}
#[test]
fn effects_and_guard_recovery_survive_zero_idioms() {
    let mut r = vector_case(7);
    let before = r
        .instructions
        .iter()
        .find(|i| i.op == Op::SseCheck)
        .unwrap()
        .clone();
    let state = r.states[before.state.unwrap().index()].clone();
    let stats = passes::run(&mut r, PassConfig::default()).unwrap();
    assert!(stats.vector_simplified > 0);
    let after = r
        .instructions
        .iter()
        .find(|i| i.op == Op::SseCheck)
        .unwrap();
    assert_eq!(format!("{before:?}"), format!("{after:?}"));
    assert_eq!(
        format!("{state:?}"),
        format!("{:?}", r.states[after.state.unwrap().index()])
    );
    assert!(r
        .instructions
        .iter()
        .any(|i| i.op == Op::VectorConst([0; 16])));
    lower(&r).unwrap();
}
#[test]
fn multi_source_shuffles_and_overlapping_lanes_are_not_unsafely_forwarded() {
    let mut r = vector_case(2);
    run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
    let last = match r.blocks[0].terminator {
        Some(Terminator::Exit(s)) => r.states[s.index()].xmm[0],
        _ => panic!(),
    };
    let Definition::Instruction(id, _) = r.values[last.index()].definition else {
        panic!()
    };
    let inst = &r.instructions[id.index()];
    assert!(matches!(inst.op, Op::VectorShuffle(_)));
    let Definition::Instruction(inner, _) = r.values[inst.args[0].index()].definition else {
        panic!()
    };
    assert!(
        matches!(r.instructions[inner.index()].op, Op::VectorShuffle(_)),
        "three leaf sources require two shuffles"
    );
    let mut r = vector_case(6);
    run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
    let extract = r
        .instructions
        .iter()
        .find(|i| i.op == Op::VectorExtract { bits: 64, lane: 0 })
        .unwrap();
    let Definition::Instruction(id, _) = r.values[extract.args[0].index()].definition else {
        panic!()
    };
    assert_eq!(
        r.instructions[id.index()].op,
        Op::VectorReplace { bits: 32, lane: 1 }
    );
}
#[test]
fn budget_and_type_failure_leave_all_arenas_unchanged() {
    let original = vector_case(3);
    let mut r = original.clone();
    let work = run(&mut r, DEFAULT_WORK_LIMIT).unwrap().work;
    for budget in [0, 1, work / 2, work - 1] {
        let mut r = original.clone();
        assert!(run(&mut r, budget).unwrap_err().contains("budget"));
        assert_eq!(format!("{r:?}"), format!("{original:?}"));
    }
    let mut bad = vector_case(10);
    let result = bad
        .instructions
        .iter()
        .find(|i| matches!(i.op, Op::VectorConst(_)))
        .unwrap()
        .results[0];
    bad.values[result.index()].ty = Type::I64;
    let before = format!("{bad:?}");
    assert!(run(&mut bad, DEFAULT_WORK_LIMIT).is_err());
    assert_eq!(format!("{bad:?}"), before);
}
#[test]
fn individual_switch_and_zero_rounds_leave_simplification_disabled() {
    for config in [
        PassConfig {
            rounds: 0,
            ..PassConfig::default()
        },
        PassConfig {
            prune: false,
            merge: false,
            phis: false,
            fold: false,
            gvn: false,
            dce: false,
            simplify: false,
            simplify_work_limit: DEFAULT_WORK_LIMIT,
            licm: false,
            licm_work_limit: super::super::licm::DEFAULT_WORK_LIMIT,
            rounds: 1,
        },
    ] {
        let mut r = vector_case(7);
        let original = format!("{r:?}");
        let stats = passes::run(&mut r, config).unwrap();
        assert_eq!(stats.vector_simplified, 0);
        assert_eq!(format!("{r:?}"), original);
    }
}
