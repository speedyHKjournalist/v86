# Bounded REP semantic engine

The shared CPU string executor now exposes an explicit semantic batch result:
`StringExecution { outcome: Complete | Repeat | Fault, iterations }`. The [REP HIR adapter](ir-rep.md) now uses this result for progress exits and final
commit accounting. Online scheduling remains pending; this document describes the
underlying semantic primitive and its separate engine-level evidence.

## Execution and progress contract

`execute_rep(kind, bytes, asize32, segment, repne, limit)` supports the seven
recognized MOVS/CMPS/STOS/LODS/SCAS/INS/OUTS families. It receives semantic operands,
not an opcode to interpret. The existing shared implementation retains its
native page-bounded copies/fills, overlapping-element copies, safe-memory slow
paths, port operations, source/destination translation order and CPU-owned faults.
STOS/SCAS/INS retain their fixed ES semantics. REPNE affects comparison families;
the other families repeat by count, as in their legacy wrappers.

Zero count completes before segment, permission or device access, even when the
element budget is zero. With nonzero count and zero budget, the engine returns
Repeat with zero iterations and previous_ip, without accessing operands.

A positive limit bounds completed elements in both the native page batch and the
slow loop. Normal page boundaries can return earlier than the limit. Reaching the
limit with remaining work commits ECX/ESI/EDI progress, restores previous_ip and
returns Repeat. Completed comparisons and exhausted counts return Complete.

The result does not infer failure or continuation by comparing instruction_pointer
to an expected address. Early segment/permission/translation faults return Fault
with zero progress; a slow-loop fault returns Fault after the existing epilogue
has materialized exactly the completed iterations. Exception delivery remains
CPU-owned and is never repeated by this interface. This distinction still works
when the fault handler IP happens to equal decoded next IP.

CMPS/SCAS retain the pinned comparison-FLAGS update points. A budget cut while the
comparison continues preserves the incoming FLAGS and last_op1, like an unfinished
native page batch. Termination updates them from the last comparison. Early
comparison termination takes precedence over a coincident budget boundary.
LODS retains accumulator writes from completed elements. Prior writes and port
reads are not rolled back when a later access faults.

Each new entry exposes its committed progress to its device callbacks. Added
budget boundaries therefore introduce valid intermediate snapshots relative to
an unbounded slow call. The tests compare exact events within equivalent batches
and final architectural state across smaller budget cuts; they do not claim
identical cross-batch device snapshots or established online IRQ scheduling.

The caller owns decoded-PC preparation and scheduling. This API **does not update
instruction_counter**. The IR adapter distinguishes element work, partial REP progress and final
instruction commit, charging only final completion. Online scheduler integration
remains pending.

Legacy string entry points retain their signatures and call the same engine with
`u32::MAX`. Their old page/slow-loop policy remains intact. The extra slow-loop
budget test is explicitly disabled for this constant limit, allowing inlining to
remove iteration/result bookkeeping unused by legacy callers.

## Independent reference and evidence

`make ir-rep-engine-tests` builds a temporary isolated CPU with the original
`string_instruction` body from commit
`8ee73e538daaab15411344d39a1f271e778ac7f3`. Its legacy wrappers call that pinned body;
the rest of the CPU comes from the current working tree. This isolates the string
refactor while retaining current test hooks. The original body is obtained with
`git show`, not copied from the new executor. The builder writes source/wasm hashes
and reference scope to `build/rep-reference.json`, and does not change live sources.

The dedicated test-only `ir_test_rep_batch` packs explicit outcome and iteration
count for the Node harness. It is absent from ordinary CPU exports. Both CPUs run
actual guest REP encodings, including the existing native and device paths.

- 1,344 old/new legacy-wrapper and explicit-engine comparisons across seven
  families, code/address widths, element widths, F2/F3, DF and zero/nonzero counts.
- 84 complete memory/port device sequences and 252 segment/page fault or partial
  progress states.
- 588 zero/exact/partial budgets and 84 sequences of bounded reentry without
  duplicated iterations.
- 288 REPE/REPNE early terminations across budget boundaries; 84 maximal unsigned
  counters stopped after two elements; 84 zero-count/zero-budget precedence cases.
- 42 page-batch reentry faults and 84 faults whose handler IP equals decoded next
  IP, proving the outcome is not guessed from EIP.
- 144 physical-alias/LZ overlap copies across budget cuts, 24 explicit REP I/O
  permission failures, and 504 source overrides/fixed ES cases.

Comparisons include GPRs, FLAGS/last_op1, IP, CR2, CPL, code/stack modes, segment
selectors/bases, memory and fault frames. The engine's lack of counter ownership
is checked independently. This establishes a bounded semantic primitive; the full
online REP scheduling, runtime integration and performance/OS acceptance remain open.
