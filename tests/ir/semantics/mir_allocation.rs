use super::*;
use crate::ir::{
    backend::wasm::emit_cpu,
    frontend::{
        decode::{GuestEip, LinearAddress},
        region::lift_cpu_cfg,
    },
    lowering::lower,
    passes::{run, PassConfig},
};

#[test]
fn dense_machine_allocation_preserves_colors_and_generated_code() {
    // Cold fault snapshots, loop phis, irreducible CFGs, narrow and vector
    // values, and CPU-reload helpers all contribute independent liveness roots.
    let programs: &[&[u8]] = &[
        &[0x40, 0x49, 0x75, 0xFC],
        &[0x03, 0x06, 0x49, 0x75, 0xFB],
        &[0x66, 0x0F, 0xEF, 0xC1, 0xE2, 0xFA],
        &[0x74, 0x02, 0x75, 0x02, 0x40, 0x90, 0x48, 0x90],
        &[0x74, 0x02, 0xEB, 0x02, 0xEB, 0xFC, 0xEB, 0xFC],
        &[0x11, 0xD8, 0x19, 0xD1, 0x40, 0x49, 0x75, 0xF8],
        &[0xFF, 0x06, 0x8B, 0x06, 0x49, 0x75, 0xF9],
        &[0xEC, 0x43, 0x49, 0x75, 0xFB],
        &[0x0F, 0x31, 0x43, 0x49, 0x75, 0xFA],
    ];
    for (index, bytes) in programs.iter().enumerate() {
        for mode in [false, true] {
            let mut bytes = bytes.to_vec();
            if !mode && matches!(index, 1 | 6) {
                // Address [SI] in 16-bit mode, preserving the instruction
                // lengths of the [ESI] loops and their backedge displacement.
                bytes[1] = 4;
                if index == 6 {
                    bytes[3] = 4;
                }
            }
            let original =
                lift_cpu_cfg(&bytes, GuestEip(0x1000), LinearAddress(0x201000), mode, 8)
                    .unwrap_or_else(|e| panic!("mode={mode}, bytes={bytes:x?}: {e:?}"));
            for optimized in [false, true] {
                let mut hir = original.clone();
                if optimized {
                    run(&mut hir, PassConfig::default()).unwrap();
                }
                for stack in [false, true] {
                    let mut mir = lower(&hir).unwrap();
                    if stack {
                        mir.fold_constants().unwrap();
                        mir.schedule_operand_stack(262_144).unwrap();
                    }
                    let initial_allocation = mir.allocation.clone();
                    let initial_control = mir.control.clone();
                    let reference = reallocate_with::<BTreeSet<ValueId>>(&mut mir.data, 4_000_000)
                        .unwrap();
                    mir.verify().unwrap();
                    let reference_allocation = mir.allocation.clone();
                    let reference_control = mir.control.clone();
                    let reference_code = emit_cpu(&mir, 256).unwrap().bytes;
                    mir.data.allocation = initial_allocation.clone();
                    mir.data.control = initial_control.clone();
                    assert_eq!(reallocate(&mut mir.data, 4_000_000).unwrap(), reference);
                    assert_eq!(mir.allocation, reference_allocation);
                    assert_eq!(mir.control, reference_control);
                    // The verifier intentionally retains its independent tree
                    // liveness solver; output equivalence is not its only check.
                    mir.verify().unwrap();
                    assert_eq!(emit_cpu(&mir, 256).unwrap().bytes, reference_code);
                    for budget in [1, 128, 1024] {
                        mir.data.allocation = initial_allocation.clone();
                        mir.data.control = initial_control.clone();
                        if reallocate(&mut mir.data, budget).is_err() {
                            assert_eq!(mir.allocation, initial_allocation);
                            assert_eq!(mir.control, initial_control);
                        }
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "allocator timing only, not an XP performance gate"]
fn machine_allocation_paired_benchmark() {
    for size in [8, 32, 64] {
        let mut bytes = vec![0x40; size];
        bytes.extend([0x49, 0x75, (-(size as i32 + 3)) as u8]);
        let hir = lift_cpu_cfg(&bytes, GuestEip(0), LinearAddress(0), true, 8).unwrap();
        let mut mir = lower(&hir).unwrap();
        mir.schedule_operand_stack(262_144).unwrap();
        let mut samples = [vec![], vec![]];
        for round in 0..7 {
            for which in [round % 2, 1 - round % 2] {
                let start = std::time::Instant::now();
                for _ in 0..20 {
                    let result = if which == 0 {
                        reallocate_with::<BTreeSet<ValueId>>(&mut mir.data, 4_000_000)
                    }
                    else {
                        reallocate(&mut mir.data, 4_000_000)
                    };
                    std::hint::black_box(result.unwrap());
                }
                samples[which].push(start.elapsed().as_secs_f64() * 1e6 / 20.0);
            }
        }
        for sample in &mut samples {
            sample.sort_by(f64::total_cmp);
        }
        println!(
            "{size} instructions: MIR tree {:.1} us, MIR dense {:.1} us",
            samples[0][3], samples[1][3]
        );
    }
}
