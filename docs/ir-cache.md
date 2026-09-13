# Experimental IR publication and CPU dispatch

Explicitly compiled IR modules can now occupy the existing CPU function table and
execute through `cycle_internal`. This is a runtime integration step toward IR-09
and IR-12. An [opt-in automatic policy](ir-auto.md) now supplies bounded IR region
selection and promotion; the production default remains legacy. The full implementation plan,
remaining ISA and OS/performance gates remain outstanding.

## Ownership and publication

`CPU.prototype.ir_compile_cached(length, tier, optimize, cfg, budget, rep_budget)`
uses the live compiler, synchronously copies its Wasm bytes, transfers the matching
live result to a cache reservation, and asynchronously instantiates the copy. It
returns a Promise<boolean>. Without the experimental exports it resolves false.
This is an internal CPU API, not a new V86 starter/Worker RPC or backend option.

The cache holds at most 32 records, including pending and retired records awaiting
collection. Slots come from the same 899-slot pool as legacy modules. IR owners
are included in the pool's disjointness/capacity invariants; they do not enter
legacy page entry tables or legacy direct links. Legacy capacity eviction operates
on its own published queue and leaves IR reservations intact. If no slot is free,
reservation fails. No extra table range or unbounded allocation is introduced.

Each reservation records its nonzero, non-wrapping live job ID as its slot
generation. IDs are scoped to the original CPU Wasm instance. Rust reset/cache
invalidation retires the records, and JS also checks the captured Wasm instance,
exports and table identities. Obsolete handles cannot release a reused slot.
The live result is consumed on reservation; callers must copy bytes first.

The state sequence is Pending → Validated → Published → Retired → collected.
Only Published entries participate in CPU lookup. JS checks the function export
and calls `ir_cache_validate` immediately before table.set, then calls
`ir_cache_finish` synchronously. No guest execution or asynchronous yield belongs
between those operations. The real WebAssembly.Table.set operation does not invoke
guest callbacks. Validation checks exact current source bytes and ordered physical
mappings; the CPU IP may have moved while instantiation was pending. Execution
admission separately checks the saved IP/CS/default-width key.

Missing exports, synchronous/asynchronous instantiation errors, table installation
errors and stale completions resolve false and retire only the matching owner.
A failed replacement leaves the older published entry available. A successful
replacement retires older published records with the same entry key. Compilation
through this API is explicit. The separate automatic policy adds failure suppression
and tier scheduling without changing this explicit request contract.

Experimental Rust exports:

| Export | Result / purpose |
|---|---|
| `ir_cache_reserve(id: i64)` | Shared slot index, or 0; consumes the matching live result |
| `ir_cache_validate(id, slot)` | Pending ownership/source validation before installation |
| `ir_cache_finish(id, slot)` | Makes a validated matching reservation visible |
| `ir_cache_cancel(id, slot)` | Retires only the matching non-retired record |
| `ir_cache_collect()` | Number of retired records collected at a cold point |
| `ir_cache_stat(field)` | 0 published, 1 retained records, 2 actual hits, 3 validation rejections, 4 cancellations, 5 collected records |

JS uses BigInt IDs and full-width i32 slot values; high-bit slot forgeries are
rejected. Experimental builds also expose the actual shared free-slot count through
the existing `jit_get_wasm_table_index_free_list_count` export, without requiring
the profiler feature. Ordinary production builds retain its previous behavior.

## Code watches, admission and retirement

Reservation registers every unique physical source page with JitState and marks
existing TLB aliases with TLB_HAS_CODE. Future normal translations also consult
these watches. Guest stores therefore take the existing code-write path; host
`write_blob`/`write_memory` uses the existing dirty-cache notification. Writes retire
all dependent IR records, including pending jobs, and remove their watches while
preserving other IR/legacy owners. Same-byte writes still invalidate. Reset/cache
clear retires all records without adding runtime metadata to guest snapshots.
Exact byte/mapping capture at publication and admission additionally detects raw
writes and remaps that bypass notification. This is not a complete global page
version registry or an audit of every shared-memory/device write ingress.

After matching an entry, the CPU performs its normal initial instruction-fetch
translation, including page-table accessed-bit effects. It then revalidates the
source: code can alias its own page table and be changed by that A-bit write.
Every source mapping must already have a usable CPU TLB translation before an IR
call. Missing secondary translations cause a cache miss and ordinary execution;
admission does not eagerly access unreachable pages. A later entry can hit once
the relevant translations exist. This conservative guard preserves cold fetch
behavior while per-instruction fetch planning remains future work.

Execution holds no cache/JIT lock or borrowed artifact metadata across the Wasm
call. An active flag prevents nested compilation, collection and cache execution.
Publication and collection also require the legacy JIT lock to be available; a
synchronous generator host callback cannot reenter its reservation/free machinery.
Dirty/reset/cancel hooks can retire the running record during a guest I/O callback,
but its function slot remains owned until the call returns. The outer dispatcher
then collects retired slots. The existing terminal store/helper and CPU-owned
slow exits prevent continuing stale guest code after a side effect. No IR links
or nested IR activations are introduced. This is quiescent reclamation for this
cold dispatch path, not the completed planned cross-backend link dependency graph.
Host-aborting traps are not a recoverable guest exit protocol.

Actual IR calls contribute retirement steps to performance recording with recording
on or off selecting the same entry. A zero-retirement exit retires the entry, so
e.g. a zero-budget REP request cannot repeatedly stall CPU scheduling; subsequent
cycles interpret. Automatic promotion is supplied separately; adaptive IR budgeting
remains future work.

## Validation

`make ir-cache-tests` runs the actual JS publication bridge and normal CPU dispatcher
in debug/release builds with JIT pool invariants, plus a release build with only
`ir-experimental`. It tests both tier requests, optimization and recording settings;
independently expected loop/store state and wrapped counts; precise data #PF;
cold/cross-page/unreachable fetch A bits; code/PTE alias invalidation; pending and
forged/duplicate/ABA publication; changed mappings, raw/same-byte/secondary-page
writes; running SMC; restore; synchronous I/O write/reset with deferred collection;
nested compilation refusal; zero-budget REP recovery; and browser/export/table
failures and out-of-order completion.
Host identity changes and a synchronous legacy-generator callback are also checked.

Each invariants build additionally publishes 900 actual legacy modules while 32 IR
records remain installed, exercises legacy eviction and executes a surviving IR
entry, then checks complete reclamation to 899 free slots. These are Node-hosted
CPU tests. They do not establish browser/Worker IR scheduling, all-ISA production
coverage, OS boot or workload performance acceptance.
