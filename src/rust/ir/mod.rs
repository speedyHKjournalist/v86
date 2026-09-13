//! Experimental IR components. Production still uses the legacy backend until coverage gates pass.
#![allow(dead_code)]
pub mod analysis;
pub mod backend;
pub mod builder;
#[cfg(test)]
#[path = "../../../tests/ir/semantics/control.rs"]
mod control_tests;
#[cfg(test)]
#[path = "../../../tests/ir/semantics/core.rs"]
mod core_tests;
pub mod dump;
pub mod effects;
pub mod frontend;
pub mod helper;
#[cfg(test)]
#[path = "../../../tests/ir/semantics/helpers.rs"]
mod helper_tests;
pub mod hir;
pub mod ids;
pub mod lowering;
#[cfg(test)]
#[path = "../../../tests/ir/semantics/memory.rs"]
mod memory_tests;
pub mod mir;
pub mod passes;
pub mod runtime;
pub mod simd;
#[cfg(test)]
#[path = "../../../tests/ir/semantics/stack.rs"]
mod stack_tests;
pub mod state;
pub mod types;
pub mod verify;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/shifts.rs"]
mod shift_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/multiply.rs"]
mod multiply_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/bits.rs"]
mod bit_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/enter.rs"]
mod enter_tests;
#[cfg(test)]
#[path = "../../../tests/ir/semantics/exchange.rs"]
mod exchange_tests;
#[cfg(test)]
#[path = "../../../tests/ir/semantics/loops.rs"]
mod loop_tests;
#[cfg(test)]
#[path = "../../../tests/ir/semantics/misc.rs"]
mod misc_tests;
#[cfg(test)]
#[path = "../../../tests/ir/semantics/segments.rs"]
mod segment_tests;
#[cfg(test)]
#[path = "../../../tests/ir/semantics/system_stack.rs"]
mod system_stack_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/strings.rs"]
mod string_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/io.rs"]
mod io_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/rep.rs"]
mod rep_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/cpu_info.rs"]
mod cpu_info_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/cpu_system.rs"]
mod cpu_system_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/control_regs.rs"]
mod control_regs_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/descriptor.rs"]
mod descriptor_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/task_regs.rs"]
mod task_regs_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/selector_query.rs"]
mod selector_query_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/verr.rs"]
mod verr_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/cmpxchg8b.rs"]
mod cmpxchg8b_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/simd_moves.rs"]
mod simd_move_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/simd_integer.rs"]
mod simd_integer_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/simd_immediate.rs"]
mod simd_immediate_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/simd_shuffle.rs"]
mod simd_shuffle_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/simd_transfer.rs"]
mod simd_transfer_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/simd_lane.rs"]
mod simd_lane_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/simd_masked.rs"]
mod simd_masked_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/mir_memory.rs"]
mod mir_memory_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/mir_effect.rs"]
mod mir_effect_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/mir_arithmetic.rs"]
mod mir_arithmetic_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/dynamic_count.rs"]
mod dynamic_count_tests;
#[cfg(test)]
#[path = "../../../tests/ir/semantics/mir_call.rs"]
mod mir_call_tests;
#[cfg(test)]
#[path = "../../../tests/ir/semantics/mir_control.rs"]
mod mir_control_tests;
#[cfg(test)]
#[path = "../../../tests/ir/semantics/mir_state.rs"]
mod mir_state_tests;
#[cfg(test)]
#[path = "../../../tests/ir/semantics/mir_value.rs"]
mod mir_value_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/cfg_frontend.rs"]
mod cfg_frontend_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/merge.rs"]
mod merge_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/dataflow.rs"]
mod dataflow_tests;

#[cfg(test)]
#[path = "../../../tests/ir/semantics/entry.rs"]
mod entry_tests;
