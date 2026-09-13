use super::*;
use crate::ir::{
    backend::wasm::{emit, StateLayout},
    frontend::{
        decode::{GuestEip, LinearAddress},
        integer::IntegerBuilder,
        region::lift_cpu_cfg,
    },
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
fn owner(r: &Region, value: ValueId) -> BlockId {
    match r.values[value.index()].definition {
        Definition::Parameter(block, _) => block,
        Definition::Instruction(id, _) => r.instructions[id.index()].block,
    }
}
fn layout() -> StateLayout {
    StateLayout { gpr: 0, flags: 32, eip: 36, committed: 40, flag_operand: 44 }
}

// EAX += ECX * (EDX * EBX + 7), ECX = 0. The header tests for zero
// before the body, so ECX=0 also exercises speculative hoists on zero trips.
fn counted_loop(poll: bool) -> (Region, [ValueId; 3], BlockId) {
    let mut b = IntegerBuilder::new();
    let input = b.gpr;
    let flags = b.flags.clone();
    let entry = b.block;
    // Body deliberately precedes its dominator in the arena.
    let body = b.region.block(false);
    let header = b.region.block(false);
    let exit = b.region.block(false);
    let he = b.region.param(header, Type::Effect);
    let counter = b.region.param(header, Type::I32);
    let sum = b.region.param(header, Type::I32);
    let be = b.region.param(body, Type::Effect);
    let xe = b.region.param(exit, Type::Effect);
    let zero = b.constant(0, Type::I32);
    let one = b.constant(1, Type::I32);
    b.region.terminate(entry, Terminator::Branch(Edge {
        target: header, args: vec![b.effect, input[1], input[0]],
    }));
    let mut state_gpr = input;
    state_gpr[0] = sum;
    state_gpr[1] = counter;
    for (block, pc) in [(header, 0x2000), (body, 0x3000), (exit, 0x4000)] {
        let s = snapshot(&mut b.region, state_gpr, &flags, pc);
        b.region.blocks[block.index()].entry_state = Some(s);
    }
    b.block = header;
    let finished = b.binary(Binary::Eq, counter, zero);
    b.region.terminate(header, Terminator::CondBranch {
        condition: finished,
        taken: Edge { target: exit, args: vec![he] },
        not_taken: Edge { target: body, args: vec![he] },
    });
    b.block = body;
    let seven = b.constant(7, Type::I32);
    let product = b.binary(Binary::Mul, input[2], input[3]);
    let invariant = b.binary(Binary::Add, product, seven);
    let mut effect = be;
    if poll {
        let mut recovery = state_gpr;
        recovery[2] = invariant; // A recovery-only reference must survive motion.
        let s = snapshot(&mut b.region, recovery, &flags, 0x3001);
        effect = b.region.append(body, Op::PollBudget, vec![effect], &[Type::Effect], Some(s))[0];
    }
    let next_sum = b.binary(Binary::Add, sum, invariant);
    let next_counter = b.binary(Binary::Sub, counter, one);
    b.region.terminate(body, Terminator::Branch(Edge {
        target: header, args: vec![effect, next_counter, next_sum],
    }));
    b.block = exit;
    b.effect = xe;
    let state = snapshot(&mut b.region, state_gpr, &flags, 0x4000);
    b.region.terminate(exit, Terminator::Exit(state));
    (b.region, [seven, product, invariant], body)
}

#[test]
fn hoists_dependency_chains_and_preserves_loop_carried_values() {
    let (mut r, values, body) = counted_loop(false);
    let states = format!("{:?}", r.states);
    let before = r.instructions.len();
    let stats = run(&mut r, Config::default()).unwrap();
    assert_eq!(stats.loops, 1);
    assert_eq!(stats.hoisted, 3);
    for v in values { assert_eq!(owner(&r, v), BlockId(0)); }
    assert_eq!(r.instructions.len(), before, "stable arena identities");
    assert_eq!(format!("{:?}", r.states), states);
    assert_eq!(r.blocks[body.index()].instructions.len(), 2);
    verify(&r).unwrap();
    lower(&r).unwrap();
    assert_eq!(run(&mut r, Config::default()).unwrap().hoisted, 0);
}

#[test]
fn keeps_polls_and_recovery_only_uses_in_place() {
    let (mut r, values, body) = counted_loop(true);
    let polls: Vec<_> = r.instructions.iter().enumerate()
        .filter(|(_, i)| i.op == Op::PollBudget)
        .map(|(n, i)| (n, format!("{:?}", i))).collect();
    assert_eq!(polls.len(), 1);
    assert_eq!(run(&mut r, Config::default()).unwrap().hoisted, 3);
    assert_eq!(owner(&r, values[2]), BlockId(0));
    for (n, before) in polls {
        assert_eq!(r.instructions[n].block, body);
        assert_eq!(format!("{:?}", r.instructions[n]), before);
    }
    lower(&r).unwrap();
}

#[test]
fn budget_failures_are_atomic_even_after_successful_hoists() {
    let (original, _, _) = counted_loop(false);
    let before = format!("{:?}", original);
    let work = run(&mut original.clone(), Config::default()).unwrap().work;
    for config in [
        Config { max_work: 0, ..Config::default() },
        Config { max_work: work - 1, ..Config::default() },
        Config { max_hoisted: 1, ..Config::default() },
        Config { max_work: 1_048_577, ..Config::default() },
    ] {
        let mut r = original.clone();
        assert!(run(&mut r, config).is_err());
        assert_eq!(format!("{:?}", r), before);
    }
    assert_eq!(run(&mut original.clone(), Config {
        max_work: work, ..Config::default()
    }).unwrap().hoisted, 3);
    let mut disabled = original.clone();
    assert_eq!(run(&mut disabled, Config {
        max_hoisted: 0, ..Config::default()
    }).unwrap(), Stats::default());
    assert_eq!(format!("{:?}", disabled), before);
}

// Structural tests need no exits or guest state; both conditional successors
// are CFG edges regardless of the literal condition's value.
fn graph(edges: &[&[usize]], entries: &[usize]) -> Region {
    let mut r = Region::default();
    for i in 0..edges.len() { r.block(entries.contains(&i)); }
    for (i, targets) in edges.iter().enumerate() {
        let b = BlockId(i as u32);
        let condition = r.append(b, Op::Const(1), vec![], &[Type::I1], None)[0];
        let edge = |n: usize| Edge { target: BlockId(n as u32), args: vec![] };
        r.terminate(b, if targets.len() == 1 {
            Terminator::Branch(edge(targets[0]))
        } else {
            assert_eq!(targets.len(), 2);
            Terminator::CondBranch {
                condition, taken: edge(targets[0]), not_taken: edge(targets[1]),
            }
        });
    }
    r
}

#[test]
fn merges_multiple_latches_and_handles_self_loops() {
    let mut r = graph(&[&[1], &[2, 3], &[1], &[1]], &[0]);
    let stats = run(&mut r, Config::default()).unwrap();
    assert_eq!(stats.loops, 1);
    assert_eq!(stats.hoisted, 3);
    assert!(r.instructions.iter().all(|i| i.block == BlockId(0)));
    let mut r = graph(&[&[1], &[1]], &[0]);
    assert_eq!(run(&mut r, Config::default()).unwrap().hoisted, 1);
}

#[test]
fn inner_loop_invariants_can_move_again_in_the_outer_loop() {
    let mut r = graph(&[&[1], &[2, 5], &[3], &[3, 4], &[1], &[1]], &[0]);
    let stats = run(&mut r, Config::default()).unwrap();
    assert_eq!(stats.loops, 2);
    assert_eq!(stats.hoisted, 6, "five values plus an inner-to-outer move");
    assert!(r.instructions.iter().all(|i| i.block == BlockId(0)));
    verify(&r).unwrap();
}

#[test]
fn rejects_irreducible_multi_entry_and_missing_preheader_cases() {
    for mut r in [
        graph(&[&[1, 2], &[2], &[1]], &[0]),
        graph(&[&[1], &[2], &[1], &[2]], &[0, 3]),
        graph(&[&[0]], &[0]),
        graph(&[&[1], &[1]], &[0, 1]),
        graph(&[&[1, 3], &[2], &[1], &[1]], &[0]),
        graph(&[&[1, 2], &[1], &[2]], &[0]),
    ] {
        let before = format!("{:?}", r);
        assert_eq!(run(&mut r, Config::default()).unwrap().hoisted, 0);
        assert_eq!(format!("{:?}", r), before);
    }
}

#[test]
fn mutable_cpu_reads_faults_and_effects_are_not_speculation_candidates() {
    let (r, values, _) = counted_loop(false);
    let Definition::Instruction(id, _) = r.values[values[0].index()].definition else { panic!() };
    let template = r.instructions[id.index()].clone();
    for op in [
        Op::ReadGpr(0), Op::ReadXmm(0), Op::ReadFlags, Op::ReadRawFlags,
        Op::ReadFlagChanges, Op::ReadFlagOperand, Op::ReadStack32, Op::ReadSegment(3),
        Op::GuestLoad { bytes: 4 }, Op::GuestStore { bytes: 4 },
        Op::GuestCheck { bytes: 4, write: false },
        Op::RmwLoad { bytes: 4, order: RmwOrder::Locked },
        Op::Divide { bits: 32, signed: true }, Op::SseCheck,
        Op::CallHelper(HelperId(0)), Op::PollBudget,
    ] {
        let mut i = template.clone();
        i.op = op;
        assert!(!speculatable(&i), "{:?}", i.op);
    }
    let mut i = template.clone();
    i.state = Some(StateId(0));
    assert!(!speculatable(&i));
    i = template.clone(); i.commit = Some(StateId(0));
    assert!(!speculatable(&i));
    i = template.clone(); i.trap_after_fault = true;
    assert!(!speculatable(&i));
    i = template; i.unmasked_word_store = true;
    assert!(!speculatable(&i));
}

#[test]
fn real_memory_loop_keeps_all_ordered_operations_and_state_maps() {
    let bytes = [0xB9, 2, 0, 0, 0, 0x8B, 0x06, 0x49, 0x75, 0xFB, 0x90];
    let mut r = lift_cpu_cfg(&bytes, GuestEip(0x1000), LinearAddress(0x100000), true, 8).unwrap();
    let ordered: Vec<_> = r.instructions.iter().enumerate().filter(|(_, i)| i.op.ordered())
        .map(|(n, i)| (n, format!("{:?}", i))).collect();
    assert!(r.instructions.iter().any(|i| matches!(i.op, Op::GuestLoad { .. })));
    let states = format!("{:?}", r.states);
    run(&mut r, Config::default()).unwrap();
    for (n, before) in ordered { assert_eq!(format!("{:?}", r.instructions[n]), before); }
    assert_eq!(format!("{:?}", r.states), states);
    lower(&r).unwrap();
}

#[test]
fn malformed_ir_and_oversized_regions_are_rejected_without_mutation() {
    let (mut r, _, body) = counted_loop(false);
    let id = r.blocks[body.index()].instructions[1];
    r.instructions[id.index()].args[0] = ValueId(u32::MAX);
    let before = format!("{:?}", r);
    assert!(run(&mut r, Config::default()).is_err());
    assert_eq!(format!("{:?}", r), before);
    let mut r = Region::default();
    for _ in 0..65 { r.block(false); }
    assert_eq!(run(&mut r, Config::default()).unwrap_err(), "LICM region budget exceeded");
}

#[test]
fn emits_optimized_and_unoptimized_loops_for_executable_oracles() {
    std::fs::create_dir_all("build/ir-licm").unwrap();
    for poll in [false, true] {
        for budget in [1, 2, 3, 4, 5, 8, 16, 32, 100] {
            for optimized in [false, true] {
                let (mut r, _, _) = counted_loop(poll);
                if optimized { run(&mut r, Config::default()).unwrap(); }
                let bytes = emit(&lower(&r).unwrap(), layout(), budget).unwrap().bytes;
                std::fs::write(format!("build/ir-licm/loop-{poll}-{budget}-{optimized}.wasm"), bytes).unwrap();
            }
        }
    }
}
