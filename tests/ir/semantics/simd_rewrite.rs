use super::{run, DEFAULT_WORK_LIMIT};
use crate::ir::{
    backend::wasm::emit_cpu,
    frontend::{decode::GuestEip, integer::IntegerBuilder},
    hir::*,
    ids::*,
    lowering::lower,
    simd::PackedOp,
    state::{ResumeKind, StateMap},
    types::Type,
    verify::verify,
};
const CASES: usize = 19;
fn fixture(case: usize) -> Region {
    let mut b = IntegerBuilder::new();
    let x: Vec<_> = (0..8)
        .map(|reg| b.node(Op::ReadXmm(reg), vec![], Type::V128))
        .collect();
    b.xmm = x.clone();
    let identity = std::array::from_fn(|i| i as u8);
    let alternating = std::array::from_fn(|i| i as u8 + if i % 2 == 0 { 0 } else { 16 });
    match case {
        0 => b.xmm[0] = b.node(Op::VectorShuffle(identity), vec![x[0], x[1]], Type::V128),
        1 => {
            b.xmm[0] = b.node(
                Op::VectorShuffle(std::array::from_fn(|i| 16 + i as u8)),
                vec![x[0], x[1]],
                Type::V128,
            )
        },
        2 => b.xmm[0] = b.node(Op::VectorShuffle(alternating), vec![x[0], x[0]], Type::V128),
        3 => {
            let inner = b.node(Op::VectorShuffle(alternating), vec![x[0], x[1]], Type::V128);
            b.xmm[0] = b.node(
                Op::VectorShuffle(std::array::from_fn(|i| 15 - i as u8)),
                vec![inner, inner],
                Type::V128,
            );
        },
        4 => {
            let left = b.node(Op::VectorShuffle(alternating), vec![x[0], x[1]], Type::V128);
            let right = b.node(Op::VectorShuffle(alternating), vec![x[2], x[3]], Type::V128);
            let mask = std::array::from_fn(|i| i as u8 + if i % 4 < 2 { 0 } else { 16 });
            b.xmm[0] = b.node(Op::VectorShuffle(mask), vec![left, right], Type::V128);
        },
        5 | 6 | 7 | 10 => {
            let bits = if case == 6 {
                64
            } else if case == 7 {
                16
            } else {
                32
            };
            let lane = if bits == 64 { 1 } else { 2 };
            let scalar = if bits == 64 {
                let low = b.node(Op::Extend { signed: false }, vec![b.gpr[2]], Type::I64);
                let high = b.node(Op::Extend { signed: false }, vec![b.gpr[3]], Type::I64);
                let shift = b.node(Op::Const(32), vec![], Type::I64);
                let high = b.binary(Binary::Shl, high, shift);
                b.binary(Binary::Or, low, high)
            } else {
                b.gpr[2]
            };
            let replaced = b.node(
                Op::VectorReplace { bits, lane },
                vec![x[0], scalar],
                Type::V128,
            );
            b.xmm[0] = replaced;
            let extracted = b.node(
                Op::VectorExtract {
                    bits,
                    lane: if case == 10 { 0 } else { lane },
                },
                vec![replaced],
                if bits == 64 { Type::I64 } else { Type::I32 },
            );
            if bits == 64 {
                b.gpr[0] = b.node(Op::Truncate, vec![extracted], Type::I32);
                let shift = b.node(Op::Const(32), vec![], Type::I64);
                let high = b.binary(Binary::Shr, extracted, shift);
                b.gpr[1] = b.node(Op::Truncate, vec![high], Type::I32);
            } else {
                b.gpr[0] = extracted;
            }
        },
        8 => {
            let extracted = b.node(
                Op::VectorExtract { bits: 16, lane: 7 },
                vec![x[0]],
                Type::I32,
            );
            b.xmm[0] = b.node(
                Op::VectorReplace { bits: 16, lane: 7 },
                vec![x[0], extracted],
                Type::V128,
            );
        },
        9 => {
            let inner = b.node(
                Op::VectorReplace { bits: 32, lane: 3 },
                vec![x[0], b.gpr[2]],
                Type::V128,
            );
            b.xmm[1] = inner; // Recovery keeps the overwritten insert live.
            b.xmm[0] = b.node(
                Op::VectorReplace { bits: 32, lane: 3 },
                vec![inner, b.gpr[3]],
                Type::V128,
            );
        },
        11..=18 => {
            let op = [
                PackedOp::And,
                PackedOp::Or,
                PackedOp::MinU8,
                PackedOp::MaxU8,
                PackedOp::MinS16,
                PackedOp::MaxS16,
                PackedOp::AvgU8,
                PackedOp::AvgU16,
            ][case - 11];
            b.xmm[0] = b.node(Op::VectorBinary(op), vec![x[0], x[0]], Type::V128);
        },
        _ => unreachable!(),
    }
    let state = b.region.state(StateMap {
        instruction_pc: GuestEip(0x1000),
        next_pc: GuestEip(0x1001),
        next_value: None,
        resume: ResumeKind::BeforeInstruction,
        gpr: b.gpr,
        flags: b.flags.clone(),
        xmm: b.xmm,
        x87: vec![],
        committed_instructions: 0,
        count_base: None,
        rep_progress: None,
    });
    b.region.terminate(b.block, Terminator::Exit(state));
    b.region
}
#[test]
fn rewrites_vectors_without_losing_lane_width_or_recovery_uses() {
    for case in 0..CASES {
        let mut r = fixture(case);
        verify(&r).unwrap();
        let stats = run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
        if case == 4 || case == 7 {
            assert_eq!(stats.rewritten, 0, "case {case}");
        } else {
            assert!(stats.rewritten > 0, "case {case}");
        }
        verify(&r).unwrap();
        lower(&r).unwrap();
        assert_eq!(
            run(&mut r, DEFAULT_WORK_LIMIT).unwrap().rewritten,
            0,
            "fixed point {case}"
        );
    }
}
#[test]
fn preserves_effect_nodes_and_rejects_bad_or_overbudget_transactions() {
    let mut r = fixture(0);
    let term = r.blocks[0].terminator.take().unwrap();
    let Terminator::Exit(state) = term else {
        unreachable!()
    };
    let effect = r.blocks[0].params[0];
    let guard = r.append(
        BlockId(0),
        Op::SseCheck,
        vec![effect],
        &[Type::Effect],
        Some(state),
    )[0];
    r.terminate(BlockId(0), Terminator::Exit(state));
    let Definition::Instruction(id, _) = r.values[guard.index()].definition else {
        unreachable!()
    };
    verify(&r).unwrap();
    run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
    assert!(r.blocks[0].instructions.contains(&id));
    assert_eq!(r.instructions[id.index()].op, Op::SseCheck);
    assert_eq!(r.instructions[id.index()].state, Some(state));
    let original = fixture(3);
    let work = run(&mut original.clone(), DEFAULT_WORK_LIMIT).unwrap().work;
    for budget in [0, 1, work / 2, work - 1] {
        let mut r = original.clone();
        let before = format!("{r:?}");
        assert!(run(&mut r, budget).is_err());
        assert_eq!(format!("{r:?}"), before);
    }
    let mut r = fixture(0);
    let id = *r.blocks[0].instructions.last().unwrap();
    r.instructions[id.index()].op = Op::VectorShuffle([32; 16]);
    let before = format!("{r:?}");
    assert!(run(&mut r, DEFAULT_WORK_LIMIT).is_err());
    assert_eq!(format!("{r:?}"), before);
}
#[test]
fn emits_vector_rewrite_oracles() {
    std::fs::create_dir_all("build/ir-simd-rewrite").unwrap();
    for case in 0..CASES {
        for opt in [false, true] {
            let mut r = fixture(case);
            if opt {
                run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
            }
            let mir = lower(&r).unwrap();
            drop(r);
            std::fs::write(
                format!("build/ir-simd-rewrite/{case}-{opt}.wasm"),
                emit_cpu(&mir, 100).unwrap().bytes,
            )
            .unwrap();
        }
    }
}
