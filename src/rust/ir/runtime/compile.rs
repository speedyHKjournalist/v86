//! Immutable experimental compiler entry. Deliberately separate from the live JIT cache.
use super::entry::{CpuEntryKey, EntryContract};
use crate::ir::{
    backend::wasm::{emit, emit_cpu_entry, Artifact, StateLayout},
    frontend::{
        decode::{GuestEip, LinearAddress, PhysicalAddress},
        lift::{lift, lift_cpu_with_rep_budget},
        region::lift_cpu_cfg,
    },
    lowering::{lower, CompileError},
    passes::{licm, run, PassConfig, PassStats},
};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Backend {
    Legacy,
    Ir,
}
impl Backend {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        match value {
            "legacy" => Ok(Self::Legacy),
            "ir" => Ok(Self::Ir),
            _ => Err("unknown backend"),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tier {
    One,
    Two,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodeDependency {
    pub page: PhysicalAddress,
    pub version: u64,
}
#[derive(Clone, Debug)]
pub struct ImmutableCodeSnapshot {
    pub bytes: Vec<u8>,
    pub dependencies: Vec<CodeDependency>,
    pub mappings: Vec<CodeMapping>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeMapping {
    pub linear: LinearAddress,
    pub physical: PhysicalAddress,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublicationKey {
    pub job: u64,
    pub vm_generation: u64,
    pub slot: u32,
    pub slot_generation: u64,
}
pub struct CompileRequest {
    pub key: PublicationKey,
    pub pc: GuestEip,
    pub linear: LinearAddress,
    pub default_32: bool,
    pub tier: Tier,
}
impl CompileRequest {
    pub fn cpu_entry(&self) -> CpuEntryKey {
        CpuEntryKey {
            pc: self.pc,
            linear: self.linear,
            default_32: self.default_32,
        }
    }
}
pub struct IrConfig {
    pub optimize: bool,
    pub passes: PassConfig,
    pub execution_budget: u32,
    /// Maximum REP elements per cold entry (0..=4096), distinct from block work.
    pub rep_iteration_budget: u32,
    pub max_code_bytes: usize,
    pub layout: StateLayout,
}
pub struct CompiledArtifact {
    pub key: PublicationKey,
    pub tier: Tier,
    pub dependencies: Vec<CodeDependency>,
    pub code: Artifact,
    pub passes: PassStats,
    /// Separate Tier 2 loop-transform work; zero for Tier 1 or disabled optimization.
    pub loop_passes: licm::Stats,
    pub mir_folds: usize,
    pub guest_bytes: usize,
    pub entry: EntryContract,
    pub mappings: Vec<CodeMapping>,
}
impl CompiledArtifact {
    /// Must run before installing a table slot; equality includes dependency versions and reset generation.
    /// This is a compiler-side check, not yet the production JS publication protocol.
    pub fn current(
        &self,
        key: PublicationKey,
        dependencies: &[CodeDependency],
        entry: EntryContract,
        mappings: &[CodeMapping],
    ) -> bool {
        self.key == key
            && self.dependencies == dependencies
            && self.entry == entry
            && self.mappings == mappings
    }
}
pub fn compile_region(
    request: &CompileRequest,
    snapshot: &ImmutableCodeSnapshot,
    config: &IrConfig,
) -> Result<CompiledArtifact, CompileError> {
    compile_inner(request, snapshot, config, false, false)
}
/// Compile a cold CPU-entry artifact. Publication/cache installation is still separate.
pub fn compile_cpu_region(
    request: &CompileRequest,
    snapshot: &ImmutableCodeSnapshot,
    config: &IrConfig,
) -> Result<CompiledArtifact, CompileError> {
    compile_inner(request, snapshot, config, true, false)
}
/// Compile reachable direct control flow, with bounded execution and SSA loop state.
/// The snapshot and publication validation contract is identical to the linear API.
pub fn compile_cpu_cfg_region(
    request: &CompileRequest,
    snapshot: &ImmutableCodeSnapshot,
    config: &IrConfig,
) -> Result<CompiledArtifact, CompileError> {
    compile_inner(request, snapshot, config, true, true)
}
fn compile_inner(
    request: &CompileRequest,
    snapshot: &ImmutableCodeSnapshot,
    config: &IrConfig,
    cpu: bool,
    cfg: bool,
) -> Result<CompiledArtifact, CompileError> {
    if snapshot.bytes.len() > config.max_code_bytes || snapshot.bytes.len() > 15 * 128 {
        return Err(CompileError::Budget("code snapshot"));
    }
    let max_pages = if request.tier == Tier::One { 2 } else { 8 };
    if snapshot.dependencies.is_empty() || snapshot.dependencies.len() > max_pages {
        return Err(CompileError::Budget("code dependencies"));
    }
    for (i, dependency) in snapshot.dependencies.iter().enumerate() {
        if dependency.page.0 & 4095 != 0
            || snapshot.dependencies[..i]
                .iter()
                .any(|p| p.page == dependency.page)
        {
            return Err(CompileError::InvalidIr(
                "invalid or duplicate physical code page".into(),
            ));
        }
    }
    let needed_pages = ((request.linear.0 & 4095) as usize + snapshot.bytes.len() + 4095) / 4096;
    if snapshot.mappings.len() != needed_pages || needed_pages > max_pages {
        return Err(CompileError::InvalidIr(
            "incomplete code mapping snapshot".into(),
        ));
    }
    for (index, mapping) in snapshot.mappings.iter().enumerate() {
        let expected = (request.linear.0 & !4095).wrapping_add(index as u32 * 4096);
        if mapping.linear.0 != expected
            || mapping.physical.0 & 4095 != 0
            || !snapshot
                .dependencies
                .iter()
                .any(|d| d.page == mapping.physical)
        {
            return Err(CompileError::InvalidIr("invalid code page mapping".into()));
        }
    }
    if snapshot
        .dependencies
        .iter()
        .any(|d| !snapshot.mappings.iter().any(|m| m.physical == d.page))
    {
        return Err(CompileError::InvalidIr("unused code dependency".into()));
    }
    let mut region = if cfg {
        lift_cpu_cfg(
            &snapshot.bytes,
            request.pc,
            request.linear,
            request.default_32,
            config.rep_iteration_budget,
        )?
    } else if cpu {
        lift_cpu_with_rep_budget(
            &snapshot.bytes,
            request.pc,
            request.linear,
            request.default_32,
            config.rep_iteration_budget,
        )?
    } else {
        lift(
            &snapshot.bytes,
            request.pc,
            request.linear,
            request.default_32,
        )?
    };
    let passes = if config.optimize {
        run(&mut region, config.passes).map_err(CompileError::InvalidIr)?
    } else {
        PassStats::default()
    };
    let loop_passes =
        if config.optimize && request.tier == Tier::Two && config.passes.rounds != 0 {
            licm::run(&mut region, licm::Config::default()).map_err(CompileError::InvalidIr)?
        } else {
            licm::Stats::default()
        };
    let mut mir = lower(&region)?;
    drop(region);
    let mir_folds = if config.optimize { mir.fold_constants()? } else { 0 };
    let code = if cpu {
        emit_cpu_entry(&mir, config.execution_budget, request.cpu_entry())?
    } else {
        emit(&mir, config.layout, config.execution_budget)?
    };
    Ok(CompiledArtifact {
        key: request.key,
        tier: request.tier,
        dependencies: snapshot.dependencies.clone(),
        code,
        passes,
        loop_passes,
        mir_folds,
        guest_bytes: snapshot.bytes.len(),
        mappings: snapshot.mappings.clone(),
        entry: if cpu {
            EntryContract::Cpu(request.cpu_entry())
        } else {
            EntryContract::Standalone
        },
    })
}
