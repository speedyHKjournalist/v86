use super::{run, Limits};
use crate::ir::{
    backend::wasm::{emit, StateLayout},
    frontend::{decode::GuestEip, integer::IntegerBuilder},
    hir::*,
    ids::*,
    lowering::lower,
    passes::{self, PassConfig},
    state::{FlagState, ResumeKind, StateMap},
    types::Type,
    verify::verify,
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

// Intentionally non-topological arena: entry, exit, body, header. Zero-trip
// execution and every dispatcher-budget exit have independent StateMaps.
fn counted_loop() -> (Region, BlockId, BlockId, BlockId, ValueId) {
    let mut b = IntegerBuilder::new();
    let entry = b.block;
    let input = b.gpr;
    let flags = b.flags.clone();
    let exit = b.region.block(false);
    let body = b.region.block(false);
    let header = b.region.block(false);
    let effect = b.region.param(header, Type::Effect);
    let count = b.region.param(header, Type::I32);
    let sum = b.region.param(header, Type::I32);
    let be = b.region.param(body, Type::Effect);
    b.region.param(exit, Type::Effect);
    b.region.terminate(
        entry,
        Terminator::Branch(Edge {
            target: header,
            args: vec![b.effect, input[1], input[0]],
        }),
    );
    let mut state = input;
    state[0] = sum;
    state[1] = count;
    for block in [header, body, exit] {
        let s = snapshot(&mut b.region, state, &flags, 0x1000 + block.0);
        b.region.blocks[block.index()].entry_state = Some(s);
    }
    b.block = header;
    let zero = b.constant(0, Type::I32);
    let condition = b.binary(Binary::Eq, count, zero);
    b.region.terminate(
        header,
        Terminator::CondBranch {
            condition,
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
    b.effect = be;
    // A dependent invariant chain, including mixed machine widths and wrap.
    let a = b.node(Op::Extend { signed: false }, vec![input[2]], Type::I64);
    let c = b.node(Op::Extend { signed: false }, vec![input[3]], Type::I64);
    let wide = b.binary(Binary::Mul, a, c);
    let invariant = b.node(Op::Truncate, vec![wide], Type::I32);
    let next_sum = b.binary(Binary::Add, sum, invariant);
    let one = b.constant(1, Type::I32);
    let next_count = b.binary(Binary::Sub, count, one);
    b.region.terminate(
        body,
        Terminator::Branch(Edge {
            target: header,
            args: vec![be, next_count, next_sum],
        }),
    );
    let end = snapshot(&mut b.region, state, &flags, 0x2000);
    b.region.terminate(exit, Terminator::Exit(end));
    (b.region, entry, header, body, invariant)
}
fn owner(r: &Region, value: ValueId) -> BlockId {
    match r.values[value.index()].definition {
        Definition::Instruction(id, _) => r.instructions[id.index()].block,
        Definition::Parameter(block, _) => block,
    }
}
fn placement(r: &Region) -> String {
    format!("{r:?}")
}

#[test]
fn invariant_chain_moves_without_changing_recovery_or_cfg() {
    let (mut r, entry, header, body, value) = counted_loop();
    verify(&r).unwrap();
    let states = format!("{:?}", r.states);
    let terms = format!(
        "{:?}",
        r.blocks
            .iter()
            .map(|b| (&b.terminator, &b.params, &b.entry_state))
            .collect::<Vec<_>>()
    );
    let stats = run(&mut r, Limits::default()).unwrap();
    assert_eq!(stats.loops, 1);
    assert_eq!(stats.hoisted, 6);
    assert_eq!(owner(&r, value), entry);
    assert_eq!(states, format!("{:?}", r.states));
    assert_eq!(
        terms,
        format!(
            "{:?}",
            r.blocks
                .iter()
                .map(|b| (&b.terminator, &b.params, &b.entry_state))
                .collect::<Vec<_>>()
        )
    );
    assert_eq!(r.blocks[header.index()].instructions.len(), 1);
    assert_eq!(r.blocks[body.index()].instructions.len(), 2);
    verify(&r).unwrap();
    lower(&r).unwrap();
    assert_eq!(run(&mut r, Limits::default()).unwrap().hoisted, 0);
}

#[test]
fn budgets_fail_atomically_and_hoist_cap_is_safe() {
    let (original, _, _, _, _) = counted_loop();
    let expected = placement(&original);
    let mut full = original.clone();
    let stats = run(&mut full, Limits::default()).unwrap();
    assert!(stats.work > 1);
    for work in [0, 1, stats.work / 2, stats.work - 1] {
        let mut r = original.clone();
        let error = run(
            &mut r,
            Limits {
                work,
                ..Limits::default()
            },
        )
        .unwrap_err();
        assert!(error.contains("budget"));
        assert_eq!(placement(&r), expected, "failed planning must be atomic");
    }
    for cap in 0..=stats.hoisted {
        let mut r = original.clone();
        let stats = run(
            &mut r,
            Limits {
                hoisted: cap,
                ..Limits::default()
            },
        )
        .unwrap();
        assert_eq!(stats.hoisted, cap);
        verify(&r).unwrap();
        lower(&r).unwrap();
    }
    let mut oversized = original.clone();
    while oversized.blocks.len() <= 64 {
        oversized.block(false);
    }
    let before = placement(&oversized);
    assert!(run(&mut oversized, Limits::default())
        .unwrap_err()
        .contains("region budget"));
    assert_eq!(placement(&oversized), before);
    let mut invalid = original;
    invalid.blocks[0].terminator = None;
    let before = placement(&invalid);
    assert!(run(&mut invalid, Limits::default()).is_err());
    assert_eq!(placement(&invalid), before);
}

#[test]
fn loop_variant_operands_and_cpu_observations_stay_put() {
    let (mut r, entry, _, body, _) = counted_loop();
    let original_reads: Vec<_> = r.blocks[entry.index()]
        .instructions
        .iter()
        .copied()
        .filter(|id| {
            matches!(
                r.instructions[id.index()].op,
                Op::ReadGpr(_)
                    | Op::ReadFlags
                    | Op::ReadRawFlags
                    | Op::ReadFlagChanges
                    | Op::ReadFlagOperand
            )
        })
        .collect();
    run(&mut r, Limits::default()).unwrap();
    for id in original_reads {
        assert_eq!(r.instructions[id.index()].block, entry);
        assert!(!super::movable(&r.instructions[id.index()]));
    }
    // These observations are only legal at entries in current HIR. Never relax
    // that verifier rule merely to construct a loop test with mutable reads.
    for op in [
        Op::ReadGpr(0),
        Op::ReadFlags,
        Op::ReadRawFlags,
        Op::ReadFlagChanges,
        Op::ReadFlagOperand,
        Op::ReadStack32,
        Op::ReadSegment(3),
        Op::ReadXmm(0),
    ] {
        let inst = Instruction {
            block: entry,
            op,
            args: vec![],
            results: vec![ValueId(0)],
            state: None,
            commit: None,
            trap_after_fault: false,
            unmasked_word_store: false,
        };
        assert!(!super::movable(&inst));
    }
    let before = r.blocks[body.index()].instructions.clone();
    assert_eq!(
        before.len(),
        2,
        "loop-carried ADD and SUB remain in the body"
    );
    assert_eq!(run(&mut r, Limits::default()).unwrap().hoisted, 0);
    assert_eq!(r.blocks[body.index()].instructions, before);
}

#[test]
fn ordered_operations_are_not_speculatable() {
    // Op::ordered is not the motion criterion: exercise the actual allowlist
    // even for operators whose full CPU contracts require richer test fixtures.
    for op in [
        Op::GuestLoad { bytes: 4 },
        Op::GuestStore { bytes: 4 },
        Op::GuestCheck {
            bytes: 4,
            write: false,
        },
        Op::SseCheck,
        Op::Divide {
            bits: 32,
            signed: false,
        },
        Op::PollBudget,
        Op::CallHelper(HelperId(0)),
        Op::SegmentAddress { segment: 3 },
        Op::RmwLoad {
            bytes: 4,
            order: RmwOrder::Locked,
        },
        Op::RmwStore {
            bytes: 4,
            order: RmwOrder::Locked,
        },
    ] {
        let inst = Instruction {
            block: BlockId(0),
            op,
            args: vec![],
            results: vec![ValueId(0)],
            state: None,
            commit: None,
            trap_after_fault: false,
            unmasked_word_store: false,
        };
        assert!(!super::movable(&inst));
    }
    let mut inst = Instruction {
        block: BlockId(0),
        op: Op::Const(1),
        args: vec![],
        results: vec![ValueId(0)],
        state: Some(StateId(0)),
        commit: None,
        trap_after_fault: false,
        unmasked_word_store: false,
    };
    assert!(!super::movable(&inst));
    inst.state = None;
    inst.commit = Some(StateId(0));
    assert!(!super::movable(&inst));
}

#[test]
fn conditional_preheader_and_external_roots_are_rejected() {
    let (mut r, entry, _, _, _) = counted_loop();
    let Terminator::Branch(edge) = r.blocks[entry.index()].terminator.take().unwrap() else {
        panic!()
    };
    let condition = r.append(entry, Op::Const(1), vec![], &[Type::I1], None)[0];
    // Even two parallel edges to the same header are not an unconditional
    // preheader. CFG simplification, when enabled, owns that separate rewrite.
    r.terminate(
        entry,
        Terminator::CondBranch {
            condition,
            taken: edge.clone(),
            not_taken: edge,
        },
    );
    let before = placement(&r);
    assert_eq!(run(&mut r, Limits::default()).unwrap().hoisted, 0);
    assert_eq!(placement(&r), before);
    // A separate, valid entry-header loop uses only its own parameters.
    let mut r = Region::default();
    let h = r.block(true);
    let x = r.param(h, Type::I32);
    r.append(h, Op::Const(7), vec![], &[Type::I32], None);
    r.terminate(
        h,
        Terminator::Branch(Edge {
            target: h,
            args: vec![x],
        }),
    );
    let before = placement(&r);
    assert_eq!(run(&mut r, Limits::default()).unwrap().loops, 0);
    assert_eq!(placement(&r), before);
}

#[test]
fn full_pipeline_emits_budgeted_loop_oracles() {
    std::fs::create_dir_all("build/ir-licm").unwrap();
    let layout = StateLayout {
        gpr: 0,
        flags: 32,
        eip: 36,
        committed: 40,
        flag_operand: 44,
    };
    for budget in [1, 2, 3, 4, 5, 7, 11, 32, 100] {
        for mode in 0..3 {
            let (mut r, _, _, _, _) = counted_loop();
            if mode == 1 {
                assert_eq!(run(&mut r, Limits::default()).unwrap().hoisted, 6);
            } else if mode == 2 {
                let stats = passes::run(&mut r, PassConfig::default()).unwrap();
                assert!(stats.hoisted > 0);
            }
            let bytes = emit(&lower(&r).unwrap(), layout, budget).unwrap().bytes;
            std::fs::write(format!("build/ir-licm/loop-{budget}-{mode}.wasm"), bytes).unwrap();
        }
    }
    let (mut disabled, _, _, _, _) = counted_loop();
    assert_eq!(
        passes::run(
            &mut disabled,
            PassConfig {
                licm: false,
                ..PassConfig::default()
            }
        )
        .unwrap()
        .hoisted,
        0
    );
}

fn branch(r: &mut Region, from: BlockId, to: BlockId) {
    r.terminate(
        from,
        Terminator::Branch(Edge {
            target: to,
            args: vec![],
        }),
    );
}
fn choose(r: &mut Region, from: BlockId, condition: ValueId, a: BlockId, b: BlockId) {
    r.terminate(
        from,
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
}
#[test]
fn self_loop_and_multiple_latches_use_one_natural_loop() {
    for multiple in [false, true] {
        let mut r = Region::default();
        let pre = r.block(true);
        let h = r.block(false);
        let condition = r.param(pre, Type::I1);
        branch(&mut r, pre, h);
        let value = r.append(h, Op::Const(7), vec![], &[Type::I32], None)[0];
        if multiple {
            let a = r.block(false);
            let b = r.block(false);
            choose(&mut r, h, condition, a, b);
            for latch in [a, b] {
                r.append(
                    latch,
                    Op::Binary(Binary::Mul),
                    vec![value, value],
                    &[Type::I32],
                    None,
                );
                branch(&mut r, latch, h);
            }
        } else {
            branch(&mut r, h, h);
        }
        let stats = run(&mut r, Limits::default()).unwrap();
        assert_eq!(stats.loops, 1);
        assert_eq!(stats.hoisted, if multiple { 3 } else { 1 });
        assert_eq!(owner(&r, value), pre);
        verify(&r).unwrap();
    }
}
#[test]
fn irreducible_cycles_and_multiple_outside_predecessors_are_unchanged() {
    for irreducible in [false, true] {
        let mut r = Region::default();
        let entry = r.block(true);
        let a = r.block(false);
        let b = r.block(false);
        let condition = r.param(entry, Type::I1);
        choose(&mut r, entry, condition, a, b);
        if irreducible {
            r.append(a, Op::Const(1), vec![], &[Type::I32], None);
            r.append(b, Op::Const(2), vec![], &[Type::I32], None);
            branch(&mut r, a, b);
            branch(&mut r, b, a);
        } else {
            let h = r.block(false);
            branch(&mut r, a, h);
            branch(&mut r, b, h);
            r.append(h, Op::Const(7), vec![], &[Type::I32], None);
            branch(&mut r, h, h);
        }
        let before = placement(&r);
        assert_eq!(run(&mut r, Limits::default()).unwrap().hoisted, 0);
        assert_eq!(placement(&r), before);
    }
}
#[test]
fn nested_loop_motion_uses_updated_owners_and_dominance() {
    let mut r = Region::default();
    let entry = r.block(true);
    let outer = r.block(false);
    let pre = r.block(false);
    let inner = r.block(false);
    let latch = r.block(false);
    let input = r.param(entry, Type::I32);
    let condition = r.param(entry, Type::I1);
    let phi = r.param(outer, Type::I32);
    r.terminate(
        entry,
        Terminator::Branch(Edge {
            target: outer,
            args: vec![input],
        }),
    );
    branch(&mut r, outer, pre);
    branch(&mut r, pre, inner);
    let c = r.append(inner, Op::Const(7), vec![], &[Type::I32], None)[0];
    let full = r.append(
        inner,
        Op::Binary(Binary::Mul),
        vec![c, input],
        &[Type::I32],
        None,
    )[0];
    let partial = r.append(
        inner,
        Op::Binary(Binary::Add),
        vec![full, phi],
        &[Type::I32],
        None,
    )[0];
    choose(&mut r, inner, condition, inner, latch);
    r.terminate(
        latch,
        Terminator::Branch(Edge {
            target: outer,
            args: vec![phi],
        }),
    );
    let stats = run(&mut r, Limits::default()).unwrap();
    assert_eq!(stats.loops, 2);
    assert_eq!(
        stats.hoisted, 5,
        "two full invariants move twice; partial invariant once"
    );
    assert_eq!(owner(&r, c), entry);
    assert_eq!(owner(&r, full), entry);
    assert_eq!(owner(&r, partial), pre);
    verify(&r).unwrap();
}
#[test]
fn budget_polls_and_state_bearing_nodes_keep_their_positions() {
    let (mut r, _, header, body, _) = counted_loop();
    let term = r.blocks[body.index()].terminator.take().unwrap();
    let effect = r.blocks[body.index()].params[0];
    let state = r.blocks[body.index()].entry_state;
    let observed = r.append(body, Op::Const(23), vec![], &[Type::I32], state)[0];
    let poll = r.append(body, Op::PollBudget, vec![effect], &[Type::Effect], state)[0];
    let Terminator::Branch(mut edge) = term else {
        panic!()
    };
    edge.args[0] = poll;
    r.terminate(body, Terminator::Branch(edge));
    run(&mut r, Limits::default()).unwrap();
    assert_eq!(owner(&r, observed), body);
    assert_eq!(owner(&r, poll), body);
    let hstate = r.blocks[header.index()].entry_state;
    assert!(hstate.is_some());
    verify(&r).unwrap();
    lower(&r).unwrap();
}
