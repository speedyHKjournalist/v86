use super::{plan, verify, Forwarding, DEFAULT_WORK_LIMIT};
use crate::ir::{
    backend::wasm::emit_cpu,
    frontend::{
        decode::{GuestEip, LinearAddress},
        lift::lift_cpu,
    },
    hir::{Op, Terminator},
    ids::*,
    lowering::{lower, lower_draft},
    mir::memory::NativeMemory,
    passes::{run, PassConfig},
    types::Type,
};

fn region(bytes: &[u8]) -> crate::ir::hir::Region {
    let mut r = lift_cpu(bytes, GuestEip(0x100000), LinearAddress(0x100000), true).unwrap();
    run(&mut r, PassConfig::default()).unwrap();
    r
}
fn loads(r: &crate::ir::hir::Region) -> Vec<InstId> {
    r.blocks
        .iter()
        .flat_map(|b| b.instructions.iter())
        .copied()
        .filter(|id| matches!(r.instructions[id.index()].op, Op::GuestLoad { .. }))
        .collect()
}

#[test]
fn repeated_reads_use_owned_machine_certificates_and_separate_cache_locals() {
    for bytes in [
        vec![0x8A, 0x06, 0x8A, 0x1E],
        vec![0x66, 0x8B, 0x06, 0x66, 0x8B, 0x1E],
        vec![0x8B, 0x06, 0x8B, 0x1E],
        vec![0x8B, 0x06, 0x8B, 0x1E, 0x8B, 0x16],
    ] {
        let r = region(&bytes);
        let ids = loads(&r);
        let mut m = lower(&r).unwrap();
        drop(r);
        let plain = emit_cpu(&m, 32).unwrap();
        assert_eq!(
            m.forward_ram_reads(DEFAULT_WORK_LIMIT).unwrap(),
            ids.len() - 1
        );
        assert_eq!(m.ram_forwarding(ids[0]), Some(Forwarding::Begin));
        for pair in ids.windows(2) {
            assert_eq!(
                m.ram_forwarding(pair[1]),
                Some(Forwarding::Reuse { previous: pair[0] })
            );
        }
        let optimized = emit_cpu(&m, 32).unwrap();
        assert!(
            optimized.locals >= plain.locals + 2,
            "validity and cached data outlive coalesced SSA slots"
        );
        assert_eq!(
            m.forward_ram_reads(DEFAULT_WORK_LIMIT).unwrap(),
            ids.len() - 1
        );
    }
}

#[test]
fn widths_addresses_segments_and_runtime_barriers_are_not_alias_guesses() {
    for bytes in [
        vec![0x8B, 0x06, 0x8A, 0x1E],             // different widths
        vec![0x8B, 0x06, 0x8B, 0x1F],             // ESI vs EDI
        vec![0x8B, 0x06, 0x64, 0x8B, 0x1E],       // DS vs FS
        vec![0x8B, 0x06, 0x46, 0x8B, 0x1E],       // changed offset
        vec![0x8B, 0x06, 0x8B, 0x17, 0x8B, 0x1E], // unrelated read may be MMIO
        vec![0x8B, 0x06, 0x01, 0x1E],             // RMW never reuses ordinary load
    ] {
        let r = region(&bytes);
        assert_eq!(
            lower(&r)
                .unwrap()
                .forward_ram_reads(DEFAULT_WORK_LIMIT)
                .unwrap(),
            0
        );
    }
    // Insert an ordered check/poll between the same-width, same-address reads.
    // A check remains a barrier. A one-unit PollBudget stays in place but its
    // continuation has no guest effect; its observation arm leaves the frame.
    for op in [
        Op::PollBudget,
        Op::GuestCheck {
            bytes: 4,
            write: false,
        },
    ] {
        let mut r = region(&[0x8B, 0x06, 0x8B, 0x1E]);
        let ids = loads(&r);
        let second = ids[1];
        let block = r.instructions[second.index()].block;
        let old_effect = *r.instructions[second.index()].args.last().unwrap();
        let state = r.instructions[second.index()].state;
        let term = r.blocks[block.index()].terminator.take().unwrap();
        let mut args = if matches!(op, Op::GuestCheck { .. }) {
            vec![r.instructions[second.index()].args[0]]
        } else {
            vec![]
        };
        args.push(old_effect);
        let new_effect = r.append(block, op, args, &[Type::Effect], state)[0];
        let added = r.blocks[block.index()].instructions.pop().unwrap();
        let at = r.blocks[block.index()]
            .instructions
            .iter()
            .position(|id| *id == second)
            .unwrap();
        r.blocks[block.index()].instructions.insert(at, added);
        *r.instructions[second.index()].args.last_mut().unwrap() = new_effect;
        r.terminate(block, term);
        crate::ir::verify::verify(&r).unwrap();
        let poll = r.instructions[added.index()].op == Op::PollBudget;
        assert_eq!(
            lower(&r)
                .unwrap()
                .forward_ram_reads(DEFAULT_WORK_LIMIT)
                .unwrap(),
            usize::from(poll)
        );
    }
}

#[test]
fn forwarding_failures_are_atomic_and_forged_certificates_are_rejected() {
    let r = region(&[0x8B, 0x06, 0x8B, 0x1E]);
    let ids = loads(&r);
    let mut m = lower(&r).unwrap();
    assert!(m.forward_ram_reads(0).is_err());
    assert!(!m.has_ram_forwarding());
    assert_eq!(m.forward_ram_reads(DEFAULT_WORK_LIMIT).unwrap(), 1);
    let before: Vec<_> = ids.iter().map(|id| m.ram_forwarding(*id)).collect();
    assert!(m.forward_ram_reads(1).is_err());
    assert_eq!(
        before,
        ids.iter()
            .map(|id| m.ram_forwarding(*id))
            .collect::<Vec<_>>()
    );
    for mutation in 0..4 {
        let mut draft = lower_draft(&r).unwrap();
        draft.ram_forwarding = plan(&draft, DEFAULT_WORK_LIMIT).unwrap();
        match mutation {
            0 => draft.ram_forwarding[ids[0].index()] = None,
            1 => {
                draft.ram_forwarding[ids[1].index()] = Some(Forwarding::Reuse { previous: ids[1] })
            },
            2 => draft.ram_forwarding[0] = Some(Forwarding::Begin),
            _ => {
                draft.ram_forwarding.pop();
            },
        }
        assert!(verify(&draft).is_err());
        assert!(draft.finish().is_err(), "forged certificate {mutation}");
    }
}

#[test]
fn forwarding_never_crosses_a_block_boundary_or_external_entry() {
    let bytes = [0x8B, 0x06, 0xEB, 0, 0x8B, 0x1E];
    let r = crate::ir::frontend::region::lift_cpu_cfg(
        &bytes,
        GuestEip(0x100000),
        LinearAddress(0x100000),
        true,
        8,
    )
    .unwrap();
    let ids = loads(&r);
    assert_ne!(
        r.instructions[ids[0].index()].block,
        r.instructions[ids[1].index()].block
    );
    assert_eq!(
        lower(&r)
            .unwrap()
            .forward_ram_reads(DEFAULT_WORK_LIMIT)
            .unwrap(),
        0
    );
    assert!(r
        .blocks
        .iter()
        .any(|b| matches!(b.terminator, Some(Terminator::Branch(_)))));
}

#[test]
fn emits_guarded_forwarding_cpu_corpus_after_dropping_hir() {
    std::fs::create_dir_all("build/ir-forwarding").unwrap();
    let programs = [
        vec![0x8A, 0x06, 0x8A, 0x1E, 0x8A, 0x16],
        vec![0x66, 0x8B, 0x06, 0x66, 0x8B, 0x1E, 0x66, 0x8B, 0x16],
        vec![0x8B, 0x06, 0x8B, 0x1E, 0x8B, 0x16],
    ];
    let mut cases = vec![];
    for (n, bytes) in programs.iter().enumerate() {
        for forward in [false, true] {
            let r = region(bytes);
            let mut m = lower(&r).unwrap();
            drop(r);
            if forward {
                assert_eq!(m.forward_ram_reads(DEFAULT_WORK_LIMIT).unwrap(), 2);
            }
            assert_eq!(
                m.memory
                    .iter()
                    .flatten()
                    .filter(|p| matches!(p.native, NativeMemory::ScalarLoad { ticket: None, .. }))
                    .count(),
                3
            );
            std::fs::write(
                format!("build/ir-forwarding/{n}-{forward}.wasm"),
                emit_cpu(&m, 32).unwrap().bytes,
            )
            .unwrap();
        }
        for budget in [1, 2, 3, 4, 8] {
            for forward in [false, true] {
                let mut r = crate::ir::frontend::region::lift_cpu_cfg(
                    bytes,
                    GuestEip(0x100000),
                    LinearAddress(0x100000),
                    true,
                    8,
                )
                .unwrap();
                run(&mut r, PassConfig::default()).unwrap();
                let mut m = lower(&r).unwrap();
                drop(r);
                assert!(m.control.polls.iter().any(Option::is_some));
                if forward {
                    assert_eq!(m.forward_ram_reads(DEFAULT_WORK_LIMIT).unwrap(), 2);
                }
                std::fs::write(
                    format!("build/ir-forwarding/cfg-{n}-{budget}-{forward}.wasm"),
                    emit_cpu(&m, budget).unwrap().bytes,
                )
                .unwrap();
            }
        }
        cases.push(format!("{bytes:?}"));
    }
    std::fs::write(
        "build/ir-forwarding/cases.json",
        format!("[{}]", cases.join(",")),
    )
    .unwrap();
}
