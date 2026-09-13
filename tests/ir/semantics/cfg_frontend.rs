use crate::ir::{
    backend::wasm::{emit_cpu, StateLayout},
    frontend::{
        decode::{GuestEip, LinearAddress, PhysicalAddress},
        region::lift_cpu_cfg,
    },
    lowering::{lower, CompileError},
    passes::{licm, run, simd, PassConfig},
    runtime::compile::*,
};
#[test]
fn reachable_cfg_fixtures() {
    std::fs::create_dir_all("build/ir-cfg").unwrap();
    let programs: Vec<Vec<u8>> = vec![
        vec![0x40, 0x49, 0x75, 0xFC],
        vec![0x03, 0x06, 0x49, 0x75, 0xFB],
        vec![0x66, 0x0F, 0xEF, 0xC1, 0xE2, 0xFA],
        vec![0xEB, 0xFE],
        vec![0x74, 0xFE],
        vec![0xE2, 0xFE],
        vec![0xE3, 3, 0x40, 0xEB, 1, 0x48, 0x90],
        vec![0xEB, 1, 0x0F, 0x40],
        vec![0x66, 0x40, 0x67, 0xE2, 0xFB],
        vec![0x8C, 0xD8, 0x49, 0x75, 0xFB],
        vec![0x89, 0x06, 0x0F], // Store owns completion; trailing byte isn't decoded.
        vec![0x0F, 0xA2, 0x0F], // CPUID boundary.
        vec![0x40, 0x03, 0x06, 0x81, 0xC6, 0, 0x10, 0, 0, 0xE2, 0xF5],
        vec![
            0x66, 0x0F, 0xEF, 0xC1, 0xF3, 0x0F, 0x6F, 0x16, 0x81, 0xC6, 0, 0x10, 0, 0, 0xE2, 0xF0,
        ],
        vec![0xB9, 0, 0, 0, 0, 0xE3, 3, 0x03, 0x06, 0x90, 0x40],
        vec![
            0xB8, 1, 0, 0, 0, 0x83, 0xF8, 1, 0x75, 3, 0x40, 0xEB, 2, 0x03, 0x06, 0x90,
        ],
        vec![0xB9, 0, 0, 0, 0, 0xE3, 2, 0x0F, 0xA2, 0x40],
        // Explicit preheader; invariant EBX + EDX feeds a loop-carried EAX.
        vec![0x90, 0x89, 0xD8, 0x01, 0xD0, 0x49, 0x75, 0xF9],
        // Two packed permutations compose to identity, but both SSE guards
        // and their instruction accounting must remain observable.
        vec![0x66, 0x0F, 0x70, 0xC1, 0x1B, 0x66, 0x0F, 0x70, 0xC0, 0x1B],
    ];
    let mut cases = vec![];
    for (n, bytes) in programs.iter().enumerate() {
        for mode in [false, true] {
            // Memory fixture uses [ESI] and the vector prefix requires 32-bit default.
            if !mode && matches!(n, 1 | 2 | 10 | 12..=16 | 18) {
                continue;
            }
            for pc in [0x1000u32, 0xFFFFFFFC] {
                for budget in [1, 2, 3, 4, 5, 9, 16, 100] {
                    let original =
                        lift_cpu_cfg(bytes, GuestEip(pc), LinearAddress(0x100000), mode, 8)
                            .unwrap_or_else(|e| panic!("case {n}, mode {mode}, pc {pc:x}: {e:?}"));
                    for opt in [false, true] {
                        let mut r = original.clone();
                        if opt {
                            run(&mut r, PassConfig::default()).unwrap();
                            let vectors = simd::run(&mut r, simd::DEFAULT_WORK_LIMIT).unwrap();
                            if n == 18 {
                                assert!(vectors.eliminated > 0);
                            }
                            let loops = licm::run(&mut r, licm::DEFAULT_WORK_LIMIT).unwrap();
                            if n == 17 && mode && pc == 0x1000 {
                                assert!(loops.hoisted > 0);
                            }
                        }
                        let mir = lower(&r).unwrap();
                        assert!(mir.control.dynamic_counts);
                        let artifact = emit_cpu(&mir, budget).unwrap();
                        std::fs::write(
                            format!("build/ir-cfg/{}.wasm", cases.len()),
                            artifact.bytes,
                        )
                        .unwrap();
                        cases.push(format!("[{n},{bytes:?},{mode},{pc},{budget},{opt}]"));
                    }
                }
            }
        }
    }
    std::fs::write("build/ir-cfg/cases.json", format!("[{}]", cases.join(","))).unwrap();
}
#[test]
fn cfg_boundaries_and_immutable_compile() {
    let lift =
        |bytes: &[u8]| lift_cpu_cfg(bytes, GuestEip(0x1000), LinearAddress(0x100000), true, 8);
    assert!(lift(&[]).is_err());
    assert!(lift(&[0x0F]).is_err());
    assert!(matches!(
        lift(&[0x74, 1, 0xB8, 0, 0, 0, 0]),
        Err(CompileError::Unsupported(
            "overlapping guest instruction streams"
        ))
    ));
    assert!(matches!(
        lift(&vec![0x90; 65]),
        Err(CompileError::Budget(_))
    ));
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
        tier: Tier::Two,
    };
    let mut snapshot = ImmutableCodeSnapshot {
        bytes: vec![0xEB, 0xFE],
        mappings: vec![CodeMapping {
            linear: LinearAddress(0x100000),
            physical: PhysicalAddress(0x2000),
        }],
        dependencies: vec![CodeDependency {
            page: PhysicalAddress(0x2000),
            version: 4,
        }],
    };
    let config = IrConfig {
        optimize: true,
        passes: PassConfig::default(),
        execution_budget: 16,
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
    assert!(artifact.current(
        request.key,
        &snapshot.dependencies,
        artifact.entry,
        &snapshot.mappings
    ));
    snapshot.dependencies[0].version += 1;
    assert!(!artifact.current(
        request.key,
        &snapshot.dependencies,
        artifact.entry,
        &snapshot.mappings
    ));
    snapshot.dependencies.clear();
    assert!(compile_cpu_cfg_region(&request, &snapshot, &config).is_err());
}
