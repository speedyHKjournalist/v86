use super::{run, DEFAULT_WORK_BUDGET};
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

fn builder() -> IntegerBuilder {
    let mut b = IntegerBuilder::new();
    b.xmm = (0..8)
        .map(|r| {
            b.region
                .append(b.block, Op::ReadXmm(r), vec![], &[Type::V128], None)[0]
        })
        .collect();
    b
}
fn finish(mut b: IntegerBuilder, vector: ValueId, scalar: Option<ValueId>) -> Region {
    b.xmm[0] = vector;
    if let Some(value) = scalar {
        b.gpr[0] = value;
    }
    let state = b.region.state(StateMap {
        instruction_pc: GuestEip(0x1000),
        next_pc: GuestEip(0x1001),
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
fn shuffle_case(first: [u8; 16], second: [u8; 16]) -> Region {
    let mut b = builder();
    let inner = b.node(
        Op::VectorShuffle(first),
        vec![b.xmm[0], b.xmm[1]],
        Type::V128,
    );
    let outer = b.node(Op::VectorShuffle(second), vec![inner, inner], Type::V128);
    finish(b, outer, None)
}
fn lane_case(kind: &str, bits: u8, lane: u8) -> Region {
    let mut b = builder();
    let ty = if bits == 64 { Type::I64 } else { Type::I32 };
    let value = if bits == 64 {
        let lo = b.node(Op::Extend { signed: false }, vec![b.gpr[0]], Type::I64);
        let hi = b.node(Op::Extend { signed: false }, vec![b.gpr[1]], Type::I64);
        let shift = b.node(Op::Const(32), vec![], Type::I64);
        let high = b.binary(Binary::Shl, hi, shift);
        b.binary(Binary::Or, lo, high)
    } else {
        b.gpr[0]
    };
    let vector = b.xmm[0];
    let replace = Op::VectorReplace { bits, lane };
    let extract = Op::VectorExtract { bits, lane };
    if kind == "roundtrip" {
        let scalar = b.node(extract, vec![vector], ty);
        let result = b.node(replace, vec![vector, scalar], Type::V128);
        finish(b, result, None)
    } else if kind == "forward" {
        let result = b.node(replace, vec![vector, value], Type::V128);
        let scalar = b.node(extract, vec![result], ty);
        let low = if bits == 64 { b.node(Op::Truncate, vec![scalar], Type::I32) } else { scalar };
        finish(b, result, Some(low))
    } else {
        let old = b.node(replace.clone(), vec![vector, value], Type::V128);
        let another = if bits == 64 {
            b.node(Op::Extend { signed: false }, vec![b.gpr[2]], Type::I64)
        } else {
            b.gpr[2]
        };
        let result = b.node(replace, vec![old, another], Type::V128);
        finish(b, result, None)
    }
}

fn overlap_case() -> Region {
    let mut b = builder();
    let low = b.node(Op::Extend { signed: false }, vec![b.gpr[0]], Type::I64);
    let high = b.node(Op::Extend { signed: false }, vec![b.gpr[1]], Type::I64);
    let shift = b.node(Op::Const(32), vec![], Type::I64);
    let high = b.binary(Binary::Shl, high, shift);
    let wide = b.binary(Binary::Or, low, high);
    let first = b.node(
        Op::VectorReplace { bits: 64, lane: 0 },
        vec![b.xmm[0], wide],
        Type::V128,
    );
    let second = b.node(
        Op::VectorReplace { bits: 32, lane: 1 },
        vec![first, b.gpr[2]],
        Type::V128,
    );
    finish(b, second, None)
}

#[test]
fn differently_sized_overlapping_writes_are_not_folded() {
    let mut r = overlap_case();
    let stats = run(&mut r, DEFAULT_WORK_BUDGET).unwrap();
    assert_eq!(stats.overwritten_lanes, 0);
    assert_eq!(stats.eliminated, 0);
    lower(&r).unwrap();
}

#[test]
fn composes_shuffle_masks_and_rewrites_snapshot_only_values() {
    let first = std::array::from_fn(|i| ((i * 7 + 3) % 32) as u8);
    let second = std::array::from_fn(|i| (31 - i) as u8);
    let mut r = shuffle_case(first, second);
    let stats = run(&mut r, DEFAULT_WORK_BUDGET).unwrap();
    assert_eq!(stats.composed, 1);
    let final_value = r.states[0].xmm[0];
    let Definition::Instruction(id, _) = r.values[final_value.index()].definition else {
        panic!()
    };
    assert_eq!(
        r.instructions[id.index()].op,
        Op::VectorShuffle(std::array::from_fn(|i| first[15 - i]))
    );
    verify(&r).unwrap();
    lower(&r).unwrap();
    for offset in [0, 16] {
        let mut identity = shuffle_case(first, std::array::from_fn(|i| i as u8 + offset));
        assert!(run(&mut identity, DEFAULT_WORK_BUDGET).unwrap().eliminated > 0);
    }
}

#[test]
fn lane_forwarding_respects_narrow_truncation_and_bitwise_identities() {
    for bits in [16, 32, 64] {
        for lane in 0..128 / bits {
            let mut r = lane_case("forward", bits, lane);
            let stats = run(&mut r, DEFAULT_WORK_BUDGET).unwrap();
            assert_eq!(stats.eliminated, usize::from(bits != 16));
            let mut roundtrip = lane_case("roundtrip", bits, lane);
            assert_eq!(
                run(&mut roundtrip, DEFAULT_WORK_BUDGET).unwrap().eliminated,
                1
            );
            let mut overwritten = lane_case("overwrite", bits, lane);
            assert_eq!(
                run(&mut overwritten, DEFAULT_WORK_BUDGET)
                    .unwrap()
                    .overwritten_lanes,
                1
            );
        }
    }
    for operation in [PackedOp::And, PackedOp::Or] {
        let mut b = builder();
        let v = b.xmm[0];
        let same = b.node(Op::VectorBinary(operation), vec![v, v], Type::V128);
        let mut r = finish(b, same, None);
        assert_eq!(run(&mut r, DEFAULT_WORK_BUDGET).unwrap().eliminated, 1);
        assert_eq!(r.states[0].xmm[0], v);
    }
}

#[test]
fn budget_failure_is_atomic_and_observed_values_are_not_removed() {
    let r = shuffle_case(
        std::array::from_fn(|i| (i * 7 % 32) as u8),
        std::array::from_fn(|i| (31 - i) as u8),
    );
    let before = format!("{r:?}");
    let mut success = false;
    for budget in 0..2048 {
        let mut copy = r.clone();
        if run(&mut copy, budget).is_ok() {
            success = true;
            break;
        }
        assert_eq!(format!("{copy:?}"), before);
    }
    assert!(success);
    let mut b = builder();
    let v = b.xmm[0];
    let same = b.node(Op::VectorBinary(PackedOp::And), vec![v, v], Type::V128);
    let mut r = finish(b, v, None);
    let Definition::Instruction(id, _) = r.values[same.index()].definition else {
        panic!()
    };
    r.instructions[id.index()].state = Some(StateId(0));
    assert_eq!(run(&mut r, DEFAULT_WORK_BUDGET).unwrap().eliminated, 0);
    assert!(r.blocks[0].instructions.contains(&id));
}

#[test]
fn emitted_simd_simplification_corpus() {
    std::fs::create_dir_all("build/ir-simd-simplify").unwrap();
    let mut cases = vec![];
    let mut emit_case = |case: String, region: Region| {
        for opt in [false, true] {
            let mut r = region.clone();
            if opt {
                run(&mut r, DEFAULT_WORK_BUDGET).unwrap();
            }
            std::fs::write(
                format!("build/ir-simd-simplify/{}-{opt}.wasm", cases.len()),
                emit_cpu(&lower(&r).unwrap(), 100).unwrap().bytes,
            )
            .unwrap();
        }
        cases.push(case);
    };
    emit_case("[\"overlap\",0,0]".into(), overlap_case());
    for seed in 0..64 {
        let first = std::array::from_fn(|i| ((i * 7 + seed) % 32) as u8);
        let second = std::array::from_fn(|i| ((i * 3 + seed * 5) % 32) as u8);
        emit_case(
            format!("[\"shuffle\",{first:?},{second:?}]"),
            shuffle_case(first, second),
        );
    }
    for bits in [16, 32, 64] {
        for lane in 0..128 / bits {
            for kind in ["roundtrip", "forward", "overwrite"] {
                emit_case(
                    format!("[\"{kind}\",{bits},{lane}]"),
                    lane_case(kind, bits, lane),
                );
            }
        }
    }
    std::fs::write(
        "build/ir-simd-simplify/cases.json",
        format!("[{}]", cases.join(",")),
    )
    .unwrap();
}
