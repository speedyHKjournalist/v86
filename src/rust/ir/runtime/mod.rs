pub mod compile;
pub mod diagnostics;
pub mod entry;
pub mod far_control;
pub mod memory;
pub mod region;
pub mod system;
mod flags;

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
#[cfg(any(feature = "ir-experimental", test))]
mod hot_index;
#[cfg(any(feature = "ir-experimental", test))]
mod peers;
#[cfg(any(feature = "ir-experimental", test))]
mod promotion;
pub mod live;
#[cfg(feature = "ir-experimental")]
pub mod schedule;
pub mod snapshot;

pub mod fp_state;

mod continuation;
mod sse_fp;

mod mmx;

mod coverage;

/// Host diagnostics: the portable build deliberately declines vector IR emission.
#[no_mangle]
pub extern "C" fn ir_wasm_simd_supported() -> u32 { u32::from(cfg!(target_feature = "simd128")) }
