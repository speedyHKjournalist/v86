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
use std::{collections::VecDeque, sync::Mutex};
use super::hot_index::HotIndex;
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
    discovered: Option<f64>,
}
struct Pending {
    id: u64,
    slot: u32,
    entries: Vec<(CpuEntryKey, Option<f64>)>,
    tier: u32,
}
struct Scheduler {
    debug: crate::ir::debug::Config,
    opt_level: u32,
    passes_disabled: u32,
    config: Config,
    hot: Vec<Hot>,
    hot_index: HotIndex,
    hot_hints: [u16; 256],
    hot_filter: bool,
    probation: [Option<(CpuEntryKey, Option<f64>)>; 256],
    cursor: usize,
    replacement: usize,
    pending: Option<Pending>,
    ready: VecDeque<Job>,
    credit: bool,
    scan_credit: bool,
    interpreted_probe: bool,
    stats: [u32; 24],
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
    hot_index: HotIndex::new(),
    hot_hints: [u16::MAX; 256],
    hot_filter: false,
    probation: [None; 256],
    cursor: 0,
    replacement: 0,
    pending: None,
    ready: VecDeque::new(),
    credit: false,
    scan_credit: false,
    interpreted_probe: false,
    stats: [0; 24],
});
#[link(wasm_import_module = "env")]
extern "C" {
    fn ir_codegen_finalize(id: u64, slot: u32, ptr: u32, len: u32);
}
pub fn invalidate() {
    let mut s = SCHEDULER.try_lock().unwrap();
    s.hot.clear();
    s.hot_index.clear();
    s.probation.fill(None);
    s.cursor = 0;
    s.replacement = 0;
    s.pending = None;
    s.ready.clear();
    s.credit = false;
    s.scan_credit = false;
    s.interpreted_probe = false;
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
fn rebuild_hot_index(s: &mut Scheduler) {
    s.hot_index.clear();
    for (index, h) in s.hot.iter().enumerate() {
        s.hot_index.insert(h.entry, index);
    }
    s.cursor = 0;
    s.replacement = 0;
}
pub fn enabled() -> bool { SCHEDULER.try_lock().unwrap().config.enabled }
/// Startup-only experiment: admission filtering can save bookkeeping while
/// increasing compilation of marginally hot PCs. Keep it opt-in after XP A/B.
#[no_mangle]
pub unsafe fn ir_auto_set_hot_filter(enabled: u32) -> bool {
    if enabled > 1 || !cold() { return false; }
    let mut s = SCHEDULER.try_lock().unwrap();
    if !s.hot.is_empty() || s.pending.is_some() || !s.ready.is_empty() { return false; }
    s.hot_filter = enabled != 0; s.probation.fill(None); true
}
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
    let mut s = SCHEDULER.try_lock().unwrap();
    s.credit = true;
    s.scan_credit = true;
    s.interpreted_probe = false;
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
        s.probation.fill(None);
        s.cursor = 0;
        s.replacement = 0;
        s.credit = false;
        s.scan_credit = false;
        s.interpreted_probe = false;
        s.pending.take()
    };
    if let Some(p) = pending {
        cache::ir_cache_cancel(p.id, p.slot);
        cache::ir_cache_collect();
    }
    true
}
unsafe fn record(entry: CpuEntryKey, interpreted: bool) {
    let mut s = SCHEDULER.try_lock().unwrap();
    if !s.config.enabled || *gp::prefixes != 0 || *gp::in_hlt {
        return;
    }
    // Only new interpreted work can justify another current-entry probe after
    // the frame's bounded ring scan. Cached Tier-2 activations must stay cheap.
    s.interpreted_probe |= interpreted;
    // Profile the exact same visits, but bypass tree lookup for stable hot PCs.
    // Replacement/compaction cannot give the hint authority: compare full entry.
    let hint = ((entry.linear.0 >> 1 ^ entry.pc.0 >> 12) & 255) as usize;
    let saved = s.hot_hints[hint] as usize;
    let index = if s.hot.get(saved).is_some_and(|h| h.entry == entry) { Some(saved) }
        else { s.hot_index.get(entry) };
    if let Some(index) = index {
        s.hot_hints[hint] = index as u16;
        let h = &mut s.hot[index];
        h.hits = h.hits.saturating_add(1);
    } else {
        // A single-use PC does not displace a recurrent entry. The witness owns
        // no code and carries no admission authority; count both observed visits
        // when promoting it into the hot ring. Threshold-one tests remain exact.
        let witness = s.probation[hint].filter(|(key, _)| *key == entry);
        if s.hot_filter && s.config.threshold > 1 && witness.is_none() {
            s.probation[hint] = Some((entry, super::diagnostics::discovery_start()));
            s.stats[19] = s.stats[19].wrapping_add(1);
            return;
        }
        s.probation[hint] = None;
        let new = Hot {
            entry,
            hits: if witness.is_some() { 2 } else { 1 },
            source: None,
            failed: 0,
            discovered: witness.and_then(|(_, start)| start).or_else(super::diagnostics::discovery_start),
        };
        let index = if s.hot.len() == 128 {
            let index = s.replacement;
            s.replacement = (index + 1) % 128;
            let old = std::mem::replace(&mut s.hot[index], new);
            s.hot_index.remove(old.entry);
            s.stats[18] = s.stats[18].wrapping_add(1);
            index
        } else {
            let index = s.hot.len();
            s.hot.push(new);
            index
        };
        s.hot_index.insert(entry, index);
        s.hot_hints[hint] = index as u16;
    }
}
/// Admission already knows whether this entry can benefit from more heat.
/// Finished Tier-2 traces must not evict unpublished PCs from the small hot ring.
#[inline(always)]
pub unsafe fn note_cached(entry: CpuEntryKey, linked: bool, needs_heat: bool) {
    if linked {
        let mut s = SCHEDULER.try_lock().unwrap();
        if s.config.enabled { s.stats[1] = s.stats[1].wrapping_add(1); }
    }
    if needs_heat { record(entry, false); }
}
pub unsafe fn note_interpreted() { record(live::entry(), true); }
pub(super) fn diagnose_missing(entry: CpuEntryKey) {
    let s = SCHEDULER.try_lock().unwrap();
    let reason = if s.pending.as_ref().is_some_and(|p| p.entries.iter().any(|(key, _)| *key == entry))
        || s.ready.iter().any(|j| j.artifact.accepts_entry(entry)) { 3 }
        else { match s.hot_index.get(entry).map(|i| &s.hot[i]) {
            None => {
                let hint = ((entry.linear.0 >> 1 ^ entry.pc.0 >> 12) & 255) as usize;
                u32::from(s.probation[hint].is_some_and(|(key, _)| key == entry)) as usize
            }, Some(h) if h.failed != 0 => 4,
            Some(h) if h.hits < s.config.threshold => 1, Some(_) => 2,
        } };
    super::diagnostics::missing(reason);
}
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
/// True only when this call submitted a new module to the asynchronous host
/// installer. The CPU may hand off once at this cold point; an existing pending
/// Promise returns false, so delayed/failed compilation never stalls the guest.
pub unsafe fn visit() -> bool {
    {
        let mut s = SCHEDULER.try_lock().unwrap();
        if s.config.enabled && *gp::prefixes == 0 && !*gp::in_hlt {
            s.stats[0] = s.stats[0].wrapping_add(1);
        }
        if !s.config.enabled || s.pending.is_some() {
            return false;
        }
        // There is no work requiring a quiescence check once compilation or
        // scan credit is exhausted. Ready artifacts have a separate publication
        // budget and MUST bypass this rejection (including sibling entries).
        // This is only a negative work hint, never execution/publication authority.
        if s.ready.is_empty() && (!s.credit || !s.scan_credit && !s.interpreted_probe) {
            s.stats[22] = s.stats[22].wrapping_add(1);
            return false;
        }
        s.stats[23] = s.stats[23].wrapping_add(1);
    }
    if !cold() {
        return false;
    }
    let ready = {
        let mut s = SCHEDULER.try_lock().unwrap();
        // Publication and compilation have different budgets. An already
        // compiled sibling must not wait another frame merely to reach JS.
        s.ready.pop_front()
    };
    if let Some(job) = ready {
        return publish(job);
    }
    let selected = {
        let mut s = SCHEDULER.try_lock().unwrap();
        if !s.config.enabled || !s.credit || s.pending.is_some()
            || !s.scan_credit && !s.interpreted_probe {
            return false;
        }
        s.interpreted_probe = false;
        // A fruitless scan must not consume the frame's compilation credit.
        // Bound full-ring scans separately, but still admit the currently
        // interpreted entry as soon as it actually reaches its heat threshold.
        let config = s.config;
        let current = live::entry();
        let mut selected = s.hot_index.get(current).and_then(|index| {
            let hot = &s.hot[index];
            (hot.hits >= config.threshold && hot.failed == 0 && cache::tier(current) == 0)
                .then_some((current, 1, config))
        });
        if selected.is_none() && s.scan_credit {
            s.scan_credit = false;
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
                if selected.is_some() { break; }
            }
        }
        if selected.is_some() { s.credit = false; }
        selected
    };
    let Some((entry, tier, config)) = selected else {
        return false;
    };
    let _compile_context = super::diagnostics::CompileContext::new(entry.linear.0, tier);
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
        return false;
    }
    let Some(snapshot) = snapshot else {
        failed(entry, tier, None);
        return false;
    };
    if !cache::can_make_room(entry) {
        return false;
    }
    let Some(key) = live::publication_key() else {
        failed(entry, tier, Some(snapshot));
        return false;
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
    // Tier 1 can share capture across four hot entries; total sibling source
    // bytes stay within one window, no more aggregate input than the old pair.
    // Tier 2 retains the two-entry limit for its more expensive machine passes.
    let peers = {
        let s = SCHEDULER.try_lock().unwrap();
        let mut candidates: Vec<_> = s.hot.iter().filter(|h| {
            h.entry.cs_base() == entry.cs_base()
                && h.entry.default_32 == entry.default_32
                && h.entry.linear.0.wrapping_sub(entry.linear.0) > 0
                && (h.entry.linear.0.wrapping_sub(entry.linear.0) as usize) < snapshot.bytes.len()
                && h.hits >= if tier == 1 { config.threshold } else { config.promote }
                && h.failed != tier && cache::tier(h.entry) + 1 == tier
        }).map(|h| (h.entry, h.hits)).collect();
        candidates.sort_by_key(|(entry, hits)| (std::cmp::Reverse(*hits), entry.linear.0));
        let mut bytes = 0;
        candidates.into_iter().filter_map(|(peer, _)| {
            let length = snapshot.bytes.len() - peer.linear.0.wrapping_sub(entry.linear.0) as usize;
            if bytes + length > snapshot.bytes.len() { return None; }
            bytes += length; Some(peer)
        }).take(if tier == 1 { 3 } else { 1 }).collect::<Vec<_>>()
    };
    let mut entries = vec![CpuEntryRequest { offset: 0, key }];
    for peer in peers {
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
        return false; // One failed fusion attempt leaves the working Tier 2 intact.
    } else if entries.len() > 1 {
        match compile_cpu_shared_entries(&request, &snapshot, &entries, &config) {
            Ok(artifact) => Ok(vec![(artifact, snapshot.clone(), 0)]),
            // Overlapping x86 streams, graph budgets and unsupported side
            // entries retain independent, fully validated suffix artifacts.
            Err(_) => compile_cpu_entries_available(&request, &snapshot, &entries, &config),
        }
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
            return false;
        },
    };
    {
        let mut s = SCHEDULER.try_lock().unwrap();
        let peers = compiled.len() as u32 - 1;
        s.stats[tier as usize + 1] = s.stats[tier as usize + 1].wrapping_add(peers);
        s.stats[13] = s.stats[13].wrapping_add(peers);
        for (artifact, source, retries) in compiled {
            if !artifact.alternate_entries.is_empty() {
                s.stats[20] = s.stats[20].wrapping_add(1);
                s.stats[21] = s.stats[21].wrapping_add(artifact.alternate_entries.len() as u32);
            }
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
    publish(job)
}
unsafe fn publish(job: Job) -> bool {
    let super::entry::EntryContract::Cpu(entry) = job.artifact.entry else { return false; };
    let tier = if job.artifact.tier == Tier::One { 1 } else { 2 };
    let key = job.artifact.key;
    if !live::generation_current(key) { return false; }
    if !cache::make_room(entry) {
        return false;
    }
    let members = job.artifact.cpu_entries().collect::<Vec<_>>();
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
        return false;
    }
    {
        let mut s = SCHEDULER.try_lock().unwrap();
        s.pending = Some(Pending {
            id: key.job,
            slot,
            entries: members.into_iter().map(|key| {
                let discovered = s.hot_index.get(key).and_then(|i| s.hot[i].discovered);
                (key, discovered)
            }).collect(),
            tier,
        });
    }
    // JS copies bytes now; all Rust locks have been released. Completion is a
    // later microtask, never recursive compilation/publication in this frame.
    super::entry::ir_admission_barrier();
    ir_codegen_finalize(key.job, slot, ptr, len);
    true
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
    if success {
        for (_, discovered) in &p.entries { super::diagnostics::discovery_complete(*discovered, p.tier); }
    }
    let stat = if success { p.tier as usize + 3 } else { 7 };
    s.stats[stat] = s.stats[stat].wrapping_add(1);
    for hot in &mut s.hot {
        if p.entries.iter().any(|(key, _)| *key == hot.entry) {
            hot.hits = 0;
            hot.failed = if success { 0 } else { p.tier };
        }
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
        22 => s.stats[18],
        23 => s.stats[19],
        24 => u32::from(s.hot_filter),
        25 => s.stats[20], // compiled shared functions (publication counted separately)
        26 => s.stats[21], // additional entries supplied by shared functions
        27 => s.stats[22], // idle visits rejected before cache/jit quiescence checks
        28 => s.stats[23], // visits requiring the original cold-work path
        _ => 0,
    }
}
