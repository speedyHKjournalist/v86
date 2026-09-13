use crate::ir::{
    backend::wasm::{emit, StateLayout},
    effects::Effects,
    frontend::decode::GuestEip,
    helper::*,
    hir::*,
    ids::*,
    lowering::lower,
    passes::{run, PassConfig},
    state::*,
    types::Type,
};
pub(super) fn fixture(owner: ExceptionOwner) -> Region {
    let mut r = Region::default();
    let b = r.block(true);
    let effect = r.param(b, Type::Effect);
    let x = r.append(b, Op::Const(77), vec![], &[Type::I32], None)[0];
    let zero = r.append(b, Op::Const(0), vec![], &[Type::I32], None)[0];
    let flag = r.append(b, Op::Const(1), vec![], &[Type::I1], None)[0];
    let before = r.state(StateMap {
        instruction_pc: GuestEip(0x1000),
        next_pc: GuestEip(0x1002),
        next_value: None,
        resume: ResumeKind::BeforeInstruction,
        gpr: [x, zero, zero, zero, zero, zero, zero, zero],
        flags: FlagState {
            arithmetic: [flag; 6],
            system: zero,
            last_op1: None,
            raw_zero: None,
            zero_is_lazy: None,
        },
        xmm: vec![],
        x87: vec![],
        committed_instructions: 7,
        count_base: None,
        rep_progress: None,
    });
    r.helpers.push(HelperDescriptor {
        name: "audited_operation".into(),
        params: vec![Type::I32],
        results: vec![Type::I32, Type::I8],
        effects: if owner == ExceptionOwner::CannotFault {
            Effects::pure()
        } else {
            Effects::conservative()
        },
        exception_owner: owner,
        abi: HelperAbi::Outcome {
            fault_delivery: (owner == ExceptionOwner::Caller).then(|| "deliver_fault".into()),
            normal_preserves_state: true,
        },
    });
    let values = r.append(
        b,
        Op::CallHelper(HelperId(0)),
        vec![x, effect],
        &[Type::I32, Type::I8, Type::Effect],
        Some(before),
    );
    let byte = r.append(
        b,
        Op::Extend { signed: false },
        vec![values[1]],
        &[Type::I32],
        None,
    )[0];
    let mut after = r.states[before.index()].clone();
    after.gpr[0] = values[0];
    after.gpr[1] = byte;
    after.resume = ResumeKind::AfterInstruction;
    after.committed_instructions = 8;
    let after = r.state(after);
    r.terminate(b, Terminator::Exit(after));
    r
}
#[test]
fn helper_outcome_modules_and_snapshot_slot_reuse() {
    std::fs::create_dir_all("build/ir-helpers").unwrap();
    for (name, owner) in [
        ("caller", ExceptionOwner::Caller),
        ("helper", ExceptionOwner::Helper),
        ("pure", ExceptionOwner::CannotFault),
    ] {
        let mut r = fixture(owner);
        for opt in [0, 1] {
            if opt == 1 {
                run(&mut r, PassConfig::default()).unwrap();
            }
            let mir = lower(&r).unwrap();
            let call = r
                .instructions
                .iter()
                .find(|i| matches!(i.op, Op::CallHelper(_)))
                .unwrap();
            let snapshot = &r.states[call.state.unwrap().index()];
            assert!(
                call.results[..2].iter().any(|result| snapshot
                    .values()
                    .iter()
                    .any(|source| mir.allocation.value_local[result.index()]
                        == mir.allocation.value_local[source.index()])),
                "exercise result/snapshot slot reuse across faulting call"
            );
            let artifact = emit(
                &mir,
                StateLayout {
                    gpr: 0,
                    flags: 32,
                    eip: 36,
                    committed: 40,
                    flag_operand: 44,
                },
                100,
            )
            .unwrap();
            assert!(
                artifact.locals >= mir.allocation.local_types.len() + 5,
                "local metrics include dispatcher, outcome and staged return values"
            );
            std::fs::write(
                format!("build/ir-helpers/{name}-{opt}.wasm"),
                artifact.bytes,
            )
            .unwrap();
        }
    }
}
#[test]
fn helper_legalization_requires_audited_abi_and_complete_materialization() {
    let mut r = fixture(ExceptionOwner::Caller);
    r.states[0].resume = ResumeKind::AfterInstruction;
    assert!(lower(&r).is_err(), "faulting calls cannot restore next EIP");
    let mut r = fixture(ExceptionOwner::Caller);
    r.helpers[0].abi = HelperAbi::Unadapted;
    assert!(
        lower(&r).err().unwrap()
            == crate::ir::lowering::CompileError::Unsupported("unadapted helper ABI")
    );
    let mut r = fixture(ExceptionOwner::Caller);
    r.helpers[0].abi = HelperAbi::Outcome {
        fault_delivery: None,
        normal_preserves_state: true,
    };
    assert!(lower(&r).is_err());
    let mut r = fixture(ExceptionOwner::Helper);
    r.helpers[0].abi = HelperAbi::Outcome {
        fault_delivery: None,
        normal_preserves_state: false,
    };
    assert!(lower(&r).is_err());
    let mut r = fixture(ExceptionOwner::Caller);
    r.helpers[0].name = "deliver_fault".into();
    assert!(
        lower(&r).is_err(),
        "conflicting ABI must be a compile error, not a builder panic"
    );
    let mut r = fixture(ExceptionOwner::Caller);
    let map = &mut r.states[0];
    map.resume = ResumeKind::RepProgress;
    map.rep_progress = Some([map.gpr[1], map.gpr[6], map.gpr[7]]);
    assert!(
        lower(&r).is_ok(),
        "aliased REP GPR progress has complete materialization"
    );
    r.states[0].rep_progress.as_mut().unwrap()[0] = r.states[0].gpr[0];
    assert!(
        lower(&r).is_err(),
        "never silently omit distinct REP values"
    );
}
