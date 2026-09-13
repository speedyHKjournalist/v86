use super::*;
use crate::ir::{
    backend::wasm::{emit, StateLayout},
    frontend::{decode::GuestEip, integer::IntegerBuilder},
    lowering::lower,
    state::{FlagState, ResumeKind, StateMap},
    types::Type,
};

fn snapshot(r: &mut Region, gpr: [ValueId; 8], flags: &FlagState, pc: u32) -> StateId {
    r.state(StateMap {
        instruction_pc: GuestEip(pc),
        next_pc: GuestEip(pc + 1),
        next_value: None,
        resume: ResumeKind::BeforeInstruction,
        gpr,
        flags: flags.clone(),
        xmm: vec![],
        x87: vec![],
        committed_instructions: 0,
        count_base: None,
        rep_progress: None,
    })
}

// EAX iterations; EBX accumulator; invariant = (ECX + EDX) * 7.
// Allocate the body before its header to test dominance order, not arena order.
fn counting_loop() -> (Region, BlockId, BlockId, ValueId) {
    let mut b = IntegerBuilder::new();
    let input = b.gpr;
    let entry = b.block;
    let body = b.region.block(false);
    let header = b.region.block(false);
    let exit = b.region.block(false);
    let he = b.region.param(header, Type::Effect);
    let count = b.region.param(header, Type::I32);
    let acc = b.region.param(header, Type::I32);
    let be = b.region.param(body, Type::Effect);
    let xe = b.region.param(exit, Type::Effect);
    b.region.terminate(
        entry,
        Terminator::Branch(Edge {
            target: header,
            args: vec![b.effect, input[0], input[1]],
        }),
    );
    b.gpr[0] = count;
    b.gpr[1] = acc;
    for block in [header, body, exit] {
        let state = snapshot(&mut b.region, b.gpr, &b.flags, 0x1000 + block.0);
        b.region.blocks[block.index()].entry_state = Some(state);
    }
    b.block = header;
    b.effect = he;
    let zero = b.constant(0, Type::I32);
    let done = b.binary(Binary::Eq, count, zero);
    b.region.terminate(
        header,
        Terminator::CondBranch {
            condition: done,
            taken: Edge {
                target: exit,
                args: vec![he],
            },
            not_taken: Edge {
                target: body,
                args: vec![he],
            },
        },
    );
    b.block = body;
    b.effect = be;
    let seven = b.constant(7, Type::I32);
    let sum = b.binary(Binary::Add, input[2], input[3]);
    let invariant = b.binary(Binary::Mul, sum, seven);
    let next_acc = b.binary(Binary::Add, acc, invariant);
    let one = b.constant(1, Type::I32);
    let next_count = b.binary(Binary::Sub, count, one);
    b.region.terminate(
        body,
        Terminator::Branch(Edge {
            target: header,
            args: vec![be, next_count, next_acc],
        }),
    );
    b.block = exit;
    b.effect = xe;
    let state = snapshot(&mut b.region, b.gpr, &b.flags, 0x2000);
    b.region.terminate(exit, Terminator::Exit(state));
    (b.region, entry, body, invariant)
}

fn owner(r: &Region, value: ValueId) -> BlockId {
    let Definition::Instruction(id, _) = r.values[value.index()].definition else {
        panic!()
    };
    r.instructions[id.index()].block
}

#[test]
fn hoists_chains_without_changing_cfg_or_state_maps() {
    let (mut r, entry, body, invariant) = counting_loop();
    verify(&r).unwrap();
    let terms = format!(
        "{:?}",
        r.blocks.iter().map(|b| &b.terminator).collect::<Vec<_>>()
    );
    let states = format!("{:?}", r.states);
    let stats = run(&mut r, LicmConfig::default()).unwrap();
    assert_eq!(stats.loops, 1);
    assert!(stats.hoisted >= 3);
    assert_eq!(owner(&r, invariant), entry);
    assert!(r.blocks[body.index()]
        .instructions
        .iter()
        .any(|id| r.instructions[id.index()].op == Op::Binary(Binary::Sub)));
    assert_eq!(states, format!("{:?}", r.states));
    assert_eq!(
        terms,
        format!(
            "{:?}",
            r.blocks.iter().map(|b| &b.terminator).collect::<Vec<_>>()
        )
    );
    verify(&r).unwrap();
    lower(&r).unwrap();
    assert_eq!(run(&mut r, LicmConfig::default()).unwrap().hoisted, 0);
}

#[test]
fn budget_failures_are_atomic() {
    let (r, _, _, _) = counting_loop();
    for config in [
        LicmConfig {
            max_work: 0,
            max_hoisted: 256,
        },
        LicmConfig {
            max_work: 18,
            max_hoisted: 256,
        },
        LicmConfig {
            max_work: 1_000_000,
            max_hoisted: 1,
        },
    ] {
        let mut attempted = r.clone();
        assert!(run(&mut attempted, config).is_err());
        assert_eq!(format!("{r:?}"), format!("{attempted:?}"));
    }
    let mut invalid = r;
    invalid.blocks[0].instructions.push(InstId(u32::MAX));
    let before = format!("{invalid:?}");
    assert!(run(&mut invalid, LicmConfig::default()).is_err());
    assert_eq!(before, format!("{invalid:?}"));
}

#[test]
fn cpu_observations_and_ordered_nodes_are_never_speculated() {
    let (mut r, _, body, _) = counting_loop();
    let term = r.blocks[body.index()].terminator.take().unwrap();
    let before = r.blocks[body.index()].entry_state;
    let input = r.states[before.unwrap().index()].gpr[2];
    let effect = r.blocks[body.index()].params[0];
    let address = r.append(
        body,
        Op::SegmentAddress { segment: 3 },
        vec![input, effect],
        &[Type::LinearAddress, Type::Effect],
        before,
    );
    let loaded = r.append(
        body,
        Op::GuestLoad { bytes: 4 },
        vec![address[0], address[1]],
        &[Type::I32, Type::Effect],
        before,
    );
    let read = loaded[0];
    let derived = r.append(
        body,
        Op::Binary(Binary::Add),
        vec![read, read],
        &[Type::I32],
        None,
    )[0];
    let after = r.append(
        body,
        Op::PollBudget,
        vec![loaded[1]],
        &[Type::Effect],
        before,
    )[0];
    let Terminator::Branch(mut edge) = term else {
        panic!()
    };
    edge.args[0] = after;
    r.terminate(body, Terminator::Branch(edge));
    run(&mut r, LicmConfig::default()).unwrap();
    assert_eq!(owner(&r, read), body);
    assert_eq!(owner(&r, derived), body);
    assert_eq!(owner(&r, after), body);
    for op in [
        Op::ReadGpr(0),
        Op::ReadFlags,
        Op::ReadXmm(0),
        Op::ReadSegment(0),
        Op::GuestLoad { bytes: 4 },
        Op::GuestCheck {
            bytes: 4,
            write: false,
        },
        Op::GuestStore { bytes: 4 },
        Op::Divide {
            bits: 32,
            signed: false,
        },
        Op::CallHelper(HelperId(0)),
        Op::SseCheck,
        Op::PollBudget,
    ] {
        let inst = Instruction {
            block: body,
            op,
            args: vec![],
            results: vec![read],
            state: None,
            commit: None,
            trap_after_fault: false,
            unmasked_word_store: false,
        };
        assert!(!movable(&inst));
    }
}

#[test]
fn existing_unconditional_preheader_is_required() {
    let (mut r, entry, _, invariant) = counting_loop();
    let old_owner = owner(&r, invariant);
    let term = r.blocks[entry.index()].terminator.take().unwrap();
    let Terminator::Branch(edge) = term else {
        panic!()
    };
    let condition = r.append(entry, Op::Const(1), vec![], &[Type::I1], None)[0];
    r.terminate(
        entry,
        Terminator::CondBranch {
            condition,
            taken: edge.clone(),
            not_taken: edge,
        },
    );
    assert_eq!(run(&mut r, LicmConfig::default()).unwrap().hoisted, 0);
    assert_eq!(owner(&r, invariant), old_owner);
}

#[test]
fn loop_entry_and_irreducible_cycles_are_left_alone() {
    // A self-loop is also an external entry. No preheader may be invented.
    let mut r = Region::default();
    let entry = r.block(true);
    r.append(entry, Op::Const(9), vec![], &[Type::I32], None);
    r.terminate(
        entry,
        Terminator::Branch(Edge {
            target: entry,
            args: vec![],
        }),
    );
    assert_eq!(run(&mut r, LicmConfig::default()).unwrap().hoisted, 0);
    // Two external entries into an A <-> B SCC: neither dominates the other.
    let mut r = Region::default();
    let a = r.block(true);
    let b = r.block(true);
    for (from, to) in [(a, b), (b, a)] {
        r.append(from, Op::Const(9), vec![], &[Type::I32], None);
        r.terminate(
            from,
            Terminator::Branch(Edge {
                target: to,
                args: vec![],
            }),
        );
    }
    assert_eq!(run(&mut r, LicmConfig::default()).unwrap().hoisted, 0);
}

#[test]
fn emits_optimized_and_reference_modules() {
    let layout = StateLayout {
        gpr: 0,
        flags: 32,
        eip: 36,
        committed: 40,
        flag_operand: 44,
    };
    std::fs::create_dir_all("build/ir-licm").unwrap();
    for budget in [1, 2, 3, 4, 5, 9, 16, 100] {
        for optimized in [false, true] {
            let (mut r, _, _, _) = counting_loop();
            if optimized {
                run(&mut r, LicmConfig::default()).unwrap();
            }
            std::fs::write(
                format!("build/ir-licm/loop-{budget}-{optimized}.wasm"),
                emit(&lower(&r).unwrap(), layout, budget).unwrap().bytes,
            )
            .unwrap();
        }
    }
}

fn nested_loop() -> (Region, BlockId, BlockId, ValueId, ValueId) {
    let mut r = Region::default();
    let entry = r.block(true);
    let outer = r.block(false);
    let preheader = r.block(false);
    let inner = r.block(false);
    let body = r.block(false);
    let latch = r.block(false);
    let input = r.param(entry, Type::I32);
    let carried = r.param(outer, Type::I32);
    let condition = r.append(entry, Op::Const(1), vec![], &[Type::I1], None)[0];
    r.terminate(
        entry,
        Terminator::Branch(Edge {
            target: outer,
            args: vec![input],
        }),
    );
    for (from, to) in [(outer, preheader), (preheader, inner), (inner, body)] {
        r.terminate(
            from,
            Terminator::Branch(Edge {
                target: to,
                args: vec![],
            }),
        );
    }
    let invariant = r.append(
        body,
        Op::Binary(Binary::Add),
        vec![input, input],
        &[Type::I32],
        None,
    )[0];
    let outer_variant = r.append(
        body,
        Op::Binary(Binary::Add),
        vec![carried, input],
        &[Type::I32],
        None,
    )[0];
    r.terminate(
        body,
        Terminator::CondBranch {
            condition,
            taken: Edge {
                target: inner,
                args: vec![],
            },
            not_taken: Edge {
                target: latch,
                args: vec![],
            },
        },
    );
    r.terminate(
        latch,
        Terminator::Branch(Edge {
            target: outer,
            args: vec![carried],
        }),
    );
    (r, entry, preheader, invariant, outer_variant)
}

#[test]
fn nested_loops_distinguish_inner_and_outer_invariance() {
    let (mut r, entry, preheader, invariant, outer_variant) = nested_loop();
    let before = format!("{r:?}");
    let mut failure = r.clone();
    // The inner loop performs two moves; the outer move then exhausts budget.
    assert!(run(
        &mut failure,
        LicmConfig {
            max_work: 1_000_000,
            max_hoisted: 2
        }
    )
    .is_err());
    assert_eq!(before, format!("{failure:?}"));
    let stats = run(&mut r, LicmConfig::default()).unwrap();
    assert_eq!(stats.loops, 2);
    assert_eq!(stats.hoisted, 3);
    assert_eq!(owner(&r, invariant), entry);
    assert_eq!(owner(&r, outer_variant), preheader);
    assert_eq!(run(&mut r, LicmConfig::default()).unwrap().hoisted, 0);
}

#[test]
fn all_latches_of_one_header_form_one_loop() {
    let mut r = Region::default();
    let entry = r.block(true);
    let header = r.block(false);
    let left = r.block(false);
    let right = r.block(false);
    let input = r.param(entry, Type::I32);
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
                target: left,
                args: vec![],
            },
            not_taken: Edge {
                target: right,
                args: vec![],
            },
        },
    );
    let mut values = Vec::new();
    for block in [left, right] {
        values.push(
            r.append(
                block,
                Op::Binary(Binary::Add),
                vec![input, input],
                &[Type::I32],
                None,
            )[0],
        );
        r.terminate(
            block,
            Terminator::Branch(Edge {
                target: header,
                args: vec![],
            }),
        );
    }
    let stats = run(&mut r, LicmConfig::default()).unwrap();
    assert_eq!(stats.loops, 1);
    assert_eq!(stats.hoisted, 2);
    assert!(values.iter().all(|&v| owner(&r, v) == entry));
}

#[test]
fn tier_two_integration_and_per_pass_opt_out() {
    use crate::ir::frontend::decode::{LinearAddress, PhysicalAddress};
    use crate::ir::passes::PassConfig;
    use crate::ir::runtime::compile::*;
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
        tier: Tier::Two,
    };
    // NOP preheader; MOV EBX,EDX; ADD EBX,ECX; DEC EAX; JNZ loop; NOP.
    let snapshot = ImmutableCodeSnapshot {
        bytes: vec![0x90, 0x89, 0xD3, 0x01, 0xCB, 0x48, 0x75, 0xF9, 0x90],
        dependencies: vec![CodeDependency {
            page: PhysicalAddress(0x2000),
            version: 1,
        }],
        mappings: vec![CodeMapping {
            linear: LinearAddress(0x100000),
            physical: PhysicalAddress(0x2000),
        }],
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
    assert!(
        compile_cpu_cfg_region(&request, &snapshot, &config)
            .unwrap()
            .passes
            .hoisted
            > 0
    );
    request.tier = Tier::One;
    assert_eq!(
        compile_cpu_cfg_region(&request, &snapshot, &config)
            .unwrap()
            .passes
            .hoisted,
        0
    );
    request.tier = Tier::Two;
    config.passes.licm = false;
    assert_eq!(
        compile_cpu_cfg_region(&request, &snapshot, &config)
            .unwrap()
            .passes
            .hoisted,
        0
    );
    config.passes.licm = true;
    config.optimize = false;
    assert_eq!(
        compile_cpu_cfg_region(&request, &snapshot, &config)
            .unwrap()
            .passes
            .hoisted,
        0
    );
}
