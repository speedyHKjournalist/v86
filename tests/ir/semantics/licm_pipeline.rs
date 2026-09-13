use crate::ir::{
    backend::wasm::StateLayout,
    frontend::decode::{GuestEip, LinearAddress, PhysicalAddress},
    passes::PassConfig,
    runtime::compile::{
        compile_cpu_cfg_region, CodeDependency, CodeMapping, CompileRequest, ImmutableCodeSnapshot,
        IrConfig, PublicationKey, Tier,
    },
};

#[test]
fn tier_two_uses_licm_and_diagnostic_switches_disable_it() {
    // xor eax,eax; jmp loop; loop: mov ebx,esi; add ebx,edi;
    // add eax,ebx; dec ecx; jnz loop; nop
    // ESI+EDI is invariant, including its SSA FLAGS computations.
    let snapshot = ImmutableCodeSnapshot {
        bytes: vec![0x31, 0xC0, 0xEB, 0, 0x89, 0xF3, 0x01, 0xFB, 0x01, 0xD8, 0x49, 0x75, 0xF7, 0x90],
        dependencies: vec![CodeDependency { page: PhysicalAddress(0x100000), version: 1 }],
        mappings: vec![CodeMapping {
            linear: LinearAddress(0x100000),
            physical: PhysicalAddress(0x100000),
        }],
    };
    let mut request = CompileRequest {
        key: PublicationKey { job: 1, vm_generation: 1, slot: 1, slot_generation: 1 },
        pc: GuestEip(0x1000),
        linear: LinearAddress(0x100000),
        default_32: true,
        tier: Tier::Two,
    };
    let mut config = IrConfig {
        optimize: true,
        passes: PassConfig::default(),
        execution_budget: 128,
        rep_iteration_budget: 8,
        max_code_bytes: 1920,
        layout: StateLayout { gpr: 0, flags: 32, eip: 36, committed: 40, flag_operand: 44 },
    };
    let optimized = compile_cpu_cfg_region(&request, &snapshot, &config).unwrap();
    assert!(optimized.passes.loop_hoisted > 0);
    assert!(optimized.current(request.key, &snapshot.dependencies, optimized.entry, &snapshot.mappings));
    request.tier = Tier::One;
    assert_eq!(
        compile_cpu_cfg_region(&request, &snapshot, &config).unwrap().passes.loop_hoisted,
        0
    );
    request.tier = Tier::Two;
    config.optimize = false;
    assert_eq!(
        compile_cpu_cfg_region(&request, &snapshot, &config).unwrap().passes.loop_hoisted,
        0
    );
    config.optimize = true;
    config.passes.gvn = false;
    assert_eq!(
        compile_cpu_cfg_region(&request, &snapshot, &config).unwrap().passes.loop_hoisted,
        0
    );
    config.passes.gvn = true;
    config.passes.rounds = 0;
    assert_eq!(
        compile_cpu_cfg_region(&request, &snapshot, &config).unwrap().passes.loop_hoisted,
        0
    );
}
