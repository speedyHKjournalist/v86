use super::{natural_loops, run, speculatable, Work, DEFAULT_WORK_BUDGET};
use crate::ir::{
    analysis::cfg::Cfg,
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

fn instruction(r: &Region, value: ValueId) -> InstId {
    match r.values[value.index()].definition {
        Definition::Instruction(id, _) => id,
        _ => panic!("expected instruction"),
    }
}

// Deliberately allocate the exit before its dominators. EAX is the remaining
// count and ECX the accumulator. EDX and EBX are runtime loop invariants.
fn fixture() -> (Region, Vec<InstId>, InstId) {
    let mut b = IntegerBuilder::new();
    let entry = b.block;
    let input = b.gpr;
    let flags = b.flags.clone();
    let exit = b.region.block(false);
    let header = b.region.block(false);
    let body = b.region.block(false);
    let he = b.region.param(header, Type::Effect);
    let count = b.region.param(header, Type::I32);
    let accumulator = b.region.param(header, Type::I32);
    let be = b.region.param(body, Type::Effect);
    let ee = b.region.param(exit, Type::Effect);
    let zero = b.constant(0, Type::I32);
    b.region.terminate(
        entry,
        Terminator::Branch(Edge {
            target: header,
            args: vec![b.effect, input[0], input[1]],
        }),
    );
    b.gpr[0] = count;
    b.gpr[1] = accumulator;
    for block in [header, body] {
        let state = snapshot(&mut b.region, b.gpr, &flags, 0x2000 + block.0 * 0x100);
        b.region.blocks[block.index()].entry_state = Some(state);
    }
    b.block = header;
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
    let one = b.constant(1, Type::I32);
    let three = b.constant(3, Type::I32);
    let sum = b.binary(Binary::Add, input[2], input[3]);
    let product = b.binary(Binary::Mul, sum, three);
    let next_count = b.binary(Binary::Sub, count, one);
    let next_accumulator = b.binary(Binary::Add, accumulator, product);
    b.region.terminate(
        body,
        Terminator::Branch(Edge {
            target: header,
            args: vec![be, next_count, next_accumulator],
        }),
    );
    let state = snapshot(&mut b.region, b.gpr, &flags, 0x9000);
    b.region.blocks[exit.index()].entry_state = Some(state);
    b.region.terminate(exit, Terminator::Exit(state));
    let _ = ee;
    let ids = [one, three, sum, product]
        .map(|v| instruction(&b.region, v))
        .to_vec();
    let variant = instruction(&b.region, next_accumulator);
    (b.region, ids, variant)
}

#[test]
fn invariant_chain_moves_but_loop_carried_values_and_snapshots_do_not() {
    let (mut r, invariants, variant) = fixture();
    verify(&r).unwrap();
    let states = format!("{:?}", r.states);
    let terms = format!(
        "{:?}",
        r.blocks.iter().map(|b| &b.terminator).collect::<Vec<_>>()
    );
    let params = r
        .blocks
        .iter()
        .map(|b| b.params.clone())
        .collect::<Vec<_>>();
    let stats = run(&mut r, DEFAULT_WORK_BUDGET).unwrap();
    assert_eq!(stats.hoisted, 4);
    assert_eq!(stats.loops, 1);
    for id in invariants {
        assert_eq!(r.instructions[id.index()].block, BlockId(0));
        assert!(r.blocks[0].instructions.contains(&id));
        assert!(!r.blocks[3].instructions.contains(&id));
    }
    assert_eq!(r.instructions[variant.index()].block, BlockId(3));
    assert_eq!(states, format!("{:?}", r.states));
    assert_eq!(
        terms,
        format!(
            "{:?}",
            r.blocks.iter().map(|b| &b.terminator).collect::<Vec<_>>()
        )
    );
    assert_eq!(
        params,
        r.blocks
            .iter()
            .map(|b| b.params.clone())
            .collect::<Vec<_>>()
    );
    verify(&r).unwrap();
    lower(&r).unwrap();
    assert_eq!(run(&mut r, DEFAULT_WORK_BUDGET).unwrap().hoisted, 0);
}

#[test]
fn rejects_observations_and_preserves_polls() {
    let (mut r, _, _) = fixture();
    let body = BlockId(3);
    let term = r.blocks[3].terminator.take().unwrap();
    let before = r.blocks[3].entry_state;
    let effect = r.blocks[3].params[0];
    r.helpers
        .push(crate::ir::helper::HelperDescriptor::conservative(
            "observe_cpu".into(),
            vec![],
            vec![Type::I32],
        ));
    let observed = r.append(
        body,
        Op::CallHelper(HelperId(0)),
        vec![effect],
        &[Type::I32, Type::Effect],
        before,
    );
    let (read, effect) = (observed[0], observed[1]);
    let dependent = r.append(
        body,
        Op::Binary(Binary::Add),
        vec![read, read],
        &[Type::I32],
        None,
    )[0];
    let next = r.append(body, Op::PollBudget, vec![effect], &[Type::Effect], before)[0];
    let Terminator::Branch(mut edge) = term else {
        panic!()
    };
    edge.args[0] = next;
    r.terminate(body, Terminator::Branch(edge));
    run(&mut r, DEFAULT_WORK_BUDGET).unwrap();
    for value in [read, dependent, next] {
        assert_eq!(r.instructions[instruction(&r, value).index()].block, body);
    }
    let mut inst = r.instructions[instruction(&r, dependent).index()].clone();
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
        Op::PopAddress {
            segment: 2,
            bytes: 4,
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
            order: RmwOrder::Locked,
        },
        Op::RmwStore {
            bytes: 4,
            order: RmwOrder::Plain,
        },
        Op::CompareExchange8B {
            order: RmwOrder::Locked,
        },
        Op::Divide {
            bits: 32,
            signed: true,
        },
        Op::SseCheck,
        Op::CallHelper(HelperId(0)),
        Op::PollBudget,
        Op::XmmLoad {
            bytes: 16,
            register: 0,
        },
        Op::XmmStore {
            bytes: 16,
            register: 0,
            lane: 0,
        },
        Op::XmmMaskedStore { source: 0, mask: 1 },
        Op::XmmInsertWord {
            register: 0,
            lane: 0,
        },
    ] {
        inst.op = op;
        assert!(
            !speculatable(&inst),
            "observation speculated: {:?}",
            inst.op
        );
    }
    inst.op = Op::Const(7);
    assert!(speculatable(&inst));
    inst.state = before;
    assert!(!speculatable(&inst));
    inst.state = None;
    inst.commit = before;
    assert!(!speculatable(&inst));
}

#[test]
fn exhausted_budget_is_atomic_even_after_staged_motion() {
    let (r, _, _) = fixture();
    let original = format!("{r:?}");
    let mut succeeded = false;
    let mut failures = 0;
    for budget in 0..1024 {
        let mut copy = r.clone();
        match run(&mut copy, budget) {
            Ok(stats) => {
                assert_eq!(stats.hoisted, 4);
                succeeded = true;
                break;
            },
            Err(message) => {
                assert!(message.contains("work budget"));
                assert_eq!(format!("{copy:?}"), original);
                failures += 1;
            },
        }
    }
    assert!(succeeded && failures > 30);
    let mut invalid = r.clone();
    invalid.instructions[0].block = BlockId(3);
    let before = format!("{invalid:?}");
    assert!(run(&mut invalid, DEFAULT_WORK_BUDGET).is_err());
    assert_eq!(format!("{invalid:?}"), before);
}

// Structural discovery tests intentionally do not lower these skeleton graphs.
fn graph(edges: &[&[usize]], entries: &[usize]) -> Region {
    let mut r = Region::default();
    for i in 0..edges.len() {
        r.block(entries.contains(&i));
    }
    for (b, targets) in edges.iter().enumerate() {
        let edge = |target| Edge {
            target: BlockId(target),
            args: vec![],
        };
        let term = match *targets {
            [] => Terminator::Exit(StateId(0)),
            [one] => Terminator::Branch(edge(*one as u32)),
            [one, two] => Terminator::CondBranch {
                condition: ValueId(0),
                taken: edge(*one as u32),
                not_taken: edge(*two as u32),
            },
            _ => panic!(),
        };
        r.terminate(BlockId(b as u32), term);
    }
    r
}
fn loops(r: &Region) -> Vec<super::NaturalLoop> {
    natural_loops(r, &Cfg::compute(r).unwrap(), &mut Work(DEFAULT_WORK_BUDGET)).unwrap()
}

#[test]
fn loop_discovery_handles_latches_nested_cycles_and_external_entries() {
    let multi = graph(&[&[1], &[2, 3], &[1], &[1]], &[0]);
    let found = loops(&multi);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].members, vec![false, true, true, true]);
    let nested = graph(&[&[1], &[2, 5], &[3], &[3, 4], &[1], &[]], &[0]);
    let found = loops(&nested);
    assert_eq!(found.len(), 2);
    assert_eq!((found[0].header, found[0].preheader), (3, 2));
    assert_eq!((found[1].header, found[1].preheader), (1, 0));
    assert!(loops(&graph(&[&[1], &[1]], &[0, 1])).is_empty());
    assert!(loops(&graph(&[&[1, 2], &[3], &[3], &[1, 2]], &[0])).is_empty());
    assert!(loops(&graph(&[&[1, 2], &[1], &[]], &[0])).is_empty());
    assert!(loops(&graph(&[&[1, 2], &[3], &[3], &[3]], &[0])).is_empty());
}

#[test]
fn emitted_loop_corpus() {
    let layout = StateLayout {
        gpr: 0,
        flags: 32,
        eip: 36,
        committed: 40,
        flag_operand: 44,
    };
    std::fs::create_dir_all("build/ir-licm").unwrap();
    for budget in [1, 2, 3, 4, 5, 6, 9, 16, 100] {
        for optimized in [false, true] {
            let (mut r, _, _) = fixture();
            if optimized {
                run(&mut r, DEFAULT_WORK_BUDGET).unwrap();
            }
            let bytes = emit(&lower(&r).unwrap(), layout, budget).unwrap().bytes;
            std::fs::write(
                format!("build/ir-licm/loop-{budget}-{optimized}.wasm"),
                bytes,
            )
            .unwrap();
        }
    }
}

#[test]
fn real_tier_two_compiler_runs_advanced_passes_and_tier_one_does_not() {
    use crate::ir::{
        frontend::decode::{LinearAddress, PhysicalAddress},
        passes::PassConfig,
        runtime::compile::*,
    };
    let mut request = CompileRequest {
        key: PublicationKey {
            job: 1,
            vm_generation: 1,
            slot: 0,
            slot_generation: 1,
        },
        pc: GuestEip(0x1000),
        linear: LinearAddress(0x100000),
        default_32: true,
        tier: Tier::One,
    };
    let mut config = IrConfig {
        optimize: true,
        passes: PassConfig::default(),
        execution_budget: 128,
        rep_iteration_budget: 8,
        max_code_bytes: 2048,
        layout: StateLayout {
            gpr: 0,
            flags: 32,
            eip: 36,
            committed: 40,
            flag_operand: 44,
        },
    };
    // mov ebx,eax; header: test ecx,ecx; jz exit; mov edx,eax;
    // add edx,ebx; add esi,edx; dec ecx; jmp header; exit: nop
    let mut snapshot = ImmutableCodeSnapshot {
        bytes: vec![
            0x89, 0xC3, 0x85, 0xC9, 0x74, 9, 0x89, 0xC2, 0x01, 0xDA, 0x01, 0xD6, 0x49, 0xEB, 0xF3,
            0x90,
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
    assert_eq!(
        compile_cpu_cfg_region(&request, &snapshot, &config)
            .unwrap()
            .passes
            .licm_hoisted,
        0
    );
    request.tier = Tier::Two;
    assert!(
        compile_cpu_cfg_region(&request, &snapshot, &config)
            .unwrap()
            .passes
            .licm_hoisted
            > 0
    );
    config.passes.licm = false;
    assert_eq!(
        compile_cpu_cfg_region(&request, &snapshot, &config)
            .unwrap()
            .passes
            .licm_hoisted,
        0
    );
    snapshot.bytes = vec![0x66, 0x0F, 0x70, 0xC0, 0xE4, 0x90]; // identity PSHUFD
    assert!(
        compile_cpu_cfg_region(&request, &snapshot, &config)
            .unwrap()
            .passes
            .simd_eliminated
            > 0
    );
    request.tier = Tier::One;
    assert_eq!(
        compile_cpu_cfg_region(&request, &snapshot, &config)
            .unwrap()
            .passes
            .simd_eliminated,
        0
    );
    request.tier = Tier::Two;
    config.passes.simd = false;
    assert_eq!(
        compile_cpu_cfg_region(&request, &snapshot, &config)
            .unwrap()
            .passes
            .simd_eliminated,
        0
    );
}

#[test]
fn nested_motion_stops_at_the_innermost_legal_preheader() {
    let mut b = IntegerBuilder::new();
    let root = b.block;
    let input = b.gpr;
    let flags = b.flags.clone();
    let outer = b.region.block(false);
    let pre = b.region.block(false);
    let inner = b.region.block(false);
    let body = b.region.block(false);
    let latch = b.region.block(false);
    let exit = b.region.block(false);
    let oe = b.region.param(outer, Type::Effect);
    let oc = b.region.param(outer, Type::I32);
    let oa = b.region.param(outer, Type::I32);
    let pe = b.region.param(pre, Type::Effect);
    let ie = b.region.param(inner, Type::Effect);
    let ic = b.region.param(inner, Type::I32);
    let ia = b.region.param(inner, Type::I32);
    let be = b.region.param(body, Type::Effect);
    let le = b.region.param(latch, Type::Effect);
    b.region.param(exit, Type::Effect);
    let zero = b.constant(0, Type::I32);
    let one = b.constant(1, Type::I32);
    b.region.terminate(
        root,
        Terminator::Branch(Edge {
            target: outer,
            args: vec![b.effect, input[0], input[2]],
        }),
    );
    b.block = outer;
    let outer_done = b.binary(Binary::Eq, oc, zero);
    b.region.terminate(
        outer,
        Terminator::CondBranch {
            condition: outer_done,
            taken: Edge {
                target: exit,
                args: vec![oe],
            },
            not_taken: Edge {
                target: pre,
                args: vec![oe],
            },
        },
    );
    b.region.terminate(
        pre,
        Terminator::Branch(Edge {
            target: inner,
            args: vec![pe, input[1], oa],
        }),
    );
    b.block = inner;
    let inner_done = b.binary(Binary::Eq, ic, zero);
    b.region.terminate(
        inner,
        Terminator::CondBranch {
            condition: inner_done,
            taken: Edge {
                target: latch,
                args: vec![ie],
            },
            not_taken: Edge {
                target: body,
                args: vec![ie],
            },
        },
    );
    b.block = body;
    let global = b.binary(Binary::Add, input[3], input[5]);
    let outer_variant = b.binary(Binary::Add, oc, input[3]);
    let step = b.binary(Binary::Add, global, outer_variant);
    let sum = b.binary(Binary::Add, ia, step);
    let next_inner = b.binary(Binary::Sub, ic, one);
    b.region.terminate(
        body,
        Terminator::Branch(Edge {
            target: inner,
            args: vec![be, next_inner, sum],
        }),
    );
    b.block = latch;
    let next_outer = b.binary(Binary::Sub, oc, one);
    b.region.terminate(
        latch,
        Terminator::Branch(Edge {
            target: outer,
            args: vec![le, next_outer, ia],
        }),
    );
    for block in [outer, pre, inner, body, latch, exit] {
        let mut regs = input;
        regs[0] = oc;
        regs[2] = if [inner, body, latch].contains(&block) { ia } else { oa };
        let state = snapshot(&mut b.region, regs, &flags, 0xA000 + block.0);
        b.region.blocks[block.index()].entry_state = Some(state);
        if block == exit {
            b.region.terminate(exit, Terminator::Exit(state));
        }
    }
    let stats = run(&mut b.region, DEFAULT_WORK_BUDGET).unwrap();
    assert_eq!(stats.hoisted, 4); // global crosses two loop boundaries
    assert_eq!(
        b.region.instructions[instruction(&b.region, global).index()].block,
        root
    );
    for value in [outer_variant, step] {
        assert_eq!(
            b.region.instructions[instruction(&b.region, value).index()].block,
            pre
        );
    }
    lower(&b.region).unwrap();
}
