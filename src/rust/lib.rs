#[macro_use]
mod dbg;

#[macro_use]
mod paging;

pub mod cpu;

pub mod js_api;
pub mod profiler;

// Shared read-only frontend; the rest of IR remains experimental.
#[allow(dead_code)]
#[path = "ir/frontend/decode.rs"]
pub(crate) mod decode;
mod analysis;
mod codegen;
mod config;
mod control_flow;
mod cpu_context;
mod gen;
mod jit;
#[cfg(any(test, feature = "ir-experimental"))]
mod ir;
mod jit_instructions;
mod leb;
mod modrm;
mod opstats;
mod page;
mod prefix;
mod regs;
mod softfloat;
mod x87_profiler;
mod x87_codegen;
mod simd_codegen;
mod state_flags;
mod wasmgen;
mod zstd;
