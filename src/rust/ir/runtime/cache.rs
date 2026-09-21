//! Explicit publication into the shared table pool and cold CPU dispatch.
//! No lock or artifact reference survives a guest activation. Dirty/reset hooks
//! retire immediately; slot reuse waits until that activation has returned.
use super::diagnostics::{self as diag, Admission, Scope, Stage};
use super::{
    compile::CapturedRegion,
    entry::{admission_epoch, ir_admission_barrier, ir_entry_matches, EntryContract},
    live::{self, Job},
    snapshot::{capture, cached_match, mappings_cached, CachedMatch},
};
use crate::ir::frontend::{decode::GuestEip, region::PredictedEdge};
use crate::{
    cpu::{cpu, global_pointers as gp},
    jit,
    page::Page,
    profiler,
};
use std::{collections::BTreeMap, sync::Mutex};
type EntryIndexKey = (u32, u32, bool);
fn index_key(entry: super::entry::CpuEntryKey) -> EntryIndexKey {
    (entry.linear.0, entry.pc.0, entry.default_32)
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Pending,
    Validated,
    Published,
    Retired,
}
#[derive(Clone, Copy)]
struct Owner { index: usize, id: u64 }
#[derive(Clone, Copy)]
struct Successor { key: EntryIndexKey, owner: Owner }
struct Record {
    /// Active index aliases. A replacement may supersede one entry without
    /// destroying a shared owner still serving its other entries.
    entries: Vec<super::entry::CpuEntryKey>,
    job: Job,
    slot: u32,
    phase: Phase,
    automatic: bool,
    /// Monotonic use stamp for deterministic cold-point eviction.
    last_used: u64,
    hits: u32,
    guest_steps: u32,
    max_guest_steps: u32,
    zero_step_exits: u32,
    validated_epoch: u64,
    hot_exit: Option<(PredictedEdge, super::entry::CpuEntryKey, u32)>,
    fusion_attempted: bool,
    /// Scheduling hint only. Full eligibility and source validation remain at
    /// cold selection. Refreshed when heat or the published owner set changes.
    fusion_candidate: bool,
    successor: Option<Successor>,
}
struct Cache {
    records: Vec<Record>,
    capacity: usize,
    evictions: u32,
    published: BTreeMap<EntryIndexKey, usize>,
    targets: [Option<(EntryIndexKey, usize)>; 64],
    missing_targets: [Option<EntryIndexKey>; 64],
    negative_hits: u32,
    fast_validation: bool,
    fast_checks: u32,
    full_checks: u32,
    post_fetch_reuses: u32,
    target_hits: u32,
    successor_hits: u32,
    fusion_enabled: bool,
    fused_publications: u32,
    shared_publications: u32,
    fused_hits: u32,
    fused_steps: u32,
    needs_collection: bool,
    active: bool,
    active_owner: Option<Owner>,
    observer_checks: u32,
    observer_rejections: u32,
    hits: u32,
    rejected: u32,
    failed: u32,
    reclaimed: u32,
    clock: u64,
    links: u32,
    link_misses: u32,
    cached_checks: u32,
    capture_fallbacks: u32,
    guest_steps: u32,
    max_guest_steps: u32,
    zero_step_exits: u32,
    structured_publications: u32,
    generic_publications: u32,
    structured_backedges: u32,
    structured_edges: u32,
    generic_dispatch_edges: u32,
}
static CACHE: Mutex<Cache> = Mutex::new(Cache {
    records: Vec::new(),
    capacity: 256,
    evictions: 0,
    published: BTreeMap::new(),
    targets: [None; 64],
    missing_targets: [None; 64],
    negative_hits: 0,
    fast_validation: true,
    fast_checks: 0,
    full_checks: 0,
    post_fetch_reuses: 0,
    target_hits: 0,
    successor_hits: 0,
    fusion_enabled: true,
    fused_publications: 0,
    shared_publications: 0,
    fused_hits: 0,
    fused_steps: 0,
    needs_collection: false,
    active: false,
    active_owner: None,
    observer_checks: 0,
    observer_rejections: 0,
    hits: 0,
    rejected: 0,
    failed: 0,
    reclaimed: 0,
    clock: 0,
    links: 0,
    link_misses: 0,
    cached_checks: 0,
    capture_fallbacks: 0,
    guest_steps: 0,
    max_guest_steps: 0,
    zero_step_exits: 0,
    structured_publications: 0,
    generic_publications: 0,
    structured_backedges: 0,
    structured_edges: 0,
    generic_dispatch_edges: 0,
});
#[no_mangle]
pub fn ir_cache_capacity() -> u32 { CACHE.try_lock().unwrap().capacity as u32 }
/// Offline compiler replay inspection. The publication bridge copies these
/// immutable inputs synchronously; no pointer may survive a cache mutation.
/// This API never captures guest memory or grants execution authority.
#[no_mangle]
pub fn ir_cache_replay_info(id: u64, group: u32, index: u32, field: u32) -> u32 {
    let cache = CACHE.try_lock().unwrap();
    let Some(record) = cache.records.iter().find(|r| r.job.artifact.key.job == id) else { return 0; };
    let job = &record.job;
    let EntryContract::Cpu(entry) = job.artifact.entry else { return 0; };
    let source = |i: usize| if i == 0 { Some((entry, &job.source)) }
        else { job.artifact.fused_sources.get(i - 1).map(|s| (s.entry, &s.source)) };
    match group {
        0 => match field {
            0 => 1 + job.artifact.fused_sources.len() as u32,
            1 => job.artifact.alternate_entries.len() as u32,
            2 => job.artifact.fused_edges.len() as u32,
            3 => if job.artifact.tier == super::compile::Tier::One {1} else {2},
            4 => entry.default_32 as u32,
            _ => 0,
        },
        1 => source(index as usize).map_or(0, |(entry, source)| match field {
            0 => entry.pc.0, 1 => entry.linear.0,
            2 => source.bytes.as_ptr() as u32, 3 => source.bytes.len() as u32,
            4 => source.mappings.len() as u32, _ => 0,
        }),
        2 => job.artifact.alternate_entries.get(index as usize).map_or(0, |e| e.pc.0.wrapping_sub(entry.pc.0)),
        3 => job.artifact.fused_edges.get(index as usize).map_or(0, |e| if field == 0 {e.from.0} else {e.target.0}),
        4 => source((index >> 16) as usize).and_then(|(_, s)| s.mappings.get((index & 65535) as usize))
            .map_or(0, |m| if field == 0 {m.linear.0} else {m.physical.0}),
        _ => 0,
    }
}
/// Host policy for bounded working-set experiments; does not grow the table pool.
#[no_mangle]
pub unsafe fn ir_cache_set_capacity(capacity: u32) -> bool {
    if !(256..=768).contains(&capacity) || !cold() { return false; }
    let mut cache=CACHE.try_lock().unwrap();
    if cache.records.len()>capacity as usize { return false; }
    cache.capacity=capacity as usize; true
}
extern "C" {
    fn call_indirect1(f: i32, x: u16);
}
pub fn busy() -> bool {
    CACHE.try_lock().unwrap().active
}
pub fn invalidate() {
    ir_admission_barrier();
    let mut cache = CACHE.try_lock().unwrap();
    for record in &mut cache.records {
        record.phase = Phase::Retired;
    }
    cache.needs_collection = true;
}
pub fn dirty_page(page: u32) {
    super::entry::code_write_barrier();
    let mut cache = CACHE.try_lock().unwrap();
    let mut retired = false;
    for record in &mut cache.records {
        if record
            .job
            .artifact
            .dependencies
            .iter()
            .any(|d| d.page.0 == page)
        {
            record.phase = Phase::Retired;
            retired = true;
        }
    }
    cache.needs_collection |= retired;
}
unsafe fn cold() -> bool {
    !cpu::in_jit && !busy() && jit::ir_cache_quiescent()
}
/// Diagnostic A/B switch; disabling restores full pre/post-fetch validation.
#[no_mangle]
pub unsafe fn ir_cache_set_fast_validation(enabled: u32) -> bool {
    if enabled > 1 || !cold() { return false; }
    ir_admission_barrier();
    CACHE.try_lock().unwrap().fast_validation = enabled != 0;
    true
}
fn target(cache: &mut Cache, key: EntryIndexKey) -> Option<usize> {
    let slot = ((key.0 >> 1 ^ key.0 >> 12 ^ key.1) & 63) as usize;
    if let Some((saved, index)) = cache.targets[slot] {
        if saved == key && cache.records.get(index).is_some_and(|r| r.phase == Phase::Published && r.entries.iter().any(|entry| index_key(*entry) == key)) {
            cache.target_hits = cache.target_hits.wrapping_add(1);
            return Some(index);
        }
    }
    // Misses dominate some interpreted loops. This only caches absence from the
    // published index, never an admission decision. Publication and compaction
    // clear these witnesses before a newly published owner can be dispatched.
    if cache.missing_targets[slot] == Some(key) {
        cache.negative_hits = cache.negative_hits.wrapping_add(1);
        return None;
    }
    let Some(index) = cache.published.get(&key).copied() else {
        cache.missing_targets[slot] = Some(key);
        return None;
    };
    if cache.records[index].phase != Phase::Published { return None; }
    cache.targets[slot] = Some((key, index));
    Some(index)
}
fn successor_target(cache: &mut Cache, key: EntryIndexKey, previous: Option<Owner>) -> Option<usize> {
    // Indices are hints, never owners: compaction, replacement and slot reuse
    // must all fail the non-repeating publication identity check.
    let successor = previous.and_then(|p| cache.records.get(p.index)
        .filter(|r| r.phase == Phase::Published && r.job.artifact.key.job == p.id)
        .and_then(|r| r.successor));
    if let Some(s) = successor.filter(|s| s.key == key) {
        if cache.records.get(s.owner.index).is_some_and(|r|
            r.phase == Phase::Published && r.job.artifact.key.job == s.owner.id
                && r.entries.iter().any(|entry| index_key(*entry) == key)) {
            cache.successor_hits = cache.successor_hits.wrapping_add(1);
            return Some(s.owner.index);
        }
    }
    target(cache, key)
}
unsafe fn cached_current(job: &Job) -> CachedMatch {
    if !live::generation_current(job.artifact.key) {
        return CachedMatch::Stale;
    }
    let EntryContract::Cpu(entry) = job.artifact.entry else {
        return CachedMatch::Stale;
    };
    let first = cached_match(entry.linear.0, &job.source);
    if first != CachedMatch::Match { return first; }
    for peer in &job.artifact.fused_sources {
        let current = cached_match(peer.entry.linear.0, &peer.source);
        if current != CachedMatch::Match { return current; }
    }
    CachedMatch::Match
}
/// A host callback can mutate RAM without a dirty notification. Reuse no epoch
/// shortcut here: verify every source and mapping of the still-published owner.
/// Never perform a guest fetch, publish work, or run a second observer.
pub(super) unsafe fn observer_continuation() -> bool {
    let mut cache = CACHE.try_lock().unwrap();
    cache.observer_checks = cache.observer_checks.wrapping_add(1);
    let valid = cache.active && cache.active_owner.is_some_and(|owner|
        cache.records.get(owner.index).is_some_and(|record|
            record.job.artifact.key.job == owner.id && record.phase == Phase::Published
                && cached_current(&record.job) == CachedMatch::Match));
    if valid {
        // The observer just performed the same full source/mapping validation
        // used by cold admission. Reuse that certificate until the next host,
        // mapping or code barrier instead of repeating it on a normal exit.
        let index = cache.active_owner.unwrap().index;
        cache.records[index].validated_epoch = admission_epoch();
    }
    if !valid { cache.observer_rejections = cache.observer_rejections.wrapping_add(1); }
    valid
}
unsafe fn mappings_current(job: &Job) -> bool {
    mappings_cached(&job.source) && job.artifact.fused_sources.iter()
        .all(|s| mappings_cached(&s.source))
}
unsafe fn source_current(entry: super::entry::CpuEntryKey, source: &super::compile::ImmutableCodeSnapshot) -> bool {
    capture(entry.linear.0, source.bytes.len())
        .is_ok_and(|current| current.bytes == source.bytes && current.mappings == source.mappings)
}
unsafe fn unchanged_full(job: &Job) -> bool {
    if !live::generation_current(job.artifact.key) {
        return false;
    }
    let EntryContract::Cpu(entry) = job.artifact.entry else {
        return false;
    };
    source_current(entry, &job.source) && job.artifact.fused_sources.iter()
        .all(|s| source_current(s.entry, &s.source))
}
/// At most four immutable sources, added one witnessed hot peer at a time.
/// the generated dynamic edge still tests the actual guest target.
pub(super) fn fusion_ready(entry: super::entry::CpuEntryKey) -> bool {
    let cache = CACHE.try_lock().unwrap();
    fusion_indices(&cache, entry).is_some()
}
fn fusion_indices(cache: &Cache, entry: super::entry::CpuEntryKey) -> Option<(usize, usize)> {
    if !cache.fusion_enabled { return None; }
    let a = *cache.published.get(&index_key(entry))?;
    let root = &cache.records[a];
    // The snapshot starts at the canonical root, not at an arbitrary alias.
    // Alias promotion may compile separately; never reinterpret root bytes at
    // an alias PC while constructing a fused source.
    if root.job.artifact.entry != EntryContract::Cpu(entry)
        || root.phase != Phase::Published || root.fusion_attempted {
        return None;
    }
    let (_, target, hits) = root.hot_exit?;
    // At the source cap, a later witnessed edge can still close the existing
    // trace. Reject only an already installed prediction, not its captured
    // target; otherwise four-source cycles remain permanently open.
    let (observed, _, _) = root.hot_exit?;
    if root.job.artifact.fused_edges.iter().any(|edge| edge.from == observed.from && edge.target == observed.target) { return None; }
    if hits < 8 || entry == target && root.job.artifact.fused_sources.is_empty()
        || entry.cs_base() != target.cs_base()
        || entry.default_32 != target.default_32 { return None; }
    // Closing an edge into already captured bytes needs no additional owner or
    // source slot. The target may be an interior instruction, not a published
    // entry. The frontend still verifies the instruction boundary and the
    // generated dynamic edge still checks the actual target.
    if !root.job.artifact.fused_sources.is_empty()
        && (target.pc.0.wrapping_sub(entry.pc.0) < root.job.source.bytes.len() as u32
            || root.job.artifact.fused_sources.iter().any(|s|
                target.pc.0.wrapping_sub(s.entry.pc.0) < s.source.bytes.len() as u32)) {
        return Some((a, a));
    }
    let b = *cache.published.get(&index_key(target))?;
    let peer = &cache.records[b];
    // The root's witnessed edge is sufficient authority to attempt fusion.
    // A newly published peer need not wait for another eight activations: its
    // unknown successors remain ordinary guarded exits, never guessed links.
    if peer.phase != Phase::Published { return None; }
    let mut entries = vec![entry];
    entries.extend(root.job.artifact.fused_sources.iter().map(|s| s.entry));
    for candidate in std::iter::once(target).chain(peer.job.artifact.fused_sources.iter().map(|s| s.entry)) {
        if !entries.contains(&candidate) { entries.push(candidate); }
    }
    if entries.len() > 4 { return None; }
    Some((a,b))
}
fn refresh_fusion_candidate(cache: &mut Cache, index: usize) {
    let candidate = match cache.records[index].job.artifact.entry {
        EntryContract::Cpu(entry) => fusion_indices(cache, entry)
            .is_some_and(|(root, _)| root == index),
        _ => false,
    };
    cache.records[index].fusion_candidate = candidate;
}
fn refresh_fusion_candidates(cache: &mut Cache) {
    for index in 0..cache.records.len() { refresh_fusion_candidate(cache, index); }
}
pub(super) unsafe fn take_fusion(entry: super::entry::CpuEntryKey)
    -> Option<(super::compile::ImmutableCodeSnapshot, Vec<CapturedRegion>, Vec<PredictedEdge>)> {
    let mut cache = CACHE.try_lock().unwrap();
    let (a,b) = fusion_indices(&cache, entry)?;
    cache.records[a].fusion_attempted = true;
    cache.records[a].fusion_candidate = false;
    if !unchanged_full(&cache.records[a].job) || !unchanged_full(&cache.records[b].job) { return None; }
    let root = &cache.records[a];
    let mut sources = vec![CapturedRegion { entry, source: root.job.source.clone() }];
    sources.extend(root.job.artifact.fused_sources.clone());
    let mut edges = root.job.artifact.fused_edges.clone();
    let mut visited = vec![a];
    let mut next = Some(b);
    // Close a witnessed chain in one cold compile instead of repeatedly
    // rebuilding A+B, then A+B+C, then A+B+C+D. Count/page/graph budgets and
    // exact dynamic-target guards remain unchanged.
    while let Some(index) = next {
        if visited.contains(&index) { break; }
        let record = &cache.records[index];
        if record.phase != Phase::Published || !unchanged_full(&record.job) { break; }
        let EntryContract::Cpu(peer_entry) = record.job.artifact.entry else { break; };
        if peer_entry.cs_base() != entry.cs_base() || peer_entry.default_32 != entry.default_32 { break; }
        let candidates = std::iter::once(CapturedRegion { entry: peer_entry, source: record.job.source.clone() })
            .chain(record.job.artifact.fused_sources.iter().cloned());
        let mut additions = Vec::new();
        for candidate in candidates {
            if !sources.iter().any(|s| s.entry == candidate.entry)
                && !additions.iter().any(|s: &CapturedRegion| s.entry == candidate.entry) {
                additions.push(candidate);
            }
        }
        if sources.len() + additions.len() > 4 { break; }
        sources.extend(additions);
        visited.push(index);
        next = record.hot_exit.filter(|(_, _, hits)| *hits >= 8)
            .and_then(|(_, target, _)| cache.published.get(&index_key(target)).copied());
    }
    for index in visited {
        let record = &cache.records[index];
        let observed = record.hot_exit.map(|(edge, _, _)| edge);
        for edge in record.job.artifact.fused_edges.iter().copied().chain(observed) {
            if let Some(old) = edges.iter_mut().find(|e| e.from == edge.from) { *old = edge; }
            else if edges.len() < 4 { edges.push(edge); }
        }
    }
    if sources.len() < 2 { return None; }
    let primary = sources.remove(0).source;
    Some((primary, sources, edges))
}
#[no_mangle]
pub unsafe fn ir_cache_set_fusion(enabled: u32) -> bool {
    if enabled > 1 || !cold() { return false; }
    ir_admission_barrier();
    let mut cache = CACHE.try_lock().unwrap();
    cache.fusion_enabled = enabled != 0;
    refresh_fusion_candidates(&mut cache);
    if enabled == 0 {
        for r in &mut cache.records {
            if !r.job.artifact.fused_sources.is_empty() { r.phase = Phase::Retired; }
        }
        cache.needs_collection = true;
    }
    true
}
/// Release only retired owners, outside any guest activation or CACHE lock.
#[no_mangle]
pub unsafe fn ir_cache_collect() -> u32 {
    // The normal activation path has nothing to reclaim. Avoid re-entering the
    // quiescence protocol several times per short region just to discover that.
    if !CACHE.try_lock().unwrap().needs_collection { return 0; }
    if !cold() {
        return 0;
    }
    let retired = {
        let mut cache = CACHE.try_lock().unwrap();
        if !cache.needs_collection {
            return 0;
        }
        let mut retired = Vec::new();
        // Vec compaction changes indices; never retain a slot-only hint.
        cache.targets.fill(None);
        cache.missing_targets.fill(None);
        cache.records.retain(|r| {
            if r.phase == Phase::Retired {
                retired.push((r.slot, r.job.artifact.key.job));
                false
            } else {
                true
            }
        });
        cache.published = cache.records.iter().enumerate()
            .filter(|(_, record)| record.phase == Phase::Published)
            .flat_map(|(index, record)| record.entries.iter().map(move |entry| (index_key(*entry), index)))
            .collect();
        cache.needs_collection = false;
        refresh_fusion_candidates(&mut cache);
        cache.reclaimed = cache.reclaimed.wrapping_add(retired.len() as u32);
        retired
    };
    for &(slot, id) in &retired {
        jit::ir_release_slot(slot, id);
    }
    retired.len() as u32
}
/// Consumes the matching live result. Bytes must already have been copied by JS.
#[no_mangle]
pub unsafe fn ir_cache_reserve(id: u64) -> u32 {
    if !cold() {
        return 0;
    }
    ir_cache_collect();
    if { let c=CACHE.try_lock().unwrap(); c.records.len() >= c.capacity } {
        return 0;
    }
    let Some(job) = live::take_for_cache(id) else {
        return 0;
    };
    reserve_job(job, false)
}
pub(super) unsafe fn reserve_job(mut job: Job, automatic: bool) -> u32 {
    if !job.artifact.fused_sources.is_empty() && !CACHE.try_lock().unwrap().fusion_enabled {
        return 0;
    }
    if !cold() || !unchanged_full(&job) {
        return 0;
    }
    ir_cache_collect();
    if { let c=CACHE.try_lock().unwrap(); c.records.len() >= c.capacity } {
        return 0;
    }
    let id = job.artifact.key.job;
    let pages = job
        .artifact
        .dependencies
        .iter()
        .map(|d| Page::page_of(d.page.0))
        .collect();
    let Some(slot) = jit::ir_reserve_slot(id, pages) else {
        return 0;
    };
    job.artifact.key.slot = slot;
    // IDs never repeat in this Wasm instance, including reset. They also identify
    // this reservation generation; a reused slot must have a different owner.
    job.artifact.key.slot_generation = id;
    let mut cache = CACHE.try_lock().unwrap();
    cache.clock = cache.clock.wrapping_add(1);
    let last_used = cache.clock;
    let entries = job.artifact.cpu_entries().collect();
    cache.records.push(Record {
        entries,
        job,
        slot,
        phase: Phase::Pending,
        automatic,
        last_used,
        hits: 0,
        guest_steps: 0,
        max_guest_steps: 0,
        zero_step_exits: 0,
        validated_epoch: 0,
        hot_exit: None,
        fusion_attempted: false,
        fusion_candidate: false,
        successor: None,
    });
    slot
}
/// Automatic policy can reclaim only its own published records, at a cold point.
pub(super) fn can_make_room(entry: super::entry::CpuEntryKey) -> bool {
    let cache = CACHE.try_lock().unwrap();
    cache.records.len() < cache.capacity
        || cache.records.iter().any(|r| {
            r.phase == Phase::Retired
                || r.automatic
                    && r.phase == Phase::Published
                    && !r.entries.contains(&entry)
        })
}
pub(super) unsafe fn make_room(entry: super::entry::CpuEntryKey) -> bool {
    if !cold() {
        return false;
    }
    ir_cache_collect();
    let evicted = {
        let mut cache = CACHE.try_lock().unwrap();
        if cache.records.len() < cache.capacity {
            return true;
        }
        let victim = cache
            .records
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.automatic
                    && r.phase == Phase::Published
                    && !r.entries.contains(&entry)
            })
            .min_by_key(|(_, r)| (r.last_used, r.job.artifact.key.job))
            .map(|(index, _)| index);
        if let Some(index) = victim {
            let r = &mut cache.records[index];
            r.phase = Phase::Retired;
            let entries = r.entries.clone();
            cache.evictions = cache.evictions.wrapping_add(1);
            cache.needs_collection = true;
            Some(entries)
        } else {
            None
        }
    };
    if let Some(entries) = evicted {
        for entry in entries { super::schedule::evicted(entry); }
    }
    ir_cache_collect();
    { let c=CACHE.try_lock().unwrap(); c.records.len() < c.capacity }
}
pub(super) fn tier(entry: super::entry::CpuEntryKey) -> u32 {
    let cache = CACHE.try_lock().unwrap();
    cache.published.get(&index_key(entry))
        .map(|&index| &cache.records[index])
        .filter(|r| r.phase == Phase::Published)
        .map(|r| if r.job.artifact.tier == super::compile::Tier::One { 1 } else { 2 })
        .unwrap_or(0)
}
/// Pending/validated results have not completed the publication transaction.
pub(super) fn completion_state(id: u64) -> u32 {
    match CACHE
        .try_lock()
        .unwrap()
        .records
        .iter()
        .find(|r| r.job.artifact.key.job == id)
        .map(|r| r.phase)
    {
        Some(Phase::Published) => 1,
        Some(Phase::Pending | Phase::Validated) => 2,
        _ => 0,
    }
}
/// Validate immediately before table.set. No guest memory/MMIO writes or callbacks.
/// IP may have moved while the browser compiled; lookup checks the saved entry key.
#[no_mangle]
pub unsafe fn ir_cache_validate(id: u64, slot: u32) -> bool {
    if !cold() {
        return false;
    }
    let mut cache = CACHE.try_lock().unwrap();
    let valid = if let Some(r) = cache
        .records
        .iter_mut()
        .find(|r| r.job.artifact.key.job == id && r.slot == slot)
    {
        if r.phase == Phase::Pending && unchanged_full(&r.job) {
            r.phase = Phase::Validated;
            true
        } else {
            if r.phase == Phase::Pending {
                r.phase = Phase::Retired;
            }
            false
        }
    } else {
        false
    };
    if !valid {
        cache.needs_collection = true;
        cache.rejected = cache.rejected.wrapping_add(1);
    }
    valid
}
#[no_mangle]
pub unsafe fn ir_cache_finish(id: u64, slot: u32) -> bool {
    if !cold() {
        return false;
    }
    let mut cache = CACHE.try_lock().unwrap();
    let Some(index) = cache
        .records
        .iter()
        .position(|r| r.job.artifact.key.job == id && r.slot == slot)
    else {
        return false;
    };
    if cache.records[index].phase != Phase::Validated {
        return false;
    }
    if !unchanged_full(&cache.records[index].job) {
        cache.records[index].phase = Phase::Retired;
        cache.needs_collection = true;
        return false;
    }
    let entries = cache.records[index].entries.clone();
    for (i, record) in cache.records.iter_mut().enumerate() {
        if i != index && record.phase == Phase::Published {
            record.entries.retain(|entry| !entries.contains(entry));
            if record.entries.is_empty() { record.phase = Phase::Retired; }
        }
    }
    cache.records[index].phase = Phase::Published;
    cache.needs_collection = true;
    for entry in &entries { cache.published.insert(index_key(*entry), index); }
    refresh_fusion_candidates(&mut cache);
    // Cached predecessor hints must not retain authority over superseded aliases.
    cache.targets.fill(None);
    cache.missing_targets.fill(None);
    if entries.len() > 1 { cache.shared_publications = cache.shared_publications.wrapping_add(1); }
    let structured = cache.records[index].job.artifact.code.structured_cfg;
    if !cache.records[index].job.artifact.fused_sources.is_empty() {
        cache.fused_publications = cache.fused_publications.wrapping_add(1);
    }
    let backedges = cache.records[index].job.artifact.code.structured_backedges;
    let structured_edges = cache.records[index].job.artifact.code.structured_edges;
    let dispatch_edges = cache.records[index].job.artifact.code.generic_dispatch_edges;
    if structured {
        cache.structured_publications = cache.structured_publications.wrapping_add(1);
        cache.structured_backedges = cache.structured_backedges.wrapping_add(backedges);
        cache.structured_edges = cache.structured_edges.wrapping_add(structured_edges);
    } else {
        cache.generic_publications = cache.generic_publications.wrapping_add(1);
        cache.generic_dispatch_edges = cache.generic_dispatch_edges.wrapping_add(dispatch_edges);
    }
    true
}
#[no_mangle]
pub unsafe fn ir_cache_cancel(id: u64, slot: u32) -> bool {
    let mut cache = CACHE.try_lock().unwrap();
    let Some(r) = cache
        .records
        .iter_mut()
        .find(|r| r.job.artifact.key.job == id && r.slot == slot && r.phase != Phase::Retired)
    else {
        return false;
    };
    r.phase = Phase::Retired;
    cache.needs_collection = true;
    cache.failed = cache.failed.wrapping_add(1);
    true
}
#[no_mangle]
pub fn ir_cache_stat(field: u32) -> u32 {
    let cache = CACHE.try_lock().unwrap();
    match field {
        0 => cache
            .records
            .iter()
            .filter(|r| r.phase == Phase::Published)
            .count() as u32,
        1 => cache.records.len() as u32,
        2 => cache.hits,
        3 => cache.rejected,
        4 => cache.failed,
        5 => cache.reclaimed,
        6 => cache.links,
        7 => cache.link_misses,
        8 => cache.cached_checks,
        9 => cache.capture_fallbacks,
        10 => cache.guest_steps,
        11 => cache.max_guest_steps,
        12 => cache.zero_step_exits,
        13 => cache.structured_publications,
        14 => cache.generic_publications,
        15 => cache.structured_backedges,
        16 => cache.generic_dispatch_edges,
        17 => cache.structured_edges,
        18 => cache.fast_checks,
        19 => cache.full_checks,
        20 => cache.post_fetch_reuses,
        21 => cache.target_hits,
        22 => u32::from(cache.fast_validation),
        23 => cache.fused_publications,
        24 => cache.fused_hits,
        25 => cache.fused_steps,
        26 => u32::from(cache.fusion_enabled),
        27 => cache.evictions,
        28 => cache.negative_hits,
        29 => cache.successor_hits,
        30 => cache.records.iter().filter(|r| r.phase == Phase::Published).map(|r| r.entries.len() as u32).sum(),
        31 => cache.shared_publications,
        32 => cache.observer_checks,
        33 => cache.observer_rejections,
        _ => 0,
    }
}

#[no_mangle]
pub fn ir_cache_entry_stat(
    linear: u32,
    cs_base: u32,
    default_32: u32,
    field: u32,
) -> u32 {
    let cache = CACHE.try_lock().unwrap();
    let Some(record) = cache.records.iter().find(|record| {
        if record.phase != Phase::Published {
            return false;
        }
        record.entries.iter().any(|entry| entry.linear.0 == linear
            && entry.cs_base() == cs_base
            && u32::from(entry.default_32) == default_32)
    }) else {
        return 0;
    };
    match field {
        0 => 1,
        1 => record.hits,
        2 => record.guest_steps,
        3 => record.max_guest_steps,
        4 => record.zero_step_exits,
        5 => {
            if record.job.artifact.tier == super::compile::Tier::One {
                1
            } else {
                2
            }
        },
        6 => u32::from(record.job.artifact.code.structured_cfg),
        7 => record.job.artifact.code.structured_backedges,
        8 => record.job.artifact.code.generic_dispatch_edges,
        9 => record.job.artifact.code.structured_edges,
        10 => 1 + record.job.artifact.fused_sources.len() as u32,
        11 => record.entries.len() as u32,
        12 => record.slot,
        _ => 0,
    }
}

/// Cold lookup used by a compiled-exit/link bridge. The returned slot is only a
/// hint: the callee still performs its normal entry/generation/mapping admission
/// before execution. This never publishes, compiles or holds a cache lock across
/// guest activation.
pub unsafe fn link_target() -> Option<(u32, u64)> {
    // This is a cold graph lookup, not a guest activation. Unlike execute(),
    // requiring JIT_STATE try_lock here would race the immediately following
    // dependency query with our own short-lived cache inspection and makes the
    // diagnostic/link API spuriously miss. All mutation/publication still uses
    // the normal quiescent protocols.
    if cpu::in_jit || busy() {
        return None;
    }
    let entry = live::entry();
    let candidate = {
        let cache = CACHE.try_lock().unwrap();
        cache
            .records
            .iter()
            .find(|r| {
                r.phase == Phase::Published && r.entries.contains(&entry)
            })
            .map(|r| (r.job.artifact.key.job, r.job.artifact.key, r.job.artifact.entry,
                r.job.source.clone(), r.job.artifact.fused_sources.clone()))
    };
    let Some((id, key, EntryContract::Cpu(canonical), source, peers)) = candidate else {
        let mut cache = CACHE.try_lock().unwrap();
        cache.link_misses = cache.link_misses.wrapping_add(1);
        return None;
    };
    let valid = live::generation_current(key)
        && source_current(canonical, &source)
        && peers.iter().all(|s| source_current(s.entry, &s.source));
    let mut cache = CACHE.try_lock().unwrap();
    let Some(index) = cache
        .records
        .iter()
        .position(|r| r.job.artifact.key.job == id && r.phase == Phase::Published)
    else {
        cache.link_misses = cache.link_misses.wrapping_add(1);
        return None;
    };
    if !valid {
        cache.records[index].phase = Phase::Retired;
        cache.needs_collection = true;
        cache.link_misses = cache.link_misses.wrapping_add(1);
        return None;
    }
    cache.clock = cache.clock.wrapping_add(1);
    let stamp = cache.clock;
    cache.records[index].last_used = stamp;
    cache.links = cache.links.wrapping_add(1);
    Some((cache.records[index].slot, id))
}

#[no_mangle]
pub unsafe fn ir_cache_link_target() -> u64 {
    let Some((slot, id)) = link_target() else {
        return 0;
    };
    // Slot zero is never allocated. Pack slot and a truncated owner witness for
    // diagnostics; execution must still use normal cache admission, never this
    // value as an unchecked call target.
    ((id as u32 as u64) << 32) | slot as u64
}

/// Called by the ordinary CPU dispatcher, before legacy cache lookup.
/// No request means no IR entry; compilation policy/tier promotion remain separate.
pub unsafe fn execute() -> bool {
    if diag::enabled() { execute_mode::<true>() } else { execute_mode::<false>() }
}
unsafe fn execute_mode<const PROFILE: bool>() -> bool {
    use super::entry::take_link_request;
    take_link_request();
    let control = *gp::flags & (cpu::FLAG_INTERRUPT | cpu::FLAG_TRAP | cpu::FLAG_VM);
    let mut owner = None;
    if !execute_one::<PROFILE>(false, None, &mut owner) { return false; }
    // Iterative cold chaining keeps host stack bounded and retains full entry,
    // source, mapping and post-fetch admission checks at each successor.
    let mut limit = true;
    for _ in 0..64 {
        let stop = if !take_link_request() { Some(0) }
            else if !cpu::ir_link_budget_available() { Some(1) }
            else if *gp::in_hlt { Some(2) }
            else if *gp::flags & (cpu::FLAG_INTERRUPT | cpu::FLAG_TRAP | cpu::FLAG_VM) != control { Some(3) }
            else { None };
        if let Some(reason) = stop { if PROFILE { diag::chain(reason); } limit = false; break; }
        let previous = owner.take();
        if !execute_one::<PROFILE>(true, previous, &mut owner) { if PROFILE { diag::chain(4); } limit = false; break; }
    }
    if limit { if PROFILE { diag::chain(5); } }
    take_link_request();
    true
}
unsafe fn execute_one<const PROFILE: bool>(linked: bool, previous: Option<Owner>, owner: &mut Option<Owner>) -> bool {
    let admission_scope = PROFILE.then(|| Scope::new(Stage::Admission));
    if PROFILE { diag::admission(Admission::Attempt); }
    if !cold() {
        if PROFILE { diag::admission(Admission::Busy); }
        return false;
    }
    ir_cache_collect();
    let entry = live::entry();
    let selected = {
        let mut cache = CACHE.try_lock().unwrap();
        let mut selected = None;
        let index = successor_target(&mut cache, index_key(entry), previous);
        if index.is_none() { if PROFILE { diag::admission(Admission::Missing); super::schedule::diagnose_missing(entry); } }
        let index = index.filter(|_| {
            let valid = ir_entry_matches(entry.linear.0, entry.cs_base(), entry.default_32 as u32);
            if !valid { if PROFILE { diag::admission(Admission::Context); } }
            valid
        });
        if let Some(index) = index {
            let epoch = admission_epoch();
            let reuse = cache.fast_validation && epoch != u64::MAX
                && cache.records[index].validated_epoch == epoch
                && live::generation_current(cache.records[index].job.artifact.key)
                && mappings_current(&cache.records[index].job);
            let cached = if reuse {
                cache.fast_checks = cache.fast_checks.wrapping_add(1);
                CachedMatch::Match
            } else {
                cache.full_checks = cache.full_checks.wrapping_add(1);
                let _scope = PROFILE.then(|| Scope::new(Stage::ByteValidation));
                cached_current(&cache.records[index].job)
            };
            let valid = match cached {
                CachedMatch::Match => {
                    cache.cached_checks = cache.cached_checks.wrapping_add(1);
                    true
                },
                CachedMatch::Unavailable => {
                    cache.capture_fallbacks = cache.capture_fallbacks.wrapping_add(1);
                    if PROFILE { diag::admission(Admission::Capture); }
                    let _scope = PROFILE.then(|| Scope::new(Stage::SourceCapture));
                    unchanged_full(&cache.records[index].job)
                },
                CachedMatch::Stale => false,
            };
            if !valid {
                if PROFILE { diag::admission(Admission::StaleBefore); }
                cache.records[index].phase = Phase::Retired;
                cache.needs_collection = true;
            } else {
                selected = Some((
                    cache.records[index].slot,
                    cache.records[index].job.artifact.key.job,
                    index,
                    cache.fast_validation && cached == CachedMatch::Match,
                    epoch,
                ));
            }
        }
        selected
    };
    let Some((slot, id, selected_index, warm_fetch, epoch)) = selected else {
        ir_cache_collect();
        return false;
    };
    // Preserve the dispatcher's actual initial fetch translation and A-bit
    // updates. A cached fast check above is sufficient only when every source
    // mapping is already CPU-visible; otherwise the read-only capture fallback
    // validates without creating A-bit side effects.
    *gp::previous_ip = *gp::instruction_pointer;
    let fetch = { let _scope = PROFILE.then(|| Scope::new(Stage::Fetch)); cpu::get_phys_eip() };
    if fetch.is_err() {
        if PROFILE { diag::admission(Admission::FetchFault); }
        ir_admission_barrier();
        return true;
    }
    let admitted = {
        let mut cache = CACHE.try_lock().unwrap();
        let index = cache.records.get(selected_index)
            .filter(|r| r.job.artifact.key.job == id && r.phase == Phase::Published)
            .map(|_| selected_index);
        let valid = if let Some(index) = index {
            // Page tables can themselves alias code: the A-bit update must not
            // leave a module compiled from the pre-fetch bytes admissible. After
            // the architectural fetch, admission requires cached mapping identity;
            // an unavailable secondary mapping remains published for a later hit.
            let current = if warm_fetch && epoch == admission_epoch() && epoch != u64::MAX {
                // All translations were already visible: get_phys_eip cannot
                // walk page tables, write A bits or invoke a device callback.
                cache.post_fetch_reuses = cache.post_fetch_reuses.wrapping_add(1);
                CachedMatch::Match
            } else {
                cache.full_checks = cache.full_checks.wrapping_add(1);
                let _scope = PROFILE.then(|| Scope::new(Stage::ByteValidation));
                cached_current(&cache.records[index].job)
            };
            match current {
                CachedMatch::Match => {
                    cache.cached_checks = cache.cached_checks.wrapping_add(1);
                    cache.records[index].validated_epoch = admission_epoch();
                    true
                },
                CachedMatch::Unavailable => { if PROFILE { diag::admission(Admission::UnavailableAfter); } false },
                CachedMatch::Stale => {
                    if PROFILE { diag::admission(Admission::StaleAfter); }
                    cache.records[index].phase = Phase::Retired;
                    cache.needs_collection = true;
                    false
                },
            }
        } else {
            if PROFILE { diag::admission(Admission::LostOwner); }
            false
        };
        if valid {
            if PROFILE { diag::admission(Admission::Accepted); }
            cache.clock = cache.clock.wrapping_add(1);
            let stamp = cache.clock;
            cache.records[index.unwrap()].last_used = stamp;
            cache.active = true;
            cache.active_owner = Some(Owner { index: index.unwrap(), id });
            cache.hits = cache.hits.wrapping_add(1);
        }
        if valid {
            let index = index.unwrap(); let record = &cache.records[index];
            let needs_heat = record.job.artifact.tier == super::compile::Tier::One
                || record.fusion_candidate && record.job.artifact.entry == EntryContract::Cpu(entry);
            let current = Owner { index, id };
            *owner = Some(current);
            if let Some(p) = previous {
                if let Some(r) = cache.records.get_mut(p.index)
                    .filter(|r| r.phase == Phase::Published && r.job.artifact.key.job == p.id) {
                    r.successor = Some(Successor { key: index_key(entry), owner: current });
                }
            }
            Some((index, needs_heat))
        } else { None }
    };
    let Some((admitted_index, needs_heat)) = admitted else {
        ir_cache_collect();
        return false;
    };
    drop(admission_scope);
    let before = *gp::instruction_counter;
    let diagnostic_cr3 = if PROFILE { diag::cr3() } else { 0 };
    super::entry::take_link_request();
    super::schedule::note_cached(entry, linked, needs_heat);
    if linked {
        let mut cache = CACHE.try_lock().unwrap();
        cache.links = cache.links.wrapping_add(1);
    }
    let sample = if profiler::performance_recording_enabled() {
        Some(profiler::performance_chunk_start(
            true,
            *gp::instruction_pointer as u32,
            *gp::cr.add(3) as u32,
            *gp::cpl,
        ))
    } else {
        None
    };
    if PROFILE { diag::activation_start(); }
    let execution_scope = PROFILE.then(|| Scope::new(Stage::Generated));
    call_indirect1((slot + cpu::WASM_TABLE_OFFSET) as i32, 0);
    let duration = execution_scope.and_then(Scope::finish);
    // Terminal helpers/fault delivery can observe the host even when they do
    // not pass through an emitted continuing-call barrier.
    if !super::entry::link_requested() { ir_admission_barrier(); }
    let steps = (*gp::instruction_counter).wrapping_sub(before);
    let observed_exit = if steps != 0 && super::entry::profile_link_requested() {
        let target = live::entry();
        Some((PredictedEdge { from: GuestEip((*gp::previous_ip as u32).wrapping_sub(entry.cs_base())),
            target: target.pc }, target))
    } else { None };
    if let Some(sample) = sample {
        profiler::performance_chunk_finish(sample, steps);
        profiler::performance_recording_add(1, steps as u64);
    }
    {
        let mut cache = CACHE.try_lock().unwrap();
        cache.active = false;
        cache.active_owner = None;
        cache.guest_steps = cache.guest_steps.wrapping_add(steps);
        cache.max_guest_steps = cache.max_guest_steps.max(steps);
        if steps == 0 {
            cache.zero_step_exits = cache.zero_step_exits.wrapping_add(1);
        }
        let profile = cache.fusion_enabled;
        let mut refresh_fusion = false;
        let fused = cache.records.get(admitted_index).is_some_and(|r|
            r.job.artifact.key.job == id && !r.job.artifact.fused_sources.is_empty());
        if fused {
            cache.fused_hits = cache.fused_hits.wrapping_add(1);
            cache.fused_steps = cache.fused_steps.wrapping_add(steps);
        }
        if let Some(record) = cache.records.get_mut(admitted_index)
            .filter(|record| record.job.artifact.key.job == id)
        {
            if PROFILE { diag::activation_end(entry.linear.0, diagnostic_cr3, steps, duration,
                if record.job.artifact.tier == super::compile::Tier::One { 1 } else { 2 }, fused); }
            record.hits = record.hits.wrapping_add(1);
            record.guest_steps = record.guest_steps.wrapping_add(steps);
            record.max_guest_steps = record.max_guest_steps.max(steps);
            if let Some((edge, target)) = observed_exit.filter(|_| profile) {
                let was_hot = record.hot_exit.is_some_and(|(_, _, hits)| hits >= 8);
                match &mut record.hot_exit {
                    Some((old, old_target, hits)) if old.from == edge.from && *old_target == target =>
                        *hits = hits.saturating_add(1),
                    Some((_, _, hits)) if *hits > 1 => *hits -= 1,
                    _ => record.hot_exit = Some((edge, target, 1)),
                }
                refresh_fusion = was_hot != record.hot_exit.is_some_and(|(_, _, hits)| hits >= 8);
            }
            if steps == 0 {
                record.zero_step_exits = record.zero_step_exits.wrapping_add(1);
            }
        }
        if refresh_fusion { refresh_fusion_candidate(&mut cache, admitted_index); }
        // Zero-budget REP and other no-retirement exits must not trap scheduling
        // in a repeatedly admitted entry. The next cycle may interpret instead.
        if steps == 0 {
            if let Some(r) = cache.records.get_mut(admitted_index)
                .filter(|r| r.job.artifact.key.job == id)
            {
                r.phase = Phase::Retired;
            }
            cache.needs_collection = true;
        }
    }
    ir_cache_collect();
    true
}
