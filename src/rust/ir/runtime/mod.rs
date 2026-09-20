pub mod diagnostics;
pub mod compile;
pub mod entry;
pub mod far_control;
pub mod memory;
pub mod region;
pub mod system;

pub mod io;

pub mod rep;

pub mod cpu_info;

pub mod cpu_system;

pub mod control_regs;

pub mod descriptor;

pub mod task_regs;

pub mod selector_query;

pub mod simd;

pub mod x87;

#[cfg(feature = "ir-experimental")]
pub mod cache;
pub mod live;
#[cfg(feature = "ir-experimental")]
pub mod schedule;
#[cfg(any(feature = "ir-experimental", test))]
mod hot_index;
pub mod snapshot;

pub mod fp_state;

mod sse_fp;

mod mmx;

mod coverage;

/// Host diagnostics: the portable build deliberately declines vector IR emission.
#[no_mangle]
pub extern "C" fn ir_wasm_simd_supported() -> u32 {
    u32::from(cfg!(target_feature = "simd128"))
}
