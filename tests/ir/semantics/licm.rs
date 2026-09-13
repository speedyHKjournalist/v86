use super::{run, DEFAULT_WORK_LIMIT};
use crate::ir::{
    backend::wasm::{emit, emit_cpu, StateLayout},
    frontend::{
        decode::{GuestEip, LinearAddress, PhysicalAddress},
        integer::IntegerBuilder,
    },
    hir::*,
    ids::*,
    lowering::lower,
    passes::{self, PassConfig},
    runtime::compile::*,
    state::{ResumeKind, StateMap},
    types::Type,
    verify::verify,
};

fn layout() -> StateLayout {
    StateLayout {
        gpr: 0,
        flags: 32,
        eip: 36,
        committed: 40,
        flag_operand: 44,
    }
}

struct Fixture {
    region: Region,
    entry: BlockId,
    header: BlockId,
    body: BlockId,
    exit: BlockId,
    invariant: ValueId,
    variant: ValueId,
}

// Deliberately allocate the body before its dominator. All states include the
// loop-carried progress, so early budget exits observe the precise iteration.
fn fixture() -> Fixture {
    let mut b = IntegerBuilder::new();
    let entry = b.block;
    let input = b.gpr;
    let body = b.region.block(false);
    let header = b.region.block(false);
    let exit = b.region.block(false);
    let effect = b.region.param(header, Type::Effect);
    let body_effect = b.region.param(body, Type::Effect);
    let n = b.region.param(header, Type::I32);
    let acc = b.region.param(header, Type::I32);
    let done = b.region.param(header, Type::I32);
    let zero = b.constant(0, Type::I32);
    b.region.terminate(
        entry,
        Terminator::Branch(Edge {
            target: header,
            args: vec![b.effect, input[2], input[3], zero],
        }),
    );
    let mut gpr = input;
    gpr[2] = n;
    gpr[3] = acc;
    let make_state = |b: &mut IntegerBuilder, pc| {
        b.region.state(StateMap {
            instruction_pc: GuestEip(pc),
            next_pc: GuestEip(pc + 1),
            next_value: None,
            resume: ResumeKind::BeforeInstruction,
            gpr,
            flags: b.flags.clone(),
            xmm: vec![],
            x87: vec![],
            committed_instructions: 0,
            count_base: Some(done),
            rep_progress: None,
        })
    };
    for block in [header, body, exit] {
        let state = make_state(&mut b, 0x2000 + block.0);
        b.region.blocks[block.index()].entry_state = Some(state);
    }
    b.block = header;
    let condition = b.binary(Binary::Ult, zero, n);
    b.region.terminate(
        header,
        Terminator::CondBranch {
            condition,
            taken: Edge {
                target: body,
                args: vec![effect],
            },
            not_taken: Edge {
                target: exit,
                args: vec![],
            },
        },
    );
    b.block = body;
    let a = b.binary(Binary::Add, input[0], input[1]);
    let invariant = b.binary(Binary::Mul, a, input[0]);
    let variant = b.binary(Binary::Add, acc, invariant);
    let one = b.constant(1, Type::I32);
    let next_n = b.binary(Binary::Sub, n, one);
    let next_done = b.binary(Binary::Add, done, one);
    b.region.terminate(
        body,
        Terminator::Branch(Edge {
            target: header,
            args: vec![body_effect, next_n, variant, next_done],
        }),
    );
    let state = make_state(&mut b, 0x3000);
    b.region.terminate(exit, Terminator::Exit(state));
    Fixture {
        region: b.region,
        entry,
        header,
        body,
        exit,
        invariant,
        variant,
    }
}

fn owner(r: &Region, v: ValueId) -> BlockId {
    match r.values[v.index()].definition {
        Definition::Instruction(id, _) => r.instructions[id.index()].block,
        Definition::Parameter(b, _) => b,
    }
}

#[test]
fn invariant_dependency_chain_moves_but_loop_carried_values_do_not() {
    let mut f = fixture();
    verify(&f.region).unwrap();
    let states = format!("{:?}", f.region.states);
    let stats = run(&mut f.region, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!(stats.loops, 1);
    assert_eq!(stats.hoisted, 3); // add, multiply and constant one
    assert_eq!(owner(&f.region, f.invariant), f.entry);
    assert_eq!(owner(&f.region, f.variant), f.body);
    assert_eq!(states, format!("{:?}", f.region.states));
    verify(&f.region).unwrap();
    lower(&f.region).unwrap();
    assert_eq!(run(&mut f.region, DEFAULT_WORK_LIMIT).unwrap().hoisted, 0);
}

#[test]
fn budget_exhaustion_is_atomic_even_after_staged_motion() {
    let source = fixture().region;
    let mut reference = source.clone();
    let stats = run(&mut reference, DEFAULT_WORK_LIMIT).unwrap();
    for budget in [0, 1, stats.work / 2, stats.work - 1] {
        let mut r = source.clone();
        let before = format!("{r:?}");
        assert!(run(&mut r, budget).is_err(), "budget {budget}");
        assert_eq!(format!("{r:?}"), before, "rollback at budget {budget}");
    }
    let mut exact = source.clone();
    assert_eq!(run(&mut exact, stats.work).unwrap(), stats);
}

#[test]
fn invalid_input_and_region_cap_are_rejected_without_mutation() {
    let mut r = fixture().region;
    r.blocks[0].terminator = None;
    let before = format!("{r:?}");
    assert!(run(&mut r, DEFAULT_WORK_LIMIT).is_err());
    assert_eq!(format!("{r:?}"), before);
    let mut r = fixture().region;
    r.blocks.resize_with(65, Block::default);
    let before = format!("{r:?}");
    assert!(run(&mut r, DEFAULT_WORK_LIMIT)
        .unwrap_err()
        .contains("region budget"));
    assert_eq!(format!("{r:?}"), before);
}

#[test]
fn state_attached_pure_nodes_and_polls_are_not_moved() {
    let mut f = fixture();
    let state = f.region.blocks[f.body.index()].entry_state.unwrap();
    let Definition::Instruction(id, _) = f.region.values[f.invariant.index()].definition else {
        panic!()
    };
    f.region.instructions[id.index()].state = Some(state);
    let term = f.region.blocks[f.body.index()].terminator.take().unwrap();
    let effect = f.region.blocks[f.body.index()].params[0];
    let after = f.region.append(
        f.body,
        Op::PollBudget,
        vec![effect],
        &[Type::Effect],
        Some(state),
    )[0];
    let Terminator::Branch(mut edge) = term else {
        panic!()
    };
    edge.args[0] = after;
    f.region.terminate(f.body, Terminator::Branch(edge));
    let stats = run(&mut f.region, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!(stats.hoisted, 2); // independent add and one, not the state owner
    assert_eq!(owner(&f.region, f.invariant), f.body);
    assert!(f.region.blocks[f.body.index()]
        .instructions
        .iter()
        .any(|id| matches!(f.region.instructions[id.index()].op, Op::PollBudget)));
}

#[test]
fn memory_helpers_and_cpu_reads_are_never_speculated() {
    let mut f = fixture();
    // Directly exercise the explicit allowlist; "not ordered" alone is unsafe.
    let template = f.region.instructions[0].clone();
    for op in [
        Op::ReadGpr(0),
        Op::ReadXmm(0),
        Op::ReadFlags,
        Op::ReadRawFlags,
        Op::ReadFlagChanges,
        Op::ReadFlagOperand,
        Op::ReadStack32,
        Op::ReadSegment(3),
        Op::GuestLoad { bytes: 4 },
        Op::GuestCheck {
            bytes: 4,
            write: false,
        },
        Op::SseCheck,
        Op::Divide {
            bits: 32,
            signed: false,
        },
        Op::CallHelper(HelperId(0)),
        Op::LinearOffset,
    ] {
        let mut i = template.clone();
        i.op = op;
        assert!(!super::eligible(&i), "{:?}", i.op);
    }
    let before: Vec<_> = f.region.blocks[0].instructions.clone();
    run(&mut f.region, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!(
        &f.region.blocks[0].instructions[..before.len()],
        before.as_slice()
    );
}

#[test]
fn conditional_preheader_external_loop_entry_and_multiple_predecessors_skip() {
    let mut f = fixture();
    let Terminator::Branch(edge) = f.region.blocks[f.entry.index()].terminator.take().unwrap()
    else {
        panic!()
    };
    let condition = f
        .region
        .append(f.entry, Op::Const(1), vec![], &[Type::I1], None)[0];
    // Both edges reach the header, but this is not an unconditional preheader.
    f.region.terminate(
        f.entry,
        Terminator::CondBranch {
            condition,
            taken: edge.clone(),
            not_taken: edge,
        },
    );
    assert_eq!(run(&mut f.region, DEFAULT_WORK_LIMIT).unwrap().hoisted, 0);

    // A graph-only cycle rooted directly at the loop header has no preheader.
    let mut r = Region::default();
    let h = r.block(true);
    r.append(h, Op::Const(7), vec![], &[Type::I32], None);
    r.terminate(
        h,
        Terminator::Branch(Edge {
            target: h,
            args: vec![],
        }),
    );
    assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().loops, 0);

    let mut r = Region::default();
    let a = r.block(true);
    let b = r.block(true);
    let h = r.block(false);
    for p in [a, b] {
        r.terminate(
            p,
            Terminator::Branch(Edge {
                target: h,
                args: vec![],
            }),
        );
    }
    r.append(h, Op::Const(7), vec![], &[Type::I32], None);
    r.terminate(
        h,
        Terminator::Branch(Edge {
            target: h,
            args: vec![],
        }),
    );
    assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().hoisted, 0);
}

#[test]
fn irreducible_cycle_does_not_supply_a_false_loop_header() {
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
    assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().loops, 0);
}

#[test]
fn self_loop_and_multiple_latches_are_unioned() {
    for self_loop in [true, false] {
        let mut r = Region::default();
        let p = r.block(true);
        let h = r.block(false);
        r.terminate(
            p,
            Terminator::Branch(Edge {
                target: h,
                args: vec![],
            }),
        );
        r.append(h, Op::Const(3), vec![], &[Type::I32], None);
        if self_loop {
            r.terminate(
                h,
                Terminator::Branch(Edge {
                    target: h,
                    args: vec![],
                }),
            );
        } else {
            let a = r.block(false);
            let b = r.block(false);
            let condition = r.append(h, Op::Const(1), vec![], &[Type::I1], None)[0];
            r.terminate(
                h,
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
            for latch in [a, b] {
                r.append(latch, Op::Const(4), vec![], &[Type::I32], None);
                r.terminate(
                    latch,
                    Terminator::Branch(Edge {
                        target: h,
                        args: vec![],
                    }),
                );
            }
        }
        let stats = run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
        assert_eq!(stats.loops, 1);
        assert_eq!(stats.hoisted, if self_loop { 1 } else { 4 });
    }
}

#[test]
fn nested_loops_move_inner_dependencies_in_order() {
    let mut r = Region::default();
    let p = r.block(true);
    let outer = r.block(false);
    let ip = r.block(false);
    let inner = r.block(false);
    let latch = r.block(false);
    let condition = r.append(p, Op::Const(1), vec![], &[Type::I1], None)[0];
    for (a, b) in [(p, outer), (outer, ip), (ip, inner), (latch, outer)] {
        r.terminate(
            a,
            Terminator::Branch(Edge {
                target: b,
                args: vec![],
            }),
        );
    }
    let value = r.append(inner, Op::Const(17), vec![], &[Type::I32], None)[0];
    r.terminate(
        inner,
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
    let stats = run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!(stats.loops, 2);
    assert_eq!(stats.hoisted, 2);
    assert_eq!(owner(&r, value), p);
}

#[test]
fn wasm_fixtures_cover_zero_trip_overflow_and_every_budget_boundary() {
    std::fs::create_dir_all("build/ir-licm").unwrap();
    for budget in [1, 2, 3, 4, 5, 6, 9, 16, 64] {
        for optimized in [false, true] {
            let mut f = fixture();
            if optimized {
                assert_eq!(run(&mut f.region, DEFAULT_WORK_LIMIT).unwrap().hoisted, 3);
            }
            let mir = lower(&f.region).unwrap();
            std::fs::write(
                format!("build/ir-licm/loop-{budget}-{optimized}.wasm"),
                emit(&mir, layout(), budget).unwrap().bytes,
            )
            .unwrap();
        }
    }
}

#[test]
fn compile_entry_runs_licm_only_in_optimized_tier_two() {
    let snapshot = ImmutableCodeSnapshot {
        // mov eax, ebx; add eax, esi; add edx, eax; dec ecx; jnz start
        bytes: vec![0x89, 0xD8, 0x01, 0xF0, 0x01, 0xC2, 0x49, 0x75, 0xF7],
        dependencies: vec![CodeDependency {
            page: PhysicalAddress(0x100000),
            version: 0,
        }],
        mappings: vec![CodeMapping {
            linear: LinearAddress(0x100000),
            physical: PhysicalAddress(0x100000),
        }],
    };
    for tier in [Tier::One, Tier::Two] {
        for optimize in [false, true] {
            let request = CompileRequest {
                key: PublicationKey {
                    job: 1,
                    vm_generation: 0,
                    slot: 0,
                    slot_generation: 0,
                },
                pc: GuestEip(0x1000),
                linear: LinearAddress(0x100000),
                default_32: true,
                tier,
            };
            let config = IrConfig {
                optimize,
                passes: PassConfig::default(),
                execution_budget: 64,
                rep_iteration_budget: 8,
                max_code_bytes: 128,
                layout: layout(),
            };
            let artifact = compile_cpu_cfg_region(&request, &snapshot, &config).unwrap();
            if optimize && tier == Tier::Two {
                assert!(artifact.passes.hoisted > 0);
            } else {
                assert_eq!(artifact.passes.hoisted, 0);
                assert_eq!(artifact.passes.licm_work, 0);
            }
        }
    }
    let mut r = fixture().region;
    let before = format!("{r:?}");
    let stats = passes::run_tier2(
        &mut r,
        PassConfig {
            rounds: 0,
            ..PassConfig::default()
        },
    )
    .unwrap();
    assert_eq!(stats.hoisted, 0);
    assert_eq!(format!("{r:?}"), before);
}

#[test]
fn real_guest_loads_guards_and_effect_edges_keep_their_owners() {
    use crate::ir::frontend::region::lift_cpu_cfg;
    // mov eax,[edi]; add ebx,eax; dec ecx; jnz start
    let mut r = lift_cpu_cfg(
        &[0x8B, 0x07, 0x03, 0xD8, 0x49, 0x75, 0xF9],
        GuestEip(0x1000),
        LinearAddress(0x100000),
        true,
        8,
    )
    .unwrap();
    passes::run(&mut r, PassConfig::default()).unwrap();
    let pinned: Vec<_> = r
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|id| r.instructions[id.index()].op.ordered())
        .map(|id| (*id, format!("{:?}", r.instructions[id.index()])))
        .collect();
    assert!(pinned
        .iter()
        .any(|(id, _)| matches!(r.instructions[id.index()].op, Op::GuestLoad { .. })));
    let stats = run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
    assert!(stats.loops > 0);
    for (id, before) in pinned {
        assert_eq!(format!("{:?}", r.instructions[id.index()]), before);
    }
    lower(&r).unwrap();
}

fn vector_fixture() -> Fixture {
    use crate::ir::simd::PackedOp;
    let mut f = fixture();
    let term = f.region.blocks[f.entry.index()].terminator.take().unwrap();
    // Capture architectural XMM input only in the CPU entry prologue.
    let vector = f
        .region
        .append(f.entry, Op::ReadXmm(0), vec![], &[Type::V128], None)[0];
    f.region.terminate(f.entry, term);
    let term = f.region.blocks[f.body.index()].terminator.take().unwrap();
    let previous_len = f.region.blocks[f.body.index()].instructions.len();
    let shuffled = f.region.append(
        f.body,
        Op::VectorShuffle(std::array::from_fn(|i| 15 - i as u8)),
        vec![vector, vector],
        &[Type::V128],
        None,
    )[0];
    let sum = f.region.append(
        f.body,
        Op::VectorBinary(PackedOp::Add8),
        vec![shuffled, vector],
        &[Type::V128],
        None,
    )[0];
    let mask = f.region.append(
        f.body,
        Op::VectorBitmask { bits: 8 },
        vec![sum],
        &[Type::I32],
        None,
    )[0];
    let Definition::Instruction(id, _) = f.region.values[f.variant.index()].definition else {
        panic!()
    };
    f.region.instructions[id.index()].args[1] = mask;
    let list = &mut f.region.blocks[f.body.index()].instructions;
    let new = list.split_off(previous_len);
    let at = list.iter().position(|&i| i == id).unwrap();
    list.splice(at..at, new);
    f.region.terminate(f.body, term);
    f.invariant = mask;
    f
}

#[test]
fn invariant_simd_dataflow_moves_after_its_inputs() {
    std::fs::create_dir_all("build/ir-licm").unwrap();
    for budget in [1, 2, 3, 4, 9, 64] {
        for optimized in [false, true] {
            let mut f = vector_fixture();
            verify(&f.region).unwrap();
            if optimized {
                let stats = run(&mut f.region, DEFAULT_WORK_LIMIT).unwrap();
                assert_eq!(stats.hoisted, 6);
                assert_eq!(owner(&f.region, f.invariant), f.entry);
            }
            let mir = lower(&f.region).unwrap();
            std::fs::write(
                format!("build/ir-licm/vector-{budget}-{optimized}.wasm"),
                emit_cpu(&mir, budget).unwrap().bytes,
            )
            .unwrap();
        }
    }
}
