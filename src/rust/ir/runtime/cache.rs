//! Explicit publication into the shared table pool and cold CPU dispatch.
//! No lock or artifact reference survives a guest activation. Dirty/reset hooks
//! retire immediately; slot reuse waits until that activation has returned.
use super::{
    entry::{ir_entry_matches, EntryContract},
    live::{self, Job},
    snapshot::capture,
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
}
struct Cache {
    records: Vec<Record>,
    active: bool,
    hits: u32,
    rejected: u32,
    failed: u32,
    reclaimed: u32,
}
static CACHE: Mutex<Cache> = Mutex::new(Cache {
    records: Vec::new(),
    active: false,
    hits: 0,
    rejected: 0,
    failed: 0,
    reclaimed: 0,
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
unsafe fn unchanged(job: &Job) -> bool {
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
    if !cold() || !unchanged(&job) {
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
    CACHE.try_lock().unwrap().records.push(Record {
        job,
        slot,
        phase: Phase::Pending,
        automatic,
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
        if let Some(r) = cache.records.iter_mut().find(|r| {
            r.automatic
                && r.phase == Phase::Published
                && r.job.artifact.entry != EntryContract::Cpu(entry)
        }) {
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
        if r.phase == Phase::Pending && unchanged(&r.job) {
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
    if !unchanged(&cache.records[index].job) {
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
        _ => 0,
    }
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
        for r in &mut cache.records {
            if r.phase != Phase::Published {
                continue;
            }
            let EntryContract::Cpu(entry) = r.job.artifact.entry else {
                continue;
            };
            if !ir_entry_matches(entry.linear.0, entry.cs_base(), entry.default_32 as u32) {
                continue;
            }
            if !unchanged(&r.job) {
                r.phase = Phase::Retired;
                continue;
            }
            selected = Some((r.slot, r.job.artifact.key.job));
            break;
        }
        selected
    };
    let Some((slot, id)) = selected else {
        ir_cache_collect();
        return false;
    };
    // Preserve the dispatcher's actual initial fetch translation and A-bit
    // updates. The read-only capture above is only a compilation/admission check.
    *gp::previous_ip = *gp::instruction_pointer;
    if cpu::get_phys_eip().is_err() {
        return true;
    }
    let admitted = {
        let mut cache = CACHE.try_lock().unwrap();
        let record = cache
            .records
            .iter_mut()
            .find(|r| r.job.artifact.key.job == id && r.phase == Phase::Published);
        let valid = if let Some(r) = record {
            // Page tables can themselves alias code: the A-bit update must not
            // leave a module compiled from the pre-fetch bytes admissible.
            if !unchanged(&r.job) {
                r.phase = Phase::Retired;
                false
            } else {
                super::snapshot::mappings_cached(&r.job.source)
            }
        } else {
            false
        };
        if valid {
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
