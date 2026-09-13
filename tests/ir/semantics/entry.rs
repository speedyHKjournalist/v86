use crate::ir::{
    backend::wasm::StateLayout,
    frontend::decode::{GuestEip, LinearAddress, PhysicalAddress},
    runtime::{compile::*, entry::EntryContract},
};
fn config(optimize: bool) -> IrConfig {
    IrConfig {
        optimize,
        passes: Default::default(),
        execution_budget: 32,
        rep_iteration_budget: 8,
        max_code_bytes: 128,
        layout: StateLayout {
            gpr: 0,
            flags: 32,
            eip: 36,
            committed: 40,
            flag_operand: 44,
        },
    }
}
fn request(pc: u32, linear: u32, mode: bool) -> CompileRequest {
    CompileRequest {
        key: PublicationKey {
            job: 17,
            vm_generation: 2,
            slot: 3,
            slot_generation: 9,
        },
        pc: GuestEip(pc),
        linear: LinearAddress(linear),
        default_32: mode,
        tier: Tier::One,
    }
}
fn snapshot(bytes: Vec<u8>, linear: u32) -> ImmutableCodeSnapshot {
    ImmutableCodeSnapshot {
        bytes,
        mappings: vec![CodeMapping {
            linear: LinearAddress(linear),
            physical: PhysicalAddress(0x100000),
        }],
        dependencies: vec![CodeDependency {
            page: PhysicalAddress(0x100000),
            version: 7,
        }],
    }
}
#[test]
fn entry_publication_contract() {
    let req = request(0xFFFFFFFC, 0x100000, true);
    assert_eq!(req.cpu_entry().cs_base(), 0x100004);
    let bytes = snapshot(vec![0x40], 0x100000);
    let cpu = compile_cpu_region(&req, &bytes, &config(true)).unwrap();
    assert!(cpu.current(
        req.key,
        &bytes.dependencies,
        EntryContract::Cpu(req.cpu_entry()),
        &bytes.mappings
    ));
    for other in [
        request(0, 0x100000, true),
        request(0xFFFFFFFC, 0x800000, true),
        request(0xFFFFFFFC, 0x100000, false),
    ] {
        assert!(!cpu.current(
            req.key,
            &bytes.dependencies,
            EntryContract::Cpu(other.cpu_entry()),
            &bytes.mappings
        ));
    }
    assert!(!cpu.current(
        req.key,
        &bytes.dependencies,
        EntryContract::Standalone,
        &bytes.mappings
    ));
    let standalone = compile_region(&req, &bytes, &config(true)).unwrap();
    assert!(!standalone.current(req.key, &bytes.dependencies, cpu.entry, &bytes.mappings));
    let mut changed = bytes.dependencies.clone();
    changed[0].version += 1;
    assert!(!cpu.current(req.key, &changed, cpu.entry, &bytes.mappings));
    assert!(!cpu.current(
        PublicationKey {
            slot_generation: 10,
            ..req.key
        },
        &bytes.dependencies,
        cpu.entry,
        &bytes.mappings
    ));
}
#[test]
fn entry_execution_fixtures() {
    std::fs::create_dir_all("build/ir-entry").unwrap();
    let mut cases = vec![];
    for mode in [false, true] {
        let address_prefix = if mode { vec![] } else { vec![0x67] };
        let mut load = address_prefix.clone();
        load.extend([0x8B, 0x06]);
        let mut store = address_prefix;
        store.extend([0x89, 0x06]);
        let programs = [
            vec![0x40],
            load,
            store,
            vec![0x0F, 0xA2],
            vec![0x40, 0x49, 0x75, 0xFC],
            vec![0x66, 0x0F, 0xEF, 0xC1],
        ];
        for (kind, bytes) in programs.iter().enumerate() {
            for cfg in [false, true] {
                if kind == 4 && !cfg {
                    continue;
                }
                for pc in [0x1000, 0xFFFFFFFC] {
                    for linear in [0x100000, 0x800000] {
                        for opt in [false, true] {
                            let req = request(pc, linear, mode);
                            let source = snapshot(bytes.clone(), linear);
                            let artifact = if cfg {
                                compile_cpu_cfg_region(&req, &source, &config(opt))
                            } else {
                                compile_cpu_region(&req, &source, &config(opt))
                            }
                            .unwrap();
                            assert_eq!(artifact.entry, EntryContract::Cpu(req.cpu_entry()));
                            std::fs::write(
                                format!("build/ir-entry/{}.wasm", cases.len()),
                                artifact.code.bytes,
                            )
                            .unwrap();
                            cases.push(format!(
                                "[{kind},{mode},{pc},{linear},{opt},{cfg},{bytes:?}]"
                            ));
                        }
                    }
                }
            }
        }
    }
    std::fs::write(
        "build/ir-entry/cases.json",
        format!("[{}]", cases.join(",")),
    )
    .unwrap();
}

#[test]
fn explicit_code_mappings_and_physical_aliases() {
    let req = request(0x1FFF, 0x1FFF, true);
    let mut source = snapshot(vec![0xB8, 1, 0, 0, 0], 0x1000);
    source.mappings.push(CodeMapping {
        linear: LinearAddress(0x2000),
        physical: PhysicalAddress(0x100000),
    });
    let artifact = compile_cpu_region(&req, &source, &config(true)).unwrap();
    assert_eq!(artifact.dependencies.len(), 1);
    assert_eq!(artifact.mappings.len(), 2);
    assert!(artifact.current(
        req.key,
        &source.dependencies,
        artifact.entry,
        &source.mappings
    ));
    for change in 0..6 {
        let mut bad = source.clone();
        match change {
            0 => {
                bad.mappings.pop();
            },
            1 => bad.mappings.swap(0, 1),
            2 => bad.mappings[1].linear = LinearAddress(0x1000),
            3 => bad.mappings[1].physical = PhysicalAddress(0x100001),
            4 => bad.mappings[1].physical = PhysicalAddress(0x200000),
            _ => bad.dependencies.push(CodeDependency {
                page: PhysicalAddress(0x300000),
                version: 1,
            }),
        }
        assert!(compile_cpu_region(&req, &bad, &config(true)).is_err());
        assert!(!artifact.current(req.key, &bad.dependencies, artifact.entry, &bad.mappings));
    }
    let wrap = request(0xFFFFFFFF, 0xFFFFFFFF, true);
    source.mappings[0].linear = LinearAddress(0xFFFFF000);
    source.mappings[1].linear = LinearAddress(0);
    assert!(compile_cpu_region(&wrap, &source, &config(false)).is_ok());
}

#[test]
fn loop_motion_is_tier_two_only_and_honors_diagnostic_disable() {
    // A direct preheader followed by an invariant add in a counted loop.
    // mov ebp,0; mov eax,edx; add eax,ebx; add ebp,eax; dec ecx; jnz loop
    let bytes = snapshot(
        vec![
            0xBD, 0, 0, 0, 0, 0x89, 0xD0, 0x01, 0xD8, 0x01, 0xC5, 0x49, 0x75, 0xF7,
        ],
        0x100000,
    );
    for tier in [Tier::One, Tier::Two] {
        for optimize in [false, true] {
            for rounds in [0, 2] {
                let mut req = request(0x1000, 0x100000, true);
                req.tier = tier;
                let mut options = config(optimize);
                options.passes.rounds = rounds;
                let artifact = compile_cpu_cfg_region(&req, &bytes, &options).unwrap();
                if tier == Tier::Two && optimize && rounds != 0 {
                    assert!(
                        artifact.passes.loop_hoisted > 0,
                        "optimized Tier 2 must actually run LICM"
                    );
                } else {
                    assert_eq!(artifact.passes.loop_hoisted, 0);
                }
                assert_eq!(artifact.entry, EntryContract::Cpu(req.cpu_entry()));
            }
        }
    }
}

#[test]
fn guarded_ram_forwarding_is_tier_two_only_in_both_cpu_compile_entry_points() {
    let bytes = snapshot(vec![0x8B, 0x06, 0x8B, 0x1E], 0x100000);
    for tier in [Tier::One, Tier::Two] {
        for optimize in [false, true] {
            for rounds in [0, 2] {
                for cfg in [false, true] {
                    let mut req = request(0x1000, 0x100000, true);
                    req.tier = tier;
                    let mut options = config(optimize);
                    options.passes.rounds = rounds;
                    let artifact = if cfg {
                        compile_cpu_cfg_region(&req, &bytes, &options)
                    } else {
                        compile_cpu_region(&req, &bytes, &options)
                    }
                    .unwrap();
                    assert_eq!(
                        artifact.passes.ram_forwarded,
                        usize::from(tier == Tier::Two && optimize && rounds != 0)
                    );
                    assert_eq!(artifact.entry, EntryContract::Cpu(req.cpu_entry()));
                }
            }
        }
    }
}
