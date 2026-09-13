//! Owned machine plans. HIR is borrowed only while checking the lowering boundary.
pub mod arithmetic;
pub mod call;
pub mod control;
pub mod effect;
pub mod forwarding;
pub mod materialize;
pub mod memory;
mod optimize;
pub mod value;
pub mod vector;
use super::{backend::locals::Allocation, hir::Region, lowering::CompileError, types::Type};
use crate::wasmgen::wasm_builder::Signature;
use std::ops::{Deref, DerefMut};
/// Legalized import and cold-path policy, distinct from the HIR descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelperCall {
    pub name: String,
    /// CpuExit or CpuRep: no cached SSA state may resume after this call.
    pub cpu_exit: bool,
    pub signature: Signature,
    pub fault_delivery: Option<String>,
    pub exit_outcomes: Vec<u32>,
}
pub struct MirData {
    pub value_types: Vec<Type>,
    pub allocation: Allocation,
    pub helpers: Vec<Option<HelperCall>>,
    pub memory: Vec<Option<memory::MemoryPlan>>,
    pub effects: Vec<Option<effect::EffectPlan>>,
    pub calls: Vec<Option<call::CallPlan>>,
    pub control: control::ControlFlow,
    pub values: Vec<Option<value::ValuePlan>>,
    pub states: Vec<materialize::StatePlan>,
    pub(super) ram_forwarding: Vec<Option<forwarding::Forwarding>>,
}

/// Verified, owned MIR. No mutable dereference: transformations must preserve the
/// checked machine contracts through an explicit MIR API.
pub struct MirRegion {
    data: MirData,
}
impl Deref for MirRegion {
    type Target = MirData;
    fn deref(&self) -> &MirData {
        &self.data
    }
}
impl MirRegion {
    /// Enable guarded repeated scalar reads from ordinary RAM. This transformation
    /// uses owned MIR only, with transactional work-budget failure.
    pub fn forward_ram_reads(&mut self, work_limit: usize) -> Result<usize, CompileError> {
        forwarding::optimize(&mut self.data, work_limit)
    }
    pub fn ram_forwarding(&self, id: super::ids::InstId) -> Option<forwarding::Forwarding> {
        self.data.ram_forwarding[id.index()]
    }
    pub fn has_ram_forwarding(&self) -> bool {
        self.data.ram_forwarding.iter().any(Option::is_some)
    }

    /// Fold literal machine operations without changing definitions, effects,
    /// local assignments or recovery points. The update is transactional.
    pub fn fold_constants(&mut self) -> Result<usize, CompileError> {
        optimize::fold_constants(&mut self.data)
    }
}

/// Transient lowering transaction. It cannot be emitted and never clones HIR.
pub struct Draft<'a> {
    pub(super) hir: &'a Region,
    pub(super) data: MirData,
}
impl Deref for Draft<'_> {
    type Target = MirData;
    fn deref(&self) -> &MirData {
        &self.data
    }
}
impl DerefMut for Draft<'_> {
    fn deref_mut(&mut self) -> &mut MirData {
        &mut self.data
    }
}
impl Draft<'_> {
    pub fn finish(self) -> Result<MirRegion, CompileError> {
        super::verify::verify(self.hir).map_err(|e| CompileError::InvalidIr(e.0))?;
        let data = &self.data;
        if data.value_types != self.hir.values.iter().map(|v| v.ty).collect::<Vec<_>>()
            || data.allocation
                != super::backend::locals::allocate(self.hir).map_err(CompileError::Budget)?
        {
            return Err(CompileError::InvalidIr(
                "invalid machine types or local allocation".into(),
            ));
        }
        memory::verify(self.hir, &data.memory)?;
        effect::verify(self.hir, &data.effects)?;
        call::verify(self.hir, &data.helpers, &data.calls)?;
        control::verify(self.hir, &data.allocation, &data.control)?;
        value::verify(self.hir, &data.values)?;
        materialize::verify(self.hir, &data.states)?;
        forwarding::verify(data)?;
        Ok(MirRegion { data: self.data })
    }
}

/// Built-in emitter/plan adapters cannot be redeclared as generic helpers.
pub const CPU_IMPORTS: &[&str] = &[
    "ir_pop_address",
    "ir_enter",
    "ir_entry_matches",
    "ir_divide_fault",
    "ir_tlb_base",
    "get_eflags",
    "ir_segment_address",
    "ir_memory_read",
    "ir_memory_check",
    "ir_memory_write",
    "ir_memory_write_unmasked_word",
    "ir_rmw_read",
    "ir_rmw_write",
    "ir_rmw_value",
    "ir_cmpxchg8b",
    "ir_sse_guard",
    "ir_xmm_load",
    "ir_xmm_store",
    "ir_xmm_binary",
    "ir_xmm_shuffle",
    "ir_xmm_transfer_load",
    "ir_xmm_insert_word",
    "ir_xmm_masked_store",
];
