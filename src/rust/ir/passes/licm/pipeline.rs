use crate::ir::{
    backend::wasm::StateLayout,
    frontend::{
        decode::{GuestEip, LinearAddress, PhysicalAddress},
        region::lift_cpu_cfg,
    },
    passes::{run_tier2, PassConfig},
    runtime::compile::{
        compile_cpu_cfg_region, compile_cpu_region, compile_region, CodeDependency, CodeMapping,
        CompileRequest, ImmutableCodeSnapshot, IrConfig, PublicationKey, Tier,
    },
};

fn request(tier: Tier) -> CompileRequest {
    CompileRequest {
        key: PublicationKey {
            job: 1,
            vm_generation: 2,
            slot: 3,
            slot_generation: 4,
        },
        pc: GuestEip(0x1000),
        linear: LinearAddress(0x100000),
        default_32: true,
        tier,
    }
}
fn snapshot(bytes: &[u8]) -> ImmutableCodeSnapshot {
    ImmutableCodeSnapshot {
        bytes: bytes.to_vec(),
        dependencies: vec![CodeDependency {
            page: PhysicalAddress(0x300000),
            version: 1,
        }],
        mappings: vec![CodeMapping {
            linear: LinearAddress(0x100000),
            physical: PhysicalAddress(0x300000),
        }],
    }
}
fn config(optimize: bool) -> IrConfig {
    IrConfig {
        optimize,
        passes: PassConfig::default(),
        execution_budget: 100,
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

#[test]
fn all_compile_entries_apply_loop_passes_only_in_optimized_tier_two() {
    let bytes = snapshot(&[0x89, 0xD0, 0x01, 0xF0]);
    for compile in [compile_region, compile_cpu_region, compile_cpu_cfg_region] {
        for tier in [Tier::One, Tier::Two] {
            for optimize in [false, true] {
                let artifact = compile(&request(tier), &bytes, &config(optimize)).unwrap();
                assert_eq!(artifact.passes.licm_work > 0, tier == Tier::Two && optimize);
                assert_eq!(artifact.passes.licm_hoisted, 0, "straight-line region");
                assert_eq!(artifact.tier, tier);
                assert_eq!(artifact.dependencies, bytes.dependencies);
                assert_eq!(artifact.mappings, bytes.mappings);
                assert!(!artifact.code.bytes.is_empty());
            }
        }
    }
}

#[test]
fn cpu_cfg_compilation_really_hoists_and_zero_rounds_disable_loop_motion() {
    // mov ecx,3; L: mov eax,edx; add eax,esi; dec ecx; jnz L
    let bytes = snapshot(&[0xB9, 3, 0, 0, 0, 0x89, 0xD0, 0x01, 0xF0, 0x49, 0x75, 0xF9]);
    let artifact = compile_cpu_cfg_region(&request(Tier::Two), &bytes, &config(true)).unwrap();
    assert!(artifact.passes.licm_loops > 0);
    assert!(artifact.passes.licm_hoisted > 0);
    let mut disabled = config(true);
    disabled.passes.rounds = 0;
    let artifact = compile_cpu_cfg_region(&request(Tier::Two), &bytes, &disabled).unwrap();
    assert_eq!(artifact.passes.licm_work, 0);
    assert_eq!(artifact.passes.licm_hoisted, 0);
}

#[test]
fn optimized_loop_pipeline_keeps_input_unchanged_with_zero_rounds() {
    let bytes = [0xB9, 3, 0, 0, 0, 0x89, 0xD0, 0x01, 0xF0, 0x49, 0x75, 0xF9];
    let mut r = lift_cpu_cfg(&bytes, GuestEip(0x1000), LinearAddress(0x100000), true, 8).unwrap();
    let before = format!("{:?}", r);
    let stats = run_tier2(
        &mut r,
        PassConfig {
            rounds: 0,
            ..PassConfig::default()
        },
    )
    .unwrap();
    assert_eq!(stats.licm_hoisted, 0);
    assert_eq!(format!("{:?}", r), before);
}
