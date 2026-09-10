// One test owns production recorder globals; a fake clock exposes hot-path calls.
#![allow(dead_code)]
mod cpu {
    pub mod cpu {
        pub mod js {
            use std::sync::atomic::{AtomicU64, Ordering};
            pub static CLOCK: AtomicU64 = AtomicU64::new(0);
            pub static CALLS: AtomicU64 = AtomicU64::new(0);
            pub unsafe fn microtick() -> f64 {
                CALLS.fetch_add(1, Ordering::Relaxed);
                CLOCK.load(Ordering::Relaxed) as f64
            }
        }
    }
}
#[path = "../../src/rust/profiler.rs"]
mod profiler;

#[test]
fn execution_sampling_has_a_time_budget_and_retains_late_pages() {
    use profiler::*;
    use cpu::cpu::js::{CLOCK, CALLS};
    use std::sync::atomic::Ordering::Relaxed;
    unsafe {
        performance_recording_enable(false);
        assert!(performance_batch_start().is_none());
        for _ in 0..1000 {
            let sample = performance_chunk_start(true, 0x401234, 0x1000, 3);
            assert!(!sample);
            performance_chunk_finish(sample, 16);
        }
        assert_eq!(CALLS.load(Relaxed), 0);
        performance_recording_enable(true);
        let start = performance_batch_start();
        let calls = CALLS.load(Relaxed);
        for _ in 0..1_000_000 {
            let sample = performance_chunk_start(true, 0x401234, 0x1000, 3);
            performance_chunk_finish(sample, 16);
        }
        assert_eq!(CALLS.load(Relaxed), calls, "a million chunks must not read the clock");
        CLOCK.store(1, Relaxed);
        performance_timer_finish(start, 0);
        assert_eq!(performance_recording_get(16), 1_000_000.0);
        assert_eq!(performance_recording_get(20), 1.0);
        assert_eq!(performance_recording_get(8), 1.0);
        assert_eq!(performance_recording_get(18), 0.0, "no fake tiny-chunk timing");

        // Many batches within one 10ms budget cannot create more samples.
        for _ in 0..1000 {
            let start = performance_batch_start();
            performance_chunk_finish(performance_chunk_start(false, 0x401234, 0x2000, 3), 8);
            performance_timer_finish(start, 0);
        }
        assert_eq!(performance_recording_get(20) + performance_recording_get(21), 1.0);
        assert_eq!(performance_recording_get(17), 1000.0);

        performance_recording_enable(true);
        for batch in 0..20_000 {
            CLOCK.fetch_add(10, Relaxed);
            let start = performance_batch_start();
            for _ in 0..2 {
                let sample = performance_chunk_start(batch % 2 == 0, batch * 4096 + 123, 0x1000, 3);
                performance_chunk_finish(sample, 16);
            }
            performance_timer_finish(start, 0);
        }
        assert_eq!(performance_recording_get(20), 10_000.0);
        assert_eq!(performance_recording_get(21), 10_000.0);
        assert_eq!(performance_recording_hotspot_count(), 8192);
        assert_eq!(performance_recording_get(22), 20_000.0 - 8192.0);
        assert!((0..8192).any(|r| performance_recording_hotspot_get(r, 1) > (18_000 * 4096) as f64),
            "later pages are not permanently excluded by early samples");
        for row in 0..8192 {
            assert_eq!(performance_recording_hotspot_get(row, 4), 1.0);
            assert_eq!(performance_recording_hotspot_get(row, 5), 0.0);
            assert_eq!(performance_recording_hotspot_get(row, 6), 16.0);
        }
        assert_eq!(performance_recording_hotspot_get(8192, 0), 0.0);
        assert_eq!(performance_recording_hotspot_get(0, 10), 0.0);

        // Alternating long and short batches exposes missed target positions.
        for batch in 0..100 {
            CLOCK.fetch_add(10, Relaxed);
            let start = performance_batch_start();
            for _ in 0..if batch % 2 == 0 { 1000 } else { 1 } {
                performance_chunk_finish(performance_chunk_start(true, 0x4000, 0x1000, 0), 1);
            }
            performance_timer_finish(start, 0);
        }
        assert!(performance_recording_get(23) > 0.0);
        performance_recording_enable(false);
        let calls = CALLS.load(Relaxed);
        performance_timer_finish(performance_batch_start(), 0);
        assert_eq!(calls, CALLS.load(Relaxed));
        performance_recording_enable(true);
        assert_eq!(performance_recording_hotspot_count(), 0);
        for index in 0..24 { assert_eq!(performance_recording_get(index), 0.0); }
        assert_eq!(performance_main_loop_exit(100.0, true), 100.0);
        assert_eq!(performance_main_loop_exit(0.0, false), 0.0);
        assert_eq!(performance_recording_get(13), 1.0);
        assert_eq!(performance_recording_get(14), 1.0);
        assert_eq!(performance_recording_get(15), 1.0);
    }
}
