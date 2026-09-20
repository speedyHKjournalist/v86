//! Bounded automatic compilation at outer CPU dispatch points. Linked entries
//! contribute heat only; no compiler or publisher runs with guest locals alive.
use super::diagnostics::{CompileScope, Scope, Stage};
use super::{
    cache,
    compile::*,
    entry::CpuEntryKey,
    live::{self, Job},
    region,
};
use crate::{
    cpu::{cpu, global_pointers as gp},
    ir::backend::wasm::StateLayout,
    jit,
};
use std::{collections::{BTreeMap, VecDeque}, sync::Mutex};
#[derive(Clone, Copy)]
struct Config {
    enabled: bool,
    threshold: u32,
    promote: u32,
    window: u32,
    budget: u32,
    rep: u32,
}
struct Hot {
    entry: CpuEntryKey,
    hits: u32,
    source: Option<ImmutableCodeSnapshot>,
    failed: u32,
}
struct Pending {
    id: u64,
    slot: u32,
    entry: CpuEntryKey,
    tier: u32,
}
struct Scheduler {
    debug: crate::ir::debug::Config,
    opt_level: u32,
    passes_disabled: u32,
    config: Config,
    hot: Vec<Hot>,
    hot_index: BTreeMap<(u32, u32, bool), usize>,
    hot_hints: [u16; 256],
    cursor: usize,
    replacement: usize,
    pending: Option<Pending>,
    ready: VecDeque<Job>,
    credit: bool,
    stats: [u32; 18],
}
static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler {
    debug: crate::ir::debug::Config { verify: crate::ir::debug::VerifyMode::Debug, dump: crate::ir::debug::DumpMode::Off },
    opt_level: 2,
    passes_disabled: 0,
    config: Config {
        enabled: false,
        threshold: 16,
        promote: 64,
        window: 192,
        budget: 256,
        rep: 64,
    },
    hot: Vec::new(),
    hot_index: BTreeMap::new(),
    hot_hints: [u16::MAX; 256],
    cursor: 0,
    replacement: 0,
    pending: None,
    ready: VecDeque::new(),
    credit: false,
    stats: [0; 18],
});
#[link(wasm_import_module = "env")]
extern "C" {
    fn ir_codegen_finalize(id: u64, slot: u32, ptr: u32, len: u32);
}
pub fn invalidate() {
    let mut s = SCHEDULER.try_lock().unwrap();
    s.hot.clear();
    s.hot_index.clear();
    s.cursor = 0;
    s.replacement = 0;
    s.pending = None;
    s.ready.clear();
    s.credit = false;
}
pub fn dirty_page(page: u32) {
    let mut s = SCHEDULER.try_lock().unwrap();
    let previous_len = s.hot.len();
    s.ready.retain(|job| !job.artifact.dependencies.iter().any(|d| d.page.0 == page));
    s.hot.retain(|h| {
        !h.source
            .as_ref()
            .is_some_and(|source| source.dependencies.iter().any(|d| d.page.0 == page))
    });
    if s.hot.len() != previous_len {
        rebuild_hot_index(&mut s);
    }
}
fn hot_key(entry: CpuEntryKey) -> (u32, u32, bool) {
    (entry.linear.0, entry.pc.0, entry.default_32)
}
fn rebuild_hot_index(s: &mut Scheduler) {
    s.hot_index.clear();
    for (index, h) in s.hot.iter().enumerate() {
        s.hot_index.insert(hot_key(h.entry), index);
    }
    s.cursor = 0;
    s.replacement = 0;
}
pub fn enabled() -> bool { SCHEDULER.try_lock().unwrap().config.enabled }
/// Startup-only policy: no enabled scheduler, pending work or cached artifacts.
/// Snapshot restore/reset keep the destination policy, like backend selection.
#[no_mangle]
pub unsafe fn ir_auto_optimizations(level: u32, disabled: u32) -> bool {
    if level > 2 || disabled & !crate::ir::passes::PassConfig::MASK != 0
        || !cold() || cache::ir_cache_stat(1) != 0 { return false; }
    let mut s = SCHEDULER.try_lock().unwrap();
    if s.config.enabled || s.pending.is_some() || !s.ready.is_empty() { return false; }
    s.opt_level = level; s.passes_disabled = disabled;
    true
}
#[no_mangle]
pub unsafe fn ir_auto_debug(verify: u32, dump: u32) -> bool {
    let Some(config) = crate::ir::debug::Config::from_raw(verify, dump) else { return false; };
    if !cold() || cache::ir_cache_stat(1) != 0 { return false; }
    let mut s = SCHEDULER.try_lock().unwrap();
    if s.config.enabled || s.pending.is_some() || !s.ready.is_empty() { return false; }
    s.debug = config;
    crate::ir::debug::ir_dump_clear();
    true
}
#[no_mangle]
pub fn ir_auto_optimization_stat(field: u32) -> u32 {
    let s = SCHEDULER.try_lock().unwrap();
    match field { 0 => s.opt_level, 1 => s.passes_disabled, 2 => s.debug.verify as u32, 3 => s.debug.dump as u32, _ => 0 }
}
pub fn begin_frame() {
    SCHEDULER.try_lock().unwrap().credit = true;
}
/// An evicted region must earn fresh heat. Historical visits must not make a
/// working set larger than the cache continually recompile inactive entries.
pub(super) fn evicted(entry: CpuEntryKey) {
    if let Some(h) = SCHEDULER
        .try_lock()
        .unwrap()
        .hot
        .iter_mut()
        .find(|h| h.entry == entry)
    {
        h.hits = 0;
    }
}
unsafe fn cold() -> bool {
    !cpu::in_jit && !cache::busy() && jit::ir_cache_quiescent()
}
#[no_mangle]
pub unsafe fn ir_auto_config(
    enabled: u32,
    threshold: u32,
    promote: u32,
    window: u32,
    budget: u32,
    rep: u32,
) -> bool {
    if !cold()
        || enabled > 1
        || !(1..=1_000_000).contains(&threshold)
        || !(1..=1_000_000).contains(&promote)
        || !(15..=960).contains(&window)
        || !(1..=4096).contains(&budget)
        || !(1..=4096).contains(&rep)
    {
        return false;
    }
    let pending = {
        let mut s = SCHEDULER.try_lock().unwrap();
        s.config = Config {
            enabled: enabled != 0,
            threshold,
            promote,
            window,
            budget,
            rep,
        };
        s.hot.clear();
        s.ready.clear();
        s.hot_index.clear();
        s.cursor = 0;
        s.replacement = 0;
        s.credit = false;
        s.pending.take()
    };
    if let Some(p) = pending {
        cache::ir_cache_cancel(p.id, p.slot);
        cache::ir_cache_collect();
    }
    true
}
unsafe fn record(entry: CpuEntryKey) {
    let mut s = SCHEDULER.try_lock().unwrap();
    if !s.config.enabled || *gp::prefixes != 0 || *gp::in_hlt {
        return;
    }
    // Profile the exact same visits, but bypass tree lookup for stable hot PCs.
    // Replacement/compaction cannot give the hint authority: compare full entry.
    let hint = ((entry.linear.0 >> 1 ^ entry.pc.0 >> 12) & 255) as usize;
    let saved = s.hot_hints[hint] as usize;
    let index = if s.hot.get(saved).is_some_and(|h| h.entry == entry) { Some(saved) }
        else { s.hot_index.get(&hot_key(entry)).copied() };
    if let Some(index) = index {
        s.hot_hints[hint] = index as u16;
        let h = &mut s.hot[index];
        h.hits = h.hits.saturating_add(1);
    } else {
        let new = Hot {
            entry,
            hits: 1,
            source: None,
            failed: 0,
        };
        let index = if s.hot.len() == 128 {
            let index = s.replacement;
            s.replacement = (index + 1) % 128;
            let old = std::mem::replace(&mut s.hot[index], new);
            s.hot_index.remove(&hot_key(old.entry));
            index
        } else {
            let index = s.hot.len();
            s.hot.push(new);
            index
        };
        s.hot_index.insert(hot_key(entry), index);
        s.hot_hints[hint] = index as u16;
    }
}
/// Admission already knows whether this entry can benefit from more heat.
/// Finished Tier-2 traces must not evict unpublished PCs from the small hot ring.
pub unsafe fn note_cached(entry: CpuEntryKey, linked: bool, needs_heat: bool) {
    if linked {
        let mut s = SCHEDULER.try_lock().unwrap();
        if s.config.enabled { s.stats[1] = s.stats[1].wrapping_add(1); }
    }
    if needs_heat { record(entry); }
}
pub unsafe fn note_interpreted() { record(live::entry()); }
pub unsafe fn note_legacy_link() { note_cached(live::entry(), true, true); }
/// Tier-aware reachable-CFG source selection. Direct targets outside the
/// bounded immutable window remain explicit exits in the shared frontend.
unsafe fn source(entry: CpuEntryKey, window: u32, tier: u32) -> Option<ImmutableCodeSnapshot> {
    let _clock = CompileScope::new(1);
    region::capture_region(entry, if tier == 1 { Tier::One } else { Tier::Two }, window)
}
fn same(a: &ImmutableCodeSnapshot, b: &ImmutableCodeSnapshot) -> bool {
    a.bytes == b.bytes && a.mappings == b.mappings
}
fn failed(entry: CpuEntryKey, tier: u32, source: Option<ImmutableCodeSnapshot>) {
    let mut s = SCHEDULER.try_lock().unwrap();
    s.stats[6] = s.stats[6].wrapping_add(1);
    if let Some(h) = s.hot.iter_mut().find(|h| h.entry == entry) {
        h.source = source;
        h.failed = tier;
    }
}
pub unsafe fn visit() {
    {
        let mut s = SCHEDULER.try_lock().unwrap();
        if s.config.enabled && *gp::prefixes == 0 && !*gp::in_hlt {
            s.stats[0] = s.stats[0].wrapping_add(1);
        }
        if !s.config.enabled || !s.credit || s.pending.is_some() {
            return;
        }
    }
    if !cold() {
        return;
    }
    let ready = {
        let mut s = SCHEDULER.try_lock().unwrap();
        let ready = s.ready.pop_front();
        if ready.is_some() { s.credit = false; }
        ready
    };
    if let Some(job) = ready {
        publish(job);
        return;
    }
    let selected = {
        let mut s = SCHEDULER.try_lock().unwrap();
        if !s.config.enabled || !s.credit || s.pending.is_some() {
            return;
        }
        // Bound candidate scanning even when no entry is ready. Heat continues
        // accumulating during the frame; the next frame can compile it.
        s.credit = false;
        let config = s.config;
        let mut selected = None;
        for _ in 0..s.hot.len() {
            let index = s.cursor;
            s.cursor = (s.cursor + 1) % s.hot.len();
            let h = &s.hot[index];
            let tier = cache::tier(h.entry);
            let needed = if tier == 0 { config.threshold } else { config.promote };
            if selected.is_none() && h.hits >= needed &&
                (tier < 2 || cache::fusion_ready(h.entry)) {
                selected = Some((h.entry, (tier + 1).min(2), config));
            }
            if selected.is_some() {
                break;
            }
        }
        selected
    };
    let Some((entry, tier, config)) = selected else {
        return;
    };
    let fusion_only = cache::tier(entry) == 2;
    let snapshot = source(entry, config.window, tier);
    let suppressed = {
        let mut s = SCHEDULER.try_lock().unwrap();
        let same = s.hot.iter().find(|h| h.entry == entry).is_some_and(|h| {
            h.failed == tier
                && match (&h.source, &snapshot) {
                    (Some(a), Some(b)) => same(a, b),
                    (None, None) => true,
                    _ => false,
                }
        });
        if same {
            s.stats[8] = s.stats[8].wrapping_add(1);
        }
        same
    };
    if suppressed {
        return;
    }
    let Some(snapshot) = snapshot else {
        failed(entry, tier, None);
        return;
    };
    if !cache::can_make_room(entry) {
        return;
    }
    let Some(key) = live::publication_key() else {
        failed(entry, tier, Some(snapshot));
        return;
    };
    {
        let mut s = SCHEDULER.try_lock().unwrap();
        s.stats[tier as usize + 1] = s.stats[tier as usize + 1].wrapping_add(1);
    }
    let compile_tier = if tier == 1 { Tier::One } else { Tier::Two };
    let region_policy = region::Policy::for_tier(compile_tier, config.window);
    let request = CompileRequest {
        key,
        pc: entry.pc,
        linear: entry.linear,
        default_32: entry.default_32,
        tier: compile_tier,
    };
    // At most one nearby hot peer per batch: share immutable code capture while
    // bounding cold compilation latency and keeping browser publication serial.
    let peer = {
        let s = SCHEDULER.try_lock().unwrap();
        s.hot.iter().find(|h| {
            h.entry.cs_base() == entry.cs_base()
                && h.entry.default_32 == entry.default_32
                && h.entry.linear.0.wrapping_sub(entry.linear.0) > 0
                && (h.entry.linear.0.wrapping_sub(entry.linear.0) as usize) < snapshot.bytes.len()
                && h.hits >= if tier == 1 { config.threshold } else { config.promote }
                && h.failed != tier && cache::tier(h.entry) + 1 == tier
        }).map(|h| h.entry)
    };
    let mut entries = vec![CpuEntryRequest { offset: 0, key }];
    if let Some(peer) = peer {
        if let Some(key) = live::publication_key() {
            entries.push(CpuEntryRequest {
                offset: peer.linear.0.wrapping_sub(entry.linear.0) as usize, key,
            });
        }
    }
    let (opt_level, disabled, debug) = { let s = SCHEDULER.try_lock().unwrap(); (s.opt_level, s.passes_disabled, s.debug) };
    let config = IrConfig {
        optimize: opt_level != 0,
        passes: crate::ir::passes::PassConfig { debug, ..(if tier == 1 || opt_level == 1 {
            crate::ir::passes::PassConfig::tier1()
        } else {
            Default::default()
        }).disable(disabled) },
        execution_budget: config.budget,
        rep_iteration_budget: config.rep,
        max_code_bytes: region_policy.max_bytes,
        layout: StateLayout {
            gpr: 0,
            flags: 32,
            eip: 36,
            committed: 40,
            flag_operand: 44,
        },
    };
    let compile_scope = Scope::new(Stage::Compile);
    let compile_clock = CompileScope::new(0);
    let started = crate::profiler::performance_codegen_start();
    let fusion = if tier == 2 { cache::take_fusion(entry) } else { None };
    let fused = fusion.map(|(primary, peer, edges)|
        compile_cpu_fused_regions(&request, &primary, &peer, &edges, &config)
            .map(|artifact| vec![(artifact, primary, 0)]));
    if let Some(result) = &fused {
        let mut s = SCHEDULER.try_lock().unwrap();
        s.stats[14] = s.stats[14].wrapping_add(1);
        if let Err(error) = result {
            let index = match error {
                crate::ir::lowering::CompileError::Budget(_) => 15,
                crate::ir::lowering::CompileError::Unsupported(_) => 16,
                crate::ir::lowering::CompileError::InvalidIr(_) => 17,
            };
            s.stats[index] = s.stats[index].wrapping_add(1);
        }
    }
    let compiled = if let Some(Ok(fused)) = fused {
        Ok(fused)
    } else if fusion_only {
        crate::profiler::performance_codegen_finish(started);
        return; // One failed fusion attempt leaves the working Tier 2 intact.
    } else if entries.len() > 1 {
        compile_cpu_entries_bounded(&request, &snapshot, &entries, &config)
            // A difficult peer must not prevent a valid primary from publishing.
            .or_else(|_| compile_cpu_cfg_bounded(&request, &snapshot, &config).map(|a| vec![a]))
    } else {
        compile_cpu_cfg_bounded(&request, &snapshot, &config).map(|a| vec![a])
    };
    crate::profiler::performance_codegen_finish(started);
    drop(compile_clock);
    drop(compile_scope);
    let compiled = match compiled {
        Ok(compiled) => compiled,
        Err(error) => {
            use crate::ir::lowering::CompileError;
            let field = match error {
                CompileError::Unsupported(_) => 9,
                CompileError::Budget(_) => 10,
                CompileError::InvalidIr(_) => 11,
            };
            {
                let mut s = SCHEDULER.try_lock().unwrap();
                s.stats[field] = s.stats[field].wrapping_add(1);
            }
            failed(entry, tier, Some(snapshot));
            return;
        },
    };
    {
        let mut s = SCHEDULER.try_lock().unwrap();
        let peers = compiled.len() as u32 - 1;
        s.stats[tier as usize + 1] = s.stats[tier as usize + 1].wrapping_add(peers);
        s.stats[13] = s.stats[13].wrapping_add(peers);
        for (artifact, source, retries) in compiled {
            s.stats[12] = s.stats[12].wrapping_add(retries);
            if let super::entry::EntryContract::Cpu(compiled_entry) = artifact.entry {
                if let Some(h) = s.hot.iter_mut().find(|h| h.entry == compiled_entry) {
                    h.source = Some(source.clone());
                    h.failed = 0;
                }
            }
            s.ready.push_back(Job {
                observed: artifact.dependencies.clone(), artifact, source, stale: false,
            });
        }
        if let Some(h) = s.hot.iter_mut().find(|h| h.entry == entry) {
            h.source = Some(snapshot);
            h.failed = 0;
        }
    }
    let job = SCHEDULER.try_lock().unwrap().ready.pop_front().unwrap();
    publish(job);
}
unsafe fn publish(job: Job) {
    let super::entry::EntryContract::Cpu(entry) = job.artifact.entry else { return; };
    let tier = if job.artifact.tier == Tier::One { 1 } else { 2 };
    let key = job.artifact.key;
    if !live::generation_current(key) { return; }
    if !cache::make_room(entry) {
        return;
    }
    let ptr = job.artifact.code.bytes.as_ptr() as u32;
    let len = job.artifact.code.bytes.len() as u32;
    // Keep the original capture fingerprint for bounded-prefix compilation:
    // a browser rejection must not retry just because the artifact is shorter.
    let snapshot = SCHEDULER.try_lock().unwrap().hot.iter()
        .find(|h| h.entry == entry).and_then(|h| h.source.clone())
        .unwrap_or_else(|| job.source.clone());
    let slot = cache::reserve_job(job, true);
    if slot == 0 {
        failed(entry, tier, Some(snapshot));
        return;
    }
    {
        let mut s = SCHEDULER.try_lock().unwrap();
        s.pending = Some(Pending {
            id: key.job,
            slot,
            entry,
            tier,
        });
    }
    // JS copies bytes now; all Rust locks have been released. Completion is a
    // later microtask, never recursive compilation/publication in this frame.
    super::entry::ir_admission_barrier();
    ir_codegen_finalize(key.job, slot, ptr, len);
}
#[no_mangle]
pub fn ir_auto_complete(id: u64, success: u32) {
    let state = cache::completion_state(id);
    if state == 2 || success != 1 && state == 1 {
        return;
    }
    let success = success == 1 && state == 1;
    let mut s = SCHEDULER.try_lock().unwrap();
    if !s.pending.as_ref().is_some_and(|p| p.id == id) {
        return;
    }
    let p = s.pending.take().unwrap();
    let stat = if success { p.tier as usize + 3 } else { 7 };
    s.stats[stat] = s.stats[stat].wrapping_add(1);
    if let Some(h) = s.hot.iter_mut().find(|h| h.entry == p.entry) {
        h.hits = 0;
        h.failed = if success { 0 } else { p.tier };
    }
}
#[no_mangle]
pub fn ir_auto_stat(field: u32) -> u32 {
    let s = SCHEDULER.try_lock().unwrap();
    match field {
        0..=8 => s.stats[field as usize],
        9 => s.hot.len() as u32,
        10 => s.pending.is_some() as u32,
        11 => s.config.enabled as u32,
        12..=15 => s.stats[field as usize - 3],
        16 => s.stats[13],
        17 => s.ready.len() as u32,
        18..=21 => s.stats[field as usize - 4],
        _ => 0,
    }
}
