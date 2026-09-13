use crate::ir::{
    analysis::loops::LoopAnalysis,
    backend::wasm::{emit, StateLayout},
    frontend::{decode::GuestEip, integer::IntegerBuilder},
    hir::*,
    ids::*,
    lowering::lower,
    passes::{self, licm, PassConfig},
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
fn owner(r: &Region, v: ValueId) -> BlockId {
    match r.values[v.index()].definition {
        Definition::Instruction(id, _) => r.instructions[id.index()].block,
        Definition::Parameter(block, _) => block,
    }
}
struct Fixture {
    region: Region,
    entry: BlockId,
    header: BlockId,
    body: BlockId,
    invariant: ValueId,
    dependent: ValueId,
    variant: ValueId,
}
fn counted_loop() -> Fixture {
    let mut b = IntegerBuilder::new();
    let input = b.gpr;
    let flags = b.flags.clone();
    let entry = b.block;
    // Intentionally not dominance order: body depends on a later-allocated header.
    let body = b.region.block(false);
    let header = b.region.block(false);
    let exit = b.region.block(false);
    let effect = b.region.param(header, Type::Effect);
    let count = b.region.param(header, Type::I32);
    let acc = b.region.param(header, Type::I32);
    let body_effect = b.region.param(body, Type::Effect);
    b.region.param(exit, Type::Effect);
    let zero = b.constant(0, Type::I32);
    let one = b.constant(1, Type::I32);
    b.region.terminate(
        entry,
        Terminator::Branch(Edge {
            target: header,
            args: vec![b.effect, input[2], input[0]],
        }),
    );
    b.block = header;
    b.gpr[0] = acc;
    b.gpr[2] = count;
    let state = snapshot(&mut b.region, b.gpr, &flags, 0x2000);
    b.region.blocks[header.index()].entry_state = Some(state);
    let invariant = b.binary(Binary::Add, input[1], input[3]);
    let a = b.node(Op::Extend { signed: false }, vec![input[1]], Type::I64);
    let c = b.node(Op::Extend { signed: false }, vec![input[3]], Type::I64);
    let wide = b.binary(Binary::Add, a, c);
    let shift = b.node(Op::Const(32), vec![], Type::I64);
    let hi = b.binary(Binary::Shr, wide, shift);
    let high = b.node(Op::Truncate, vec![hi], Type::I32);
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
    // A recovery-only use must remain available at the body entry after motion.
    let mut recovery = b.gpr;
    recovery[7] = invariant;
    let state = snapshot(&mut b.region, recovery, &flags, 0x2001);
    b.region.blocks[body.index()].entry_state = Some(state);
    let dependent = b.binary(Binary::Mul, invariant, input[6]);
    let variant = b.binary(Binary::Add, acc, dependent);
    let next_count = b.binary(Binary::Sub, count, one);
    b.region.terminate(
        body,
        Terminator::Branch(Edge {
            target: header,
            args: vec![body_effect, next_count, variant],
        }),
    );
    b.gpr[0] = acc;
    b.gpr[2] = count;
    b.gpr[3] = high;
    b.gpr[7] = invariant;
    let state = snapshot(&mut b.region, b.gpr, &flags, 0x3000);
    b.region.blocks[exit.index()].entry_state = Some(state);
    b.region.terminate(exit, Terminator::Exit(state));
    verify(&b.region).unwrap();
    Fixture {
        region: b.region,
        entry,
        header,
        body,
        invariant,
        dependent,
        variant,
    }
}
#[test]
fn licm_hoists_dependency_chains_and_preserves_budget_recovery() {
    let mut f = counted_loop();
    let before = format!("{:?}", f.region.states);
    let analysis = LoopAnalysis::compute(&f.region, licm::DEFAULT_WORK_BUDGET).unwrap();
    assert_eq!(analysis.loops.len(), 1);
    assert_eq!(analysis.loops[0].preheader, Some(f.entry));
    let stats = licm::run(&mut f.region, licm::DEFAULT_WORK_BUDGET).unwrap();
    assert_eq!(stats.loops, 1);
    assert_eq!(stats.hoisted, 8);
    assert_eq!(owner(&f.region, f.invariant), f.entry);
    assert_eq!(owner(&f.region, f.dependent), f.entry);
    assert_eq!(owner(&f.region, f.variant), f.body);
    assert_eq!(format!("{:?}", f.region.states), before);
    verify(&f.region).unwrap();
    lower(&f.region).unwrap();
    assert_eq!(
        licm::run(&mut f.region, licm::DEFAULT_WORK_BUDGET)
            .unwrap()
            .hoisted,
        0
    );
    std::fs::create_dir_all("build/ir-licm").unwrap();
    let layout = StateLayout {
        gpr: 0,
        flags: 32,
        eip: 36,
        committed: 40,
        flag_operand: 44,
    };
    for budget in [1, 2, 3, 4, 5, 8, 16, 31, 100] {
        for opt in [false, true] {
            let mut f = counted_loop();
            if opt {
                licm::run(&mut f.region, licm::DEFAULT_WORK_BUDGET).unwrap();
            }
            std::fs::write(
                format!("build/ir-licm/loop-{budget}-{opt}.wasm"),
                emit(&lower(&f.region).unwrap(), layout, budget)
                    .unwrap()
                    .bytes,
            )
            .unwrap();
        }
    }
}
#[test]
fn licm_budget_failure_is_atomic_even_after_planned_moves() {
    let f = counted_loop();
    let before = format!("{:?}", f.region);
    let work = licm::run(&mut f.region.clone(), licm::DEFAULT_WORK_BUDGET)
        .unwrap()
        .work;
    let analysis_work = LoopAnalysis::compute(&f.region, licm::DEFAULT_WORK_BUDGET)
        .unwrap()
        .work;
    for budget in [0, 1, analysis_work + 1, work - 1] {
        let mut candidate = f.region.clone();
        assert!(licm::run(&mut candidate, budget)
            .unwrap_err()
            .contains("budget"));
        assert_eq!(format!("{:?}", candidate), before);
    }
    let mut candidate = f.region.clone();
    assert!(licm::run(&mut candidate, work).is_ok());
}
#[test]
fn licm_has_an_independent_disable_switch() {
    let f = counted_loop();
    let config = PassConfig {
        prune: false,
        merge: false,
        phis: false,
        fold: false,
        gvn: false,
        licm: false,
        dce: false,
        rounds: 1,
    };
    let mut r = f.region.clone();
    assert_eq!(passes::run(&mut r, config).unwrap().hoisted, 0);
    assert_eq!(format!("{:?}", r), format!("{:?}", f.region));
    let stats = passes::run(
        &mut r,
        PassConfig {
            licm: true,
            ..config
        },
    )
    .unwrap();
    assert_eq!(stats.hoisted, 8);
}
#[test]
fn licm_does_not_move_reads_memory_helpers_or_observation_points() {
    let mut f = counted_loop();
    let r = &mut f.region;
    let mut term = r.blocks[f.header.index()].terminator.take().unwrap();
    let state = r.blocks[f.header.index()].entry_state;
    let start = r.instructions.len();
    // Even a constant is pinned when it owns a state observation.
    r.append(f.header, Op::Const(7), vec![], &[Type::I32], state);
    let mut effect = r.blocks[f.header.index()].params[0];
    effect = r.append(
        f.header,
        Op::PollBudget,
        vec![effect],
        &[Type::Effect],
        state,
    )[0];
    let offset = r.states[state.unwrap().index()].gpr[6];
    let address = r.append(
        f.header,
        Op::SegmentAddress { segment: 3 },
        vec![offset, effect],
        &[Type::LinearAddress, Type::Effect],
        state,
    );
    let load = r.append(
        f.header,
        Op::GuestLoad { bytes: 4 },
        address,
        &[Type::I32, Type::Effect],
        state,
    );
    effect = load[1];
    r.helpers
        .push(crate::ir::helper::HelperDescriptor::conservative(
            "licm_observer".into(),
            vec![],
            vec![],
        ));
    effect = r.append(
        f.header,
        Op::CallHelper(HelperId(0)),
        vec![effect],
        &[Type::Effect],
        state,
    )[0];
    for edge in term.edges_mut() {
        edge.args[0] = effect;
    }
    r.terminate(f.header, term);
    verify(r).unwrap();
    let before: Vec<_> = r.instructions[start..]
        .iter()
        .map(|i| format!("{:?}", i))
        .collect();
    licm::run(r, licm::DEFAULT_WORK_BUDGET).unwrap();
    assert_eq!(
        r.instructions[start..]
            .iter()
            .map(|i| format!("{:?}", i))
            .collect::<Vec<_>>(),
        before
    );
    for id in &r.blocks[f.header.index()].instructions {
        assert_eq!(r.instructions[id.index()].block, f.header);
    }
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
fn conditional(r: &mut Region, from: BlockId, condition: ValueId, a: BlockId, b: BlockId) {
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
fn licm_merges_multiple_latches_and_handles_nested_loops() {
    let mut r = Region::default();
    let entry = r.block(true);
    let header = r.block(false);
    let a = r.block(false);
    let b = r.block(false);
    let condition = r.append(entry, Op::Const(1), vec![], &[Type::I1], None)[0];
    branch(&mut r, entry, header);
    conditional(&mut r, header, condition, a, b);
    let x = r.append(a, Op::Const(17), vec![], &[Type::I32], None)[0];
    let y = r.append(b, Op::Const(23), vec![], &[Type::I32], None)[0];
    branch(&mut r, a, header);
    branch(&mut r, b, header);
    let analysis = LoopAnalysis::compute(&r, licm::DEFAULT_WORK_BUDGET).unwrap();
    assert_eq!(analysis.loops.len(), 1);
    assert_eq!(analysis.loops[0].members.iter().filter(|&&x| x).count(), 3);
    assert_eq!(
        licm::run(&mut r, licm::DEFAULT_WORK_BUDGET)
            .unwrap()
            .hoisted,
        2
    );
    assert_eq!(owner(&r, x), entry);
    assert_eq!(owner(&r, y), entry);

    let mut r = Region::default();
    let entry = r.block(true);
    let outer = r.block(false);
    let pre = r.block(false);
    let inner = r.block(false);
    let latch = r.block(false);
    let condition = r.append(entry, Op::Const(1), vec![], &[Type::I1], None)[0];
    branch(&mut r, entry, outer);
    branch(&mut r, outer, pre);
    branch(&mut r, pre, inner);
    let x = r.append(inner, Op::Const(91), vec![], &[Type::I32], None)[0];
    conditional(&mut r, inner, condition, inner, latch);
    branch(&mut r, latch, outer);
    let stats = licm::run(&mut r, licm::DEFAULT_WORK_BUDGET).unwrap();
    assert_eq!(stats.loops, 2);
    assert_eq!(stats.hoisted, 2);
    assert_eq!(owner(&r, x), entry);
}
#[test]
fn licm_rejects_external_entries_irreducible_cycles_and_ambiguous_preheaders() {
    for mode in 0..3 {
        let mut r = Region::default();
        let entry = r.block(true);
        let a = r.block(mode == 0);
        let b = r.block(false);
        let condition = r.append(entry, Op::Const(1), vec![], &[Type::I1], None)[0];
        let x = r.append(a, Op::Const(17), vec![], &[Type::I32], None)[0];
        let y = r.append(b, Op::Const(23), vec![], &[Type::I32], None)[0];
        if mode == 0 {
            branch(&mut r, entry, a);
        } else if mode == 1 {
            conditional(&mut r, entry, condition, a, b);
        } else {
            conditional(&mut r, entry, condition, a, a);
        }
        branch(&mut r, a, b);
        branch(&mut r, b, a);
        verify(&r).unwrap();
        let before = format!("{:?}", r);
        assert_eq!(
            licm::run(&mut r, licm::DEFAULT_WORK_BUDGET)
                .unwrap()
                .hoisted,
            0
        );
        assert_eq!(owner(&r, x), a);
        assert_eq!(owner(&r, y), b);
        assert_eq!(format!("{:?}", r), before);
    }
}
#[test]
fn licm_vector_values_need_dominating_immutable_inputs() {
    let mut f = counted_loop();
    let entry_term = f.region.blocks[f.entry.index()].terminator.take().unwrap();
    let vector = f
        .region
        .append(f.entry, Op::ReadXmm(0), vec![], &[Type::V128], None)[0];
    f.region.terminate(f.entry, entry_term);
    let header_term = f.region.blocks[f.header.index()].terminator.take().unwrap();
    let shuffle = f.region.append(
        f.header,
        Op::VectorShuffle([15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0]),
        vec![vector, vector],
        &[Type::V128],
        None,
    )[0];
    let mask = f.region.append(
        f.header,
        Op::VectorBitmask { bits: 8 },
        vec![shuffle],
        &[Type::I32],
        None,
    )[0];
    let count = f.region.blocks[f.header.index()].params[1];
    let variant = f.region.append(
        f.header,
        Op::VectorReplace { bits: 32, lane: 0 },
        vec![vector, count],
        &[Type::V128],
        None,
    )[0];
    let pinned = f.region.append(
        f.header,
        Op::VectorBitmask { bits: 8 },
        vec![variant],
        &[Type::I32],
        None,
    )[0];
    f.region.terminate(f.header, header_term);
    verify(&f.region).unwrap();
    licm::run(&mut f.region, licm::DEFAULT_WORK_BUDGET).unwrap();
    assert_eq!(owner(&f.region, shuffle), f.entry);
    assert_eq!(owner(&f.region, mask), f.entry);
    assert_eq!(owner(&f.region, vector), f.entry);
    assert_eq!(owner(&f.region, variant), f.header);
    assert_eq!(owner(&f.region, pinned), f.header);
}

#[test]
fn licm_rejects_misplaced_cpu_initialization_without_repairing_it() {
    for (op, ty) in [
        (Op::ReadGpr(0), Type::I32),
        (Op::ReadFlags, Type::I32),
        (Op::ReadRawFlags, Type::I32),
        (Op::ReadFlagChanges, Type::I32),
        (Op::ReadFlagOperand, Type::I32),
        (Op::ReadXmm(0), Type::V128),
        (Op::ReadSegment(3), Type::I16),
        (Op::ReadStack32, Type::I1),
    ] {
        let mut f = counted_loop();
        let term = f.region.blocks[f.header.index()].terminator.take().unwrap();
        f.region.append(f.header, op, vec![], &[ty], None);
        f.region.terminate(f.header, term);
        let before = format!("{:?}", f.region);
        assert!(verify(&f.region).is_err());
        assert!(licm::run(&mut f.region, licm::DEFAULT_WORK_BUDGET).is_err());
        assert_eq!(format!("{:?}", f.region), before);
    }
}
