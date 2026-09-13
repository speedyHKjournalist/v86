use crate::ir::{
    backend::wasm::{emit, emit_cpu, StateLayout},
    frontend::{
        decode::{GuestEip, LinearAddress},
        lift::lift_cpu,
    },
    helper::ExceptionOwner,
    hir::Op,
    lowering::lower,
    mir::call::Observation,
    passes::{run, PassConfig},
    state::ResumeKind,
    types::Type,
};
fn standalone(
    m: &crate::ir::mir::MirRegion,
) -> Result<crate::ir::backend::wasm::Artifact, crate::ir::lowering::CompileError> {
    emit(
        m,
        StateLayout {
            gpr: 0,
            flags: 32,
            eip: 36,
            committed: 40,
            flag_operand: 44,
        },
        100,
    )
}
#[test]
fn call_sites_legalize_observation_staging_and_outcome_edges() {
    for owner in [
        ExceptionOwner::Caller,
        ExceptionOwner::Helper,
        ExceptionOwner::CannotFault,
    ] {
        for opt in [false, true] {
            let mut r = crate::ir::helper_tests::fixture(owner);
            if opt {
                run(&mut r, PassConfig::default()).unwrap();
            }
            let m = lower(&r).unwrap();
            let plan = m.calls.iter().flatten().next().unwrap();
            assert_eq!(plan.cpu_observation, Observation::DecodedNextPc);
            assert_eq!(plan.standalone_observation, Observation::CapturedState);
            assert_eq!(plan.normal, Some(0));
            let inst = r
                .instructions
                .iter()
                .find(|i| matches!(i.op, Op::CallHelper(_)))
                .unwrap();
            assert_eq!(
                plan.staged.iter().map(|r| r.value).collect::<Vec<_>>(),
                vec![inst.results[1], inst.results[0]]
            );
            assert_eq!(
                plan.staged.iter().map(|r| r.ty).collect::<Vec<_>>(),
                vec![Type::I8, Type::I32]
            );
            assert_eq!(plan.delivery.is_some(), owner == ExceptionOwner::Caller);
            if let Some(d) = &plan.delivery {
                assert_eq!(d.outcome, 1);
                assert_eq!(d.restore, plan.state);
                assert_eq!(d.name, "deliver_fault");
                assert!(d.signature.params.is_empty() && d.signature.results.is_empty());
            }
            assert_eq!(
                plan.exits,
                if owner == ExceptionOwner::CannotFault { vec![] } else { vec![2, 3, 4] }
            );
            standalone(&m).unwrap();
        }
    }
    let mut r = crate::ir::helper_tests::fixture(ExceptionOwner::CannotFault);
    r.states[0].resume = ResumeKind::AfterInstruction;
    let m = lower(&r).unwrap();
    assert_eq!(
        m.calls.iter().flatten().next().unwrap().cpu_observation,
        Observation::CapturedState
    );
    standalone(&m).unwrap();
    for (bytes, exits) in [
        (vec![0x0F, 0xA2], vec![2, 4]),
        (vec![0xF3, 0xA4], vec![2, 3, 4]),
    ] {
        let r = lift_cpu(&bytes, GuestEip(0), LinearAddress(0), true).unwrap();
        let m = lower(&r).unwrap();
        let p = m.calls.iter().flatten().next().unwrap();
        assert_eq!(p.normal, None);
        assert_eq!(p.exits, exits);
        assert!(p.staged.is_empty() && p.delivery.is_none());
        emit_cpu(&m, 100).unwrap();
    }
}
#[test]
fn stale_helper_tables_and_call_sites_are_rejected() {
    for mutation in 0..13 {
        let r = crate::ir::helper_tests::fixture(ExceptionOwner::Caller);
        let mut m = crate::ir::lowering::lower_draft(&r).unwrap();
        let p = m.calls.iter_mut().flatten().next().unwrap();
        match mutation {
            0 => p.cpu_observation = Observation::CapturedState,
            1 => p.standalone_observation = Observation::DecodedNextPc,
            2 => {
                p.args.clear();
            },
            3 => p.staged.reverse(),
            4 => p.staged[0].ty = Type::I64,
            5 => p.delivery.as_mut().unwrap().outcome = 0,
            6 => p.delivery = None,
            7 => p.exits.push(0),
            8 => p.normal = None,
            9 => m.helpers[0].as_mut().unwrap().name = "changed".into(),
            10 => m.helpers[0].as_mut().unwrap().signature.results.clear(),
            11 => m.helpers[0].as_mut().unwrap().cpu_exit = true,
            12 => m.helpers[0] = None,
            _ => unreachable!(),
        }
        assert!(m.finish().is_err(), "mutation {mutation}");
    }
    let r = crate::ir::helper_tests::fixture(ExceptionOwner::Caller);
    let mut m = crate::ir::lowering::lower_draft(&r).unwrap();
    m.calls.clear();
    assert!(m.finish().is_err());
    let mut m = crate::ir::lowering::lower_draft(&r).unwrap();
    m.helpers.clear();
    assert!(m.finish().is_err());
}
#[test]
fn arena_tombstones_do_not_require_live_calls() {
    let mut r = lift_cpu(&[0x0F, 0xA2], GuestEip(0), LinearAddress(0), true).unwrap();
    for b in &mut r.blocks {
        b.instructions
            .retain(|id| !matches!(r.instructions[id.index()].op, Op::CallHelper(_)));
    }
    let m = lower(&r).unwrap();
    assert!(m.helpers.iter().all(Option::is_none));
    assert!(m.calls.iter().all(Option::is_none));
    emit_cpu(&m, 100).unwrap();
}
