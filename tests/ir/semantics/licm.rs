use super::{run, DEFAULT_WORK_LIMIT};
use crate::ir::{
    backend::wasm::{emit_cpu, StateLayout},
    effects::Effects,
    frontend::{
        decode::{GuestEip, LinearAddress, PhysicalAddress},
        integer::IntegerBuilder,
    },
    helper::{ExceptionOwner, HelperAbi, HelperDescriptor},
    hir::*,
    ids::*,
    lowering::lower,
    passes::{self, PassConfig},
    runtime::compile::{
        compile_cpu_cfg_region, CodeDependency, CodeMapping, CompileRequest, ImmutableCodeSnapshot,
        IrConfig, PublicationKey, Tier,
    },
    state::{ResumeKind, StateMap},
    types::Type,
    verify::verify,
};

struct Fixture {
    region: Region,
    preheader: BlockId,
    header: BlockId,
    body: BlockId,
    invariant: InstId,
    dependent: InstId,
    variant: InstId,
    read: InstId,
}
fn definition(region: &Region, value: ValueId) -> InstId {
    match region.values[value.index()].definition {
        Definition::Instruction(id, _) => id,
        _ => panic!("expected instruction definition"),
    }
}
fn loop_fixture(audit: bool, reverse_ids: bool) -> Fixture {
    let mut b = IntegerBuilder::new();
    let input = b.gpr;
    let preheader = b.block;
    // The body may have a smaller arena ID than its dominating header.
    let first = b.region.block(false);
    let second = b.region.block(false);
    let (header, body) = if reverse_ids { (second, first) } else { (first, second) };
    let out = b.region.block(false);
    let x = b.region.param(header, Type::I32);
    let n = b.region.param(header, Type::I32);
    let count = b.region.param(header, Type::I32);
    let effect = b.region.param(header, Type::Effect);
    let body_effect = b.region.param(body, Type::Effect);
    let zero = b.constant(0, Type::I32);
    let one = b.constant(1, Type::I32);
    b.region.terminate(
        preheader,
        Terminator::Branch(Edge {
            target: header,
            args: vec![input[0], input[1], zero, b.effect],
        }),
    );
    b.gpr[0] = x;
    b.gpr[1] = n;
    let before = b.region.state(StateMap {
        instruction_pc: GuestEip(0x8000),
        next_pc: GuestEip(0x8001),
        next_value: None,
        resume: ResumeKind::BeforeInstruction,
        gpr: b.gpr,
        flags: b.flags.clone(),
        xmm: vec![],
        x87: vec![],
        committed_instructions: 0,
        count_base: Some(count),
        rep_progress: None,
    });
    for block in [header, body, out] {
        b.region.blocks[block.index()].entry_state = Some(before);
    }
    b.block = header;
    let invariant_value = b.binary(Binary::Add, input[2], input[3]);
    let invariant = definition(&b.region, invariant_value);
    let done = b.binary(Binary::Eq, n, zero);
    b.region.terminate(
        header,
        Terminator::CondBranch {
            condition: done,
            taken: Edge {
                target: out,
                args: vec![],
            },
            not_taken: Edge {
                target: body,
                args: vec![effect],
            },
        },
    );
    b.block = body;
    // CPU reads are pinned initialization operations in the existing entry ABI.
    let read = definition(&b.region, input[7]);
    let dependent_value = b.binary(Binary::Xor, invariant_value, input[6]);
    let dependent = definition(&b.region, dependent_value);
    let next_x = b.binary(Binary::Add, x, dependent_value);
    let variant = definition(&b.region, next_x);
    let effect = if audit {
        b.region.helpers.push(HelperDescriptor {
            name: "licm_audit".into(),
            params: vec![Type::I32],
            results: vec![],
            effects: Effects::conservative(),
            exception_owner: ExceptionOwner::Caller,
            abi: HelperAbi::Outcome {
                fault_delivery: Some("licm_fault".into()),
                normal_preserves_state: true,
            },
        });
        b.region.append(
            body,
            Op::CallHelper(HelperId(0)),
            vec![next_x, body_effect],
            &[Type::Effect],
            Some(before),
        )[0]
    } else {
        body_effect
    };
    let next_n = b.binary(Binary::Sub, n, one);
    let next_count = b.binary(Binary::Add, count, one);
    b.region.terminate(
        body,
        Terminator::Branch(Edge {
            target: header,
            args: vec![next_x, next_n, next_count, effect],
        }),
    );
    let mut after = b.region.states[before.index()].clone();
    after.resume = ResumeKind::AfterInstruction;
    let after = b.region.state(after);
    b.region.terminate(out, Terminator::Exit(after));
    verify(&b.region).unwrap();
    Fixture {
        region: b.region,
        preheader,
        header,
        body,
        invariant,
        dependent,
        variant,
        read,
    }
}
fn only_licm() -> PassConfig {
    PassConfig {
        prune: false,
        merge: false,
        phis: false,
        fold: false,
        gvn: false,
        dce: false,
        simplify: false,
        simplify_work_limit: crate::ir::passes::simplify::DEFAULT_WORK_LIMIT,
        licm: true,
        licm_work_limit: DEFAULT_WORK_LIMIT,
        rounds: 1,
    }
}
#[test]
fn moves_dependency_order_not_arena_order_and_preserves_observers() {
    for reversed in [false, true] {
        for audit in [false, true] {
            let mut f = loop_fixture(audit, reversed);
            let before = f.region.clone();
            let stats = run(&mut f.region, DEFAULT_WORK_LIMIT).unwrap();
            assert_eq!((stats.loops, stats.hoisted), (1, 2));
            assert_eq!(
                f.region.instructions[f.invariant.index()].block,
                f.preheader
            );
            assert_eq!(
                f.region.instructions[f.dependent.index()].block,
                f.preheader
            );
            assert_eq!(f.region.instructions[f.variant.index()].block, f.body);
            assert_eq!(f.region.instructions[f.read.index()].block, f.preheader);
            let ids = &f.region.blocks[f.preheader.index()].instructions;
            assert_eq!(&ids[ids.len() - 2..], &[f.invariant, f.dependent]);
            assert_eq!(
                format!("{:?}", f.region.states),
                format!("{:?}", before.states)
            );
            for (a, b) in f.region.blocks.iter().zip(&before.blocks) {
                assert_eq!(format!("{:?}", a.terminator), format!("{:?}", b.terminator));
            }
            for (index, inst) in before.instructions.iter().enumerate() {
                if inst.op.ordered() || inst.state.is_some() {
                    assert_eq!(
                        format!("{:?}", f.region.instructions[index]),
                        format!("{inst:?}")
                    );
                }
            }
            verify(&f.region).unwrap();
            assert_eq!(run(&mut f.region, DEFAULT_WORK_LIMIT).unwrap().hoisted, 0);
        }
    }
}
#[test]
fn disabled_pass_and_budget_failure_are_nonmutating() {
    let original = loop_fixture(false, false).region;
    let mut region = original.clone();
    let stats = passes::run(
        &mut region,
        PassConfig {
            simplify: false,
            simplify_work_limit: crate::ir::passes::simplify::DEFAULT_WORK_LIMIT,
            licm: false,
            ..only_licm()
        },
    )
    .unwrap();
    assert_eq!((stats.loops, stats.hoisted, stats.licm_work), (0, 0, 0));
    assert_eq!(format!("{region:?}"), format!("{original:?}"));
    let work = run(&mut region, DEFAULT_WORK_LIMIT).unwrap().work;
    assert!(work > 10);
    // Include budgets which fail after instructions have already moved in staging.
    for limit in [0, 1, work / 2, work - 1] {
        let mut region = original.clone();
        assert!(run(&mut region, limit).unwrap_err().contains("budget"));
        assert_eq!(format!("{region:?}"), format!("{original:?}"));
    }
    let mut invalid = original.clone();
    invalid.instructions[0].block = BlockId(u32::MAX);
    let unchanged = format!("{invalid:?}");
    assert!(run(&mut invalid, DEFAULT_WORK_LIMIT).is_err());
    assert_eq!(format!("{invalid:?}"), unchanged);
    let mut oversized = original;
    oversized.blocks.resize_with(65, Block::default);
    assert!(run(&mut oversized, DEFAULT_WORK_LIMIT)
        .unwrap_err()
        .contains("region budget"));
}
#[test]
fn conditional_preheader_is_not_speculated() {
    let mut f = loop_fixture(false, false);
    let term = f.region.blocks[f.preheader.index()]
        .terminator
        .take()
        .unwrap();
    let Terminator::Branch(edge) = term else {
        panic!()
    };
    let condition = f.region.states[0].flags.arithmetic[0];
    // Duplicate edges still do not constitute an unconditional preheader. CFG
    // simplification may prove it later, but LICM itself must not rewrite edges.
    f.region.terminate(
        f.preheader,
        Terminator::CondBranch {
            condition,
            taken: edge.clone(),
            not_taken: edge,
        },
    );
    let unchanged = format!("{:?}", f.region);
    assert_eq!(run(&mut f.region, DEFAULT_WORK_LIMIT).unwrap().hoisted, 0);
    assert_eq!(format!("{:?}", f.region), unchanged);
}
fn cyclic_graph(entries: &[usize], edges: &[Vec<usize>]) -> Region {
    let mut r = Region::default();
    for b in 0..edges.len() {
        r.block(entries.contains(&b));
    }
    for (b, successors) in edges.iter().enumerate() {
        let block = BlockId(b as u32);
        // Eligible constants should move only when the graph itself is safe.
        let condition = r.append(block, Op::Const(1), vec![], &[Type::I1], None)[0];
        let edge = |target: usize| Edge {
            target: BlockId(target as u32),
            args: vec![],
        };
        r.terminate(
            block,
            if successors.len() == 1 {
                Terminator::Branch(edge(successors[0]))
            } else {
                Terminator::CondBranch {
                    condition,
                    taken: edge(successors[0]),
                    not_taken: edge(successors[1]),
                }
            },
        );
    }
    verify(&r).unwrap();
    r
}
#[test]
fn external_entries_and_irreducible_cycles_are_not_hoisted() {
    for mut r in [
        cyclic_graph(&[0], &[vec![0]]),
        cyclic_graph(&[0, 1], &[vec![1], vec![2], vec![1]]),
        cyclic_graph(&[0], &[vec![1, 2], vec![2], vec![1]]),
    ] {
        let before = format!("{r:?}");
        assert_eq!(run(&mut r, DEFAULT_WORK_LIMIT).unwrap().hoisted, 0);
        assert_eq!(format!("{r:?}"), before);
    }
}
#[test]
fn union_of_latches_and_nested_loops_is_stable() {
    let mut multiple = cyclic_graph(&[0], &[vec![1], vec![2, 3], vec![1], vec![1]]);
    let stats = run(&mut multiple, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!((stats.loops, stats.hoisted), (1, 3));
    // 1 is the outer header; 2 is the inner header with preheader 1.
    let mut nested = cyclic_graph(&[0], &[vec![1], vec![2], vec![3, 4], vec![2], vec![1]]);
    let stats = run(&mut nested, DEFAULT_WORK_LIMIT).unwrap();
    assert_eq!((stats.loops, stats.hoisted), (2, 6));
    verify(&multiple).unwrap();
    verify(&nested).unwrap();
}
#[test]
fn memory_helpers_and_cpu_state_are_never_eligible() {
    let f = loop_fixture(false, false);
    let template = f.region.instructions[f.invariant.index()].clone();
    let forbidden = [
        Op::ReadGpr(0),
        Op::ReadXmm(0),
        Op::ReadFlags,
        Op::ReadRawFlags,
        Op::ReadFlagChanges,
        Op::ReadFlagOperand,
        Op::ReadStack32,
        Op::ReadSegment(3),
        Op::GuestLoad { bytes: 4 },
        Op::GuestStore { bytes: 4 },
        Op::RmwLoad {
            bytes: 4,
            order: RmwOrder::Locked,
        },
        Op::RmwStore {
            bytes: 4,
            order: RmwOrder::Locked,
        },
        Op::GuestCheck {
            bytes: 4,
            write: false,
        },
        Op::SegmentAddress { segment: 3 },
        Op::Divide {
            bits: 32,
            signed: false,
        },
        Op::CallHelper(HelperId(0)),
        Op::SseCheck,
        Op::PollBudget,
        Op::XmmLoad {
            bytes: 16,
            register: 0,
        },
        Op::XmmStore {
            bytes: 16,
            lane: 0,
            register: 0,
        },
    ];
    for op in forbidden {
        let inst = Instruction {
            op,
            ..template.clone()
        };
        assert!(!super::eligible(&inst), "{:?}", inst.op);
    }
    for flag in 0..4 {
        let mut inst = template.clone();
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
fn compiler_pipeline_and_switch_generate_cpu_fixtures() {
    std::fs::create_dir_all("build/ir-licm").unwrap();
    for audit in [false, true] {
        for opt in [false, true] {
            let mut fixture = loop_fixture(audit, true);
            if opt {
                let stats = passes::run(&mut fixture.region, only_licm()).unwrap();
                assert_eq!(stats.hoisted, 2);
                assert_eq!(stats.loops, 1);
                assert!(stats.licm_work > 0);
            }
            let mir = lower(&fixture.region).unwrap();
            for budget in [1, 2, 3, 4, 5, 9, 16, 100] {
                std::fs::write(
                    format!("build/ir-licm/{audit}-{opt}-{budget}.wasm"),
                    emit_cpu(&mir, budget).unwrap().bytes,
                )
                .unwrap();
            }
        }
    }
    let request = CompileRequest {
        key: PublicationKey {
            job: 1,
            vm_generation: 1,
            slot: 0,
            slot_generation: 1,
        },
        pc: GuestEip(0x1000),
        linear: LinearAddress(0x1000),
        default_32: true,
        tier: Tier::Two,
    };
    let snapshot = ImmutableCodeSnapshot {
        // mov ecx,3; loop: lea eax,[esi+edi*4+123]; dec ecx; jnz loop
        bytes: vec![0xB9, 3, 0, 0, 0, 0x8D, 0x44, 0xBE, 123, 0x49, 0x75, 0xF9],
        dependencies: vec![CodeDependency {
            page: PhysicalAddress(0x1000),
            version: 1,
        }],
        mappings: vec![CodeMapping {
            linear: LinearAddress(0x1000),
            physical: PhysicalAddress(0x1000),
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
    let artifact = compile_cpu_cfg_region(&request, &snapshot, &config).unwrap();
    assert!(artifact.passes.hoisted > 0, "live CPU compiler uses LICM");
    let tier_one = CompileRequest { tier: Tier::One, ..request };
    assert_eq!(
        compile_cpu_cfg_region(&tier_one, &snapshot, &config).unwrap().passes.hoisted,
        0,
        "Tier 1 skips regional LICM even when explicitly enabled"
    );
    config.passes.licm = false;
    assert_eq!(
        compile_cpu_cfg_region(&request, &snapshot, &config)
            .unwrap()
            .passes
            .hoisted,
        0
    );
}
