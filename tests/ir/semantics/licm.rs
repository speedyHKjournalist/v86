use super::{movable, run, Config};
use crate::ir::{
    analysis::loops::{LoopAnalysis, WorkBudget},
    backend::wasm::{emit, StateLayout},
    frontend::{decode::GuestEip, integer::IntegerBuilder},
    hir::*,
    ids::*,
    lowering::lower,
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
struct Fixture {
    region: Region,
    entry: BlockId,
    header: BlockId,
    body: BlockId,
    input: [ValueId; 8],
    flags: FlagState,
    invariant: ValueId,
    variant: ValueId,
}
fn fixture() -> Fixture {
    let mut b = IntegerBuilder::new();
    let input = b.gpr;
    let flags = b.flags.clone();
    let entry = b.block;
    // Allocate the loop's dominated body before the header, deliberately.
    let exit = b.region.block(false);
    let body = b.region.block(false);
    let header = b.region.block(false);
    let he = b.region.param(header, Type::Effect);
    let counter = b.region.param(header, Type::I32);
    let acc = b.region.param(header, Type::I32);
    let be = b.region.param(body, Type::Effect);
    let ee = b.region.param(exit, Type::Effect);
    let zero = b.constant(0, Type::I32);
    b.region.terminate(
        entry,
        Terminator::Branch(Edge {
            target: header,
            args: vec![b.effect, input[1], input[0]],
        }),
    );
    b.gpr[0] = acc;
    b.gpr[1] = counter;
    for (block, pc) in [(header, 0x1100), (body, 0x1200), (exit, 0x1300)] {
        let state = snapshot(&mut b.region, b.gpr, &flags, pc);
        b.region.blocks[block.index()].entry_state = Some(state);
    }
    b.block = header;
    b.effect = he;
    let done = b.binary(Binary::Eq, counter, zero);
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
    let sum = b.binary(Binary::Add, input[2], input[3]);
    let mask = b.constant(0x55AA55AA, Type::I32);
    let mixed = b.binary(Binary::Xor, sum, mask);
    let three = b.constant(3, Type::I32);
    let invariant = b.binary(Binary::Mul, mixed, three);
    let next_acc = b.binary(Binary::Add, acc, invariant);
    let one = b.constant(1, Type::I32);
    let next_counter = b.binary(Binary::Sub, counter, one);
    b.region.terminate(
        body,
        Terminator::Branch(Edge {
            target: header,
            args: vec![be, next_counter, next_acc],
        }),
    );
    b.block = exit;
    b.effect = ee;
    let state = snapshot(&mut b.region, b.gpr, &flags, 0x1400);
    b.region.terminate(exit, Terminator::Exit(state));
    Fixture {
        region: b.region,
        entry,
        header,
        body,
        input,
        flags,
        invariant,
        variant: next_acc,
    }
}
fn owner(r: &Region, value: ValueId) -> BlockId {
    match r.values[value.index()].definition {
        Definition::Instruction(id, _) => r.instructions[id.index()].block,
        Definition::Parameter(block, _) => block,
    }
}
fn layout() -> StateLayout {
    StateLayout {
        gpr: 0,
        flags: 32,
        eip: 36,
        committed: 40,
        flag_operand: 44,
    }
}

#[test]
fn licm_moves_only_invariants_and_preserves_recovery_graph() {
    let mut f = fixture();
    verify(&f.region).unwrap();
    let states = format!("{:?}", f.region.states);
    let edges = format!(
        "{:?}",
        f.region
            .blocks
            .iter()
            .map(|b| (&b.params, &b.terminator, b.entry_state))
            .collect::<Vec<_>>()
    );
    let stats = run(&mut f.region, Config::default()).unwrap();
    assert_eq!(stats.loops_seen, 1);
    assert_eq!(stats.loops_changed, 1);
    assert_eq!(stats.hoisted, 6);
    assert_eq!(owner(&f.region, f.invariant), f.entry);
    assert_eq!(owner(&f.region, f.variant), f.body);
    assert_eq!(states, format!("{:?}", f.region.states));
    assert_eq!(
        edges,
        format!(
            "{:?}",
            f.region
                .blocks
                .iter()
                .map(|b| (&b.params, &b.terminator, b.entry_state))
                .collect::<Vec<_>>()
        )
    );
    verify(&f.region).unwrap();
    lower(&f.region).unwrap();
    let after = format!("{:?}", f.region);
    assert_eq!(run(&mut f.region, Config::default()).unwrap().hoisted, 0);
    assert_eq!(after, format!("{:?}", f.region));
}

#[test]
fn licm_budget_failure_is_atomic_even_after_planned_moves() {
    let mut f = fixture();
    let before = format!("{:?}", f.region);
    let work = run(&mut f.region.clone(), Config::default()).unwrap().work;
    for config in [
        Config {
            max_hoists: 1,
            ..Config::default()
        },
        Config {
            max_work: 0,
            ..Config::default()
        },
        Config {
            max_work: work - 1,
            ..Config::default()
        },
    ] {
        let result = run(&mut f.region, config).unwrap();
        assert!(result.budget_exhausted);
        assert!(result.budget_reason.is_some());
        assert_eq!(result.hoisted, 0);
        assert_eq!(before, format!("{:?}", f.region));
        verify(&f.region).unwrap();
    }
}

#[test]
fn licm_rejects_invalid_ir_without_mutation() {
    let mut f = fixture();
    let Definition::Instruction(id, _) = f.region.values[f.invariant.index()].definition else {
        panic!()
    };
    f.region.instructions[id.index()].args[0] = ValueId(u32::MAX);
    let before = format!("{:?}", f.region);
    assert!(run(&mut f.region, Config::default()).is_err());
    assert_eq!(before, format!("{:?}", f.region));
}

#[test]
fn licm_does_not_create_preheaders_or_change_branch_budgeting() {
    let mut f = fixture();
    let Terminator::Branch(edge) = f.region.blocks[f.entry.index()].terminator.take().unwrap()
    else {
        panic!()
    };
    let cold = f.region.block(false);
    let state = snapshot(&mut f.region, f.input, &f.flags, 0x9000);
    f.region.terminate(cold, Terminator::Exit(state));
    let condition = f
        .region
        .append(f.entry, Op::Const(1), vec![], &[Type::I1], None)[0];
    f.region.terminate(
        f.entry,
        Terminator::CondBranch {
            condition,
            taken: edge,
            not_taken: Edge {
                target: cold,
                args: vec![],
            },
        },
    );
    verify(&f.region).unwrap();
    let before = format!("{:?}", f.region);
    let stats = run(&mut f.region, Config::default()).unwrap();
    assert_eq!(stats.loops_seen, 1);
    assert_eq!(stats.hoisted, 0);
    assert_eq!(before, format!("{:?}", f.region));
}

#[test]
fn licm_keeps_cpu_reads_and_snapshot_observations_in_place() {
    let mut f = fixture();
    let term = f.region.blocks[f.body.index()].terminator.take().unwrap();
    let read = f.input[6];
    let state = f.region.blocks[f.body.index()].entry_state.unwrap();
    let mut observed = f.region.states[state.index()].clone();
    observed.gpr[2] = f.invariant;
    let observed = f.region.state(observed);
    let effect = f.region.blocks[f.body.index()].params[0];
    let next = f.region.append(
        f.body,
        Op::PollBudget,
        vec![effect],
        &[Type::Effect],
        Some(observed),
    )[0];
    let Definition::Instruction(poll, _) = f.region.values[next.index()].definition else {
        panic!()
    };
    let Terminator::Branch(mut edge) = term else {
        panic!()
    };
    edge.args[0] = next;
    f.region.terminate(f.body, Terminator::Branch(edge));
    verify(&f.region).unwrap();
    assert_eq!(run(&mut f.region, Config::default()).unwrap().hoisted, 6);
    assert_eq!(owner(&f.region, read), f.entry);
    assert_eq!(owner(&f.region, f.invariant), f.entry);
    assert_eq!(f.region.instructions[poll.index()].block, f.body);
    assert_eq!(f.region.instructions[poll.index()].state, Some(observed));
    assert_eq!(f.region.states[observed.index()].gpr[2], f.invariant);
    verify(&f.region).unwrap();
    lower(&f.region).unwrap();
}

#[test]
fn licm_speculation_whitelist_excludes_every_stateful_class() {
    let template = Instruction {
        block: BlockId(0),
        op: Op::Const(7),
        args: vec![],
        results: vec![ValueId(0)],
        state: None,
        commit: None,
        trap_after_fault: false,
        unmasked_word_store: false,
    };
    assert!(movable(&template));
    for op in [
        Op::ReadGpr(0),
        Op::ReadXmm(0),
        Op::ReadFlags,
        Op::ReadRawFlags,
        Op::ReadFlagChanges,
        Op::ReadFlagOperand,
        Op::ReadStack32,
        Op::ReadSegment(3),
        Op::Divide {
            bits: 32,
            signed: false,
        },
        Op::SegmentAddress { segment: 3 },
        Op::PopAddress {
            segment: 2,
            bytes: 4,
        },
        Op::GuestLoad { bytes: 4 },
        Op::GuestStore { bytes: 4 },
        Op::PartialStore { bytes: 4 },
        Op::GuestCheck {
            bytes: 4,
            write: false,
        },
        Op::CallHelper(HelperId(0)),
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
        Op::PollBudget,
        Op::SseCheck,
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
        assert!(!movable(&Instruction {
            op,
            ..template.clone()
        }));
    }
    assert!(!movable(&Instruction {
        state: Some(StateId(0)),
        ..template.clone()
    }));
    assert!(!movable(&Instruction {
        commit: Some(StateId(0)),
        ..template.clone()
    }));
    assert!(!movable(&Instruction {
        trap_after_fault: true,
        ..template.clone()
    }));
    assert!(!movable(&Instruction {
        unmasked_word_store: true,
        ..template
    }));
}

// CFG-only fixtures intentionally omit value/state arenas: loop discovery consumes
// only graph edges, reachability and external roots, not instruction semantics.
fn graph(edges: &[&[usize]]) -> Region {
    let mut r = Region::default();
    for i in 0..edges.len() {
        r.block(i == 0);
    }
    for (b, successors) in edges.iter().enumerate() {
        let edge = |i: usize| Edge {
            target: BlockId(i as u32),
            args: vec![],
        };
        let term = match *successors {
            [] => Terminator::Exit(StateId(0)),
            [a] => Terminator::Branch(edge(*a)),
            [a, b] => Terminator::CondBranch {
                condition: ValueId(0),
                taken: edge(*a),
                not_taken: edge(*b),
            },
            _ => panic!("not a binary CFG"),
        };
        r.terminate(BlockId(b as u32), term);
    }
    r
}
fn analyze(r: &Region) -> LoopAnalysis {
    LoopAnalysis::compute(r, &mut WorkBudget::new(1_048_576)).unwrap()
}
#[test]
fn loop_analysis_handles_nested_self_and_multiple_latches() {
    let a = analyze(&graph(&[&[1], &[2, 6], &[3], &[4, 5], &[3], &[1], &[]]));
    assert_eq!(a.loops.len(), 2);
    assert_eq!(a.loops[0].header, BlockId(3));
    assert_eq!(a.loops[0].preheader, Some(BlockId(2)));
    assert_eq!(a.loops[1].header, BlockId(1));
    assert_eq!(a.loops[1].preheader, Some(BlockId(0)));
    assert_eq!(a.loops[1].members.iter().filter(|&&v| v).count(), 5);
    let a = analyze(&graph(&[&[1], &[2, 3], &[1], &[1, 4], &[]]));
    assert_eq!(a.loops.len(), 1);
    assert_eq!(a.loops[0].latches, vec![BlockId(2), BlockId(3)]);
    let a = analyze(&graph(&[&[1], &[1]]));
    assert_eq!(a.loops[0].latches, vec![BlockId(1)]);
    assert_eq!(a.loops[0].members, vec![false, true]);
}
#[test]
fn loop_analysis_excludes_irreducible_cycles_and_external_entries() {
    assert!(analyze(&graph(&[&[1, 2], &[2, 3], &[1, 3], &[]]))
        .loops
        .is_empty());
    let mut r = graph(&[&[1], &[1, 2], &[]]);
    r.entries.push(BlockId(1));
    assert_eq!(analyze(&r).loops[0].preheader, None);
    let a = analyze(&graph(&[&[1], &[1, 2], &[], &[1]]));
    assert_eq!(
        a.loops[0].preheader,
        Some(BlockId(0)),
        "dead incoming edges are not external entries"
    );
    let a = analyze(&graph(&[&[1], &[2], &[1, 1]]));
    assert_eq!(
        a.loops[0].latches,
        vec![BlockId(2)],
        "duplicate edges do not duplicate latches"
    );
    assert!(LoopAnalysis::compute(&r, &mut WorkBudget::new(0)).is_err());
}

#[test]
fn licm_wasm_execution_fixtures() {
    std::fs::create_dir_all("build/ir-licm").unwrap();
    for budget in [1, 2, 3, 4, 5, 8, 9, 16, 64, 100] {
        for optimize in [false, true] {
            let mut f = fixture();
            if optimize {
                assert_eq!(run(&mut f.region, Config::default()).unwrap().hoisted, 6);
            }
            let artifact = emit(&lower(&f.region).unwrap(), layout(), budget).unwrap();
            std::fs::write(
                format!("build/ir-licm/loop-{budget}-{optimize}.wasm"),
                artifact.bytes,
            )
            .unwrap();
        }
    }
}

#[test]
fn cpu_compile_pipeline_runs_licm_only_for_optimized_tier_two() {
    use crate::ir::{
        frontend::decode::{LinearAddress, PhysicalAddress},
        passes::PassConfig,
        runtime::compile::{
            compile_cpu_cfg_region, CodeDependency, CodeMapping, CompileRequest,
            ImmutableCodeSnapshot, IrConfig, PublicationKey, Tier,
        },
    };
    // mov ecx,5; jmp header; header: imul edx,eax,3; dec ecx; jnz header; nop
    let snapshot = ImmutableCodeSnapshot {
        bytes: vec![
            0xB9, 5, 0, 0, 0, 0xEB, 0, 0x6B, 0xD0, 3, 0x49, 0x75, 0xFA, 0x90,
        ],
        dependencies: vec![CodeDependency {
            page: PhysicalAddress(0x100000),
            version: 1,
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
                    vm_generation: 1,
                    slot: 1,
                    slot_generation: 1,
                },
                pc: GuestEip(0x1000),
                linear: LinearAddress(0x100000),
                default_32: true,
                tier,
            };
            let config = IrConfig {
                optimize,
                passes: PassConfig::default(),
                execution_budget: 100,
                rep_iteration_budget: 8,
                max_code_bytes: 64,
                layout: StateLayout {
                    gpr: 0,
                    flags: 32,
                    eip: 36,
                    committed: 40,
                    flag_operand: 44,
                },
            };
            let compiled = compile_cpu_cfg_region(&request, &snapshot, &config).unwrap();
            if tier == Tier::Two && optimize {
                assert!(compiled.licm.hoisted > 0);
            } else {
                assert_eq!(compiled.licm.hoisted, 0);
            }
        }
    }
}
