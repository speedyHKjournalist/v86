use super::{run, Config};
use crate::ir::{
    backend::wasm::{emit, StateLayout},
    effects::Effects,
    frontend::{decode::GuestEip, integer::IntegerBuilder},
    helper::{ExceptionOwner, HelperAbi, HelperDescriptor},
    hir::*,
    ids::*,
    lowering::lower,
    simd::PackedOp,
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
fn definition(r: &Region, v: ValueId) -> InstId {
    match r.values[v.index()].definition {
        Definition::Instruction(id, _) => id,
        _ => panic!("expected instruction"),
    }
}
fn fixture() -> (Region, BlockId, BlockId, BlockId, InstId, InstId, InstId) {
    let mut b = IntegerBuilder::new();
    let entry = b.block;
    let input = b.gpr;
    let flags = b.flags.clone();
    let exit = b.region.block(false);
    // Body allocated before its dominator, requiring producer-first scheduling.
    let body = b.region.block(false);
    let header = b.region.block(false);
    let he = b.region.param(header, Type::Effect);
    let count = b.region.param(header, Type::I32);
    let sum = b.region.param(header, Type::I32);
    let be = b.region.param(body, Type::Effect);
    b.region.param(exit, Type::Effect);
    let final_count = b.region.param(exit, Type::I32);
    let final_sum = b.region.param(exit, Type::I32);
    b.region.terminate(
        entry,
        Terminator::Branch(Edge {
            target: header,
            args: vec![b.effect, input[0], input[3]],
        }),
    );
    let mut current = input;
    current[0] = count;
    current[3] = sum;
    for block in [header, body] {
        let s = snapshot(&mut b.region, current, &flags, 0x8000 + block.0);
        b.region.blocks[block.index()].entry_state = Some(s);
    }
    b.block = header;
    let zero = b.constant(0, Type::I32);
    let done = b.binary(Binary::Eq, count, zero);
    let invariant = b.binary(Binary::Add, input[1], input[2]);
    b.region.terminate(
        header,
        Terminator::CondBranch {
            condition: done,
            taken: Edge {
                target: exit,
                args: vec![he, count, sum],
            },
            not_taken: Edge {
                target: body,
                args: vec![he],
            },
        },
    );
    b.block = body;
    let three = b.constant(3, Type::I32);
    let chain = b.binary(Binary::Mul, invariant, three);
    let mut observed = current;
    observed[4] = chain;
    let before = snapshot(&mut b.region, observed, &flags, 0x9000);
    b.region.helpers.push(HelperDescriptor {
        name: "observe_iteration".into(),
        params: vec![Type::I32, Type::I32],
        results: vec![Type::I32],
        effects: Effects::conservative(),
        exception_owner: ExceptionOwner::Caller,
        abi: HelperAbi::Outcome {
            fault_delivery: Some("deliver_fault".into()),
            normal_preserves_state: true,
        },
    });
    let call = b.region.append(
        body,
        Op::CallHelper(HelperId(0)),
        vec![count, chain, be],
        &[Type::I32, Type::Effect],
        Some(before),
    );
    let dependent = b.binary(Binary::Xor, call[0], chain);
    observed[5] = dependent;
    let after = snapshot(&mut b.region, observed, &flags, 0x9001);
    let effect = b.region.append(
        body,
        Op::PollBudget,
        vec![call[1]],
        &[Type::Effect],
        Some(after),
    )[0];
    let one = b.constant(1, Type::I32);
    let next_count = b.binary(Binary::Sub, count, one);
    let next_sum = b.binary(Binary::Add, sum, chain);
    b.region.terminate(
        body,
        Terminator::Branch(Edge {
            target: header,
            args: vec![effect, next_count, next_sum],
        }),
    );
    let mut final_gpr = input;
    final_gpr[0] = final_count;
    final_gpr[3] = final_sum;
    let end = snapshot(&mut b.region, final_gpr, &flags, 0xA000);
    b.region.blocks[exit.index()].entry_state = Some(end);
    b.region.terminate(exit, Terminator::Exit(end));
    let ids = (
        definition(&b.region, chain),
        definition(&b.region, call[0]),
        definition(&b.region, dependent),
    );
    verify(&b.region).unwrap();
    (b.region, entry, header, body, ids.0, ids.1, ids.2)
}
#[test]
fn licm_keeps_faulting_helpers_and_their_data_dependencies_in_loop() {
    let (mut r, entry, _, body, invariant, call, dependent) = fixture();
    let effects: Vec<_> = r
        .instructions
        .iter()
        .filter(|i| i.op.ordered())
        .map(|i| format!("{i:?}"))
        .collect();
    let states = format!("{:?}", r.states);
    assert!(run(&mut r, Config::default()).unwrap().hoisted > 0);
    assert_eq!(r.instructions[invariant.index()].block, entry);
    assert_eq!(r.instructions[call.index()].block, body);
    assert_eq!(r.instructions[dependent.index()].block, body);
    assert_eq!(
        r.instructions
            .iter()
            .filter(|i| i.op.ordered())
            .map(|i| format!("{i:?}"))
            .collect::<Vec<_>>(),
        effects
    );
    assert_eq!(format!("{:?}", r.states), states);
    lower(&r).unwrap();
}
#[test]
fn licm_hoists_pure_vector_dataflow_without_moving_xmm_reads() {
    let (mut r, entry, header, _, _, _, _) = fixture();
    let term = r.blocks[entry.index()].terminator.take().unwrap();
    let a = r.append(entry, Op::ReadXmm(0), vec![], &[Type::V128], None)[0];
    let b = r.append(entry, Op::ReadXmm(1), vec![], &[Type::V128], None)[0];
    r.terminate(entry, term);
    let term = r.blocks[header.index()].terminator.take().unwrap();
    let v = r.append(
        header,
        Op::VectorBinary(PackedOp::Add32),
        vec![a, b],
        &[Type::V128],
        None,
    )[0];
    let id = definition(&r, v);
    r.terminate(header, term);
    run(&mut r, Config::default()).unwrap();
    assert_eq!(r.instructions[id.index()].block, entry);
    assert_eq!(r.instructions[definition(&r, a).index()].op, Op::ReadXmm(0));
    lower(&r).unwrap();
}
#[test]
fn licm_observation_wasm_fixtures() {
    std::fs::create_dir_all("build/ir-licm").unwrap();
    let layout = StateLayout {
        gpr: 0,
        flags: 32,
        eip: 36,
        committed: 40,
        flag_operand: 44,
    };
    for budget in [1, 2, 3, 5, 16, 128] {
        for optimize in [false, true] {
            let (mut region, _, _, _, _, _, _) = fixture();
            if optimize {
                assert!(run(&mut region, Config::default()).unwrap().hoisted > 0);
            }
            let mir = lower(&region).unwrap();
            drop(region);
            let artifact = emit(&mir, layout, budget).unwrap();
            std::fs::write(
                format!("build/ir-licm/observed-{budget}-{optimize}.wasm"),
                artifact.bytes,
            )
            .unwrap();
        }
    }
}
