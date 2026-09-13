//! Bounded automatic compilation at outer CPU dispatch points. Linked entries
//! contribute heat only; no compiler or publisher runs with guest locals alive.
use super::{
    cache,
    compile::*,
    entry::CpuEntryKey,
    live::{self, Job},
    snapshot::capture,
};
use crate::{
    cpu::{cpu, global_pointers as gp},
    ir::{
        backend::wasm::StateLayout,
        frontend::decode::{decode, DecodeStop, Flow, GuestEip, LinearAddress},
    },
    jit,
};
use std::{collections::VecDeque, sync::Mutex};
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
    config: Config,
    hot: VecDeque<Hot>,
    pending: Option<Pending>,
    credit: bool,
    stats: [u32; 9],
}
static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler {
    config: Config {
        enabled: false,
        threshold: 16,
        promote: 64,
        window: 192,
        budget: 256,
        rep: 64,
    },
    hot: VecDeque::new(),
    pending: None,
    credit: false,
    stats: [0; 9],
});
#[link(wasm_import_module = "env")]
extern "C" {
    fn ir_codegen_finalize(id: u64, slot: u32, ptr: u32, len: u32);
}
pub fn invalidate() {
    let mut s = SCHEDULER.try_lock().unwrap();
    s.hot.clear();
    s.pending = None;
    s.credit = false;
}
pub fn dirty_page(page: u32) {
    SCHEDULER.try_lock().unwrap().hot.retain(|h| {
        !h.source
            .as_ref()
            .is_some_and(|source| source.dependencies.iter().any(|d| d.page.0 == page))
    });
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
        s.credit = false;
        s.pending.take()
    };
    if let Some(p) = pending {
        cache::ir_cache_cancel(p.id, p.slot);
        cache::ir_cache_collect();
    }
    true
}
unsafe fn record(link: bool) {
    let mut s = SCHEDULER.try_lock().unwrap();
    if !s.config.enabled || *gp::prefixes != 0 || *gp::in_hlt {
        return;
    }
    s.stats[link as usize] = s.stats[link as usize].wrapping_add(1);
    let entry = live::entry();
    if let Some(h) = s.hot.iter_mut().find(|h| h.entry == entry) {
        h.hits = h.hits.saturating_add(1);
    } else {
        if s.hot.len() == 128 {
            s.hot.pop_front();
        }
        s.hot.push_back(Hot {
            entry,
            hits: 1,
            source: None,
            failed: 0,
        });
    }
}
pub unsafe fn note() {
    record(true);
}
/// A conservative sequential boundary finder. Direct targets outside the selected
/// byte window remain explicit region exits in the shared CFG frontend.
unsafe fn source(entry: CpuEntryKey, window: u32, tier: u32) -> Option<ImmutableCodeSnapshot> {
    let window = if tier == 2 { (window * 2).min(960) } else { window };
    let mut length = window.min(4096 - (entry.linear.0 & 4095)) as usize;
    let mut bytes = capture(entry.linear.0, length).ok()?;
    if matches!(
        decode(&bytes.bytes, entry.pc, entry.linear, entry.default_32),
        Err(DecodeStop::Incomplete { .. })
    ) && length < 15
    {
        length = 15;
        bytes = capture(entry.linear.0, length).ok()?;
    }
    let mut offset = 0;
    for _ in 0..if tier == 2 { 48 } else { 32 } {
        let Ok(i) = decode(
            &bytes.bytes[offset..],
            GuestEip(entry.pc.0.wrapping_add(offset as u32)),
            LinearAddress(entry.linear.0.wrapping_add(offset as u32)),
            entry.default_32,
        ) else {
            break;
        };
        offset += i.length as usize;
        if offset == bytes.bytes.len()
            || match i.flow {
                Flow::Next => false,
                Flow::Relative {
                    displacement,
                    conditional,
                    call,
                } => !conditional || call || displacement < 0,
                _ => true,
            }
        {
            break;
        }
    }
    // Retain an undecodable first instruction as a failed input fingerprint.
    if offset > 0 {
        capture(entry.linear.0, offset).ok()
    } else {
        Some(bytes)
    }
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
    record(false);
    if !cold() {
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
            let h = s.hot.pop_front().unwrap();
            let tier = cache::tier(h.entry);
            let needed = if tier == 0 { config.threshold } else { config.promote };
            if selected.is_none() && tier < 2 && h.hits >= needed {
                selected = Some((h.entry, tier + 1, config));
            }
            s.hot.push_back(h);
            if selected.is_some() {
                break;
            }
        }
        selected
    };
    let Some((entry, tier, config)) = selected else {
        return;
    };
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
    let request = CompileRequest {
        key,
        pc: entry.pc,
        linear: entry.linear,
        default_32: entry.default_32,
        tier: if tier == 1 { Tier::One } else { Tier::Two },
    };
    let config = IrConfig {
        optimize: tier == 2,
        passes: Default::default(),
        execution_budget: config.budget,
        rep_iteration_budget: config.rep,
        max_code_bytes: 960,
        layout: StateLayout {
            gpr: 0,
            flags: 32,
            eip: 36,
            committed: 40,
            flag_operand: 44,
        },
    };
    let Ok(artifact) = compile_cpu_cfg_region(&request, &snapshot, &config) else {
        failed(entry, tier, Some(snapshot));
        return;
    };
    if !cache::make_room(entry) {
        return;
    }
    let ptr = artifact.code.bytes.as_ptr() as u32;
    let len = artifact.code.bytes.len() as u32;
    let job = Job {
        observed: snapshot.dependencies.clone(),
        artifact,
        source: snapshot.clone(),
        stale: false,
    };
    let slot = cache::reserve_job(job, true);
    if slot == 0 {
        failed(entry, tier, Some(snapshot));
        return;
    }
    {
        let mut s = SCHEDULER.try_lock().unwrap();
        if let Some(h) = s.hot.iter_mut().find(|h| h.entry == entry) {
            h.source = Some(snapshot);
            h.failed = 0;
        }
        s.pending = Some(Pending {
            id: key.job,
            slot,
            entry,
            tier,
        });
    }
    // JS copies bytes now; all Rust locks have been released. Completion is a
    // later microtask, never recursive compilation/publication in this frame.
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
        _ => 0,
    }
}
