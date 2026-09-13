use super::{run, LicmConfig};
use crate::ir::{
    backend::wasm::{emit, emit_cpu, StateLayout},
    frontend::{
        decode::{GuestEip, LinearAddress},
        integer::IntegerBuilder,
        region::lift_cpu_cfg,
    },
    hir::*,
    ids::*,
    lowering::lower,
    passes::{self, PassConfig},
    simd::PackedOp,
    state::{FlagState, ResumeKind, StateMap},
    types::Type,
    verify::verify,
};

fn instruction(r: &Region, value: ValueId) -> InstId {
    let Definition::Instruction(id, _) = r.values[value.index()].definition else {
        panic!()
    };
    id
}
fn state(
    r: &mut Region,
    gpr: [ValueId; 8],
    flags: &FlagState,
    pc: u32,
    count: Option<ValueId>,
    xmm: &[ValueId],
) -> StateId {
    r.state(StateMap {
        instruction_pc: GuestEip(pc),
        next_pc: GuestEip(pc + 1),
        next_value: None,
        resume: ResumeKind::BeforeInstruction,
        gpr,
        flags: flags.clone(),
        xmm: xmm.to_vec(),
        x87: vec![],
        committed_instructions: 0,
        count_base: count,
        rep_progress: None,
    })
}
struct Fixture {
    region: Region,
    preheader: BlockId,
    header: BlockId,
    body: BlockId,
    invariant: InstId,
    dependent: InstId,
    variant: InstId,
    poll: InstId,
}
fn fixture(vector: bool) -> Fixture {
    let mut b = IntegerBuilder::new();
    if vector {
        b.xmm = (0..8)
            .map(|r| b.node(Op::ReadXmm(r), vec![], Type::V128))
            .collect();
    }
    let input = b.gpr;
    let preheader = b.block;
    // Allocate the exit before the header/body to avoid relying on arena order.
    let exit = b.region.block(false);
    let header = b.region.block(false);
    let body = b.region.block(false);
    let effect = b.region.param(header, Type::Effect);
    let n = b.region.param(header, Type::I32);
    let sum = b.region.param(header, Type::I32);
    let count = b.region.param(header, Type::I32);
    let body_effect = b.region.param(body, Type::Effect);
    let exit_effect = b.region.param(exit, Type::Effect);
    let zero = b.constant(0, Type::I32);
    b.region.terminate(
        preheader,
        Terminator::Branch(Edge {
            target: header,
            args: vec![b.effect, input[2], input[3], zero],
        }),
    );
    b.gpr[2] = n;
    b.gpr[3] = sum;
    for (block, pc) in [(header, 0x8000), (body, 0x8001), (exit, 0x8002)] {
        let s = state(&mut b.region, b.gpr, &b.flags, pc, Some(count), &b.xmm);
        b.region.blocks[block.index()].entry_state = Some(s);
    }
    b.block = header;
    let done = b.binary(Binary::Eq, n, zero);
    b.region.terminate(
        header,
        Terminator::CondBranch {
            condition: done,
            taken: Edge {
                target: exit,
                args: vec![effect],
            },
            not_taken: Edge {
                target: body,
                args: vec![effect],
            },
        },
    );
    b.block = body;
    let after_poll = b.region.append(
        body,
        Op::PollBudget,
        vec![body_effect],
        &[Type::Effect],
        b.region.blocks[body.index()].entry_state,
    )[0];
    let poll = instruction(&b.region, after_poll);
    let a = if vector {
        let add = b.node(
            Op::VectorBinary(PackedOp::Add8),
            vec![b.xmm[0], b.xmm[1]],
            Type::V128,
        );
        let reversed = b.node(
            Op::VectorShuffle(std::array::from_fn(|i| (15 - i) as u8)),
            vec![add, add],
            Type::V128,
        );
        b.node(Op::VectorBitmask { bits: 8 }, vec![reversed], Type::I32)
    } else {
        b.binary(Binary::Add, input[0], input[1])
    };
    let invariant = instruction(&b.region, a);
    let product = b.binary(Binary::Mul, a, input[4]);
    let dependent = instruction(&b.region, product);
    let next_sum = b.binary(Binary::Add, sum, product);
    let variant = instruction(&b.region, next_sum);
    let one = b.constant(1, Type::I32);
    let next_n = b.binary(Binary::Sub, n, one);
    let next_count = b.binary(Binary::Add, count, one);
    b.region.terminate(
        body,
        Terminator::Branch(Edge {
            target: header,
            args: vec![after_poll, next_n, next_sum, next_count],
        }),
    );
    b.effect = exit_effect;
    let out = state(&mut b.region, b.gpr, &b.flags, 0x8002, Some(count), &b.xmm);
    b.region.states[out.index()].resume = ResumeKind::AfterInstruction;
    b.region.terminate(exit, Terminator::Exit(out));
    verify(&b.region).unwrap();
    Fixture {
        region: b.region,
        preheader,
        header,
        body,
        invariant,
        dependent,
        variant,
        poll,
    }
}

#[test]
fn invariant_chains_move_but_loop_carried_values_and_polls_do_not() {
    for vector in [false, true] {
        let mut f = fixture(vector);
        let states = format!("{:?}", f.region.states);
        let poll_args = f.region.instructions[f.poll.index()].args.clone();
        let stats = run(&mut f.region, LicmConfig::default()).unwrap();
        assert!(stats.hoisted >= 3);
        assert_eq!(stats.natural_loops, 1);
        for id in [f.invariant, f.dependent] {
            assert_eq!(f.region.instructions[id.index()].block, f.preheader);
        }
        assert_eq!(f.region.instructions[f.variant.index()].block, f.body);
        assert_eq!(f.region.instructions[f.poll.index()].block, f.body);
        assert_eq!(f.region.instructions[f.poll.index()].args, poll_args);
        assert_eq!(format!("{:?}", f.region.states), states);
        verify(&f.region).unwrap();
        lower(&f.region).unwrap();
        assert_eq!(
            run(&mut f.region, LicmConfig::default()).unwrap().hoisted,
            0
        );
    }
}

#[test]
fn cpu_reads_and_attached_recovery_are_not_speculated() {
    for (op, ty) in [
        (Op::ReadGpr(0), Type::I32),
        (Op::ReadFlags, Type::I32),
        (Op::ReadRawFlags, Type::I32),
        (Op::ReadFlagChanges, Type::I32),
        (Op::ReadFlagOperand, Type::I32),
        (Op::ReadSegment(3), Type::I16),
        (Op::ReadStack32, Type::I1),
        (Op::ReadXmm(0), Type::V128),
    ] {
        let mut f = fixture(false);
        let term = f.region.blocks[f.preheader.index()]
            .terminator
            .take()
            .unwrap();
        let value = f.region.append(f.preheader, op, vec![], &[ty], None)[0];
        let id = instruction(&f.region, value);
        assert!(!super::speculatable(&f.region.instructions[id.index()]));
        f.region.terminate(f.preheader, term);
        run(&mut f.region, LicmConfig::default()).unwrap();
        assert_eq!(f.region.instructions[id.index()].block, f.preheader);
    }
    // Recovery-attached arithmetic is legal in a body, but is not speculative.
    let mut f = fixture(false);
    let term = f.region.blocks[f.body.index()].terminator.take().unwrap();
    let args = f.region.instructions[f.invariant.index()].args.clone();
    let snapshot = f.region.blocks[f.body.index()].entry_state;
    let value = f.region.append(
        f.body,
        Op::Binary(Binary::Add),
        args,
        &[Type::I32],
        snapshot,
    )[0];
    let id = instruction(&f.region, value);
    f.region.terminate(f.body, term);
    run(&mut f.region, LicmConfig::default()).unwrap();
    assert_eq!(f.region.instructions[id.index()].block, f.body);
    assert_eq!(f.region.instructions[id.index()].state, snapshot);
}

#[test]
fn actual_guest_loads_and_fault_snapshots_retain_their_sites() {
    // mov eax,[esi]; add ecx,ebx; dec edx; jnz start
    let mut r = lift_cpu_cfg(
        &[0x8B, 0x06, 0x03, 0xCB, 0x4A, 0x75, 0xF9],
        GuestEip(0x1000),
        LinearAddress(0x100000),
        true,
        8,
    )
    .unwrap();
    let ordered: Vec<_> = r
        .instructions
        .iter()
        .enumerate()
        .filter(|(_, i)| i.op.ordered())
        .map(|(id, i)| (id, i.block, i.args.clone(), i.state, i.commit))
        .collect();
    assert!(r
        .instructions
        .iter()
        .any(|i| matches!(i.op, Op::GuestLoad { .. })));
    run(&mut r, LicmConfig::default()).unwrap();
    for (id, block, args, state, commit) in ordered {
        let after = &r.instructions[id];
        assert_eq!(
            (after.block, &after.args, after.state, after.commit),
            (block, &args, state, commit)
        );
    }
    lower(&r).unwrap();
}

#[test]
fn effectful_helper_order_and_observation_maps_survive_code_motion() {
    use crate::ir::{
        effects::Effects,
        helper::{ExceptionOwner, HelperAbi, HelperDescriptor},
    };
    let mut f = fixture(false);
    let term = f.region.blocks[f.body.index()].terminator.take().unwrap();
    let Terminator::Branch(mut edge) = term else {
        panic!()
    };
    let before = f.region.blocks[f.body.index()].entry_state;
    let old_effect = edge.args[0];
    f.region.helpers.push(HelperDescriptor {
        name: "licm_observer".into(),
        params: vec![],
        results: vec![],
        effects: Effects::conservative(),
        exception_owner: ExceptionOwner::Caller,
        abi: HelperAbi::Outcome {
            fault_delivery: Some("deliver_fault".into()),
            normal_preserves_state: true,
        },
    });
    let effect = f.region.append(
        f.body,
        Op::CallHelper(HelperId(0)),
        vec![old_effect],
        &[Type::Effect],
        before,
    )[0];
    let helper = instruction(&f.region, effect);
    edge.args[0] = effect;
    f.region.terminate(f.body, Terminator::Branch(edge));
    let states = format!("{:?}", f.region.states);
    assert!(run(&mut f.region, LicmConfig::default()).unwrap().hoisted > 0);
    let call = &f.region.instructions[helper.index()];
    assert_eq!(
        (call.block, call.state, call.args.as_slice()),
        (f.body, before, [old_effect].as_slice())
    );
    let effects: Vec<_> = f.region.blocks[f.body.index()]
        .instructions
        .iter()
        .copied()
        .filter(|id| f.region.instructions[id.index()].op.ordered())
        .collect();
    assert_eq!(effects, [f.poll, helper]);
    assert_eq!(format!("{:?}", f.region.states), states);
    lower(&f.region).unwrap();
}

#[test]
fn conditional_or_ambiguous_preheaders_are_not_rewritten() {
    for extra_predecessor in [false, true] {
        let mut f = fixture(false);
        let Terminator::Branch(edge) = f.region.blocks[f.preheader.index()]
            .terminator
            .take()
            .unwrap()
        else {
            panic!()
        };
        let condition = f
            .region
            .append(f.preheader, Op::Const(1), vec![], &[Type::I1], None)[0];
        let other = if extra_predecessor {
            let bridge = f.region.block(false);
            let effect = f.region.param(bridge, Type::Effect);
            let mut next = edge.clone();
            next.args[0] = effect;
            f.region.terminate(bridge, Terminator::Branch(next));
            Edge {
                target: bridge,
                args: vec![edge.args[0]],
            }
        } else {
            edge.clone()
        };
        f.region.terminate(
            f.preheader,
            Terminator::CondBranch {
                condition,
                taken: edge,
                not_taken: other,
            },
        );
        let before = format!("{:?}", f.region);
        assert_eq!(
            run(&mut f.region, LicmConfig::default()).unwrap().hoisted,
            0
        );
        assert_eq!(format!("{:?}", f.region), before);
    }
}

fn graph(edges: &[&[usize]]) -> Region {
    let mut r = Region::default();
    for index in 0..edges.len() {
        r.block(index == 0);
    }
    for (index, targets) in edges.iter().enumerate() {
        let b = BlockId(index as u32);
        let condition = r.append(b, Op::Const(1), vec![], &[Type::I1], None)[0];
        let edge = |target: usize| Edge {
            target: BlockId(target as u32),
            args: vec![],
        };
        r.terminate(
            b,
            if targets.len() == 1 {
                Terminator::Branch(edge(targets[0]))
            } else {
                Terminator::CondBranch {
                    condition,
                    taken: edge(targets[0]),
                    not_taken: edge(targets[1]),
                }
            },
        );
    }
    r
}

#[test]
fn irreducible_and_external_entry_loops_have_no_unsafe_preheader() {
    for mut r in [graph(&[&[1, 2], &[2], &[1]]), graph(&[&[1], &[0]])] {
        let before = format!("{r:?}");
        assert_eq!(run(&mut r, LicmConfig::default()).unwrap().hoisted, 0);
        assert_eq!(format!("{r:?}"), before);
    }
    let mut r = graph(&[&[1, 2], &[2], &[1]]);
    r.entries.push(BlockId(1));
    assert_eq!(run(&mut r, LicmConfig::default()).unwrap().hoisted, 0);
}

#[test]
fn nested_loops_are_processed_inner_first_and_multiple_latches_are_unioned() {
    let mut nested = graph(&[&[1], &[2], &[3], &[4, 5], &[3], &[1]]);
    let stats = run(&mut nested, LicmConfig::default()).unwrap();
    assert_eq!(stats.natural_loops, 2);
    assert!(stats.hoisted > 0);
    // All total constants from the inner loop eventually reach the outer preheader.
    for &id in &nested.blocks[0].instructions {
        assert_eq!(nested.instructions[id.index()].block, BlockId(0));
    }
    assert!(nested.blocks[4].instructions.is_empty());
    let mut multiple = graph(&[&[1], &[2, 3], &[1], &[1]]);
    let stats = run(&mut multiple, LicmConfig::default()).unwrap();
    assert_eq!(stats.natural_loops, 1);
    assert_eq!(stats.hoisted, 3);
    verify(&multiple).unwrap();
}

#[test]
fn budget_and_invalid_input_fail_without_mutating_any_arena() {
    let original = fixture(false).region;
    let work = run(&mut original.clone(), LicmConfig::default())
        .unwrap()
        .work;
    for max_work in [0, 1, work - 1] {
        let mut r = original.clone();
        let before = format!("{r:?}");
        assert!(run(&mut r, LicmConfig { max_work })
            .unwrap_err()
            .contains("budget"));
        assert_eq!(format!("{r:?}"), before);
    }
    let mut invalid = original.clone();
    let duplicated = invalid.blocks[0].instructions[0];
    invalid.blocks[0].instructions.push(duplicated);
    let before = format!("{invalid:?}");
    assert!(run(&mut invalid, LicmConfig::default()).is_err());
    assert_eq!(format!("{invalid:?}"), before);
}

#[test]
fn pipeline_gate_and_emitted_scalar_vector_budget_fixtures() {
    let f = fixture(false);
    let mut disabled = f.region.clone();
    let stats = passes::run(
        &mut disabled,
        PassConfig {
            licm: false,
            ..PassConfig::default()
        },
    )
    .unwrap();
    assert_eq!(stats.hoisted, 0);
    let stats = passes::run(&mut f.region.clone(), PassConfig::default()).unwrap();
    assert!(stats.hoisted > 0);
    let before = format!("{:?}", f.region);
    let mut zero_rounds = f.region;
    assert_eq!(
        passes::run(
            &mut zero_rounds,
            PassConfig {
                rounds: 0,
                ..PassConfig::default()
            }
        )
        .unwrap()
        .hoisted,
        0
    );
    assert_eq!(format!("{zero_rounds:?}"), before);
    std::fs::create_dir_all("build/ir-licm").unwrap();
    for vector in [false, true] {
        for opt in 0..3 {
            let mut r = fixture(vector).region;
            if opt == 1 {
                run(&mut r, LicmConfig::default()).unwrap();
            }
            if opt == 2 {
                passes::run(&mut r, PassConfig::default()).unwrap();
            }
            let mir = lower(&r).unwrap();
            for budget in [1, 2, 3, 4, 5, 7, 16, 33, 257] {
                let artifact = if vector {
                    emit_cpu(&mir, budget)
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
                }
                .unwrap();
                std::fs::write(
                    format!("build/ir-licm/{vector}-{budget}-{opt}.wasm"),
                    artifact.bytes,
                )
                .unwrap();
            }
        }
    }
}
