use super::*;
use crate::ir::{
    backend::wasm::emit_cpu,
    dump,
    frontend::{decode::GuestEip, integer::IntegerBuilder},
    lowering::lower,
    passes::{self, PassConfig},
    state::{ResumeKind, StateMap},
    types::Type,
};

/// Body deliberately precedes its dominator in arena order.
fn fixture(poll: bool) -> (Region, BlockId, BlockId, BlockId, Vec<InstId>) {
    let mut b = IntegerBuilder::new();
    let input = b.gpr;
    let entry = b.block;
    let body = b.region.block(false);
    let header = b.region.block(false);
    let out = b.region.block(false);
    let x = b.region.param(header, Type::I32);
    let n = b.region.param(header, Type::I32);
    let count = b.region.param(header, Type::I32);
    let effect = b.region.param(header, Type::Effect);
    let body_effect = b.region.param(body, Type::Effect);
    let zero = b.constant(0, Type::I32);
    let one = b.constant(1, Type::I32);
    b.region.terminate(
        entry,
        Terminator::Branch(Edge {
            target: header,
            args: vec![input[0], input[1], zero, b.effect],
        }),
    );
    b.gpr[0] = x;
    b.gpr[1] = n;
    let before = b.region.state(StateMap {
        instruction_pc: GuestEip(0x8000),
        next_pc: GuestEip(0x8001),
        next_value: None,
        resume: ResumeKind::BeforeInstruction,
        gpr: b.gpr,
        flags: b.flags.clone(),
        xmm: vec![],
        x87: vec![],
        committed_instructions: 0,
        count_base: Some(count),
        rep_progress: None,
    });
    for block in [header, body, out] {
        b.region.blocks[block.index()].entry_state = Some(before);
    }
    b.block = header;
    let done = b.binary(Binary::Eq, n, zero);
    b.region.terminate(
        header,
        Terminator::CondBranch {
            condition: done,
            taken: Edge {
                target: out,
                args: vec![],
            },
            not_taken: Edge {
                target: body,
                args: vec![effect],
            },
        },
    );
    b.block = body;
    let first = b.region.instructions.len();
    let product = b.binary(Binary::Mul, input[2], input[3]);
    let invariant = b.binary(Binary::Xor, product, one);
    let narrow = b.node(Op::Extract { lsb: 8 }, vec![input[2]], Type::I8);
    let extended = b.node(Op::Extend { signed: true }, vec![narrow], Type::I32);
    let increment = b.binary(Binary::Add, invariant, extended);
    let hoisted = (first..b.region.instructions.len())
        .map(|i| InstId(i as u32))
        .collect();
    let after_effect = if poll {
        b.region.append(
            body,
            Op::PollBudget,
            vec![body_effect],
            &[Type::Effect],
            Some(before),
        )[0]
    } else {
        body_effect
    };
    let next_x = b.binary(Binary::Add, x, increment);
    let next_n = b.binary(Binary::Sub, n, one);
    let next_count = b.binary(Binary::Add, count, one);
    b.region.terminate(
        body,
        Terminator::Branch(Edge {
            target: header,
            args: vec![next_x, next_n, next_count, after_effect],
        }),
    );
    let mut after = b.region.states[before.index()].clone();
    after.resume = ResumeKind::AfterInstruction;
    let after = b.region.state(after);
    b.region.terminate(out, Terminator::Exit(after));
    (b.region, entry, header, body, hoisted)
}

#[test]
fn moves_dependency_chains_but_not_loop_carried_values_or_polls() {
    for poll in [false, true] {
        let (mut r, entry, header, body, expected) = fixture(poll);
        verify(&r).unwrap();
        let states = format!("{:?}", r.states);
        let terms: Vec<_> = r
            .blocks
            .iter()
            .map(|b| format!("{:?}", b.terminator))
            .collect();
        let body_len = r.blocks[body.index()].instructions.len();
        let stats = run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
        assert_eq!(stats.loops, 1);
        assert_eq!(stats.hoisted, expected.len());
        for id in &expected {
            assert_eq!(r.instructions[id.index()].block, entry);
        }
        assert!(r.blocks[entry.index()].instructions.ends_with(&expected));
        assert_eq!(
            r.blocks[body.index()].instructions.len(),
            body_len - expected.len()
        );
        assert_eq!(r.blocks[header.index()].instructions.len(), 1);
        assert_eq!(format!("{:?}", r.states), states);
        assert_eq!(
            r.blocks
                .iter()
                .map(|b| format!("{:?}", b.terminator))
                .collect::<Vec<_>>(),
            terms
        );
        if poll {
            let polls: Vec<_> = r
                .instructions
                .iter()
                .filter(|i| i.op == Op::PollBudget)
                .collect();
            assert_eq!(polls.len(), 1);
            assert_eq!(polls[0].block, body);
        }
        verify(&r).unwrap();
        lower(&r).unwrap();
        assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().hoisted, 0);
    }
}

#[test]
fn rejects_cpu_observations_and_effectful_operations() {
    let (r, _, _, _, ids) = fixture(false);
    let mut inst = r.instructions[ids[0].index()].clone();
    for op in [
        Op::ReadGpr(0),
        Op::ReadXmm(0),
        Op::ReadFlags,
        Op::ReadRawFlags,
        Op::ReadFlagChanges,
        Op::ReadFlagOperand,
        Op::ReadStack32,
        Op::ReadSegment(3),
        Op::SegmentAddress { segment: 3 },
        Op::LinearOffset,
        Op::GuestLoad { bytes: 4 },
        Op::GuestStore { bytes: 4 },
        Op::GuestCheck {
            bytes: 4,
            write: false,
        },
        Op::PartialStore { bytes: 4 },
        Op::RmwLoad {
            bytes: 4,
            order: RmwOrder::Plain,
        },
        Op::RmwStore {
            bytes: 4,
            order: RmwOrder::Locked,
        },
        Op::CallHelper(HelperId(0)),
        Op::PollBudget,
        Op::SseCheck,
        Op::Divide {
            bits: 32,
            signed: true,
        },
        Op::XmmLoad {
            bytes: 16,
            register: 0,
        },
    ] {
        inst.op = op;
        assert!(!eligible(&inst), "{:?}", inst.op);
    }
    for metadata in 0..4 {
        inst = r.instructions[ids[0].index()].clone();
        match metadata {
            0 => inst.state = Some(StateId(0)),
            1 => inst.commit = Some(StateId(0)),
            2 => inst.trap_after_fault = true,
            _ => inst.unmasked_word_store = true,
        }
        assert!(!eligible(&inst));
    }
}

#[test]
fn budget_exhaustion_and_verifier_errors_are_atomic() {
    let (original, _, _, _, _) = fixture(true);
    let mut complete = original.clone();
    let work = run(&mut complete, DEFAULT_WORK_LIMIT).unwrap().work;
    assert!(work > 10);
    for budget in [0, 1, work / 2, work - 1] {
        let mut r = original.clone();
        let before = format!("{r:?}");
        assert!(run(&mut r, budget).is_err());
        assert_eq!(format!("{r:?}"), before);
    }
    let mut exact = original.clone();
    assert!(run(&mut exact, work).is_ok());
    assert_eq!(dump::text(&exact), dump::text(&complete));
    let mut invalid = original;
    invalid.entries[0] = BlockId(u32::MAX);
    let before = format!("{invalid:?}");
    assert!(run(&mut invalid, DEFAULT_WORK_LIMIT).is_err());
    assert_eq!(format!("{invalid:?}"), before);
}

#[test]
fn conditional_preheaders_are_not_speculated() {
    let (mut r, entry, header, _, _) = fixture(false);
    let Terminator::Branch(edge) = r.blocks[entry.index()].terminator.take().unwrap() else {
        panic!()
    };
    // Two syntactic edges to the same loop are deliberately not a dedicated preheader.
    let condition = r.states[0].flags.arithmetic[0];
    r.terminate(
        entry,
        Terminator::CondBranch {
            condition,
            taken: edge.clone(),
            not_taken: edge,
        },
    );
    verify(&r).unwrap();
    assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().hoisted, 0);
    assert!(!r.blocks[header.index()].instructions.is_empty());
}

#[test]
fn self_loop_discovery_stops_at_header_and_external_entries_are_preserved() {
    let mut r = Region::default();
    let entry = r.block(true);
    let header = r.block(false);
    r.terminate(
        entry,
        Terminator::Branch(Edge {
            target: header,
            args: vec![],
        }),
    );
    let value = r.append(header, Op::Const(7), vec![], &[Type::I32], None)[0];
    r.terminate(
        header,
        Terminator::Branch(Edge {
            target: header,
            args: vec![],
        }),
    );
    verify(&r).unwrap();
    let mut rooted = r.clone();
    rooted.entries.push(header);
    assert_eq!(run(&mut rooted, DEFAULT_WORK_LIMIT).unwrap().hoisted, 0);
    assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().hoisted, 1);
    let Definition::Instruction(id, _) = r.values[value.index()].definition else {
        panic!()
    };
    assert_eq!(r.instructions[id.index()].block, entry);
}

#[test]
fn multiple_latches_are_unioned_and_irreducible_cycles_are_skipped() {
    let mut r = Region::default();
    let entry = r.block(true);
    let a = r.block(false);
    let b = r.block(false);
    let header = r.block(false);
    let condition = r.append(entry, Op::Const(1), vec![], &[Type::I1], None)[0];
    r.terminate(
        entry,
        Terminator::Branch(Edge {
            target: header,
            args: vec![],
        }),
    );
    r.terminate(
        header,
        Terminator::CondBranch {
            condition,
            taken: Edge {
                target: a,
                args: vec![],
            },
            not_taken: Edge {
                target: b,
                args: vec![],
            },
        },
    );
    for block in [a, b] {
        r.append(block, Op::Const(42), vec![], &[Type::I32], None);
        r.terminate(
            block,
            Terminator::Branch(Edge {
                target: header,
                args: vec![],
            }),
        );
    }
    verify(&r).unwrap();
    assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().hoisted, 2);
    // Entry can reach either side of a cycle without passing a unique header.
    let mut r = Region::default();
    let entry = r.block(true);
    let a = r.block(false);
    let b = r.block(false);
    let condition = r.append(entry, Op::Const(1), vec![], &[Type::I1], None)[0];
    r.terminate(
        entry,
        Terminator::CondBranch {
            condition,
            taken: Edge {
                target: a,
                args: vec![],
            },
            not_taken: Edge {
                target: b,
                args: vec![],
            },
        },
    );
    for (block, target) in [(a, b), (b, a)] {
        r.append(block, Op::Const(42), vec![], &[Type::I32], None);
        r.terminate(
            block,
            Terminator::Branch(Edge {
                target,
                args: vec![],
            }),
        );
    }
    assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().hoisted, 0);
}

#[test]
fn optimized_pipeline_and_cpu_wasm_fixtures() {
    std::fs::create_dir_all("build/ir-licm").unwrap();
    for poll in [false, true] {
        let (original, _, _, _, _) = fixture(poll);
        for opt in 0..3 {
            let mut r = original.clone();
            if opt == 1 {
                assert!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().hoisted > 0);
            } else if opt == 2 {
                assert!(
                    passes::run(&mut r, PassConfig::default())
                        .unwrap()
                        .loop_hoisted
                        > 0
                );
            }
            let mir = lower(&r).unwrap();
            for budget in [1, 2, 3, 4, 5, 9, 16, 100] {
                std::fs::write(
                    format!("build/ir-licm/{poll}-{opt}-{budget}.wasm"),
                    emit_cpu(&mir, budget).unwrap().bytes,
                )
                .unwrap();
            }
        }
    }
    let (mut r, _, _, _, _) = fixture(false);
    let stats = passes::run(
        &mut r,
        PassConfig {
            licm: false,
            ..PassConfig::default()
        },
    )
    .unwrap();
    assert_eq!(stats.loop_hoisted, 0);
}

#[test]
fn nested_loops_move_invariants_through_both_preheaders() {
    let mut r = Region::default();
    let entry = r.block(true);
    let body = r.block(false);
    let outer = r.block(false);
    let inner_preheader = r.block(false);
    let inner = r.block(false);
    let latch = r.block(false);
    let condition = r.append(entry, Op::Const(1), vec![], &[Type::I1], None)[0];
    let vector = r.append(entry, Op::ReadXmm(0), vec![], &[Type::V128], None)[0];
    for (from, to) in [
        (entry, outer),
        (outer, inner_preheader),
        (inner_preheader, inner),
        (latch, outer),
    ] {
        r.terminate(
            from,
            Terminator::Branch(Edge {
                target: to,
                args: vec![],
            }),
        );
    }
    r.terminate(
        inner,
        Terminator::CondBranch {
            condition,
            taken: Edge {
                target: body,
                args: vec![],
            },
            not_taken: Edge {
                target: latch,
                args: vec![],
            },
        },
    );
    let scalar = r.append(body, Op::Const(42), vec![], &[Type::I32], None)[0];
    let shuffled = r.append(
        body,
        Op::VectorShuffle(std::array::from_fn(|i| (15 - i) as u8)),
        vec![vector, vector],
        &[Type::V128],
        None,
    )[0];
    r.terminate(
        body,
        Terminator::Branch(Edge {
            target: inner,
            args: vec![],
        }),
    );
    let stats = run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!(stats.loops, 2);
    assert_eq!(stats.hoisted, 4);
    for value in [scalar, shuffled] {
        let Definition::Instruction(id, _) = r.values[value.index()].definition else {
            panic!()
        };
        assert_eq!(r.instructions[id.index()].block, entry);
    }
    verify(&r).unwrap();
}

#[test]
fn immutable_cpu_compile_uses_advanced_passes_only_when_enabled() {
    use crate::ir::{
        backend::wasm::StateLayout,
        frontend::decode::{LinearAddress, PhysicalAddress},
        runtime::compile::*,
    };
    let snapshot = ImmutableCodeSnapshot {
        // NOP; loop: MOV EAX,EBX; IMUL EAX,EDX; ADD ESI,EAX; DEC ECX; JNZ loop
        bytes: vec![
            0x90, 0x8B, 0xC3, 0x0F, 0xAF, 0xC2, 0x01, 0xC6, 0x49, 0x75, 0xF6,
        ],
        dependencies: vec![CodeDependency {
            page: PhysicalAddress(0x2000),
            version: 1,
        }],
        mappings: vec![CodeMapping {
            linear: LinearAddress(0x100000),
            physical: PhysicalAddress(0x2000),
        }],
    };
    let request = CompileRequest {
        key: PublicationKey {
            job: 1,
            vm_generation: 1,
            slot: 1,
            slot_generation: 1,
        },
        pc: GuestEip(0x8000),
        linear: LinearAddress(0x100000),
        default_32: true,
        tier: Tier::Two,
    };
    let mut config = IrConfig {
        optimize: true,
        passes: PassConfig::default(),
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
    let compiled = compile_cpu_cfg_region(&request, &snapshot, &config).unwrap();
    assert!(compiled.passes.loop_hoisted > 0);
    assert!(compiled.current(
        request.key,
        &snapshot.dependencies,
        compiled.entry,
        &snapshot.mappings
    ));
    config.passes.licm = false;
    assert_eq!(
        compile_cpu_cfg_region(&request, &snapshot, &config)
            .unwrap()
            .passes
            .loop_hoisted,
        0
    );
    config.passes.licm = true;
    config.optimize = false;
    let compiled = compile_cpu_cfg_region(&request, &snapshot, &config).unwrap();
    assert_eq!(compiled.passes.loop_hoisted, 0);
    assert_eq!(compiled.passes.identities, 0);
}
