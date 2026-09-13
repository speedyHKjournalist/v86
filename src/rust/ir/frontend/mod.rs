pub(crate) use crate::decode;
mod bits;
mod control;
mod exchange;
pub mod integer;
pub mod lift;
pub mod region;
mod multiply;
mod shift;
mod stack;

mod misc;

mod branch;
mod system_stack;

mod adapters;
mod segment;

mod string;

pub mod io;

mod rep;

pub mod cpu_info;

pub mod cpu_system;

pub mod control_regs;

pub mod descriptor;

pub mod task_regs;

pub mod selector_query;

mod simd_integer;
mod simd_moves;

mod simd_immediate;

mod simd_shuffle;

mod simd_transfer;

mod simd_lane;

pub mod simd_masked;
