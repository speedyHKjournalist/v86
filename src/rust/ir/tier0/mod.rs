//! Tier-0: cheap page-granular compilation below the optimizing region tier.
//! See docs/ir-page-tier-design.md. A page function serves every observed
//! entry of one code page and dispatches between its basic blocks without
//! returning to the CPU loop.
pub mod analysis;
pub mod emit;

use crate::ir::{
    backend::wasm::Artifact,
    frontend::decode::{GuestEip, LinearAddress},
    lowering::CompileError,
    passes::PassStats,
    runtime::{
        compile::{CompileRequest, CompiledArtifact, ImmutableCodeSnapshot, Tier},
        entry::{CpuEntryKey, EntryContract},
    },
};

/// The pages a page function for `entries` covers: the primary page plus
/// the previous and/or next page when its code branches, calls or falls
/// into them (bounded work across page boundaries, as the legacy JIT's
/// multi-page modules). Returns (first page linear, page count).
pub fn range(
    origin: &CompileRequest,
    primary: &ImmutableCodeSnapshot,
    entries: &[CpuEntryKey],
    code_page: impl Fn(u32) -> bool,
) -> (u32, u32) {
    let base = origin.linear.0 & !4095;
    let base_pc = GuestEip(origin.pc.0.wrapping_sub(origin.linear.0 & 4095));
    if primary.bytes.len() != analysis::PAGE {
        return (base, 1);
    }
    let offsets: Vec<usize> = entries.iter().map(|e| (e.linear.0 & 4095) as usize).collect();
    let plan = analysis::analyze(&primary.bytes, base_pc, LinearAddress(base), origin.default_32, &offsets, 0..4096);
    let before = plan.external.iter().any(|&t| (-4096..0).contains(&t)) && code_page(base.wrapping_sub(4096));
    let after = plan.external.iter().any(|&t| (4096..8192).contains(&t)) && code_page(base.wrapping_add(4096));
    let first = if before { base.wrapping_sub(4096) } else { base };
    (first, 1 + before as u32 + after as u32)
}

/// Compile a page function for `entries` (primary first, all in one page)
/// from a snapshot of one to three consecutive pages containing it; `extra`
/// are further block starts (offsets from the snapshot's first page), such as
/// the targets that made neighbor pages part of it. Entries that do not start
/// a decodable block are not served.
pub fn compile_page(
    origin: &CompileRequest,
    snapshot: &ImmutableCodeSnapshot,
    entries: &[CpuEntryKey],
) -> Result<CompiledArtifact, CompileError> {
    let pages = snapshot.mappings.len();
    let Some(first) = snapshot.mappings.first().map(|m| m.linear)
    else {
        return Err(CompileError::InvalidIr("empty tier-0 snapshot".into()));
    };
    let primary = origin.linear.0 & !4095;
    if !(1..=3).contains(&pages)
        || snapshot.bytes.len() != pages * analysis::PAGE
        || snapshot.mappings.iter().enumerate().any(|(k, m)| {
            m.linear.0 != first.0.wrapping_add((k as u32) << 12)
                || !snapshot.dependencies.iter().any(|d| d.page == m.physical)
        })
        || primary.wrapping_sub(first.0) >= (pages as u32) << 12
    {
        return Err(CompileError::InvalidIr("invalid tier-0 page snapshot".into()));
    }
    let base = first;
    let base_pc = GuestEip(origin.pc.0.wrapping_sub(origin.linear.0.wrapping_sub(base.0)));
    if entries.is_empty()
        || entries.len() > 64
        || entries.iter().any(|e| {
            e.linear.0 & !4095 != primary
                || e.cs_base() != origin.cpu_entry().cs_base()
                || e.default_32 != origin.default_32
        })
    {
        return Err(CompileError::InvalidIr("invalid tier-0 page entries".into()));
    }
    let offset_of = |e: &CpuEntryKey| e.linear.0.wrapping_sub(base.0) as usize;
    let mut offsets: Vec<usize> = entries.iter().map(offset_of).collect();
    if pages > 1 {
        // The primary page's own branches into its neighbors seed their blocks.
        let alone = analysis::analyze(
            &snapshot.bytes[(primary - base.0) as usize..][..analysis::PAGE],
            GuestEip(base_pc.0.wrapping_add(primary - base.0)),
            LinearAddress(primary),
            origin.default_32,
            &entries.iter().map(|e| (e.linear.0 & 4095) as usize).collect::<Vec<_>>(),
            0..analysis::PAGE,
        );
        let shift = (primary - base.0) as i64;
        offsets.extend(
            alone
                .external
                .iter()
                .map(|t| t + shift)
                .filter(|&t| (0..snapshot.bytes.len() as i64).contains(&t))
                .map(|t| t as usize),
        );
    }
    let own = (primary - base.0) as usize;
    let plan = analysis::analyze(&snapshot.bytes, base_pc, base, origin.default_32, &offsets, own..own + analysis::PAGE);
    let served: Vec<CpuEntryKey> =
        entries.iter().copied().filter(|e| plan.block_at[offset_of(e)].is_some()).collect();
    if served.is_empty() {
        return Err(CompileError::Unsupported("no decodable tier-0 entry"));
    }
    // Specialize for the current state when it is flat 32-bit code of this
    // page's mode; the page function checks it on entry.
    let state = unsafe { *crate::cpu::global_pointers::state_flags };
    let flat = state.has_flat_segmentation() && state.ssize_32() && state.is_32() == origin.default_32;
    let mem8 = unsafe { crate::cpu::memory::mem8 as u32 };
    let hosts: Vec<u32> = snapshot.mappings.iter().map(|m| mem8.wrapping_add(m.physical.0)).collect();
    let code = emit::emit_page(&plan, base.0, &served, flat, &hosts);
    super::runtime::tier0::note_compiled(code.instructions, code.templated, code.bytes.len(), pages);
    let page_blocks = (0..pages)
        .map(|page| {
            std::array::from_fn(|word| {
                (0..64).fold(0u64, |bits, bit| {
                    bits | (plan.block_at[page * analysis::PAGE + word * 64 + bit].is_some() as u64) << bit
                })
            })
        })
        .collect();
    Ok(CompiledArtifact {
        key: origin.key,
        tier: Tier::One,
        dependencies: snapshot.dependencies.clone(),
        code: Artifact {
            bytes: code.bytes,
            locals: code.locals,
            structured_cfg: false,
            structured_backedges: 0,
            structured_edges: 0,
            generic_dispatch_edges: 0,
            budget_batch_blocks: 0,
        },
        passes: PassStats::default(),
        mir_folds: 0,
        guest_bytes: snapshot.bytes.len(),
        mappings: snapshot.mappings.clone(),
        entry: EntryContract::Cpu(served[0]),
        fused_sources: vec![],
        fused_edges: vec![],
        alternate_entries: served[1..].to_vec(),
        source_origin: Some(base),
        page_blocks: Some(page_blocks),
    })
}
