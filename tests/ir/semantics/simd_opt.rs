use super::{run, Stats, DEFAULT_WORK_LIMIT};
use crate::ir::{
    backend::wasm::emit_cpu,
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

fn builder() -> IntegerBuilder {
    let mut b = IntegerBuilder::new();
    b.xmm = (0..8)
        .map(|r| b.node(Op::ReadXmm(r), vec![], Type::V128))
        .collect();
    b
}
fn snapshot(b: &mut IntegerBuilder) -> StateId {
    b.region.state(StateMap {
        instruction_pc: GuestEip(0x1000),
        next_pc: GuestEip(0x1004),
        next_value: None,
        resume: ResumeKind::AfterInstruction,
        gpr: b.gpr,
        flags: b.flags.clone(),
        xmm: b.xmm.clone(),
        x87: vec![],
        committed_instructions: 7,
        count_base: None,
        rep_progress: None,
    })
}
fn finish(mut b: IntegerBuilder) -> Region {
    let state = snapshot(&mut b);
    b.region.terminate(b.block, Terminator::Exit(state));
    verify(&b.region).unwrap();
    b.region
}
fn write(b: &mut IntegerBuilder, vector: ValueId, bits: u8, lane: u8, reg: usize) -> ValueId {
    let mut input = b.gpr[reg];
    if bits == 64 {
        input = b.node(Op::Extend { signed: false }, vec![input], Type::I64);
        input = b.node(
            Op::Insert { lsb: 32 },
            vec![input, b.gpr[reg + 1]],
            Type::I64,
        );
    }
    b.node(
        Op::VectorReplace { bits, lane },
        vec![vector, input],
        Type::V128,
    )
}
fn read(b: &mut IntegerBuilder, vector: ValueId, bits: u8, lane: u8) -> ValueId {
    b.node(
        Op::VectorExtract { bits, lane },
        vec![vector],
        if bits == 64 { Type::I64 } else { Type::I32 },
    )
}
fn scalar_output(b: &mut IntegerBuilder, value: ValueId) {
    if b.ty(value) == Type::I64 {
        b.gpr[0] = b.extract(value, 0, Type::I32);
        b.gpr[1] = b.extract(value, 32, Type::I32);
    }
    else {
        b.gpr[0] = value;
    }
}
fn op(region: &Region, value: ValueId) -> &Instruction {
    let Definition::Instruction(id, _) = region.values[value.index()].definition
    else {
        panic!()
    };
    &region.instructions[id.index()]
}
fn exit(region: &Region) -> &StateMap {
    let Some(Terminator::Exit(state)) = region.blocks.last().unwrap().terminator
    else {
        panic!()
    };
    &region.states[state.index()]
}

#[test]
fn identities_update_recovery_only_values_without_commoning_cpu_reads() {
    let mut b = builder();
    let a = b.xmm[0];
    let second_read = b.node(Op::ReadXmm(0), vec![], Type::V128);
    let identity = b.node(
        Op::VectorShuffle(std::array::from_fn(|i| i as u8)),
        vec![a, second_read],
        Type::V128,
    );
    let redundant = b.node(
        Op::VectorBinary(PackedOp::And),
        vec![identity, identity],
        Type::V128,
    );
    b.xmm[0] = redundant;
    b.xmm[1] = second_read;
    let mut r = finish(b);
    let stats = run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!(stats.aliases, 2);
    assert_eq!(exit(&r).xmm[0], a);
    assert_eq!(exit(&r).xmm[1], second_read);
    assert_eq!(
        r.blocks[0]
            .instructions
            .iter()
            .filter(|&&id| matches!(r.instructions[id.index()].op, Op::ReadXmm(0)))
            .count(),
        2
    );
    verify(&r).unwrap();
    lower(&r).unwrap();
    assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().simplified(), 0);
}

#[test]
fn word_forwarding_masks_high_bits_and_preserves_result_type() {
    for bits in [16, 32, 64] {
        let mut b = builder();
        let a = b.xmm[0];
        let written = write(&mut b, a, bits, 0, 0);
        let extracted = read(&mut b, written, bits, 0);
        scalar_output(&mut b, extracted);
        let mut r = finish(b);
        let old_type = r.values[extracted.index()].ty;
        let stats = run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
        assert!(stats.lanes > 0);
        assert_eq!(r.values[extracted.index()].ty, old_type);
        if bits == 16 {
            let inst = op(&r, exit(&r).gpr[0]);
            assert_eq!(inst.op, Op::Binary(Binary::And));
            assert_eq!(op(&r, inst.args[1]).op, Op::Const(0xFFFF));
        }
        verify(&r).unwrap();
        emit_cpu(&lower(&r).unwrap(), 100).unwrap();
    }
}

#[test]
fn shuffle_fusion_requires_at_most_two_selected_sources() {
    let mut b = builder();
    let a = b.xmm[0];
    let c = b.xmm[1];
    let d = b.xmm[2];
    let halves = std::array::from_fn(|i| {
        if i < 8 {
            i as u8
        }
        else {
            i as u8 + 8
        }
    });
    let ac = b.node(Op::VectorShuffle(halves), vec![a, c], Type::V128);
    // This outer shuffle needs bytes of a, c AND d: no two-input fusion exists.
    let mut mask = std::array::from_fn(|i| i as u8);
    mask[15] = 31;
    let three = b.node(Op::VectorShuffle(mask), vec![ac, d], Type::V128);
    b.xmm[0] = three;
    let mut r = finish(b);
    run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!(op(&r, three).op, Op::VectorShuffle(mask));
    assert_eq!(op(&r, three).args, vec![ac, d]);
    // The third input is no longer selected, allowing composition.
    let Definition::Instruction(id, _) = r.values[three.index()].definition
    else {
        panic!()
    };
    r.instructions[id.index()].op =
        Op::VectorShuffle([0, 1, 2, 3, 8, 9, 10, 11, 0, 1, 2, 3, 8, 9, 10, 11]);
    run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!(op(&r, three).args, vec![a, c]);
}

#[test]
fn partially_overlapping_or_unaligned_lanes_do_not_forward() {
    let mut b = builder();
    let a = b.xmm[0];
    let word = write(&mut b, a, 16, 1, 0);
    let dword = read(&mut b, word, 32, 0);
    scalar_output(&mut b, dword);
    let mut r = finish(b);
    assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().simplified(), 0);
    assert_eq!(op(&r, dword).args, vec![word]);
    let mut b = builder();
    let a = b.xmm[0];
    let c = b.xmm[1];
    let mask = std::array::from_fn(|i| (i + 1) as u8);
    let shuffled = b.node(Op::VectorShuffle(mask), vec![a, c], Type::V128);
    let dword = read(&mut b, shuffled, 32, 0);
    scalar_output(&mut b, dword);
    let mut r = finish(b);
    run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!(op(&r, dword).args, vec![shuffled]);
}

#[test]
fn recovery_points_and_effect_chain_remain_pinned() {
    let mut b = builder();
    let state = snapshot(&mut b);
    b.effect = b.region.append(
        b.block,
        Op::SseCheck,
        vec![b.effect],
        &[Type::Effect],
        Some(state),
    )[0];
    let guard = b.region.blocks[0].instructions.last().copied().unwrap();
    let a = b.xmm[0];
    // A normally removable identity bearing a recovery point must remain.
    let pinned = b.region.append(
        b.block,
        Op::VectorShuffle(std::array::from_fn(|i| i as u8)),
        vec![a, a],
        &[Type::V128],
        Some(state),
    )[0];
    b.xmm[0] = pinned;
    let mut r = finish(b);
    let before = format!("{r:?}");
    assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().simplified(), 0);
    assert_eq!(before, format!("{r:?}"));
    assert!(r.blocks[0].instructions.contains(&guard));
}

#[test]
fn budget_and_invalid_ir_fail_transactionally() {
    let mut b = builder();
    let a = b.xmm[0];
    let written = write(&mut b, a, 16, 0, 0);
    let extracted = read(&mut b, written, 16, 0);
    scalar_output(&mut b, extracted);
    let baseline = finish(b);
    let mut success = baseline.clone();
    let cost = run(&mut success, DEFAULT_WORK_LIMIT).unwrap().work;
    assert!(cost > 1);
    for limit in [0, 1, cost / 2, cost - 1] {
        let mut r = baseline.clone();
        let before = format!("{r:?}");
        assert!(run(&mut r, limit).is_err(), "budget {limit}/{cost}");
        assert_eq!(before, format!("{r:?}"));
    }
    let mut exact = baseline.clone();
    assert_eq!(run(&mut exact, cost).unwrap().work, cost);
    let mut invalid = baseline;
    let Definition::Instruction(id, _) = invalid.values[extracted.index()].definition
    else {
        panic!()
    };
    invalid.instructions[id.index()].op = Op::VectorExtract { bits: 16, lane: 8 };
    let before = format!("{invalid:?}");
    assert!(run(&mut invalid, DEFAULT_WORK_LIMIT).is_err());
    assert_eq!(before, format!("{invalid:?}"));
}

#[test]
fn pipeline_switch_and_gvn_remove_obsolete_vector_work() {
    let mut b = builder();
    let a = b.xmm[0];
    let id = b.node(
        Op::VectorShuffle(std::array::from_fn(|i| i as u8)),
        vec![a, a],
        Type::V128,
    );
    b.xmm[0] = id;
    let baseline = finish(b);
    let mut off = baseline.clone();
    let mut on = baseline;
    assert_eq!(
        passes::run(
            &mut off,
            PassConfig {
                simd: false,
                ..PassConfig::default()
            }
        )
        .unwrap()
        .simd_simplified,
        0
    );
    assert!(
        passes::run(&mut on, PassConfig::default())
            .unwrap()
            .simd_simplified
            > 0
    );
    assert!(off.blocks[0].instructions.len() > on.blocks[0].instructions.len());
}

#[test]
fn aliases_rewrite_branch_arguments_and_block_recovery_maps() {
    let mut b = builder();
    let a = b.xmm[0];
    let alias = b.node(Op::VectorBinary(PackedOp::Or), vec![a, a], Type::V128);
    let join = b.region.block(false);
    let incoming = b.region.param(join, Type::V128);
    let effect = b.region.param(join, Type::Effect);
    b.region.terminate(
        b.block,
        Terminator::Branch(Edge {
            target: join,
            args: vec![alias, b.effect],
        }),
    );
    b.block = join;
    b.effect = effect;
    b.xmm[0] = incoming;
    let recovery = snapshot(&mut b);
    b.region.blocks[join.index()].entry_state = Some(recovery);
    let mut r = finish(b);
    assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().aliases, 1);
    assert_eq!(
        r.blocks[0].terminator.as_ref().unwrap().edges()[0].args[0],
        a
    );
    verify(&r).unwrap();
    emit_cpu(&lower(&r).unwrap(), 1).unwrap();
}

// Machine-independent expression DAG used only as input to the JS byte model.
// It is captured BEFORE optimization and does not contain optimized Wasm plans.
fn specification(r: &Region) -> String {
    let nodes: Vec<_> = r
        .values
        .iter()
        .map(|value| {
            let Definition::Instruction(id, _) = value.definition
            else {
                return "null".to_owned();
            };
            let inst = &r.instructions[id.index()];
            let args: Vec<_> = inst.args.iter().map(|v| v.0).collect();
            let op = match inst.op {
                Op::ReadXmm(n) => format!("[\"xmm\",{n}]"),
                Op::ReadGpr(n) => format!("[\"gpr\",{n}]"),
                Op::Const(n) => format!("[\"const\",\"{n}\"]"),
                Op::VectorShuffle(mask) => format!("[\"shuffle\",{mask:?}]"),
                Op::VectorExtract { bits, lane } => format!("[\"lane_read\",{bits},{lane}]"),
                Op::VectorReplace { bits, lane } => format!("[\"lane_write\",{bits},{lane}]"),
                Op::VectorBinary(op) => format!("[\"packed\",\"{op:?}\"]"),
                Op::Extend { signed } => format!("[\"extend\",{signed}]"),
                Op::Extract { lsb } => format!("[\"extract\",{lsb}]"),
                Op::Insert { lsb } => format!("[\"insert\",{lsb}]"),
                _ => return "null".to_owned(),
            };
            format!("[{op},{args:?},{}]", value.ty.bits().unwrap_or(128))
        })
        .collect();
    let state = exit(r);
    let gpr: Vec<_> = state.gpr.iter().map(|v| v.0).collect();
    let xmm: Vec<_> = state.xmm.iter().map(|v| v.0).collect();
    format!(
        "{{\"nodes\":[{}],\"gpr\":{gpr:?},\"xmm\":{xmm:?}}}",
        nodes.join(",")
    )
}

#[test]
fn emitted_simd_optimization_corpus() {
    std::fs::create_dir_all("build/ir-simd-opt").unwrap();
    let mut cases = Vec::new();
    let mut total = Stats::default();
    let mut emit = |name: String, b: IntegerBuilder| {
        let baseline = finish(b);
        let index = cases.len();
        let mut sizes = Vec::new();
        for mode in 0..4 {
            let mut r = baseline.clone();
            if mode == 1 {
                let stats = run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
                total.aliases += stats.aliases;
                total.rewritten += stats.rewritten;
            }
            else if mode >= 2 {
                passes::run(
                    &mut r,
                    PassConfig {
                        simd: mode == 2,
                        ..PassConfig::default()
                    },
                )
                .unwrap();
            }
            let bytes = emit_cpu(&lower(&r).unwrap(), 100).unwrap().bytes;
            sizes.push(bytes.len());
            std::fs::write(format!("build/ir-simd-opt/{index}-{mode}.wasm"), bytes).unwrap();
        }
        cases.push(format!(
            "{{\"name\":\"{name}\",\"sizes\":{sizes:?},\"spec\":{}}}",
            specification(&baseline)
        ));
    };
    for offset in 0..32u8 {
        for same in [false, true] {
            let mut b = builder();
            let a = b.xmm[0];
            let c = b.xmm[usize::from(!same)];
            let inner = b.node(
                Op::VectorShuffle(std::array::from_fn(|i| (i as u8 * 7 + offset) % 32)),
                vec![a, c],
                Type::V128,
            );
            let other = b.node(
                Op::VectorShuffle(std::array::from_fn(|i| (i as u8 + offset) % 32)),
                vec![c, a],
                Type::V128,
            );
            b.xmm[0] = b.node(
                Op::VectorShuffle(std::array::from_fn(|i| (i as u8 * 3 + offset) % 32)),
                vec![inner, other],
                Type::V128,
            );
            emit(format!("shuffle-{offset}-{same}"), b);
        }
    }
    for bits in [16, 32, 64] {
        for lane in 0..128 / bits {
            for mode in 0..4 {
                let mut b = builder();
                let a = b.xmm[0];
                let first = write(&mut b, a, bits, lane, 0);
                match mode {
                    0 => {
                        let v = read(&mut b, first, bits, lane);
                        scalar_output(&mut b, v);
                    },
                    1 => {
                        b.xmm[0] = write(&mut b, first, bits, lane, 2);
                    },
                    2 => {
                        let v = read(&mut b, a, bits, lane);
                        b.xmm[0] = b.node(Op::VectorReplace { bits, lane }, vec![a, v], Type::V128);
                    },
                    _ => {
                        let v = read(&mut b, first, bits, lane);
                        b.xmm[0] = first;
                        scalar_output(&mut b, v);
                    },
                }
                emit(format!("lane-{bits}-{lane}-{mode}"), b);
            }
            for written_bits in [16, 32, 64] {
                for written_lane in 0..128 / written_bits {
                    let mut b = builder();
                    let a = b.xmm[0];
                    let first = write(&mut b, a, written_bits, written_lane, 0);
                    let v = read(&mut b, first, bits, lane);
                    scalar_output(&mut b, v);
                    emit(
                        format!("overlap-{bits}-{lane}-{written_bits}-{written_lane}"),
                        b,
                    );
                }
            }
            for offset in 0..32u8 {
                let mut b = builder();
                let a = b.xmm[0];
                let c = b.xmm[1];
                let shuffled = b.node(
                    Op::VectorShuffle(std::array::from_fn(|i| (i as u8 + offset) % 32)),
                    vec![a, c],
                    Type::V128,
                );
                let v = read(&mut b, shuffled, bits, lane);
                scalar_output(&mut b, v);
                emit(format!("extract-shuffle-{bits}-{lane}-{offset}"), b);
            }
        }
    }
    for operation in [PackedOp::And, PackedOp::Or, PackedOp::Xor, PackedOp::AndNot] {
        let mut b = builder();
        let a = b.xmm[0];
        b.xmm[0] = b.node(Op::VectorBinary(operation), vec![a, a], Type::V128);
        emit(format!("bitwise-{operation:?}"), b);
    }
    // Three source vectors and deep sharing exercise the non-fusible cases too.
    for seed in 1..=100 {
        let mut rng = seed as u32;
        let mut next = || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            rng
        };
        let mut b = builder();
        let mut pool = b.xmm[..3].to_vec();
        for _ in 0..16 {
            let a = pool[next() as usize % pool.len()];
            let c = pool[next() as usize % pool.len()];
            let value = match next() % 3 {
                0 => b.node(
                    Op::VectorShuffle(std::array::from_fn(|_| (next() % 32) as u8)),
                    vec![a, c],
                    Type::V128,
                ),
                1 => write(&mut b, a, [16, 32, 64][next() as usize % 3], 0, 0),
                _ => b.node(
                    Op::VectorBinary(
                        [PackedOp::And, PackedOp::Or, PackedOp::Xor, PackedOp::AndNot]
                            [next() as usize % 4],
                    ),
                    vec![a, c],
                    Type::V128,
                ),
            };
            pool.push(value);
        }
        b.xmm[0] = *pool.last().unwrap();
        emit(format!("random-{seed}"), b);
    }
    assert!(total.aliases > 0 && total.rewritten > 0);
    std::fs::write(
        "build/ir-simd-opt/cases.json",
        format!("[{}]", cases.join(",")),
    )
    .unwrap();
    println!(
        "SIMD corpus: {} cases, {} aliases, {} rewrites",
        cases.len(),
        total.aliases,
        total.rewritten
    );
}

#[test]
fn independent_entries_do_not_share_cpu_values() {
    let mut b = builder();
    let first_input = b.xmm[0];
    b.xmm[0] = b.node(
        Op::VectorBinary(PackedOp::And),
        vec![first_input, first_input],
        Type::V128,
    );
    let first = snapshot(&mut b);
    b.region.terminate(b.block, Terminator::Exit(first));
    b.block = b.region.block(true);
    b.effect = b.region.param(b.block, Type::Effect);
    b.gpr = std::array::from_fn(|i| b.node(Op::ReadGpr(i as u8), vec![], Type::I32));
    b.xmm = (0..8)
        .map(|i| b.node(Op::ReadXmm(i), vec![], Type::V128))
        .collect();
    let system = b.node(Op::ReadFlags, vec![], Type::I32);
    b.flags = crate::ir::state::FlagState {
        system,
        arithmetic: std::array::from_fn(|i| b.extract(system, [0, 2, 4, 6, 7, 11][i], Type::I1)),
        last_op1: None,
        raw_zero: None,
        zero_is_lazy: None,
    };
    let second_input = b.xmm[0];
    b.xmm[0] = b.node(
        Op::VectorBinary(PackedOp::And),
        vec![second_input, second_input],
        Type::V128,
    );
    let mut r = finish(b);
    assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().aliases, 2);
    assert_eq!(r.states[first.index()].xmm[0], first_input);
    assert_eq!(exit(&r).xmm[0], second_input);
    assert_ne!(first_input, second_input);
    verify(&r).unwrap();
    emit_cpu(&lower(&r).unwrap(), 100).unwrap();
}

#[test]
fn arena_growth_failure_is_atomic_after_an_earlier_alias() {
    let mut b = builder();
    let a = b.xmm[0];
    let alias = b.node(Op::VectorBinary(PackedOp::Or), vec![a, a], Type::V128);
    let written = write(&mut b, alias, 16, 0, 0);
    let extracted = read(&mut b, written, 16, 0);
    scalar_output(&mut b, extracted);
    let mut r = finish(b);
    // DCE leaves unused arena slots legally. Fill to the bound so the scalar
    // zero-extension mask cannot allocate; the earlier alias must not escape.
    let dead = r.instructions[0].clone();
    r.instructions.resize(super::MAX_INSTRUCTIONS, dead);
    verify(&r).unwrap();
    let before = format!("{r:?}");
    assert!(run(&mut r, DEFAULT_WORK_LIMIT)
        .unwrap_err()
        .contains("arena growth"));
    assert_eq!(before, format!("{r:?}"));
}
