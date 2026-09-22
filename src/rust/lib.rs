#[macro_use]
mod dbg;

#[macro_use]
mod paging;

pub mod cpu;

pub mod js_api;
pub mod profiler;

// Shared read-only frontend; the rest of IR remains experimental.
mod analysis;
mod codegen;
mod config;
mod control_flow;
mod cpu_context;
#[allow(dead_code)]
#[path = "ir/frontend/decode.rs"]
pub(crate) mod decode;
mod decode_rules;
mod gen;
#[cfg(any(test, feature = "ir-experimental"))]
mod ir;
mod jit;
mod jit_instructions;
mod leb;
mod modrm;
mod opstats;
mod page;
mod prefix;
mod regs;
mod simd_codegen;
mod softfloat;
mod state_flags;
mod wasmgen;
mod x87_codegen;
mod x87_profiler;
mod zstd;
