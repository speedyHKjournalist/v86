//! Mixed-width and vector lifetimes through hoisted chains and budget exits.
use super::{run, Config};
use crate::ir::{
    backend::wasm::emit_cpu,
    frontend::{decode::GuestEip, integer::IntegerBuilder},
    hir::*,
    ids::*,
    lowering::lower,
    simd::PackedOp,
    state::{FlagState, ResumeKind, StateMap},
    types::Type,
    verify::verify,
};

fn owner(region: &Region, value: ValueId) -> BlockId {
    match region.values[value.index()].definition {
        Definition::Parameter(block, _) => block,
        Definition::Instruction(id, _) => region.instructions[id.index()].block,
    }
}

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

fn typed_loop(vector: bool) -> (Region, ValueId, ValueId, ValueId) {
    let mut b = IntegerBuilder::new();
    let inputs = b.gpr;
    let flags = b.flags.clone();
    let entry = b.block;
    let scalar_zero = b.constant(0, Type::I32);
    let one = b.constant(1, Type::I32);
    let ty = if vector { Type::V128 } else { Type::I64 };
    let (zero, a, c, factor) = if vector {
        let read = b.node(Op::ReadXmm(0), vec![], Type::V128);
        let zero = b.node(Op::VectorBinary(PackedOp::Xor), vec![read, read], ty);
        let a = b.node(
            Op::VectorReplace { bits: 32, lane: 0 },
            vec![zero, inputs[2]],
            ty,
        );
        let c = b.node(
            Op::VectorReplace { bits: 32, lane: 0 },
            vec![zero, inputs[3]],
            ty,
        );
        (zero, a, c, zero)
    } else {
        let zero = b.node(Op::Const(0), vec![], ty);
        let a = b.node(Op::Extend { signed: false }, vec![inputs[2]], ty);
        let c = b.node(Op::Extend { signed: false }, vec![inputs[3]], ty);
        let factor = b.node(Op::Extend { signed: false }, vec![inputs[4]], ty);
        (zero, a, c, factor)
    };
    // The use block deliberately precedes its dominating definition in the arena.
    let body = b.region.block(false);
    let header = b.region.block(false);
    let exit = b.region.block(false);
    let count = b.region.param(header, Type::I32);
    let acc = b.region.param(header, ty);
    let low = b.region.param(header, Type::I32);
    let high = b.region.param(header, Type::I32);
    let completed = b.region.param(header, Type::I32);
    let effect = b.region.param(header, Type::Effect);
    let body_effect = b.region.param(body, Type::Effect);
    b.region.param(exit, Type::Effect);
    b.region.terminate(
        entry,
        Terminator::Branch(Edge {
            target: header,
            args: vec![
                inputs[0],
                zero,
                scalar_zero,
                scalar_zero,
                scalar_zero,
                b.effect,
            ],
        }),
    );
    b.gpr[0] = count;
    b.gpr[1] = low;
    b.gpr[3] = high;
    for block in [header, body, exit] {
        let state = snapshot(&mut b.region, b.gpr, &flags, 0x4000 + block.0);
        b.region.states[state.index()].count_base = Some(completed);
        if vector {
            b.region.states[state.index()].xmm = vec![acc; 8];
        }
        b.region.blocks[block.index()].entry_state = Some(state);
    }
    b.block = header;
    let sum = if vector {
        b.node(Op::VectorBinary(PackedOp::Add32), vec![a, c], ty)
    } else {
        let sum = b.binary(Binary::Add, a, c);
        b.binary(Binary::Mul, sum, factor)
    };
    let done = b.binary(Binary::Eq, count, scalar_zero);
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
    let dependent = if vector {
        b.node(
            Op::VectorShuffle([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]),
            vec![sum, sum],
            ty,
        )
    } else {
        b.binary(Binary::Xor, sum, zero)
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
        let shifted = b.binary(Binary::Shr, next, shift);
        (low, b.node(Op::Truncate, vec![shifted], Type::I32))
    };
    let next_count = b.binary(Binary::Sub, count, one);
    let next_completed = b.binary(Binary::Add, completed, one);
    b.region.terminate(
        body,
        Terminator::Branch(Edge {
            target: header,
            args: vec![
                next_count,
                next,
                next_low,
                next_high,
                next_completed,
                body_effect,
            ],
        }),
    );
    b.block = exit;
    let after = snapshot(&mut b.region, b.gpr, &flags, 0x5000);
    b.region.states[after.index()].count_base = Some(completed);
    if vector {
        b.region.states[after.index()].xmm = vec![acc; 8];
    }
    b.region.terminate(exit, Terminator::Exit(after));
    verify(&b.region).unwrap();
    (b.region, sum, dependent, next)
}

#[test]
fn wide_and_vector_chains_preserve_dominance_and_emit_owned_mir() {
    std::fs::create_dir_all("build/ir-licm").unwrap();
    for vector in [false, true] {
        for optimized in [false, true] {
            let (mut region, sum, dependent, variant) = typed_loop(vector);
            if optimized {
                assert!(run(&mut region, Config::default()).unwrap().hoisted >= 2);
                assert_eq!(owner(&region, sum), BlockId(0));
                assert_eq!(owner(&region, dependent), BlockId(0));
                assert_eq!(owner(&region, variant), BlockId(1));
            }
            let mir = lower(&region).unwrap();
            drop(region);
            for budget in [1, 2, 3, 4, 5, 8, 17, 100] {
                std::fs::write(
                    format!("build/ir-licm/typed-{vector}-{budget}-{optimized}.wasm"),
                    emit_cpu(&mir, budget).unwrap().bytes,
                )
                .unwrap();
            }
        }
    }
}

#[test]
fn entry_only_cpu_observations_remain_a_verifier_invariant() {
    let (mut region, _, _, _) = typed_loop(false);
    let body = BlockId(1);
    let term = region.blocks[body.index()].terminator.take().unwrap();
    region.append(body, Op::ReadGpr(0), vec![], &[Type::I32], None);
    region.terminate(body, term);
    let before = format!("{region:?}");
    assert!(run(&mut region, Config::default())
        .unwrap_err()
        .contains("outside entry"));
    assert_eq!(format!("{region:?}"), before);
}

#[test]
fn no_motion_preserves_arenas_and_exact_work_accounting() {
    // These graphs have either no eligible natural loop or no movable body
    // instructions. Both must remain exact no-ops.
    for external_header in [false, true] {
        let mut r = Region::default();
        let entry = r.block(true);
        let header = r.block(external_header);
        r.append(entry, Op::Const(7), vec![], &[Type::I32], None);
        r.terminate(
            entry,
            Terminator::Branch(Edge {
                target: header,
                args: vec![],
            }),
        );
        r.terminate(
            header,
            Terminator::Branch(Edge {
                target: header,
                args: vec![],
            }),
        );
        let before = format!("{r:?}");
        let instructions = r.instructions.as_ptr();
        let values = r.values.as_ptr();
        let stats = run(&mut r, Config::default()).unwrap();
        assert_eq!(stats.loops, usize::from(!external_header));
        assert_eq!(stats.hoisted, 0);
        assert!(stats.work > 0);
        assert_eq!(r.instructions.as_ptr(), instructions);
        assert_eq!(r.values.as_ptr(), values);
        assert_eq!(format!("{r:?}"), before);
        let exact = run(
            &mut r,
            Config {
                max_work: stats.work,
                ..Config::default()
            },
        )
        .unwrap();
        assert_eq!(exact, stats);
        assert!(run(
            &mut r,
            Config {
                max_work: stats.work - 1,
                ..Config::default()
            }
        )
        .is_err());
        assert_eq!(format!("{r:?}"), before);
    }
}
