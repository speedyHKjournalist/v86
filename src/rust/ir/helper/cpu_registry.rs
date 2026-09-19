//! Registry for CPU CallHelper adapters used by the byte frontend.
//! Signatures are independent of HIR call-site inference. Effects remain fully
//! conservative until a narrower, independently audited contract is registered.
use super::{ExceptionOwner, HelperAbi, HelperDescriptor};
use crate::ir::{effects::Effects, types::Type};

pub fn arity(name: &str) -> Option<usize> {
    Some(match name {
        "ir_sti_check"
        | "ir_fwait"
        | "ir_cli"
        | "ir_clts"
        | "ir_cpuid"
        | "ir_far_control_ud"
        | "ir_flags_stack_check"
        | "ir_hlt"
        | "ir_rdmsr"
        | "ir_rdtsc"
        | "ir_sysenter"
        | "ir_sysexit"
        | "ir_wbinvd"
        | "ir_wrmsr" => 0,
        "ir_iret" | "ir_verr_mem" | "ir_verr_reg" | "ir_verw_mem" | "ir_verw_reg" => 1,
        "ir_arpl_reg"
        | "ir_rdrand"
        | "ir_descriptor_ud"
        | "ir_far_return"
        | "ir_fxrstor"
        | "ir_fxsave"
        | "ir_in"
        | "ir_invlpg"
        | "ir_io_check"
        | "ir_ldmxcsr"
        | "ir_lgdt"
        | "ir_lidt"
        | "ir_lldt_mem"
        | "ir_lldt_reg"
        | "ir_lmsw_mem"
        | "ir_lmsw_reg"
        | "ir_ltr_mem"
        | "ir_ltr_reg"
        | "ir_pop_flags"
        | "ir_read_cr"
        | "ir_read_dr"
        | "ir_sgdt"
        | "ir_sidt"
        | "ir_sldt_mem"
        | "ir_sldt_reg"
        | "ir_smsw_mem"
        | "ir_smsw_reg"
        | "ir_software_interrupt"
        | "ir_stmxcsr"
        | "ir_str_mem"
        | "ir_str_reg"
        | "ir_write_cr"
        | "ir_write_dr" => 2,
        "ir_arpl_mem" | "ir_movnti" | "ir_far_jump_mem" | "ir_lar_mem" | "ir_lar_reg"
        | "ir_lsl_mem" | "ir_lsl_reg" | "ir_out" | "ir_pop_segment" => 3,
        "ir_reserved_form"
        | "ir_mmx_mask"
        | "ir_mmx_reg"
        | "ir_sse_fp_reg_continue"
        | "ir_sse_fp_reg"
        | "ir_far_jump"
        | "ir_ins"
        | "ir_outs"
        | "ir_x87_reg" => 4,
        "ir_mmx_mem"
        | "ir_sse_fp_mem_continue"
        | "ir_sse_fp_mem"
        | "ir_load_segment"
        | "ir_rep_cmps"
        | "ir_rep_ins"
        | "ir_rep_lods"
        | "ir_rep_movs"
        | "ir_rep_outs"
        | "ir_rep_scas"
        | "ir_rep_stos"
        | "ir_x87_mem" => 5,
        _ => return None,
    })
}
fn abi(name: &str) -> HelperAbi {
    if matches!(name, "ir_sse_fp_reg_continue" | "ir_sse_fp_mem_continue") {
        HelperAbi::CpuReload
    } else if name.starts_with("ir_rep_") {
        HelperAbi::CpuRep
    } else if matches!(
        name,
        "ir_sti_check" | "ir_flags_stack_check" | "ir_io_check"
    ) {
        HelperAbi::Outcome {
            fault_delivery: None,
            normal_preserves_state: true,
        }
    } else {
        HelperAbi::CpuExit
    }
}
pub fn descriptor(name: &str, params: Vec<Type>) -> Result<HelperDescriptor, &'static str> {
    if arity(name).is_none() {
        return Err("unregistered CPU call helper");
    }
    let descriptor = HelperDescriptor {
        name: name.into(),
        params,
        results: if matches!(abi(name), HelperAbi::CpuReload) {
            super::cpu_reload_types()
        } else {
            vec![]
        },
        effects: Effects::conservative(),
        exception_owner: ExceptionOwner::Helper,
        abi: abi(name),
    };
    validate(&descriptor)?;
    Ok(descriptor)
}
pub fn validate(d: &HelperDescriptor) -> Result<(), &'static str> {
    let Some(count) = arity(&d.name) else {
        return Ok(());
    };
    if d.params.len() != count
        || d.results
            != if matches!(abi(&d.name), HelperAbi::CpuReload) {
                super::cpu_reload_types()
            } else {
                vec![]
            }
        || d.params.iter().any(|t| {
            !matches!(
                t,
                Type::I1
                    | Type::I8
                    | Type::I16
                    | Type::I32
                    | Type::EffectiveAddress
                    | Type::LinearAddress
                    | Type::RamAddress
            )
        })
    {
        return Err("CPU call helper signature mismatch");
    }
    let matching_abi = match (abi(&d.name), &d.abi) {
        (HelperAbi::CpuReload, HelperAbi::CpuReload) => true,
        (HelperAbi::CpuExit, HelperAbi::CpuExit) | (HelperAbi::CpuRep, HelperAbi::CpuRep) => true,
        (
            HelperAbi::Outcome { .. },
            HelperAbi::Outcome {
                fault_delivery: None,
                normal_preserves_state: true,
            },
        ) => true,
        _ => false,
    };
    if !matching_abi
        || d.exception_owner != ExceptionOwner::Helper
        || d.effects != Effects::conservative()
    {
        return Err("CPU call helper observation/outcome contract mismatch");
    }
    Ok(())
}
