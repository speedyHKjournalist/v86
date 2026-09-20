//! CPU-only liveness derived from owned MIR at the point it is enabled.
//! State write/helper trimming is reflected in demand; no HIR is retained and
//! Tier 1 does not pay for speculative alternative liveness masks.
use super::MirData;
use crate::ir::{hir::Region, ids::InstId, lowering::CompileError};
pub const DEFAULT_WORK_LIMIT: usize = 262_144;
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan { live: Vec<bool>, enabled: bool }
impl Plan {
    pub(crate) fn disabled(instructions: usize) -> Self { Self {live: vec![true; instructions], enabled: false} }
}
pub(super) fn verify(region: &Region, data: &MirData) -> Result<(), CompileError> {
    if data.cpu_liveness != Plan::disabled(region.instructions.len()) {
        return Err(CompileError::InvalidIr("invalid initial CPU liveness plan".into()));
    }
    Ok(())
}
pub(super) fn verify_owned(data: &MirData) -> Result<(), CompileError> {
    if data.cpu_liveness.live.len() != data.values.len() {
        return Err(CompileError::InvalidIr("invalid CPU liveness length".into()));
    }
    if data.cpu_liveness.enabled && data.cpu_liveness.live.iter().any(|&live| !live) {
        let required = super::allocation::cpu_demand(data, DEFAULT_WORK_LIMIT)?;
        if required.iter().zip(&data.cpu_liveness.live).any(|(&need, &live)| need && !live) {
            return Err(CompileError::InvalidIr("CPU liveness drops a required machine value".into()));
        }
    }
    Ok(())
}
pub(super) fn enable(data: &mut MirData, work_limit: usize) -> Result<usize, CompileError> {
    if data.values.len() > work_limit { return Err(CompileError::Budget("CPU liveness work")); }
    let live = match super::allocation::cpu_demand(data, work_limit) {
        Ok(live) => live,
        Err(CompileError::Budget(_)) => vec![true; data.values.len()],
        Err(error) => return Err(error),
    };
    let count = live.iter().enumerate().filter(|(i, live)| !**live && data.values[*i].is_some()).count();
    data.cpu_liveness = Plan { live, enabled: true };
    Ok(count)
}
pub(super) fn instruction_live(data: &MirData, id: InstId) -> bool {
    !data.cpu_liveness.enabled || data.cpu_liveness.live[id.index()]
        || data.values[id.index()].is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        backend::wasm::{emit, emit_cpu, StateLayout},
        frontend::{
            decode::{GuestEip, LinearAddress},
            region::lift_cpu_cfg,
        },
        lowering::lower,
        passes::{run, PassConfig},
    };

    fn region(bytes: &[u8]) -> crate::ir::hir::Region {
        let mut region =
            lift_cpu_cfg(bytes, GuestEip(0x1000), LinearAddress(0x100000), true, 8).unwrap();
        run(&mut region, PassConfig::default()).unwrap();
        region
    }

    fn layout() -> StateLayout {
        StateLayout {
            gpr: 256,
            flags: 288,
            eip: 292,
            committed: 296,
            flag_operand: 300,
        }
    }

    #[test]
    fn owned_demand_tracks_state_trimming_and_rejects_required_value_removal() {
        let hir = region(&[0x46, 0x0F, 0x31]);
        let mut full = lower(&hir).unwrap();
        let full_dead = full.elide_dead_cpu_values(DEFAULT_WORK_LIMIT).unwrap();
        let mut trimmed = lower(&hir).unwrap();
        trimmed.elide_redundant_cpu_state_writes(super::super::state_elision::DEFAULT_WORK_LIMIT).unwrap();
        let trimmed_dead = trimmed.elide_dead_cpu_values(DEFAULT_WORK_LIMIT).unwrap();
        assert!(trimmed_dead > full_dead, "entry-equivalent stores no longer keep unused GPR reads live");
        trimmed.verify().unwrap();
        let required = trimmed.values.iter().enumerate().find(|(i, p)| p.is_some() && trimmed.cpu_liveness.live[*i]).unwrap().0;
        trimmed.data.cpu_liveness.live[required] = false;
        assert!(trimmed.verify().is_err());
    }

    #[test]
    fn selective_sse_operands_survive_redundant_snapshot_trimming() {
        let hir = region(&[0x46, 0xF3, 0x0F, 0x58, 0xC1, 0x66, 0x0F, 0xEF, 0xC8]);
        let mut mir = lower(&hir).unwrap();
        mir.elide_redundant_cpu_state_writes(DEFAULT_WORK_LIMIT).unwrap();
        mir.elide_dead_cpu_values(DEFAULT_WORK_LIMIT).unwrap();
        let mut checked = 0;
        for call in mir.calls.iter().flatten() {
            if let Some((source, destination)) = call.xmm_observation {
                for write in &mir.states[call.state.index()].cpu.writes {
                    if [source, destination].iter().any(|&reg| write.address == super::super::value::Address::Absolute(
                        crate::cpu::global_pointers::get_reg_xmm_offset(reg as u32))) {
                        for v in super::super::allocation::expression(&write.expression) {
                            if let Some(id) = mir.value_definitions[v.index()] {
                                assert!(instruction_live(&mir, id), "selective operand must remain live");
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(checked >= 2);
        mir.verify().unwrap();
    }

    #[test]
    fn cpu_liveness_elides_concrete_flags_without_changing_standalone() {
        // ADD EAX,EBX; ADD ECX,EDX; JNZ +2; NOP; NOP; NOP.
        // CPU recovery can carry exact lazy backing across both arithmetic
        // boundaries, while JNZ explicitly demands only ZF from the second ADD.
        let region = region(&[0x01, 0xD8, 0x01, 0xD1, 0x75, 0x02, 0x90, 0x90, 0x90]);
        let baseline = lower(&region).unwrap();
        let baseline_cpu = emit_cpu(&baseline, 100).unwrap().bytes;
        let baseline_standalone = emit(&baseline, layout(), 100).unwrap().bytes;

        let mut optimized = lower(&region).unwrap();
        let count = optimized
            .elide_dead_cpu_values(DEFAULT_WORK_LIMIT)
            .unwrap();
        assert!(count > 0, "scalar ALU should expose CPU-only dead flag values");
        let optimized_cpu = emit_cpu(&optimized, 100).unwrap().bytes;
        let optimized_standalone = emit(&optimized, layout(), 100).unwrap().bytes;

        assert!(
            optimized_cpu.len() < baseline_cpu.len(),
            "CPU liveness should remove emitted value programs"
        );
        assert_eq!(
            optimized_standalone, baseline_standalone,
            "CPU-only liveness must not alter standalone emission"
        );
    }

    #[test]
    fn extended_integer_flag_families_keep_exact_recovery() {
        let region = region(&[
            0xD1, 0xE0,             // SHL EAX, 1
            0xD1, 0xC9,             // ROR ECX, 1
            0x0F, 0xA3, 0xC8,       // BT EAX, ECX
            0xF3, 0x0F, 0xB8, 0xD8, // POPCNT EBX, EAX
            0x0F, 0xAF, 0xC3,       // IMUL EAX, EBX
            0xF8,                   // CLC
            0xFC,                   // CLD
            0x75, 0x00,             // JNZ
            0x90,
        ]);
        let mir = lower(&region).unwrap();
        assert!(
            mir.states.iter().all(|state| state.lazy_flags),
            "audited shift/bit/multiply/control states should keep exact recovery"
        );
    }

    #[test]
    fn mixed_eager_lazy_integer_flags_keep_exact_recovery() {
        // ADC/SBB eagerly materialize CF/AF/OF; INC/DEC eagerly preserve CF.
        // Their remaining arithmetic flags stay lazy in the baseline.
        let region = region(&[
            0x11, 0xD8, // ADC EAX, EBX
            0x19, 0xD1, // SBB ECX, EDX
            0x40,       // INC EAX
            0x49,       // DEC ECX
            0x75, 0x00, // JNZ
            0x90,
        ]);
        let mir = lower(&region).unwrap();
        assert!(
            mir.states.iter().all(|state| state.lazy_flags),
            "audited mixed eager/lazy integer states should use exact lazy recovery"
        );
    }
}
