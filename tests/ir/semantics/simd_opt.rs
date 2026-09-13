use super::run;
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
fn extract(b: &mut IntegerBuilder, v: ValueId, bits: u8, lane: u8) -> ValueId {
    b.node(
        Op::VectorExtract { bits, lane },
        vec![v],
        if bits == 64 { Type::I64 } else { Type::I32 },
    )
}
fn replace(b: &mut IntegerBuilder, v: ValueId, value: ValueId, bits: u8, lane: u8) -> ValueId {
    b.node(Op::VectorReplace { bits, lane }, vec![v, value], Type::V128)
}
fn fixture() -> Region {
    let mut b = IntegerBuilder::new();
    let gpr = b.gpr;
    let input: [ValueId; 8] =
        std::array::from_fn(|i| b.node(Op::ReadXmm(i as u8), vec![], Type::V128));
    let and = b.node(
        Op::VectorBinary(PackedOp::And),
        vec![input[0], input[0]],
        Type::V128,
    );
    let or = b.node(Op::VectorBinary(PackedOp::Or), vec![and, and], Type::V128);
    let reverse: [u8; 16] = std::array::from_fn(|i| (15 - i) as u8);
    let first = b.node(Op::VectorShuffle(reverse), vec![or, or], Type::V128);
    let x0 = b.node(Op::VectorShuffle(reverse), vec![first, first], Type::V128);
    let inner = b.node(
        Op::VectorShuffle(std::array::from_fn(|i| (i * 7 % 32) as u8)),
        vec![input[1], input[2]],
        Type::V128,
    );
    let x1 = b.node(
        Op::VectorShuffle(std::array::from_fn(|i| (31 - i) as u8)),
        vec![input[0], inner],
        Type::V128,
    );
    let lane = extract(&mut b, input[2], 16, 5);
    let x2 = replace(&mut b, input[2], lane, 16, 5);
    let old = replace(&mut b, input[3], gpr[4], 32, 2);
    let x3 = replace(&mut b, old, gpr[5], 32, 2);
    let x4 = replace(&mut b, input[4], gpr[6], 32, 1);
    b.gpr[0] = extract(&mut b, x4, 32, 1);
    let x5 = replace(&mut b, input[5], gpr[7], 16, 3);
    b.gpr[1] = extract(&mut b, x5, 16, 3);
    let x6 = replace(&mut b, input[6], gpr[0], 16, 2);
    b.gpr[2] = extract(&mut b, x6, 16, 2);
    b.gpr[3] = extract(&mut b, x5, 32, 0); // disjoint
    b.gpr[4] = extract(&mut b, x5, 32, 1); // overlapping: must retain insertion
    let high = b.node(Op::Extend { signed: false }, vec![gpr[6]], Type::I64);
    let amount = b.node(Op::Const(32), vec![], Type::I64);
    let high = b.binary(Binary::Shl, high, amount);
    let low = b.node(Op::Extend { signed: false }, vec![gpr[7]], Type::I64);
    let wide = b.binary(Binary::Or, high, low);
    let x7 = replace(&mut b, input[7], wide, 64, 0);
    let value = extract(&mut b, x7, 64, 0);
    b.gpr[5] = b.node(Op::Truncate, vec![value], Type::I32);
    let value = b.binary(Binary::Shr, value, amount);
    b.gpr[6] = b.node(Op::Truncate, vec![value], Type::I32);
    let state = b.region.state(StateMap {
        instruction_pc: GuestEip(0x9000),
        next_pc: GuestEip(0x9001),
        next_value: None,
        resume: ResumeKind::AfterInstruction,
        gpr: b.gpr,
        flags: b.flags.clone(),
        xmm: vec![x0, x1, x2, x3, x4, x5, x6, x7],
        x87: vec![],
        committed_instructions: 1,
        count_base: None,
        rep_progress: None,
    });
    b.region.terminate(b.block, Terminator::Exit(state));
    b.region
}
#[test]
fn vector_identities_are_typed_and_recovery_aware() {
    let mut r = fixture();
    let stats = run(&mut r, 1_000_000).unwrap();
    assert!(stats.aliases >= 6);
    assert!(stats.rewritten >= 3);
    verify(&r).unwrap();
    lower(&r).unwrap();
    // A second sweep may expose another composition; convergence is bounded
    // by the enclosing Tier 2 iteration limit rather than an unbounded loop.
    run(&mut r, 1_000_000).unwrap();
    let stable = run(&mut r, 1_000_000).unwrap();
    assert_eq!(stable.aliases + stable.rewritten, 0);
}
#[test]
fn narrowing_and_overlap_are_not_identity_aliases() {
    let mut r = fixture();
    let old = r.states[0].gpr;
    run(&mut r, 1_000_000).unwrap();
    assert_eq!(r.states[0].gpr[1], old[1], "16-bit zero extension retained");
    assert_eq!(
        r.states[0].gpr[2], old[2],
        "second 16-bit zero extension retained"
    );
    let Definition::Instruction(id, _) = r.values[old[4].index()].definition else {
        panic!()
    };
    let base = r.instructions[id.index()].args[0];
    let Definition::Instruction(def, _) = r.values[base.index()].definition else {
        panic!()
    };
    assert!(matches!(
        r.instructions[def.index()].op,
        Op::VectorReplace { bits: 16, lane: 3 }
    ));
    let mut r = fixture();
    let term = r.blocks[0].terminator.take().unwrap();
    let vector = r.states[0].xmm[0];
    for op in [PackedOp::Add32, PackedOp::AndNot, PackedOp::Sub16] {
        r.append(
            BlockId(0),
            Op::VectorBinary(op),
            vec![vector, vector],
            &[Type::V128],
            None,
        );
    }
    r.terminate(BlockId(0), term);
    run(&mut r, 1_000_000).unwrap();
    for op in [PackedOp::Add32, PackedOp::AndNot, PackedOp::Sub16] {
        assert!(r.blocks[0]
            .instructions
            .iter()
            .any(|id| r.instructions[id.index()].op == Op::VectorBinary(op)));
    }
}
#[test]
fn simplification_budget_and_verifier_failures_are_atomic() {
    for budget in [0, 10, 80] {
        let mut r = fixture();
        let before = format!("{:?}", r);
        assert!(run(&mut r, budget).is_err());
        assert_eq!(format!("{:?}", r), before);
    }
    let mut r = fixture();
    let id = *r.blocks[0].instructions.last().unwrap();
    r.instructions[id.index()].args.push(ValueId(u32::MAX));
    let before = format!("{:?}", r);
    assert!(run(&mut r, 1_000_000).is_err());
    assert_eq!(format!("{:?}", r), before);
}
#[test]
fn vector_simplification_execution_fixtures() {
    std::fs::create_dir_all("build/ir-simd-opt").unwrap();
    for optimized in [false, true] {
        let mut r = fixture();
        if optimized {
            run(&mut r, 1_000_000).unwrap();
        }
        let artifact = emit_cpu(&lower(&r).unwrap(), 10).unwrap();
        std::fs::write(
            format!("build/ir-simd-opt/{optimized}.wasm"),
            artifact.bytes,
        )
        .unwrap();
    }
}
