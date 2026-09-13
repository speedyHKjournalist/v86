# Experimental automatic IR compilation and promotion

The experimental CPU can now collect entry heat, select a bounded code window,
compile IR, publish it asynchronously and promote a working Tier 1 entry to an
optimized Tier 2 entry. This uses the existing [IR cache](ir-cache.md) and normal
CPU dispatcher. It is disabled by default, including in `ir-experimental` builds.
Ordinary production builds still use legacy compilation and have no automatic IR
exports. This does not complete the implementation plan's ISA, full region/link
architecture, production-default, OS or performance gates.

## Configuration and boundaries

The experimental Wasm export
`ir_auto_config(enabled, threshold, promote, window, budget, rep) -> i32` accepts:

| Argument | Accepted values / meaning |
|---|---|
| enabled | 0 or 1; automatic request generation only |
| threshold | 1–1,000,000 observed entries before Tier 1 compilation |
| promote | 1–1,000,000 new observed entries after publication before promotion |
| window | 15–960 bytes for the Tier 1 candidate window |
| budget | 1–4,096 execution work units per compiled call |
| rep | 1–4,096 REP elements per compiled call |

The initial settings are disabled, 16, 64, 192, 256 and 64 respectively. Invalid
arguments or configuration inside an active IR/legacy frame or a held JIT lock
return false. Configuration clears heat and failure history and cancels the matching
automatic pending reservation. Already published entries remain executable; use
normal JIT cache clear to invalidate them. CPU reset/snapshot restore preserves the
configuration but drops heat, failures and pending identities with the runtime
cache. This metadata is not serialized into guest state. The public
[`jit_backend: "ir"` option](ir-backend.md) now enables this policy and disables
legacy generation, including inside a CPU Worker. `ir_region_budget` supplies
these limits and `get_jit_info()` reports copied runtime statistics. Direct use of
the Wasm configuration export remains independent of the legacy generation switch.

Heat is a count of observed entry visits, not retired guest instructions. Normal
CPU dispatch contributes visits; legacy linked entries contribute visits even when
performance recording is off. The link path only records heat. Compilation waits
until an outer CPU dispatch point with no guest locals alive and no held JIT/cache
lock. Up to 128 entry-key records are retained in a rotating bounded queue.

Candidate scanning/capture happens at most once per `main_loop` invocation, even
when no entry is ready. There is at most one current automatic job awaiting
instantiation; browser work from cancelled/reset jobs can still finish later.
These limits bound scheduling work; they do not provide a measured
wall-clock compilation deadline or Worker compiler. Actual compilation currently
runs synchronously in the CPU Wasm, and browser instantiation is asynchronous.

## Region selection and two tiers

The selector uses the shared decoder over read-only immutable bytes. Tier 1 scans
at most 32 decoded instructions in the configured window. Tier 2 scans at most 48
instructions in twice that window, capped at 960 bytes. Selection normally stays
within the current linear page; it can extend the initial capture to 15 bytes when
the first instruction crosses a page. Stop/boundary instructions, calls,
unconditional branches and backward branches terminate the sequential window.
Forward direct targets outside the selected window remain explicit exits in the
existing reachable CFG frontend. An undecodable trailing instruction ends the
window before it; an unsupported first instruction or unsupported IR lowering is
a recorded compile stop, followed by ordinary CPU execution.

Tier 1 uses the shared HIR/MIR/Wasm compiler without the optimization pipeline.
Tier 2 runs the current verified passes, including GVN, constant branch pruning,
DCE and budget-preserving block merging. Artifacts now retain their tier metadata.
This is a functioning lightweight/optimized policy for currently implemented
lowerings, not the complete planned Tier 1 ISA or mature multi-page Tier 2 selector.

Queued entry keys need not equal the CPU IP when compilation finally runs. The
compiler receives the saved IP/CS/default-width key and a fresh snapshot under the
currently visible MMU mapping; it does not specialize on live GPR or FLAGS values.
Publication/admission still checks generation, source bytes and physical mappings,
and actual execution requires the saved entry context. Cold code-fetch A-bit and
secondary-page admission rules from the cache remain in force.

## Failure, publication and capacity

Automatic jobs share the live compiler's non-wrapping instance identity allocator.
The new `env.ir_codegen_finalize(id: i64, slot, ptr, len)` bridge copies bytes before
returning to Rust, then uses the same transactional publisher as explicit requests.
JS receives the ID as BigInt. Rebuild JS and experimental CPU Wasm together; the
zstd worker's import stubs also include the new import for snapshot operations.
No Rust lock is retained across the host call. Completion is deferred to a later
microtask, including synchronous instantiation failures.

`ir_auto_complete(id, success)` consumes only the matching pending identity. A
pending/validated cache record cannot be reported complete prematurely. Reset or
configuration changes make old completions irrelevant to new jobs. Successful
publication resets that key's heat. Browser failure retains the old published
Tier 1 entry and records the failed tier and source. Unchanged failed source
bytes/mappings suppress repeated compilation. Writes remove dependent history;
fresh capture also allows retry when bytes/mappings change without notification,
or a previously unreadable mapping becomes readable. Failed snapshots themselves
do not register code watches. This is bounded input-based suppression, not a
complete global physical-page version registry.

Automatic requests can evict only automatic published records, and never evict
the entry they are attempting to upgrade. Explicitly published entries are not
automatic eviction candidates. The shared cache still holds at most 32 records
within the original 899-slot pool. A failed compilation does not evict a record;
space is reclaimed only after compilation succeeds. Evicted entry heat is reset
so inactive historical entries do not continually recompile. Active-frame and
legacy-lock quiescence rules continue to govern slot collection.

`ir_auto_stat(field)` reports cumulative wrapping counters:

| Field | Meaning |
|---|---|
| 0 / 1 | Ordinary / legacy-linked entry observations |
| 2 / 3 | Tier 1 / Tier 2 compilation attempts |
| 4 / 5 | Tier 1 / Tier 2 successful publication completions |
| 6 / 7 | Capture/compiler stops / failed publication completions |
| 8 | Suppressed unchanged failed inputs |
| 9 / 10 / 11 | Retained heat records / pending task flag / enabled flag |

## Validation scope

`make ir-auto-tests` runs in Node on debug/release builds with JIT pool invariants
and an experimental-only release without test hooks. It exercises automatic
compilation/promotion with recording off/on and independently checked loop counts.
A separate case disables legacy generation and checks zero legacy publisher calls.
Further cases cover 16-bit CS/AX behavior, a 42-instruction loop spanning lightweight regions and a
larger optimized region, unsupported-input suppression and changed-code retry,
failed upgrades retaining Tier 1, a held pending task, reset/late completions,
snapshot restore/recompilation, premature completion rejection and 40-entry
capacity eviction preserving an explicit entry. The two invariants builds also
start with actual linked legacy modules and show linked heat producing IR modules
only after return to cold dispatch. The explicit cache matrix is rerun alongside
these tests, including legacy capacity pressure and active invalidation.

The automatic policy is independent of the existing legacy-generation setting;
tests can disable legacy generation after clearing its cache and still exercise IR
with ordinary interpretation between published entries.
The automatic test workloads deliberately use low thresholds to exercise transitions;
they do not modify the guest clock or constitute workload performance measurements.
Browser-hosted/Worker policy control, complete cross-backend links, the remaining
ISA and OS/performance acceptance remain outstanding.
