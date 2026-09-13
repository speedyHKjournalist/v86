use super::{run, DEFAULT_WORK_LIMIT};
use crate::ir::{
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

// sum += (input[2] + input[3]) * input[4], repeated input[0] times.
// Exit is intentionally allocated before body, not in execution order.
fn counted_loop() -> (Region, ValueId, ValueId, ValueId) {
    let mut b = IntegerBuilder::new();
    let input = b.gpr;
    let flags = b.flags.clone();
    let entry = b.block;
    let zero = b.constant(0, Type::I32);
    let header = b.region.block(false);
    let exit = b.region.block(false);
    let body = b.region.block(false);
    let he = b.region.param(header, Type::Effect);
    let count = b.region.param(header, Type::I32);
    let sum = b.region.param(header, Type::I32);
    let be = b.region.param(body, Type::Effect);
    let _xe = b.region.param(exit, Type::Effect);
    b.region.terminate(
        entry,
        Terminator::Branch(Edge {
            target: header,
            args: vec![b.effect, input[0], zero],
        }),
    );
    b.gpr[0] = count;
    b.gpr[1] = sum;
    for block in [header, body, exit] {
        let state = snapshot(&mut b.region, b.gpr, &flags, 0x2000 + block.0);
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
    let invariant = b.binary(Binary::Add, input[2], input[3]);
    let dependent = b.binary(Binary::Mul, invariant, input[4]);
    let next_sum = b.binary(Binary::Add, sum, dependent);
    let one = b.constant(1, Type::I32);
    let next_count = b.binary(Binary::Sub, count, one);
    // CPU reads are entry-only under the HIR verifier. Preserve their original
    // position; illegal body reads are tested as verifier rejection separately.
    let cpu_read = input[7];
    b.region.terminate(
        body,
        Terminator::Branch(Edge {
            target: header,
            args: vec![be, next_count, next_sum],
        }),
    );
    let state = snapshot(&mut b.region, b.gpr, &flags, 0x3000);
    b.region.terminate(exit, Terminator::Exit(state));
    (b.region, invariant, dependent, cpu_read)
}

fn owner(region: &Region, value: ValueId) -> BlockId {
    let Definition::Instruction(id, _) = region.values[value.index()].definition
    else {
        panic!("expected instruction result")
    };
    region.instructions[id.index()].block
}

#[test]
fn hoists_transitive_invariants_but_not_loop_state_or_cpu_reads() {
    let (mut r, invariant, dependent, cpu_read) = counted_loop();
    verify(&r).unwrap();
    let maps = format!("{:?}", r.states);
    let before = r.instructions.len();
    let stats = run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!(stats.loops, 1);
    assert_eq!(stats.hoisted, 3); // add, multiply, constant one
    assert_eq!(owner(&r, invariant), BlockId(0));
    assert_eq!(owner(&r, dependent), BlockId(0));
    assert_eq!(owner(&r, cpu_read), BlockId(0));
    assert_eq!(r.instructions.len(), before);
    assert_eq!(format!("{:?}", r.states), maps);
    verify(&r).unwrap();
    lower(&r).unwrap();
    assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().hoisted, 0);
}

#[test]
fn failure_is_atomic_even_after_partial_planning() {
    let (r, _, _, _) = counted_loop();
    let full = run(&mut r.clone(), DEFAULT_WORK_LIMIT).unwrap();
    for limit in [0, 1, full.work / 2, full.work - 1] {
        let mut candidate = r.clone();
        let before = format!("{candidate:?}");
        assert!(run(&mut candidate, limit).is_err(), "limit {limit}");
        assert_eq!(format!("{candidate:?}"), before);
    }
    let mut invalid = r;
    invalid.instructions[0].block = BlockId(999);
    let before = format!("{invalid:?}");
    assert!(run(&mut invalid, DEFAULT_WORK_LIMIT).is_err());
    assert_eq!(format!("{invalid:?}"), before);
    let mut oversized = Region::default();
    for _ in 0..65 {
        oversized.block(true);
    }
    assert_eq!(
        run(&mut oversized, DEFAULT_WORK_LIMIT).unwrap_err(),
        "LICM region budget exceeded"
    );
}

// CFG-only fixtures need no CPU observations or budget snapshots: they test
// analysis, not emission. A two-successor block has a local non-folded condition.
fn graph(edges: &[&[usize]], entries: &[usize]) -> Region {
    let mut r = Region::default();
    for b in 0..edges.len() {
        r.block(entries.contains(&b));
    }
    for (b, successors) in edges.iter().enumerate() {
        let block = BlockId(b as u32);
        r.append(block, Op::Const(42), vec![], &[Type::I32], None);
        let edge = |target: usize| Edge {
            target: BlockId(target as u32),
            args: vec![],
        };
        let term = match *successors {
            [target] => Terminator::Branch(edge(*target)),
            [taken, not_taken] => {
                let condition = r.append(block, Op::Const(1), vec![], &[Type::I1], None)[0];
                Terminator::CondBranch {
                    condition,
                    taken: edge(*taken),
                    not_taken: edge(*not_taken),
                }
            },
            _ => panic!("fixture needs one or two edges"),
        };
        r.terminate(block, term);
    }
    verify(&r).unwrap();
    r
}

#[test]
fn skips_external_headers_multiple_entries_and_conditional_preheaders() {
    for mut r in [
        graph(&[&[1], &[2], &[1]], &[0, 1]),
        graph(&[&[1, 2], &[2], &[1]], &[0]),
        graph(&[&[1, 2], &[1], &[2]], &[0]),
        graph(&[&[1], &[2], &[1], &[1]], &[0, 3]),
    ] {
        let before = format!("{r:?}");
        assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().hoisted, 0);
        assert_eq!(format!("{r:?}"), before);
    }
}

#[test]
fn handles_self_loops_multiple_latches_and_nested_loops() {
    let mut self_loop = graph(&[&[1], &[1]], &[0]);
    assert_eq!(run(&mut self_loop, DEFAULT_WORK_LIMIT).unwrap().hoisted, 1);
    let mut latches = graph(&[&[1], &[2, 3], &[1], &[1]], &[0]);
    let stats = run(&mut latches, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!(stats.loops, 1);
    assert_eq!(stats.hoisted, 4);
    let mut nested = graph(&[&[1], &[2], &[3, 4], &[2], &[1]], &[0]);
    let stats = run(&mut nested, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!(stats.loops, 2);
    assert!(stats.hoisted >= 5);
    verify(&nested).unwrap();
    assert_eq!(run(&mut nested, DEFAULT_WORK_LIMIT).unwrap().hoisted, 0);
}

#[test]
fn whitelist_rejects_observations_faults_and_metadata() {
    let (r, invariant, _, _) = counted_loop();
    let Definition::Instruction(id, _) = r.values[invariant.index()].definition
    else {
        panic!()
    };
    let pure = &r.instructions[id.index()];
    assert!(super::eligible(pure));
    for op in [
        Op::ReadGpr(0),
        Op::ReadXmm(0),
        Op::ReadFlags,
        Op::ReadRawFlags,
        Op::ReadFlagChanges,
        Op::ReadFlagOperand,
        Op::ReadStack32,
        Op::ReadSegment(0),
        Op::GuestLoad { bytes: 4 },
        Op::GuestStore { bytes: 4 },
        Op::GuestCheck {
            bytes: 4,
            write: false,
        },
        Op::PollBudget,
        Op::SseCheck,
        Op::Divide {
            bits: 32,
            signed: true,
        },
        Op::CallHelper(HelperId(0)),
        Op::LinearOffset,
        Op::RmwLoad {
            bytes: 4,
            order: RmwOrder::Locked,
        },
    ] {
        let mut inst = pure.clone();
        inst.op = op;
        assert!(!super::eligible(&inst), "{:?}", inst.op);
    }
    for flag in 0..4 {
        let mut inst = pure.clone();
        match flag {
            0 => inst.state = Some(StateId(0)),
            1 => inst.commit = Some(StateId(0)),
            2 => inst.trap_after_fault = true,
            _ => inst.unmasked_word_store = true,
        }
        assert!(!super::eligible(&inst));
    }
}

#[test]
fn emits_optimized_and_unoptimized_budgeted_loops() {
    let layout = StateLayout {
        gpr: 0,
        flags: 32,
        eip: 36,
        committed: 40,
        flag_operand: 44,
    };
    std::fs::create_dir_all("build/ir-licm").unwrap();
    for budget in [1, 2, 3, 4, 5, 8, 17, 100] {
        for opt in [false, true] {
            let (mut r, _, _, _) = counted_loop();
            if opt {
                assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().hoisted, 3);
            }
            let mir = lower(&r).unwrap();
            drop(r);
            std::fs::write(
                format!("build/ir-licm/loop-{budget}-{opt}.wasm"),
                emit(&mir, layout, budget).unwrap().bytes,
            )
            .unwrap();
        }
    }
}

#[test]
fn rejects_cpu_initialization_inside_loop_without_mutating_input() {
    let (mut r, _, _, _) = counted_loop();
    let body = BlockId(3);
    let term = r.blocks[body.index()].terminator.take().unwrap();
    r.append(body, Op::ReadGpr(7), vec![], &[Type::I32], None);
    r.terminate(body, term);
    let before = format!("{r:?}");
    assert!(run(&mut r, DEFAULT_WORK_LIMIT)
        .unwrap_err()
        .contains("GPR initialization outside entry"));
    assert_eq!(format!("{r:?}"), before);
}

#[test]
fn compile_request_runs_licm_only_for_optimized_tier_two() {
    use crate::ir::{
        frontend::decode::{LinearAddress, PhysicalAddress},
        passes::PassConfig,
        runtime::compile::*,
    };
    // NOP; loop: MOV EAX,EBX; ADD EAX,EDX; ADD EDI,EAX; DEC ECX; JNZ loop.
    // The loop-invariant addition is dynamic (not a constant-folding example).
    let source = ImmutableCodeSnapshot {
        bytes: vec![0x90, 0x89, 0xD8, 0x01, 0xD0, 0x01, 0xC7, 0x49, 0x75, 0xF7],
        dependencies: vec![CodeDependency {
            page: PhysicalAddress(0x100000),
            version: 9,
        }],
        mappings: vec![CodeMapping {
            linear: LinearAddress(0x100000),
            physical: PhysicalAddress(0x100000),
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
                    execution_budget: 20,
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
                let compiled = compile_cpu_cfg_region(&request, &source, &config).unwrap();
                assert_eq!(
                    compiled.passes.loop_hoisted > 0,
                    optimize && rounds != 0 && tier == Tier::Two
                );
                assert!(compiled.current(
                    request.key,
                    &source.dependencies,
                    compiled.entry,
                    &source.mappings
                ));
                assert_eq!(compiled.guest_bytes, source.bytes.len());
            }
        }
    }
}

#[test]
fn real_memory_loop_preserves_ordered_instructions_and_recovery_maps() {
    use crate::ir::{
        frontend::{decode::LinearAddress, region::lift_cpu_cfg},
        passes::{self, PassConfig},
    };
    // NOP; loop: MOV EAX,[ESI]; ADD EAX,EDX; ADD EDI,EAX; DEC ECX; JNZ loop.
    let mut r = lift_cpu_cfg(
        &[0x90, 0x8B, 0x06, 0x01, 0xD0, 0x01, 0xC7, 0x49, 0x75, 0xF7],
        GuestEip(0x1000),
        LinearAddress(0x100000),
        true,
        8,
    )
    .unwrap();
    passes::run(&mut r, PassConfig::default()).unwrap();
    let effects = |r: &Region| {
        r.blocks
            .iter()
            .map(|b| {
                b.instructions
                    .iter()
                    .copied()
                    .filter(|id| r.instructions[id.index()].op.ordered())
                    .map(|id| format!("{:?}", r.instructions[id.index()]))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    let before_effects = effects(&r);
    let before_states = format!("{:?}", r.states);
    let stats = run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
    assert!(stats.hoisted > 0);
    assert_eq!(effects(&r), before_effects);
    assert_eq!(format!("{:?}", r.states), before_states);
    assert!(r
        .instructions
        .iter()
        .any(|i| matches!(i.op, Op::GuestLoad { .. })));
    lower(&r).unwrap();
}

#[test]
fn simd_expressions_can_move_but_sse_guards_cannot() {
    use crate::ir::{
        frontend::{decode::LinearAddress, region::lift_cpu_cfg},
        passes::{self, PassConfig},
    };
    // NOP; loop: MOVDQA XMM2,XMM0; PADDD XMM2,XMM1; PXOR XMM3,XMM2; DEC ECX; JNZ loop.
    let mut r = lift_cpu_cfg(
        &[
            0x90, 0x66, 0x0F, 0x6F, 0xD0, 0x66, 0x0F, 0xFE, 0xD1, 0x66, 0x0F, 0xEF, 0xDA, 0x49,
            0x75, 0xF1,
        ],
        GuestEip(0x1000),
        LinearAddress(0x100000),
        true,
        8,
    )
    .unwrap();
    passes::run(&mut r, PassConfig::default()).unwrap();
    let guards: Vec<_> = r
        .instructions
        .iter()
        .enumerate()
        .filter(|(_, i)| i.op == Op::SseCheck)
        .map(|(index, i)| (index, format!("{i:?}")))
        .collect();
    let vectors: Vec<_> = r
        .instructions
        .iter()
        .enumerate()
        .filter(|(_, i)| matches!(i.op, Op::VectorBinary(_)))
        .map(|(index, i)| (index, i.block))
        .collect();
    let states = format!("{:?}", r.states);
    let stats = run(&mut r, DEFAULT_WORK_LIMIT).unwrap();
    assert!(stats.hoisted > 0);
    assert!(vectors
        .iter()
        .any(|&(i, block)| r.instructions[i].block != block));
    for (i, before) in guards {
        assert_eq!(format!("{:?}", r.instructions[i]), before);
    }
    assert_eq!(format!("{:?}", r.states), states);
    lower(&r).unwrap();
}
