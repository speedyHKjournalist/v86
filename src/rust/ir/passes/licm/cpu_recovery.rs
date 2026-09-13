//! Bounded inputs and actual CPU-ABI execution, including typed recovery values.
use crate::ir::passes::licm::{run, Config};
use crate::ir::{
    backend::wasm::emit_cpu,
    frontend::{decode::GuestEip, integer::IntegerBuilder},
    helper::{HelperAbi, HelperDescriptor},
    hir::*,
    ids::*,
    lowering::lower,
    simd::PackedOp,
    state::{ResumeKind, StateMap},
    types::Type,
    verify::verify,
};

fn state(b: &mut IntegerBuilder, pc: u32, count: ValueId, vector: Option<ValueId>) -> StateId {
    b.region.state(StateMap {
        instruction_pc: GuestEip(pc),
        next_pc: GuestEip(pc + 1),
        next_value: None,
        resume: ResumeKind::BeforeInstruction,
        gpr: b.gpr,
        flags: b.flags.clone(),
        xmm: vector.map_or_else(Vec::new, |v| vec![v; 8]),
        x87: vec![],
        committed_instructions: 0,
        count_base: Some(count),
        rep_progress: None,
    })
}

fn typed_loop(vector: bool, poll: bool) -> (Region, [ValueId; 3]) {
    let mut b = IntegerBuilder::new();
    let input = b.gpr;
    let entry = b.block;
    let zero32 = b.constant(0, Type::I32);
    let one = b.constant(1, Type::I32);
    let ty = if vector { Type::V128 } else { Type::I64 };
    let (zero, a, c, factor) = if vector {
        let read = b.node(Op::ReadXmm(0), vec![], ty);
        let zero = b.node(Op::VectorBinary(PackedOp::Xor), vec![read, read], ty);
        let a = b.node(
            Op::VectorReplace { bits: 32, lane: 0 },
            vec![zero, input[2]],
            ty,
        );
        let c = b.node(
            Op::VectorReplace { bits: 32, lane: 0 },
            vec![zero, input[3]],
            ty,
        );
        (zero, a, c, zero)
    } else {
        let zero = b.node(Op::Const(0), vec![], ty);
        let a = b.node(Op::Extend { signed: false }, vec![input[2]], ty);
        let c = b.node(Op::Extend { signed: false }, vec![input[3]], ty);
        let factor = b.node(Op::Extend { signed: false }, vec![input[4]], ty);
        (zero, a, c, factor)
    };
    // The use block precedes its dominating definition in arena allocation order.
    let body = b.region.block(false);
    let header = b.region.block(false);
    let exit = b.region.block(false);
    let n = b.region.param(header, Type::I32);
    let acc = b.region.param(header, ty);
    let low = b.region.param(header, Type::I32);
    let high = b.region.param(header, Type::I32);
    let completed = b.region.param(header, Type::I32);
    let he = b.region.param(header, Type::Effect);
    let be = b.region.param(body, Type::Effect);
    b.region.param(exit, Type::Effect);
    b.region.terminate(
        entry,
        Terminator::Branch(Edge {
            target: header,
            args: vec![input[0], zero, zero32, zero32, zero32, b.effect],
        }),
    );
    b.gpr[0] = n;
    b.gpr[1] = low;
    b.gpr[3] = high;
    for block in [header, body, exit] {
        let map = state(&mut b, 0x4000 + block.0, completed, vector.then_some(acc));
        b.region.blocks[block.index()].entry_state = Some(map);
    }
    b.block = header;
    let sum = if vector {
        b.node(Op::VectorBinary(PackedOp::Add32), vec![a, c], ty)
    } else {
        let sum = b.binary(Binary::Add, a, c);
        b.binary(Binary::Mul, sum, factor)
    };
    let done = b.binary(Binary::Eq, n, zero32);
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
    let dependent = if vector {
        b.node(
            Op::VectorShuffle([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]),
            vec![sum, sum],
            ty,
        )
    } else {
        b.binary(Binary::Xor, sum, zero)
    };
    let effect = if poll {
        b.region.append(
            body,
            Op::PollBudget,
            vec![be],
            &[Type::Effect],
            b.region.blocks[body.index()].entry_state,
        )[0]
    } else {
        be
    };
    let next = if vector {
        b.node(Op::VectorBinary(PackedOp::Add32), vec![acc, dependent], ty)
    } else {
        b.binary(Binary::Add, acc, dependent)
    };
    let (next_low, next_high) = if vector {
        (
            b.node(
                Op::VectorExtract { bits: 32, lane: 0 },
                vec![next],
                Type::I32,
            ),
            b.node(
                Op::VectorExtract { bits: 32, lane: 1 },
                vec![next],
                Type::I32,
            ),
        )
    } else {
        let low = b.node(Op::Truncate, vec![next], Type::I32);
        let shift = b.node(Op::Const(32), vec![], Type::I64);
        let high = b.binary(Binary::Shr, next, shift);
        (low, b.node(Op::Truncate, vec![high], Type::I32))
    };
    let next_n = b.binary(Binary::Sub, n, one);
    let next_count = b.binary(Binary::Add, completed, one);
    b.region.terminate(
        body,
        Terminator::Branch(Edge {
            target: header,
            args: vec![next_n, next, next_low, next_high, next_count, effect],
        }),
    );
    let after = state(&mut b, 0x5000, completed, vector.then_some(acc));
    b.region.terminate(exit, Terminator::Exit(after));
    verify(&b.region).unwrap();
    (b.region, [sum, dependent, next])
}
fn owner(r: &Region, value: ValueId) -> BlockId {
    let Definition::Instruction(id, _) = r.values[value.index()].definition else {
        panic!()
    };
    r.instructions[id.index()].block
}

#[test]
fn typed_cpu_recovery_corpus() {
    std::fs::create_dir_all("build/ir-licm-cpu").unwrap();
    for vector in [false, true] {
        for poll in [false, true] {
            for opt in [false, true] {
                let (mut region, [sum, dependent, variant]) = typed_loop(vector, poll);
                let before = format!("{:?}", region.states);
                if opt {
                    assert!(run(&mut region, Config::default()).unwrap().hoisted >= 2);
                    assert_eq!(owner(&region, sum), BlockId(0));
                    assert_eq!(owner(&region, dependent), BlockId(0));
                    assert_eq!(owner(&region, variant), BlockId(1));
                    assert_eq!(format!("{:?}", region.states), before);
                }
                let mir = lower(&region).unwrap();
                drop(region);
                for budget in [1, 2, 3, 4, 5, 8, 17, 100] {
                    std::fs::write(
                        format!("build/ir-licm-cpu/{vector}-{poll}-{budget}-{opt}.wasm"),
                        emit_cpu(&mir, budget).unwrap().bytes,
                    )
                    .unwrap();
                }
            }
        }
    }
}

#[test]
fn variable_metadata_is_bounded_before_verification_or_cloning() {
    for kind in 0..9 {
        let (mut region, _) = typed_loop(false, false);
        let value = region.instructions[0].results[0];
        match kind {
            0 => region.instructions[0].args = vec![value; 131_073],
            1 => region.instructions[0].results = vec![value; 131_073],
            2 => region.blocks[0].params = vec![value; 131_073],
            3 => region.blocks[0].instructions = vec![InstId(0); 131_073],
            4 => {
                if let Some(Terminator::Branch(edge)) = &mut region.blocks[0].terminator {
                    edge.args = vec![value; 131_073];
                }
            },
            5 => region.states[0].xmm = vec![value; 131_073],
            6 => region.states[0].x87 = vec![value; 131_073],
            7 => region.helpers.push(HelperDescriptor::conservative(
                "x".repeat(131_073),
                vec![],
                vec![],
            )),
            _ => {
                let mut helper = HelperDescriptor::conservative("observer".into(), vec![], vec![]);
                helper.abi = HelperAbi::Outcome {
                    fault_delivery: Some("x".repeat(131_073)),
                    normal_preserves_state: false,
                };
                region.helpers.push(helper);
            },
        }
        let before = format!("{region:?}");
        assert_eq!(
            run(&mut region, Config::default()).unwrap_err(),
            "LICM metadata budget exceeded"
        );
        assert_eq!(format!("{region:?}"), before);
    }
    // Aggregate many small descriptors too; no individual vector is oversized.
    let (mut region, _) = typed_loop(false, false);
    for _ in 0..900 {
        region.helpers.push(HelperDescriptor::conservative(
            "observer".into(),
            vec![Type::I32; 160],
            vec![],
        ));
    }
    assert_eq!(
        run(&mut region, Config::default()).unwrap_err(),
        "LICM metadata budget exceeded"
    );
    let (mut region, _) = typed_loop(false, false);
    region.entries = vec![BlockId(0); 65];
    assert_eq!(
        run(&mut region, Config::default()).unwrap_err(),
        "LICM region budget exceeded"
    );
}

#[test]
fn entry_only_cpu_reads_remain_rejected_and_budget_failure_is_atomic() {
    let (mut region, _) = typed_loop(false, true);
    let full_work = run(&mut region.clone(), Config::default()).unwrap().work;
    let before = format!("{region:?}");
    assert!(run(
        &mut region,
        Config {
            max_work: full_work - 1,
            ..Config::default()
        }
    )
    .unwrap_err()
    .contains("work budget"));
    assert_eq!(format!("{region:?}"), before);
    let body = BlockId(1);
    let term = region.blocks[1].terminator.take().unwrap();
    region.append(body, Op::ReadGpr(0), vec![], &[Type::I32], None);
    region.terminate(body, term);
    let before = format!("{region:?}");
    assert!(run(&mut region, Config::default())
        .unwrap_err()
        .contains("outside entry"));
    assert_eq!(format!("{region:?}"), before);
}
