use super::{run, Config};
use crate::ir::{
    analysis::{
        cfg::Cfg,
        loops::{discover, WorkBudget},
    },
    backend::wasm::{emit, emit_cpu, StateLayout},
    frontend::{
        decode::{GuestEip, LinearAddress, PhysicalAddress},
        integer::IntegerBuilder,
    },
    hir::*,
    ids::*,
    lowering::lower,
    runtime::compile::*,
    state::{ResumeKind, StateMap},
    types::Type,
    verify::verify,
};

fn owner(region: &Region, value: ValueId) -> BlockId {
    let Definition::Instruction(id, _) = region.values[value.index()].definition else {
        panic!()
    };
    region.instructions[id.index()].block
}
fn state(b: &mut IntegerBuilder, pc: u32, count: Option<ValueId>) -> StateId {
    b.region.state(StateMap {
        instruction_pc: GuestEip(pc),
        next_pc: GuestEip(pc + 1),
        next_value: None,
        resume: ResumeKind::BeforeInstruction,
        gpr: b.gpr,
        flags: b.flags.clone(),
        xmm: vec![],
        x87: vec![],
        committed_instructions: 0,
        count_base: count,
        rep_progress: None,
    })
}
struct Fixture {
    region: Region,
    entry: BlockId,
    header: BlockId,
    body: BlockId,
    exit: BlockId,
    invariant: ValueId,
    varying: ValueId,
    probe: InstId,
}
fn fixture() -> Fixture {
    let mut b = IntegerBuilder::new();
    let input = b.gpr;
    let entry = b.block;
    // Deliberately allocate the use block before the dominating header.
    let body = b.region.block(false);
    let header = b.region.block(false);
    let exit = b.region.block(false);
    let acc = b.region.param(header, Type::I32);
    let n = b.region.param(header, Type::I32);
    let count = b.region.param(header, Type::I32);
    let he = b.region.param(header, Type::Effect);
    let be = b.region.param(body, Type::Effect);
    let zero = b.constant(0, Type::I32);
    let one = b.constant(1, Type::I32);
    b.region.terminate(
        entry,
        Terminator::Branch(Edge {
            target: header,
            args: vec![input[0], input[1], zero, b.effect],
        }),
    );
    b.gpr[0] = acc;
    b.gpr[1] = n;
    let before = state(&mut b, 0x8100, Some(count));
    for block in [header, body, exit] {
        b.region.blocks[block.index()].entry_state = Some(before);
    }
    b.block = header;
    let done = b.binary(Binary::Eq, n, zero);
    b.region.terminate(
        header,
        Terminator::CondBranch {
            condition: done,
            taken: Edge {
                target: exit,
                args: vec![],
            },
            not_taken: Edge {
                target: body,
                args: vec![he],
            },
        },
    );
    b.block = body;
    let sum = b.binary(Binary::Add, input[2], input[3]);
    let product = b.binary(Binary::Mul, sum, input[4]);
    let invariant = b.binary(Binary::Shl, product, input[5]);
    let after_poll = b.region.append(
        body,
        Op::PollBudget,
        vec![be],
        &[Type::Effect],
        Some(before),
    )[0];
    let Definition::Instruction(probe, _) = b.region.values[after_poll.index()].definition else {
        panic!()
    };
    let varying = b.binary(Binary::Add, acc, invariant);
    let next_n = b.binary(Binary::Sub, n, one);
    let next_count = b.binary(Binary::Add, count, one);
    b.region.terminate(
        body,
        Terminator::Branch(Edge {
            target: header,
            args: vec![varying, next_n, next_count, after_poll],
        }),
    );
    b.region.terminate(exit, Terminator::Exit(before));
    verify(&b.region).unwrap();
    Fixture {
        region: b.region,
        entry,
        header,
        body,
        exit,
        invariant,
        varying,
        probe,
    }
}

#[test]
fn hoists_dependency_chain_but_preserves_phis_polls_and_recovery() {
    let mut f = fixture();
    let states = format!("{:?}", f.region.states);
    let old_probe = format!("{:?}", f.region.instructions[f.probe.index()]);
    let stats = run(&mut f.region, Config::default()).unwrap();
    assert_eq!(stats.loops, 1);
    assert_eq!(stats.hoisted, 3);
    assert_eq!(owner(&f.region, f.invariant), f.entry);
    assert_eq!(owner(&f.region, f.varying), f.body);
    assert_eq!(format!("{:?}", f.region.states), states);
    assert_eq!(
        format!("{:?}", f.region.instructions[f.probe.index()]),
        old_probe
    );
    verify(&f.region).unwrap();
    lower(&f.region).unwrap();
    assert_eq!(run(&mut f.region, Config::default()).unwrap().hoisted, 0);
}

#[test]
fn budget_failures_and_invalid_input_are_atomic() {
    for config in [
        Config {
            max_work: 0,
            ..Config::default()
        },
        Config {
            max_work: 10,
            ..Config::default()
        },
        Config {
            max_hoists: 2,
            ..Config::default()
        },
    ] {
        let mut f = fixture();
        let before = format!("{:?}", f.region);
        assert!(run(&mut f.region, config).is_err());
        assert_eq!(format!("{:?}", f.region), before);
    }
    let mut f = fixture();
    f.region.instructions[f.probe.index()].state = None;
    let before = format!("{:?}", f.region);
    assert!(run(&mut f.region, Config::default()).is_err());
    assert_eq!(format!("{:?}", f.region), before);
    let mut large = fixture().region;
    for _ in 0..65 {
        large.block(false);
    }
    let before = format!("{:?}", large);
    assert!(run(&mut large, Config::default())
        .unwrap_err()
        .contains("region budget"));
    assert_eq!(format!("{:?}", large), before);
}

#[test]
fn mutable_cpu_reads_and_stateful_nodes_are_never_movable() {
    let pinned = [
        Op::ReadGpr(0),
        Op::ReadXmm(0),
        Op::ReadFlags,
        Op::ReadRawFlags,
        Op::ReadFlagChanges,
        Op::ReadFlagOperand,
        Op::ReadSegment(3),
        Op::ReadStack32,
        Op::GuestLoad { bytes: 4 },
        Op::GuestStore { bytes: 4 },
        Op::SegmentAddress { segment: 3 },
        Op::GuestCheck {
            bytes: 4,
            write: false,
        },
        Op::Divide {
            bits: 32,
            signed: true,
        },
        Op::SseCheck,
        Op::PollBudget,
        Op::CallHelper(HelperId(0)),
        Op::RmwLoad {
            bytes: 4,
            order: RmwOrder::Locked,
        },
    ];
    let f = fixture();
    let Definition::Instruction(id, _) = f.region.values[f.invariant.index()].definition else {
        panic!()
    };
    for op in pinned {
        let mut inst = f.region.instructions[id.index()].clone();
        inst.op = op;
        assert!(!super::movable(&inst), "{:?}", inst.op);
    }
    for kind in 0..4 {
        let mut inst = f.region.instructions[id.index()].clone();
        match kind {
            0 => inst.state = Some(StateId(0)),
            1 => inst.commit = Some(StateId(0)),
            2 => inst.trap_after_fault = true,
            _ => inst.unmasked_word_store = true,
        }
        assert!(!super::movable(&inst));
    }
    let mut f = fixture();
    let Terminator::Branch(mut edge) = f.region.blocks[f.body.index()].terminator.take().unwrap()
    else {
        panic!()
    };
    let effect = f.region.instructions[f.probe.index()].results[0];
    let snapshot = f.region.instructions[f.probe.index()].state;
    let input = f.region.states[snapshot.unwrap().index()].gpr[2];
    f.region.helpers.push(crate::ir::helper::HelperDescriptor {
        name: "pinned_read".into(),
        params: vec![Type::I32],
        results: vec![Type::I32],
        effects: crate::ir::effects::Effects::pure(),
        exception_owner: crate::ir::helper::ExceptionOwner::CannotFault,
        abi: crate::ir::helper::HelperAbi::Outcome {
            fault_delivery: None,
            normal_preserves_state: true,
        },
    });
    let call = f.region.append(
        f.body,
        Op::CallHelper(HelperId(0)),
        vec![input, effect],
        &[Type::I32, Type::Effect],
        snapshot,
    );
    let value = call[0];
    let dependent = f.region.append(
        f.body,
        Op::Binary(Binary::Add),
        vec![value, value],
        &[Type::I32],
        None,
    )[0];
    edge.args[3] = call[1];
    f.region.terminate(f.body, Terminator::Branch(edge));
    run(&mut f.region, Config::default()).unwrap();
    assert_eq!(owner(&f.region, value), f.body);
    assert_eq!(owner(&f.region, dependent), f.body);
}

/// Pure CFG fixtures separate loop recognition from state-map construction.
fn graph(edges: &[&[usize]], entries: &[usize]) -> Region {
    let mut r = Region::default();
    for b in 0..edges.len() {
        r.block(entries.contains(&b));
    }
    for (b, targets) in edges.iter().enumerate() {
        let b = BlockId(b as u32);
        let edge = |target: usize| Edge {
            target: BlockId(target as u32),
            args: vec![],
        };
        let term = match *targets {
            [one] => Terminator::Branch(edge(*one)),
            [one, two] => {
                let condition = r.append(b, Op::Const(0), vec![], &[Type::I1], None)[0];
                Terminator::CondBranch {
                    condition,
                    taken: edge(*one),
                    not_taken: edge(*two),
                }
            },
            _ => panic!("fixture needs one or two successors"),
        };
        r.terminate(b, term);
    }
    verify(&r).unwrap();
    r
}
fn loops(r: &Region) -> Vec<crate::ir::analysis::loops::NaturalLoop> {
    discover(r, &Cfg::compute(r).unwrap(), &mut WorkBudget::new(10000)).unwrap()
}
#[test]
fn recognizes_self_loops_multiple_latches_and_nested_loops() {
    let r = graph(&[&[1], &[1]], &[0]);
    let found = loops(&r);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].members, [false, true]);
    let r = graph(&[&[1], &[2, 3], &[1], &[1]], &[0]);
    let found = loops(&r);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].members, [false, true, true, true]);
    let r = graph(&[&[1], &[2], &[2, 3], &[1]], &[0]);
    let found = loops(&r);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].header, BlockId(2));
    assert_eq!(found[0].preheader, BlockId(1));
    assert_eq!(found[1].header, BlockId(1));
}
#[test]
fn rejects_external_entries_side_entries_and_ambiguous_preheaders() {
    // Entry-header backedge; independent external entry; irreducible two-entry SCC.
    for r in [
        graph(&[&[0]], &[0]),
        graph(&[&[1], &[1]], &[0, 1]),
        graph(&[&[1, 2], &[2], &[1]], &[0]),
        graph(&[&[1, 2], &[3], &[3], &[3]], &[0]),
        // One predecessor, but not a dedicated unconditional preheader.
        graph(&[&[1, 2], &[1], &[2]], &[0]),
    ] {
        assert!(loops(&r).is_empty());
    }
}
#[test]
fn nested_motion_keeps_dependency_order_and_is_bounded() {
    let mut r = graph(&[&[1], &[2], &[2, 3], &[1]], &[0]);
    let term = r.blocks[2].terminator.take().unwrap();
    let a = r.append(BlockId(2), Op::Const(3), vec![], &[Type::I32], None)[0];
    let b = r.append(
        BlockId(2),
        Op::Binary(Binary::Add),
        vec![a, a],
        &[Type::I32],
        None,
    )[0];
    r.terminate(BlockId(2), term);
    let stats = run(&mut r, Config::default()).unwrap();
    assert!(stats.hoisted >= 4);
    assert_eq!(owner(&r, a), BlockId(0));
    assert_eq!(owner(&r, b), BlockId(0));
    verify(&r).unwrap();
}

#[test]
fn scalar_and_simd_execution_fixtures() {
    std::fs::create_dir_all("build/ir-licm").unwrap();
    for vector in [false, true] {
        let mut f = fixture();
        if vector {
            // Pure SIMD calculation with SSA inputs, not an XMM backing read.
            let term = f.region.blocks[f.entry.index()].terminator.take().unwrap();
            let v = f
                .region
                .append(f.entry, Op::ReadXmm(0), vec![], &[Type::V128], None)[0];
            f.region.terminate(f.entry, term);
            let term = f.region.blocks[f.header.index()].terminator.take().unwrap();
            let v = f.region.append(
                f.header,
                Op::VectorBinary(crate::ir::simd::PackedOp::Add32),
                vec![v, v],
                &[Type::V128],
                None,
            )[0];
            let v = f.region.append(
                f.header,
                Op::VectorShuffle([15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0]),
                vec![v, v],
                &[Type::V128],
                None,
            )[0];
            let value = f.region.append(
                f.header,
                Op::VectorExtract { bits: 32, lane: 0 },
                vec![v],
                &[Type::I32],
                None,
            )[0];
            f.region.terminate(f.header, term);
            let Definition::Instruction(id, _) = f.region.values[f.invariant.index()].definition
            else {
                panic!()
            };
            f.region.instructions[id.index()].args[0] = value;
        }
        for optimized in [false, true] {
            let mut r = f.region.clone();
            if optimized {
                assert!(run(&mut r, Config::default()).unwrap().hoisted > 0);
            }
            for budget in [1, 2, 3, 4, 5, 9, 16, 1000] {
                let mir = lower(&r).unwrap();
                let artifact = if vector {
                    emit_cpu(&mir, budget).unwrap()
                } else {
                    emit(
                        &mir,
                        StateLayout {
                            gpr: 0,
                            flags: 32,
                            eip: 36,
                            committed: 40,
                            flag_operand: 44,
                        },
                        budget,
                    )
                    .unwrap()
                };
                std::fs::write(
                    format!("build/ir-licm/{vector}-{optimized}-{budget}.wasm"),
                    artifact.bytes,
                )
                .unwrap();
            }
        }
    }
}

#[test]
fn tier_two_compiler_entry_uses_licm_without_changing_tier_one() {
    // MOV EAX,0; body: MOV EDX,EBX; ADD EDX,ESI; ADD EAX,EDX; DEC ECX; JNZ body.
    let bytes = vec![
        0xB8, 0, 0, 0, 0, 0x89, 0xDA, 0x01, 0xF2, 0x01, 0xD0, 0x49, 0x75, 0xF7,
    ];
    let snapshot = ImmutableCodeSnapshot {
        bytes,
        dependencies: vec![CodeDependency {
            page: PhysicalAddress(0x100000),
            version: 1,
        }],
        mappings: vec![CodeMapping {
            linear: LinearAddress(0x100000),
            physical: PhysicalAddress(0x100000),
        }],
    };
    let mut request = CompileRequest {
        key: PublicationKey {
            job: 1,
            vm_generation: 1,
            slot: 1,
            slot_generation: 1,
        },
        pc: GuestEip(0x1000),
        linear: LinearAddress(0x100000),
        default_32: true,
        tier: Tier::One,
    };
    let mut config = IrConfig {
        optimize: true,
        passes: Default::default(),
        execution_budget: 100,
        rep_iteration_budget: 8,
        max_code_bytes: 128,
        layout: StateLayout {
            gpr: 0,
            flags: 32,
            eip: 36,
            committed: 40,
            flag_operand: 44,
        },
    };
    assert_eq!(
        compile_cpu_cfg_region(&request, &snapshot, &config)
            .unwrap()
            .passes
            .loop_hoisted,
        0
    );
    request.tier = Tier::Two;
    assert!(
        compile_cpu_cfg_region(&request, &snapshot, &config)
            .unwrap()
            .passes
            .loop_hoisted
            > 0
    );
    config.optimize = false;
    assert_eq!(
        compile_cpu_cfg_region(&request, &snapshot, &config)
            .unwrap()
            .passes
            .loop_hoisted,
        0
    );
}
