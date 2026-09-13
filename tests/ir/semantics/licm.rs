use super::{run, Config};
use crate::ir::{
    analysis::loops::{LoopAnalysis, WorkBudget},
    backend::wasm::{emit, StateLayout},
    frontend::{decode::GuestEip, integer::IntegerBuilder},
    hir::*,
    ids::*,
    lowering::lower,
    passes::{run as pipeline, PassConfig},
    state::{ResumeKind, StateMap},
    types::Type,
    verify::verify,
};

struct Fixture {
    region: Region,
    preheader: BlockId,
    header: BlockId,
    body: BlockId,
    invariant: Vec<InstId>,
    variant: Vec<InstId>,
    poll: InstId,
}
fn instruction(r: &Region, value: ValueId) -> InstId {
    match r.values[value.index()].definition {
        Definition::Instruction(id, _) => id,
        _ => panic!("expected an instruction result"),
    }
}
fn state(b: &mut IntegerBuilder, pc: u32, count: ValueId) -> StateId {
    b.region.state(StateMap {
        instruction_pc: GuestEip(pc),
        next_pc: GuestEip(pc),
        next_value: None,
        resume: ResumeKind::BeforeInstruction,
        gpr: b.gpr,
        flags: b.flags.clone(),
        xmm: vec![],
        x87: vec![],
        committed_instructions: 0,
        count_base: Some(count),
        rep_progress: None,
    })
}
fn fixture() -> Fixture {
    let mut b = IntegerBuilder::new();
    let preheader = b.block;
    // Allocation order deliberately disagrees with dominance order.
    let exit = b.region.block(false);
    let body = b.region.block(false);
    let header = b.region.block(false);
    let header_effect = b.region.param(header, Type::Effect);
    let acc = b.region.param(header, Type::I32);
    let remaining = b.region.param(header, Type::I32);
    let retired = b.region.param(header, Type::I32);
    let body_effect = b.region.param(body, Type::Effect);
    b.region.param(exit, Type::Effect);
    let zero = b.constant(0, Type::I32);
    b.region.terminate(
        preheader,
        Terminator::Branch(Edge {
            target: header,
            args: vec![b.effect, b.gpr[0], b.gpr[1], zero],
        }),
    );
    b.block = header;
    b.gpr[0] = acc;
    b.gpr[1] = remaining;
    let header_state = state(&mut b, 0x8000, retired);
    b.region.blocks[header.index()].entry_state = Some(header_state);
    let sum = b.binary(Binary::Add, b.gpr[2], b.gpr[3]);
    let done = b.binary(Binary::Eq, remaining, zero);
    b.region.terminate(
        header,
        Terminator::CondBranch {
            condition: done,
            taken: Edge {
                target: exit,
                args: vec![header_effect],
            },
            not_taken: Edge {
                target: body,
                args: vec![header_effect],
            },
        },
    );
    b.block = body;
    let body_state = state(&mut b, 0x8010, retired);
    b.region.blocks[body.index()].entry_state = Some(body_state);
    let effect = b.region.append(
        body,
        Op::PollBudget,
        vec![body_effect],
        &[Type::Effect],
        Some(body_state),
    )[0];
    let poll = instruction(&b.region, effect);
    let wide_a = b.node(Op::Extend { signed: false }, vec![b.gpr[2]], Type::I64);
    let wide_b = b.node(Op::Extend { signed: false }, vec![b.gpr[3]], Type::I64);
    let wide_sum = b.binary(Binary::Add, wide_a, wide_b);
    let shift = b.constant(32, Type::I64);
    let carry64 = b.binary(Binary::Shr, wide_sum, shift);
    let carry = b.node(Op::Truncate, vec![carry64], Type::I32);
    let mixed = b.binary(Binary::Xor, sum, b.gpr[4]);
    let step = b.binary(Binary::Add, mixed, carry);
    let one = b.constant(1, Type::I32);
    let next_acc = b.binary(Binary::Add, acc, step);
    let next_remaining = b.binary(Binary::Sub, remaining, one);
    let next_retired = b.binary(Binary::Add, retired, one);
    b.region.terminate(
        body,
        Terminator::Branch(Edge {
            target: header,
            args: vec![effect, next_acc, next_remaining, next_retired],
        }),
    );
    b.block = exit;
    let exit_state = state(&mut b, 0x9000, retired);
    b.region.blocks[exit.index()].entry_state = Some(exit_state);
    b.region.terminate(exit, Terminator::Exit(exit_state));
    let invariant = [
        sum, wide_a, wide_b, wide_sum, shift, carry64, carry, mixed, step, one,
    ]
    .map(|v| instruction(&b.region, v))
    .to_vec();
    let variant = [done, next_acc, next_remaining, next_retired]
        .map(|v| instruction(&b.region, v))
        .to_vec();
    verify(&b.region).unwrap();
    Fixture {
        region: b.region,
        preheader,
        header,
        body,
        invariant,
        variant,
        poll,
    }
}

#[test]
fn licm_moves_dependency_chains_without_changing_observations() {
    let mut f = fixture();
    let states = format!("{:?}", f.region.states);
    let control: Vec<_> = f
        .region
        .blocks
        .iter()
        .map(|b| format!("{:?}{:?}{:?}", b.params, b.entry_state, b.terminator))
        .collect();
    let poll = format!("{:?}", f.region.instructions[f.poll.index()]);
    let stats = run(&mut f.region, Config::default()).unwrap();
    assert_eq!(stats.loops_seen, 1);
    assert_eq!(stats.loops_changed, 1);
    assert_eq!(stats.hoisted, f.invariant.len());
    for id in f.invariant {
        assert_eq!(f.region.instructions[id.index()].block, f.preheader);
    }
    for id in f.variant {
        assert_ne!(f.region.instructions[id.index()].block, f.preheader);
    }
    assert_eq!(states, format!("{:?}", f.region.states));
    assert_eq!(
        control,
        f.region
            .blocks
            .iter()
            .map(|b| format!("{:?}{:?}{:?}", b.params, b.entry_state, b.terminator))
            .collect::<Vec<_>>()
    );
    assert_eq!(poll, format!("{:?}", f.region.instructions[f.poll.index()]));
    verify(&f.region).unwrap();
    assert_eq!(run(&mut f.region, Config::default()).unwrap().hoisted, 0);
}

#[test]
fn licm_budget_exhaustion_is_atomic_even_after_motion_started() {
    let original = fixture().region;
    let before = format!("{original:?}");
    let stats = run(&mut original.clone(), Config::default()).unwrap();
    assert!(stats.hoisted > 2 && stats.work > 1);
    for config in [
        Config {
            max_work: 0,
            ..Config::default()
        },
        Config {
            max_work: stats.work - 1,
            ..Config::default()
        },
        Config {
            max_hoists: 0,
            ..Config::default()
        },
        Config {
            max_hoists: stats.hoisted - 1,
            ..Config::default()
        },
        Config {
            max_work: usize::MAX,
            ..Config::default()
        },
    ] {
        let mut candidate = original.clone();
        assert!(run(&mut candidate, config).is_err());
        assert_eq!(before, format!("{candidate:?}"));
    }
    let mut corrupt = original.clone();
    corrupt.instructions[0].block = BlockId(1234);
    let before = format!("{corrupt:?}");
    assert!(run(&mut corrupt, Config::default()).is_err());
    assert_eq!(before, format!("{corrupt:?}"));
}

#[test]
fn licm_respects_state_annotations_and_non_speculatable_ops() {
    let mut f = fixture();
    let id = f.invariant[0];
    f.region.instructions[id.index()].state = f.region.blocks[f.header.index()].entry_state;
    let before = format!("{:?}", f.region.instructions[id.index()]);
    run(&mut f.region, Config::default()).unwrap();
    assert_eq!(before, format!("{:?}", f.region.instructions[id.index()]));
    let mut inst = f.region.instructions[f.invariant[1].index()].clone();
    for op in [
        Op::ReadGpr(0),
        Op::ReadXmm(0),
        Op::ReadFlags,
        Op::ReadRawFlags,
        Op::ReadFlagChanges,
        Op::ReadFlagOperand,
        Op::ReadStack32,
        Op::ReadSegment(0),
        Op::CallHelper(HelperId(0)),
        Op::PollBudget,
        Op::SseCheck,
        Op::SegmentAddress { segment: 0 },
        Op::Divide {
            bits: 32,
            signed: false,
        },
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
        Op::CompareExchange8B {
            order: RmwOrder::Plain,
        },
        Op::XmmLoad {
            bytes: 16,
            register: 0,
        },
        Op::XmmStore {
            lane: 0,
            bytes: 16,
            register: 0,
        },
        Op::XmmMaskedStore { source: 0, mask: 1 },
    ] {
        inst.op = op;
        assert!(!super::movable(&f.region, &inst));
    }
    inst.op = Op::Const(0);
    for ty in [
        Type::GuardProof,
        Type::RamAddress,
        Type::RmwTicket,
        Type::Effect,
        Type::F32,
        Type::F64,
        Type::F80,
    ] {
        f.region.values[inst.results[0].index()].ty = ty;
        assert!(!super::movable(&f.region, &inst), "do not move {ty:?}");
    }
}

fn graph(successors: &[&[usize]], entries: &[usize]) -> Region {
    let mut r = Region::default();
    for b in 0..successors.len() {
        r.block(entries.contains(&b));
    }
    for (b, successors) in successors.iter().enumerate() {
        let block = BlockId(b as u32);
        let edge = |target: usize| Edge {
            target: BlockId(target as u32),
            args: vec![],
        };
        let term = match *successors {
            [target] => Terminator::Branch(edge(*target)),
            [a, c] => {
                let flag = r.append(block, Op::Const(1), vec![], &[Type::I1], None)[0];
                Terminator::CondBranch {
                    condition: flag,
                    taken: edge(*a),
                    not_taken: edge(*c),
                }
            },
            _ => panic!("test CFG requires one or two successors"),
        };
        r.terminate(block, term);
    }
    r
}
fn analyze(r: &Region) -> LoopAnalysis {
    LoopAnalysis::compute(r, &mut WorkBudget::new(Config::default().max_work)).unwrap()
}
#[test]
fn licm_loop_analysis_handles_latches_nesting_and_multiple_entries() {
    let r = graph(&[&[1], &[2, 3], &[1], &[1]], &[0]);
    let a = analyze(&r);
    assert_eq!(a.loops.len(), 1);
    assert_eq!(a.loops[0].header, BlockId(1));
    assert_eq!(a.loops[0].latches, vec![BlockId(2), BlockId(3)]);
    assert_eq!(a.loops[0].members, vec![false, true, true, true]);
    assert_eq!(a.loops[0].preheader, Some(BlockId(0)));
    let nested = analyze(&graph(
        &[&[1], &[2, 6], &[3], &[4, 5], &[3], &[1], &[6]],
        &[0],
    ));
    let inner = nested
        .loops
        .iter()
        .position(|lp| lp.header == BlockId(3))
        .unwrap();
    let outer = nested
        .loops
        .iter()
        .position(|lp| lp.header == BlockId(1))
        .unwrap();
    assert!(inner < outer);
    assert_eq!(nested.loops[inner].preheader, Some(BlockId(2)));
    assert_eq!(nested.loops[outer].preheader, Some(BlockId(0)));
    let external = analyze(&graph(&[&[1], &[2], &[1]], &[0, 1]));
    assert!(external.loops.iter().all(|lp| lp.preheader.is_none()));
    let irreducible = analyze(&graph(&[&[1, 2], &[2], &[1]], &[0]));
    assert!(irreducible.loops.is_empty());
}
#[test]
fn licm_does_not_create_preheaders_or_move_external_entry_values() {
    for r in [
        graph(&[&[1, 3], &[2, 3], &[1], &[3]], &[0]),
        graph(&[&[1, 2], &[3], &[3], &[4], &[3]], &[0]),
        graph(&[&[0]], &[0]),
    ] {
        let a = analyze(&r);
        assert!(!a.loops.is_empty());
        assert!(a.loops.iter().all(|lp| lp.preheader.is_none()));
        let mut transformed = r.clone();
        assert_eq!(run(&mut transformed, Config::default()).unwrap().hoisted, 0);
        assert_eq!(format!("{r:?}"), format!("{transformed:?}"));
    }
    let mut f = fixture();
    f.region.entries.push(f.header);
    // A direct extra entry cannot read the first entry's GPR values: reject, do not miscompile.
    let before = format!("{:?}", f.region);
    assert!(run(&mut f.region, Config::default()).is_err());
    assert_eq!(before, format!("{:?}", f.region));
}

#[test]
fn licm_pipeline_fixture_generation() {
    let original = fixture().region;
    let mut optimized = original.clone();
    assert!(run(&mut optimized, Config::default()).unwrap().hoisted > 0);
    let mut full = original.clone();
    pipeline(&mut full, PassConfig::default()).unwrap();
    run(&mut full, Config::default()).unwrap();
    std::fs::create_dir_all("build/ir-licm").unwrap();
    let layout = StateLayout {
        gpr: 0,
        flags: 32,
        eip: 36,
        committed: 40,
        flag_operand: 44,
    };
    for budget in [1, 2, 3, 4, 5, 7, 8, 12, 31, 256] {
        for (name, region) in [("plain", &original), ("licm", &optimized), ("full", &full)] {
            let mir = lower(region).unwrap();
            let code = emit(&mir, layout, budget).unwrap();
            std::fs::write(
                format!("build/ir-licm/loop-{budget}-{name}.wasm"),
                code.bytes,
            )
            .unwrap();
        }
    }
}

#[test]
fn licm_runtime_tiering_and_opt_out() {
    use crate::ir::frontend::decode::{LinearAddress, PhysicalAddress};
    use crate::ir::runtime::compile::*;
    let snapshot = ImmutableCodeSnapshot {
        // Existing preheader, then INC EAX / DEC ECX / JNZ loop.
        bytes: vec![0xEB, 0, 0x40, 0x49, 0x75, 0xFC],
        dependencies: vec![CodeDependency {
            page: PhysicalAddress(0x200000),
            version: 7,
        }],
        mappings: vec![CodeMapping {
            linear: LinearAddress(0x100000),
            physical: PhysicalAddress(0x200000),
        }],
    };
    for tier in [Tier::One, Tier::Two] {
        for optimize in [false, true] {
            for rounds in [0, 2] {
                let request = CompileRequest {
                    key: PublicationKey {
                        job: 1,
                        vm_generation: 2,
                        slot: 3,
                        slot_generation: 4,
                    },
                    pc: GuestEip(0x1000),
                    linear: LinearAddress(0x100000),
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
                    max_code_bytes: 128,
                    layout: StateLayout {
                        gpr: 0,
                        flags: 32,
                        eip: 36,
                        committed: 40,
                        flag_operand: 44,
                    },
                };
                let artifact = compile_cpu_cfg_region(&request, &snapshot, &config).unwrap();
                let enabled = tier == Tier::Two && optimize && rounds != 0;
                assert_eq!(artifact.loop_passes.work != 0, enabled);
                assert_eq!(artifact.loop_passes.loops_seen != 0, enabled);
                assert_eq!(artifact.key, request.key);
                assert_eq!(artifact.dependencies, snapshot.dependencies);
                assert_eq!(artifact.mappings, snapshot.mappings);
            }
        }
    }
}

fn append_before_terminator(
    r: &mut Region,
    block: BlockId,
    op: Op,
    args: Vec<ValueId>,
    ty: Type,
) -> ValueId {
    let term = r.blocks[block.index()].terminator.take().unwrap();
    let value = r.append(block, op, args, &[ty], None)[0];
    r.terminate(block, term);
    value
}

#[test]
fn licm_nested_motion_respects_outer_and_inner_induction_values() {
    let mut r = graph(&[&[1], &[2, 6], &[3], &[4, 5], &[3], &[1], &[6]], &[0]);
    let global = append_before_terminator(&mut r, BlockId(0), Op::Const(5), vec![], Type::I32);
    let outer = r.param(BlockId(1), Type::I32);
    let inner = r.param(BlockId(3), Type::I32);
    let next_outer = append_before_terminator(
        &mut r,
        BlockId(5),
        Op::Binary(Binary::Add),
        vec![outer, global],
        Type::I32,
    );
    let next_inner = append_before_terminator(
        &mut r,
        BlockId(4),
        Op::Binary(Binary::Add),
        vec![inner, global],
        Type::I32,
    );
    for (source, argument) in [(0, global), (5, next_outer), (2, global), (4, next_inner)] {
        let Terminator::Branch(edge) = r.blocks[source].terminator.as_mut().unwrap() else {
            panic!()
        };
        edge.args.push(argument);
    }
    let invariant = append_before_terminator(
        &mut r,
        BlockId(4),
        Op::Binary(Binary::Mul),
        vec![global, global],
        Type::I32,
    );
    let per_outer = append_before_terminator(
        &mut r,
        BlockId(4),
        Op::Binary(Binary::Xor),
        vec![outer, global],
        Type::I32,
    );
    verify(&r).unwrap();
    let stats = run(&mut r, Config::default()).unwrap();
    let owner = |value| r.instructions[instruction(&r, value).index()].block;
    assert_eq!(
        owner(invariant),
        BlockId(0),
        "global invariant crosses both preheaders"
    );
    assert_eq!(
        owner(per_outer),
        BlockId(2),
        "outer induction value remains inside its loop"
    );
    assert_eq!(owner(next_inner), BlockId(4));
    assert_eq!(owner(next_outer), BlockId(5));
    assert!(stats.loops_changed >= 2);
    verify(&r).unwrap();
}
