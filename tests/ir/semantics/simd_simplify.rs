use super::{run, DEFAULT_WORK_LIMIT};
use crate::{
    cpu::global_pointers as gp,
    ir::{
        backend::wasm::emit_cpu,
        frontend::{
            decode::{GuestEip, LinearAddress},
            integer::IntegerBuilder,
            lift::lift_cpu,
        },
        hir::*,
        ids::*,
        lowering::lower,
        passes::{self, PassConfig},
        simd::PackedOp,
        state::{ResumeKind, StateMap},
        types::Type,
        verify::verify,
    },
};

fn builder() -> IntegerBuilder {
    let mut b = IntegerBuilder::new();
    for i in 0..4 {
        let v = b.node(Op::ReadXmm(i), vec![], Type::V128);
        b.xmm.push(v);
    }
    b
}
fn finish(mut b: IntegerBuilder, output: ValueId) -> Region {
    // Observe all output bytes through ordinary GPR state. No memory guard is
    // bypassed in this unit fixture; end-to-end guest tests cover actual loads.
    for lane in 0..4 {
        b.gpr[lane] = b.node(
            Op::VectorExtract {
                bits: 32,
                lane: lane as u8,
            },
            vec![output],
            Type::I32,
        );
    }
    let state = b.region.state(StateMap {
        instruction_pc: GuestEip(0x1000),
        next_pc: GuestEip(0x1001),
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
    verify(&b.region).unwrap();
    b.region
}
fn scheduled(region: &Region) -> usize {
    region.blocks.iter().map(|b| b.instructions.len()).sum()
}
fn definition(region: &Region, value: ValueId) -> &Instruction {
    let Definition::Instruction(id, _) = region.values[value.index()].definition else {
        panic!()
    };
    &region.instructions[id.index()]
}
fn identity() -> [u8; 16] {
    std::array::from_fn(|i| i as u8)
}

#[test]
fn composes_shuffles_and_updates_recovery_only_values() {
    let mut b = builder();
    let x = b.xmm[0];
    let reverse = std::array::from_fn(|i| 15 - i as u8);
    let first = b.node(Op::VectorShuffle(reverse), vec![x, x], Type::V128);
    let second = b.node(Op::VectorShuffle(reverse), vec![first, first], Type::V128);
    let mut region = finish(b, second);
    // The XMM result is also needed only for recovery, not an instruction use.
    region.states[0].xmm = vec![second; 8];
    let stats = run(&mut region, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!(stats.eliminated, 1);
    assert!(region.states[0].xmm.iter().all(|&v| v == x));
    assert_eq!(run(&mut region, DEFAULT_WORK_LIMIT).unwrap().eliminated, 0);
    verify(&region).unwrap();
    lower(&region).unwrap();
}

#[test]
fn keeps_three_source_compositions_and_narrow_extract_masks() {
    let mut b = builder();
    let [x, y, z] = [b.xmm[0], b.xmm[1], b.xmm[2]];
    let a = b.node(
        Op::VectorShuffle(std::array::from_fn(|i| {
            if i < 8 {
                i as u8
            } else {
                i as u8 + 16
            }
        })),
        vec![x, y],
        Type::V128,
    );
    let lanes = std::array::from_fn(|i| if i < 10 { i as u8 } else { i as u8 + 16 });
    let three = b.node(Op::VectorShuffle(lanes), vec![a, z], Type::V128);
    let scalar = b.gpr[4];
    let inserted = b.node(
        Op::VectorReplace { bits: 16, lane: 3 },
        vec![three, scalar],
        Type::V128,
    );
    let narrow = b.node(
        Op::VectorExtract { bits: 16, lane: 3 },
        vec![inserted],
        Type::I32,
    );
    let mut region = finish(b, inserted);
    let before = definition(&region, three).clone();
    run(&mut region, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!(
        format!("{:?}", definition(&region, three)),
        format!("{before:?}")
    );
    assert_eq!(
        definition(&region, narrow).op,
        Op::VectorExtract { bits: 16, lane: 3 }
    );
    assert_eq!(definition(&region, narrow).args, vec![inserted]);
}

#[test]
fn lane_replacement_and_idempotence_rules_keep_machine_widths() {
    for bits in [16, 32, 64] {
        for lane in 0..128 / bits {
            let mut b = builder();
            let x = b.xmm[0];
            let ty = if bits == 64 { Type::I64 } else { Type::I32 };
            let extract = b.node(Op::VectorExtract { bits, lane }, vec![x], ty);
            let restored = b.node(
                Op::VectorReplace { bits, lane },
                vec![x, extract],
                Type::V128,
            );
            let mut region = finish(b, restored);
            let count = scheduled(&region);
            assert_eq!(run(&mut region, DEFAULT_WORK_LIMIT).unwrap().eliminated, 1);
            assert_eq!(scheduled(&region), count - 1);
            lower(&region).unwrap();
        }
    }
    for operation in [
        PackedOp::And,
        PackedOp::Or,
        PackedOp::MinU8,
        PackedOp::MaxU8,
        PackedOp::MinS16,
        PackedOp::MaxS16,
        PackedOp::AvgU8,
        PackedOp::AvgU16,
    ] {
        let mut b = builder();
        let x = b.xmm[0];
        let result = b.node(Op::VectorBinary(operation), vec![x, x], Type::V128);
        let mut region = finish(b, result);
        assert_eq!(run(&mut region, DEFAULT_WORK_LIMIT).unwrap().eliminated, 1);
    }
}

#[test]
fn failures_are_atomic_and_observations_are_retained() {
    let bytes = [0x66, 0x0F, 0x70, 0x06, 0xE4, 0x66, 0x0F, 0x70, 0xC0, 0xE4];
    let mut original = lift_cpu(&bytes, GuestEip(0x1000), LinearAddress(0x1000), true).unwrap();
    passes::run(&mut original, PassConfig::default()).unwrap();
    let observations: Vec<_> = original
        .instructions
        .iter()
        .enumerate()
        .filter(|(_, i)| i.op.ordered())
        .map(|(i, inst)| (i, format!("{inst:?}")))
        .collect();
    let mut region = original.clone();
    let stats = run(&mut region, DEFAULT_WORK_LIMIT).unwrap();
    assert!(stats.eliminated > 0);
    for (i, text) in observations {
        assert_eq!(format!("{:?}", region.instructions[i]), text);
    }
    for limit in [0, 1, stats.work / 2, stats.work - 1] {
        let mut candidate = original.clone();
        assert!(run(&mut candidate, limit).is_err());
        assert_eq!(format!("{candidate:?}"), format!("{original:?}"));
    }
    let mut invalid = original;
    invalid.instructions[0].block = BlockId(u32::MAX);
    let text = format!("{invalid:?}");
    assert!(run(&mut invalid, DEFAULT_WORK_LIMIT).is_err());
    assert_eq!(format!("{invalid:?}"), text);
}

// Data-driven, independent JS byte/lane oracle. Every fixture describes its
// original expressions, so the oracle cannot accidentally mirror a rewrite.
#[test]
fn emits_shuffle_and_lane_differential_corpus() {
    std::fs::create_dir_all("build/ir-simd-simplify").unwrap();
    let mut cases = Vec::new();
    let mut seed = 0x86C0_FFEEu32;
    let mut random = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for index in 0..96 {
        let mut b = builder();
        let x = b.xmm[0];
        let y = b.xmm[1];
        let z = b.xmm[2];
        let first =
            if index == 0 { identity() } else { std::array::from_fn(|_| (random() % 32) as u8) };
        let second =
            if index < 2 { identity() } else { std::array::from_fn(|_| (random() % 32) as u8) };
        let a = b.node(Op::VectorShuffle(first), vec![x, y], Type::V128);
        let right = if index % 3 == 0 {
            a
        } else if index % 3 == 1 {
            x
        } else {
            z
        };
        let output = b.node(Op::VectorShuffle(second), vec![a, right], Type::V128);
        let description = format!("[\"shuffle\",{:?},{:?},{}]", first, second, index % 3);
        emit_case(finish(b, output), &mut cases, description);
    }
    for bits in [16, 32, 64] {
        for lane in 0..128 / bits {
            for operation in 0..5 {
                let mut b = builder();
                let x = b.xmm[0];
                let scalar = if bits == 64 {
                    let low = b.node(Op::Extend { signed: false }, vec![b.gpr[4]], Type::I64);
                    b.node(Op::Insert { lsb: 32 }, vec![low, b.gpr[5]], Type::I64)
                } else {
                    b.gpr[4]
                };
                let a = b.node(
                    Op::VectorReplace { bits, lane },
                    vec![x, scalar],
                    Type::V128,
                );
                let output = match operation {
                    0 => {
                        let extracted = b.node(
                            Op::VectorExtract { bits, lane },
                            vec![x],
                            if bits == 64 { Type::I64 } else { Type::I32 },
                        );
                        b.node(
                            Op::VectorReplace { bits, lane },
                            vec![x, extracted],
                            Type::V128,
                        )
                    },
                    1 => {
                        let extracted = b.node(
                            Op::VectorExtract { bits, lane },
                            vec![a],
                            if bits == 64 { Type::I64 } else { Type::I32 },
                        );
                        // Widen the observation so a broken 16-bit extraction cannot
                        // hide its upper-bit leak behind another 16-bit insert.
                        b.node(
                            Op::VectorReplace {
                                bits: bits.max(32),
                                lane: 0,
                            },
                            vec![x, extracted],
                            Type::V128,
                        )
                    },
                    2 => {
                        let extracted = b.node(
                            Op::VectorExtract {
                                bits,
                                lane: (lane + 1) % (128 / bits),
                            },
                            vec![a],
                            if bits == 64 { Type::I64 } else { Type::I32 },
                        );
                        b.node(
                            Op::VectorReplace { bits, lane },
                            vec![x, extracted],
                            Type::V128,
                        )
                    },
                    3 => b.node(
                        Op::VectorReplace { bits, lane },
                        vec![a, scalar],
                        Type::V128,
                    ),
                    _ => a,
                };
                emit_case(
                    finish(b, output),
                    &mut cases,
                    format!("[\"lane\",{bits},{lane},{operation}]"),
                );
            }
        }
    }
    for operation in [
        PackedOp::And,
        PackedOp::Or,
        PackedOp::MinU8,
        PackedOp::MaxU8,
        PackedOp::MinS16,
        PackedOp::MaxS16,
        PackedOp::AvgU8,
        PackedOp::AvgU16,
    ] {
        let mut b = builder();
        let x = b.xmm[0];
        let output = b.node(Op::VectorBinary(operation), vec![x, x], Type::V128);
        emit_case(finish(b, output), &mut cases, "[\"identity\"]".to_string());
    }
    std::fs::write(
        "build/ir-simd-simplify/cases.json",
        format!("[{}]", cases.join(",")),
    )
    .unwrap();
    std::fs::write("build/ir-simd-simplify/layout.json", format!(
        "{{\"gpr\":{},\"xmm\":{},\"flags\":{},\"eip\":{},\"count\":{},\"last\":{},\"changes\":{}}}",
        gp::reg32 as usize, gp::reg_xmm as usize, gp::flags as usize, gp::instruction_pointer as usize,
        gp::instruction_counter as usize, gp::last_op1 as usize, gp::flags_changed as usize,
    )).unwrap();
}
fn emit_case(original: Region, cases: &mut Vec<String>, description: String) {
    for optimize in [false, true] {
        let mut region = original.clone();
        if optimize {
            run(&mut region, DEFAULT_WORK_LIMIT).unwrap();
        }
        std::fs::write(
            format!("build/ir-simd-simplify/{}-{optimize}.wasm", cases.len()),
            emit_cpu(&lower(&region).unwrap(), 100).unwrap().bytes,
        )
        .unwrap();
    }
    cases.push(description);
}

#[test]
fn real_cpu_compiler_gates_simd_simplification_on_tier_and_pass_configuration() {
    use crate::ir::{
        backend::wasm::StateLayout, frontend::decode::PhysicalAddress, runtime::compile::*,
    };
    let snapshot = ImmutableCodeSnapshot {
        bytes: vec![0x66, 0x0F, 0x70, 0xC1, 0xE4, 0x66, 0x0F, 0x70, 0xC0, 0xE4],
        dependencies: vec![CodeDependency {
            page: PhysicalAddress(0x1000),
            version: 1,
        }],
        mappings: vec![CodeMapping {
            linear: LinearAddress(0x1000),
            physical: PhysicalAddress(0x1000),
        }],
    };
    for tier in [Tier::One, Tier::Two] {
        for optimize in [false, true] {
            for rounds in [0, 2] {
                let request = CompileRequest {
                    key: PublicationKey {
                        job: 1,
                        vm_generation: 1,
                        slot: 0,
                        slot_generation: 1,
                    },
                    pc: GuestEip(0x1000),
                    linear: LinearAddress(0x1000),
                    default_32: true,
                    tier,
                };
                let config = IrConfig {
                    optimize,
                    passes: PassConfig {
                        rounds,
                        ..PassConfig::default()
                    },
                    execution_budget: 100,
                    rep_iteration_budget: 8,
                    max_code_bytes: 1920,
                    layout: StateLayout {
                        gpr: 0,
                        flags: 32,
                        eip: 36,
                        committed: 40,
                        flag_operand: 44,
                    },
                };
                let result = compile_cpu_cfg_region(&request, &snapshot, &config).unwrap();
                assert_eq!(
                    result.passes.simd_eliminated > 0,
                    tier == Tier::Two && optimize && rounds != 0
                );
            }
        }
    }
}
