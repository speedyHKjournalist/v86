//! Explicit publication into the shared table pool and cold CPU dispatch.
//! No lock or artifact reference survives a guest activation. Dirty/reset hooks
//! retire immediately; slot reuse waits until that activation has returned.
use super::{
    entry::{ir_entry_matches, EntryContract},
    live::{self, Job},
    snapshot::{capture, cached_match, CachedMatch},
};
use crate::{
    cpu::{cpu, global_pointers as gp},
    jit,
    page::Page,
    profiler,
};
use std::sync::Mutex;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Pending,
    Validated,
    Published,
    Retired,
}
struct Record {
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
}
struct Cache {
    records: Vec<Record>,
    active: bool,
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
    generic_dispatch_edges: u32,
}
static CACHE: Mutex<Cache> = Mutex::new(Cache {
    records: Vec::new(),
    active: false,
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
    generic_dispatch_edges: 0,
});
const CAPACITY: usize = 32;
extern "C" {
    fn call_indirect1(f: i32, x: u16);
}
pub fn busy() -> bool {
    CACHE.try_lock().unwrap().active
}
pub fn invalidate() {
    for record in &mut CACHE.try_lock().unwrap().records {
        record.phase = Phase::Retired;
    }
}
pub fn dirty_page(page: u32) {
    for record in &mut CACHE.try_lock().unwrap().records {
        if record
            .job
            .source
            .dependencies
            .iter()
            .any(|d| d.page.0 == page)
        {
            record.phase = Phase::Retired;
        }
    }
}
unsafe fn cold() -> bool {
    !cpu::in_jit && !busy() && jit::ir_cache_quiescent()
}
unsafe fn cached_current(job: &Job) -> CachedMatch {
    if !live::generation_current(job.artifact.key) {
        return CachedMatch::Stale;
    }
    let EntryContract::Cpu(entry) = job.artifact.entry else {
        return CachedMatch::Stale;
    };
    cached_match(entry.linear.0, &job.source)
}
unsafe fn unchanged_full(job: &Job) -> bool {
    if !live::generation_current(job.artifact.key) {
        return false;
    }
    let EntryContract::Cpu(entry) = job.artifact.entry else {
        return false;
    };
    let Ok(current) = capture(entry.linear.0, job.source.bytes.len()) else {
        return false;
    };
    current.bytes == job.source.bytes && current.mappings == job.source.mappings
}
/// Release only retired owners, outside any guest activation or CACHE lock.
#[no_mangle]
pub unsafe fn ir_cache_collect() -> u32 {
    if !cold() {
        return 0;
    }
    let retired = {
        let mut cache = CACHE.try_lock().unwrap();
        let mut retired = Vec::new();
        cache.records.retain(|r| {
            if r.phase == Phase::Retired {
                retired.push((r.slot, r.job.artifact.key.job));
                false
            } else {
                true
            }
        });
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
    if CACHE.try_lock().unwrap().records.len() >= CAPACITY {
        return 0;
    }
    let Some(job) = live::take_for_cache(id) else {
        return 0;
    };
    reserve_job(job, false)
}
pub(super) unsafe fn reserve_job(mut job: Job, automatic: bool) -> u32 {
    if !cold() || !unchanged_full(&job) {
        return 0;
    }
    ir_cache_collect();
    if CACHE.try_lock().unwrap().records.len() >= CAPACITY {
        return 0;
    }
    let id = job.artifact.key.job;
    let pages = job
        .source
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
    cache.records.push(Record {
        job,
        slot,
        phase: Phase::Pending,
        automatic,
        last_used,
        hits: 0,
        guest_steps: 0,
        max_guest_steps: 0,
        zero_step_exits: 0,
    });
    slot
}
/// Automatic policy can reclaim only its own published records, at a cold point.
pub(super) fn can_make_room(entry: super::entry::CpuEntryKey) -> bool {
    let cache = CACHE.try_lock().unwrap();
    cache.records.len() < CAPACITY
        || cache.records.iter().any(|r| {
            r.phase == Phase::Retired
                || r.automatic
                    && r.phase == Phase::Published
                    && r.job.artifact.entry != EntryContract::Cpu(entry)
        })
}
pub(super) unsafe fn make_room(entry: super::entry::CpuEntryKey) -> bool {
    if !cold() {
        return false;
    }
    ir_cache_collect();
    let evicted = {
        let mut cache = CACHE.try_lock().unwrap();
        if cache.records.len() < CAPACITY {
            return true;
        }
        let victim = cache
            .records
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.automatic
                    && r.phase == Phase::Published
                    && r.job.artifact.entry != EntryContract::Cpu(entry)
            })
            .min_by_key(|(_, r)| (r.last_used, r.job.artifact.key.job))
            .map(|(index, _)| index);
        if let Some(index) = victim {
            let r = &mut cache.records[index];
            r.phase = Phase::Retired;
            Some(r.job.artifact.entry)
        } else {
            None
        }
    };
    if let Some(EntryContract::Cpu(entry)) = evicted {
        super::schedule::evicted(entry);
    }
    ir_cache_collect();
    CACHE.try_lock().unwrap().records.len() < CAPACITY
}
pub(super) fn tier(entry: super::entry::CpuEntryKey) -> u32 {
    CACHE
        .try_lock()
        .unwrap()
        .records
        .iter()
        .filter(|r| {
            r.phase == Phase::Published && r.job.artifact.entry == EntryContract::Cpu(entry)
        })
        .map(|r| if r.job.artifact.tier == super::compile::Tier::One { 1 } else { 2 })
        .max()
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
        return false;
    }
    let entry = cache.records[index].job.artifact.entry;
    for (i, r) in cache.records.iter_mut().enumerate() {
        if i != index && r.job.artifact.entry == entry && r.phase == Phase::Published {
            r.phase = Phase::Retired;
        }
    }
    cache.records[index].phase = Phase::Published;
    let structured = cache.records[index].job.artifact.code.structured_cfg;
    let backedges = cache.records[index].job.artifact.code.structured_backedges;
    let dispatch_edges = cache.records[index].job.artifact.code.generic_dispatch_edges;
    if structured {
        cache.structured_publications = cache.structured_publications.wrapping_add(1);
        cache.structured_backedges = cache.structured_backedges.wrapping_add(backedges);
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
        let EntryContract::Cpu(entry) = record.job.artifact.entry else {
            return false;
        };
        entry.linear.0 == linear
            && entry.cs_base() == cs_base
            && u32::from(entry.default_32) == default_32
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
                r.phase == Phase::Published && r.job.artifact.entry == EntryContract::Cpu(entry)
            })
            .map(|r| (r.job.artifact.key.job, r.job.artifact.key, r.job.source.clone()))
    };
    let Some((id, key, source)) = candidate else {
        let mut cache = CACHE.try_lock().unwrap();
        cache.link_misses = cache.link_misses.wrapping_add(1);
        return None;
    };
    let valid = live::generation_current(key)
        && capture(entry.linear.0, source.bytes.len())
            .is_ok_and(|current| current.bytes == source.bytes && current.mappings == source.mappings);
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
    if !cold() {
        return false;
    }
    ir_cache_collect();
    let selected = {
        let mut cache = CACHE.try_lock().unwrap();
        let mut selected = None;
        for index in 0..cache.records.len() {
            if cache.records[index].phase != Phase::Published {
                continue;
            }
            let EntryContract::Cpu(entry) = cache.records[index].job.artifact.entry else {
                continue;
            };
            if !ir_entry_matches(entry.linear.0, entry.cs_base(), entry.default_32 as u32) {
                continue;
            }
            let cached = cached_current(&cache.records[index].job);
            let valid = match cached {
                CachedMatch::Match => {
                    cache.cached_checks = cache.cached_checks.wrapping_add(1);
                    true
                },
                CachedMatch::Unavailable => {
                    cache.capture_fallbacks = cache.capture_fallbacks.wrapping_add(1);
                    unchanged_full(&cache.records[index].job)
                },
                CachedMatch::Stale => false,
            };
            if !valid {
                cache.records[index].phase = Phase::Retired;
                continue;
            }
            selected = Some((
                cache.records[index].slot,
                cache.records[index].job.artifact.key.job,
            ));
            break;
        }
        selected
    };
    let Some((slot, id)) = selected else {
        ir_cache_collect();
        return false;
    };
    // Preserve the dispatcher's actual initial fetch translation and A-bit
    // updates. A cached fast check above is sufficient only when every source
    // mapping is already CPU-visible; otherwise the read-only capture fallback
    // validates without creating A-bit side effects.
    *gp::previous_ip = *gp::instruction_pointer;
    if cpu::get_phys_eip().is_err() {
        return true;
    }
    let admitted = {
        let mut cache = CACHE.try_lock().unwrap();
        let index = cache
            .records
            .iter()
            .position(|r| r.job.artifact.key.job == id && r.phase == Phase::Published);
        let valid = if let Some(index) = index {
            // Page tables can themselves alias code: the A-bit update must not
            // leave a module compiled from the pre-fetch bytes admissible. After
            // the architectural fetch, admission requires cached mapping identity;
            // an unavailable secondary mapping remains published for a later hit.
            match cached_current(&cache.records[index].job) {
                CachedMatch::Match => {
                    cache.cached_checks = cache.cached_checks.wrapping_add(1);
                    true
                },
                CachedMatch::Unavailable => false,
                CachedMatch::Stale => {
                    cache.records[index].phase = Phase::Retired;
                    false
                },
            }
        } else {
            false
        };
        if valid {
            cache.clock = cache.clock.wrapping_add(1);
            let stamp = cache.clock;
            if let Some(r) = cache
                .records
                .iter_mut()
                .find(|r| r.job.artifact.key.job == id)
            {
                r.last_used = stamp;
            }
            cache.active = true;
            cache.hits = cache.hits.wrapping_add(1);
        }
        valid
    };
    if !admitted {
        ir_cache_collect();
        return false;
    }
    let before = *gp::instruction_counter;
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
    call_indirect1((slot + cpu::WASM_TABLE_OFFSET) as i32, 0);
    let steps = (*gp::instruction_counter).wrapping_sub(before);
    if let Some(sample) = sample {
        profiler::performance_chunk_finish(sample, steps);
        profiler::performance_recording_add(1, steps as u64);
    }
    {
        let mut cache = CACHE.try_lock().unwrap();
        cache.active = false;
        cache.guest_steps = cache.guest_steps.wrapping_add(steps);
        cache.max_guest_steps = cache.max_guest_steps.max(steps);
        if steps == 0 {
            cache.zero_step_exits = cache.zero_step_exits.wrapping_add(1);
        }
        if let Some(record) = cache
            .records
            .iter_mut()
            .find(|record| record.job.artifact.key.job == id)
        {
            record.hits = record.hits.wrapping_add(1);
            record.guest_steps = record.guest_steps.wrapping_add(steps);
            record.max_guest_steps = record.max_guest_steps.max(steps);
            if steps == 0 {
                record.zero_step_exits = record.zero_step_exits.wrapping_add(1);
            }
        }
        // Zero-budget REP and other no-retirement exits must not trap scheduling
        // in a repeatedly admitted entry. The next cycle may interpret instead.
        if steps == 0 {
            if let Some(r) = cache
                .records
                .iter_mut()
                .find(|r| r.job.artifact.key.job == id)
            {
                r.phase = Phase::Retired;
            }
        }
    }
    ir_cache_collect();
    true
}
