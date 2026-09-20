# IR-13 end-to-end tuning: cached admission

## Diagnostic trigger

The first IR-13 cold/warm smoke deliberately uses the same experimental release
core for both backend policies. On GitHub IR-core run 340, its tiny `INC/JMP-back`
loop reported approximately:

- IR Tier 1 publication: 45 ms
- IR Tier 2 publication: 72 ms
- IR short warm window: 192k guest instruction-counter steps/ms
- legacy short warm window: 1.26M steps/ms

These hosted-runner numbers are **diagnostic only**. They are not an application
benchmark or a release threshold. Their value is that the gap is large enough to
inspect the execution topology before attempting fine-grained optimizer tuning.

The IR run also showed roughly 799k cache admissions during the short window.
Inspection found that every admission called `capture()` before execution and again
after the architectural initial fetch. `capture()` allocates snapshot vectors,
re-observes page tables/mappings and copies source bytes. The compiled two-instruction
loop already stays inside the IR CFG until its execution budget expires, so this
cold revalidation cost is paid repeatedly at every activation.

## Fast-admission contract

Hot execution may avoid a new read-only snapshot only when all compiled source
pages already have CPU-visible cached translations.

`snapshot::cached_match()` has three outcomes:

- `Match`: every saved linear->physical mapping equals the current cached TLB
  mapping and every source byte still equals the authoritative physical RAM byte.
- `Unavailable`: at least one source page does not yet have a suitable cached
  translation. Before the architectural fetch, cache lookup falls back to the
  existing read-only `capture()`; after the fetch, execution simply declines
  admission without retiring the record.
- `Stale`: VM generation is stale, a cached mapping disagrees, or direct physical
  source bytes differ. The record is retired.

The direct byte comparison is required. It preserves the existing IR-12 guarantee
that raw/unnotified host writes cannot execute stale IR even when they bypass normal
code-page dirty notifications.

## Boundaries intentionally unchanged

- reserve/validate/finish publication stages still use full read-only capture;
- IR link discovery still uses full capture and does not require cached TLB
  visibility, preserving the IR-12 A-bit/link-discovery contract;
- the real initial instruction fetch still runs before activation and keeps its
  architectural accessed-bit behavior;
- a cold cross-page region is not allowed to execute until the CPU has made all
  required source mappings visible;
- dirty-page/reset/restore invalidation, slot ownership and active-frame reclamation
  are unchanged;
- execution budget/device scheduling semantics are unchanged.

## Evidence

`ir_cache_stat(8)` counts cached mapping/source checks and
`ir_cache_stat(9)` counts pre-fetch full-capture fallbacks. The cache differential
adds a permanent warm one-page case: after one execution has established the code
translation, the next IR admission must hit the cached validator and must not add a
capture fallback.

The IR-13 structured performance smoke records both counters alongside cache hits.
The optimization is accepted only if the full IR-12 cache/auto lifecycle, raw SMC,
page-table alias/A-bit, cold cross-page, restore and browser/Worker acceptance
matrices remain green.

## Hosted-runner observation after the change

The green IR-core run for this PR recorded the following same-core diagnostic:

- Tier 1 publication: 28.44 ms
- Tier 2 publication: 52.07 ms
- IR warm window: about 454.5k instruction-counter steps/ms
- paired legacy warm window: about 1.445M steps/ms
- IR cache hits: 1,884,623
- cached mapping/source checks: 3,769,246
- pre-fetch full-capture fallbacks: 0

Compared with the earlier hosted smoke, the IR warm diagnostic moved from roughly
192k to 454.5k steps/ms while the full-capture fallback counter stayed at zero in
the warmed one-page case. This is useful evidence that admission revalidation was
a real hot-path cost, but it is **not** a stable performance ratio: GitHub runner
timing is noisy and the synthetic loop is not an XP/application workload.

The remaining same-run gap to the paired legacy policy is still large. The next
end-to-end investigations should therefore focus on activation frequency and
IR-to-IR continuation/link consumption, region/execution-budget policy, and
per-activation state materialization before attempting smaller peephole tuning.

## Activation diagnostics and warning-free production build

The next tuning step instruments the amount of guest work completed by each
published IR activation instead of inferring activation pressure from cache-hit
counts alone. The cache now exposes cumulative retired guest instructions,
maximum retired instructions in one activation, and zero-step exits. The public
`get_jit_info()` snapshot and IR-13 performance smoke report those values and
derive average guest instructions per activation.

These counters do not alter region selection, execution budgets or scheduling.
They are intended to decide whether the next optimization should be a larger
execution budget, direct IR-to-IR continuation, or reduced entry/exit
materialization. The cache differential also checks that a normal one-instruction
activation increments retired work and does not look like a zero-step exit.

The ordinary production Wasm build had also started warning that twelve
WasmBuilder numeric helpers were unused. They are required by the IR backend but
the entire IR module is excluded from a normal production build. Those helpers are
now compiled only for tests or `ir-experimental`, matching the existing module
boundary rather than globally suppressing `dead_code`. IR-core additionally runs
production and experimental release `cargo check` with `RUSTFLAGS="-D warnings"`
so the warning cannot silently return.

The release Makefile also no longer implements an optional disabled `wasm-opt`
step by executing `false` under make's ignored-error prefix. When
`WASM_OPT=false` (the default), optimization is skipped with a successful shell
conditional, so a normal build no longer prints the misleading
`Error 1 (ignored)` line. Enabling the existing `WASM_OPT` command path retains
the previous `wasm-opt -O2 --strip-debug` behavior.

The IR-13 smoke already subsumes the separate Node and Chromium host/browser
acceptance commands. The workflow now runs that complete smoke once rather than
executing the same host/browser matrix three times.

## Execution-budget activation matrix

IR-core run 352 completed the first same-core matrix from PR #44:

| Dispatcher budget | Median IR steps/ms | vs IR-128 | vs paired legacy |
|---:|---:|---:|---:|
| 128 | 363,779 | 1.00x | 29.1% |
| 256 | 499,802 | 1.37x | 40.0% |
| 512 | 612,676 | 1.68x | 49.0% |
| 1024 | 691,443 | 1.90x | 55.4% |
| legacy | 1,249,203 | - | 100% |

Throughput rises materially as the execution budget increases, so repeated IR
activation/return is a real end-to-end cost. The gain is also diminishing:
roughly +37% from 128 to 256, +23% from 256 to 512, and +13% from 512 to 1024.
This makes it unlikely that simply increasing the runtime default budget can close
the remaining gap by itself.

The first matrix also reported an apparent activation utilization near 50% because
it divided retired guest instructions by `execution_budget`. That percentage is
not dimensionally valid. MIR defines `execution_budget` as dispatcher work units,
and each lowered block currently has its own `budget_cost`; it is not a guest
instruction count. PR #45 therefore removes that utilization percentage rather
than reinterpreting it.

The replacement matrix scopes activation counters to the exact published target
entry. Each cache record keeps its own hit count, retired guest steps, maximum
guest steps and zero-step exits, and an experimental query reads those counters by
the exact `CpuEntryKey`. The performance test waits until the target loop itself
has a Tier-2 record before taking the warm sample. This prevents BIOS, mailbox or
other automatically compiled entries from contaminating the activation average.

The upper-bound matrix now uses:

```text
128 / 256 / 512 / 1024 / 2048 / 4096
```

with three fresh-VM samples per value and the same alternating order used by PR
#44. It reports target-entry guest steps per activation as an observed property,
not as a percentage of dispatcher budget. It also records throughput relative to
the previous budget so diminishing returns are explicit.

The matrix remains reproducible with:

```sh
make ir13-budget-matrix
```

and writes `build/ir13-performance-smoke.json`. There is deliberately no timing
pass/fail threshold and no change to the runtime default budget.

Interpretation for the next step:

- if 2048/4096 continue to improve materially, entry/exit amortization remains a
  large target but device/scheduling fairness must be measured before changing policy;
- if throughput flattens, the asymptote estimates how much activation overhead can
  plausibly explain and attention should move to CFG dispatch/state materialization;
- entry-scoped average/max guest-step data distinguishes the target hot loop from
  unrelated compiled work, but it is not a direct count of dispatcher budget units.

## Structured CFG follow-up

The corrected six-point matrix from IR-core run 354 confirmed that the synthetic
target itself consumes the full configured activation budget:

```text
128 -> 127 retired guest instructions
256 -> 255
512 -> 511
1024 -> 1023
2048 -> 2047
4096 -> 4095
```

Median throughput rose from about 355k steps/ms at 128 to about 766k at 4096, but
the final doubling improved throughput by only about 3.7% and the 4096 result was
still only about 60% of the paired legacy median. The remaining gap is therefore
not plausibly explained by outer activation frequency alone.

IR-09 now adds a conservative structured-CFG fast path for single-entry natural
loops. Matching loops emit direct Wasm `loop`/branch control instead of writing a
pc local and re-running a block-id comparison dispatcher on every internal edge.
Simple conditional loops are accepted only when one arm returns to the loop header
and the other exits. All other CFGs stay on the existing generic dispatcher.

The six-point matrix now hard-requires its target Tier-2 entry to report structured
emission and is rerun unchanged. This makes the before/after comparison isolate the
internal CFG-dispatch representation rather than a different activation policy.

Further tuning remains separate: safe IR-to-IR continuation/link consumption,
Tier-1/Tier-2 region/budget policy, compiler-stage timing, and controlled XP/
application benchmarks.

## XP cold-start investigation: bounded regions and working set

The September 19 report of roughly 15 mIPS during XP startup was reproduced
with the supplied XP system disk. The fixes below do not claim XP now matches
legacy; this remains IR-13 acceptance work.

Automatic source selection permits 96 Tier-2 instructions, whereas the fragment
CFG frontend caps its pre-merge graph at 64 blocks. Long straight paths could
therefore fail promotion. Automatic compilation now uses the existing linear CPU
frontend for decoded fallthrough windows, avoiding per-instruction CFG frames
and their subsequent merge. Branching windows retain CFG lifting. Terminal CPU
adapters that the linear frontend cannot represent retain the CFG path.

On a genuine compiler budget failure, the automatic compiler can retry up to
seven smaller instruction-aligned prefixes of the same immutable snapshot. It
trims code mappings/dependencies to the compiled prefix and retains the original
input fingerprint for failure suppression. Invalid snapshot/IR errors are not
hidden by retries. Explicit compilation keeps its all-or-error contract.

The retained cache grows from 32 to 256 entries inside the existing 899-slot pool.
Published CPU keys have an ordered index; retirement collection scans/rebuilds
that index only after a retirement/publication event, rather than on every
admission. Phase, source bytes, mapping identity, entry guards and the second
check after architectural fetch remain mandatory. Active callbacks still cannot
reclaim slots or move records while guest code executes.

The 128-entry heat table also uses an index on the per-activation path instead
of scanning every record. Compilation candidates use a round-robin cursor;
replacement updates fixed slots and two index keys instead of moving/reindexing
all records. Dirty-page notifications rebuild only when records were removed.
Tier 1 retains verified locals from lowering and leaves machine folding,
operand-stack fusion and subsequent reallocation to optimized Tier 2.

Sampling the XP BIOS phase found many short activations around stack and scalar
stores. The CFG lifter previously terminated every commit-bearing store even
though the scalar-store backend already supported guarded continuation. CFG
scalar/RMW stores now use that existing continuation: successful ordinary RAM
writes may continue; cold/MMIO paths and physical aliases of code dependencies
still commit and exit. A truncated next instruction keeps the conservative exit.
Other terminal adapters retain their boundaries. The store differential covers
both linear and CFG frontends, including code aliases and a subsequent page
fault; automatic tests require a PUSH/POP loop to cross its backedge and check
ESP, registers and exact retirement independently.

`get_jit_info()` now includes cache capacity and compiler-stop categories.
Performance recordings contain copied `jit.start` and `jit.end` snapshots, and
`sync_codegen_ms` now includes automatic IR compilation (including bounded
retries); timing is only enabled during explicit recording.

Synthetic diagnostics can be repeated with:

```sh
node tests/ir/performance/working_set.mjs build/v86-ir-runtime.wasm
```

The diagnostic runs fresh 2/256/1024-instruction arithmetic working sets with
paired legacy/IR policies, reports cold-plus-warming throughput and actual IR
retirement, and independently checks the guest instruction count. It does not
represent paging-heavy XP boot, and imposes no noisy wall-clock CI threshold.
Use paired fixed images and the same boot milestone for XP acceptance.

The read-only-image XP diagnostic can be repeated in separate processes:

```sh
node tests/ir/performance/xp_boot.mjs /absolute/path/to/xp.img ir
node tests/ir/performance/xp_boot.mjs /absolute/path/to/xp.img legacy
```

It uses 2 GiB RAM, 16 MiB VRAM, the graphics-proxy PCI device, and in-memory
disk-write overlays. It reports five-second instruction deltas, JIT snapshots
and VGA mode transitions. `IR_BOOT_MS` controls duration (default 30000);
`IR_BENCH_RECORD=1` adds compiler timing and sampled PCs. It runs headless Node,
without the browser CPU Worker or WebGPU host renderer, so it diagnoses CPU/disk
startup rather than validating the complete interactive XP desktop.

### Local XP measurements (September 19)

Single sequential, unrecorded cold runs used the supplied 4 GiB XP C: image,
the same BIOS/device settings and an otherwise idle test process. The old IR
core was preserved before these dispatcher/compiler changes. The legacy policy
ran on the experimental release core to isolate backend policy.

| Observation | Old IR | Updated IR | Legacy |
| --- | ---: | ---: | ---: |
| First 30 s average, mIPS | 15.78 | 25.30 | 129.81 |
| First 60 s average, mIPS | 16.24 | 25.71 | not measured |
| First 640×480×4 transition | 35.51 s | 17.78 s | 6.66 s |
| First 800×600×32 transition | not reached within 60 s | 59.83 s | 20.24 s |

Logs are `build/xp-boot-ir-before.jsonl`, `build/xp-boot-ir-ring.jsonl` and
`build/xp-boot-legacy-final.jsonl`. These are local diagnostic runs, not medians or
browser/Worker acceptance results. VGA transitions are milestones, not proof of
a usable desktop. Original disk writes stayed in RAM overlays.

The updated 60 s IR run retired about 1.067 billion instructions through 156.7
million IR activations (about 6.8 instructions/activation), with zero legacy
compiler requests. About 69% of total guest retirement was through IR. Short
activations and remaining interpreted execution therefore still warrant further
work; this change does **not** establish legacy-equivalent XP performance or
complete IR-13/IR-14.

### Follow-up: continuation, hot entry batches and materialization

Three additional changes now run in the experimental production core:

1. Completed ordinary IR exits request bounded iterative IR-to-IR continuation.
   Each successor uses the full cold entry ABI and admission checks, contributes
   heat, and stays inside the existing CPU batch budget. No compiler runs in the
   chain. Fault, I/O, slow-memory, budget and interrupt-shadow exits still yield.
2. One immutable capture may compile two already-hot entries with matching
   CS/default width/tier. The sibling is queued for serial publication, with full
   generation/source/mapping validation and cancellation on reset/config/SMC.
3. Successful scalar stores to ordinary RAM defer CPU-state materialization until
   the next observer/fault/exit. Tier 2 can also reuse a preceding RAM translation,
   range and permission check for the same proven address with compatible width
   and permissions, while still executing each load/store. Proofs do not cross
   blocks, unknown effects or existing RAM value-cache plans. Every slow path
   clears guard validity before page walks or device callbacks.

This is not register-carry region linking, arbitrary region fusion, or general
cross-page/alias guard elimination. Native tests independently rederive the new
certificates and reject forged metadata. The mixed-width CPU differential compares
64 RAM cases, 30 faults, 24 MMIO/remapping cases and 80 budget exits with the
interpreter. Stack/memory/CFG differentials additionally check delayed store state
at later faults and observers. Automatic/cache matrices cover real successor
execution and held multi-entry publication cancellation; browser main-thread and
Worker integration continue to pass.

The follow-up and the saved pre-continuation core were then run sequentially for
60 seconds each with the same read-only XP image and settings:

| Observation | Saved pre-continuation core | Three changes enabled |
| --- | ---: | ---: |
| First 30 s average, mIPS | 25.60 | 25.86 |
| First 60 s average, mIPS | 26.30 | 26.84 |
| First 640×480×4 transition | 16.85 s | 17.12 s |
| First 800×600×32 transition | 57.68 s | 56.34 s |

Logs: `build/xp-boot-ir-continuation-reference.jsonl` and
`build/xp-boot-ir-continuation.jsonl`. This single pair shows only a small change
(about 2% in 60-second average throughput), not a stable performance acceptance
result. It remains far below the earlier legacy measurement of 129.81 mIPS.
The new run reports 100.6 million linked observations, 213 extra batched entries,
and zero legacy compiler requests. It still performs about 158.7 million IR
activations and 317.4 million cached admission checks for 1.091 billion IR-retired
instructions. Cold linking therefore does not remove the short-region admission
and state-reload cost; these measurements do not establish the requested eventual
advantage over legacy.

### Fast entry validation follow-up

The experimental core now enables bounded byte-validation certificates, warm
post-fetch proof reuse and a 64-slot entry hint cache. All source mappings and
entry context still require validation; observers/interpretation/new CPU batches
revoke certificates. See [the implementation and safety boundary](ir-fast-entry.md).
In one sequential same-core XP A/B pair, the validation switch changed the first
60-second average from 28.75 to 30.06 mIPS and the first 800×600×32 transition from
51.39 to 48.20 seconds. Both modes retained the hint cache. This is a modest local
diagnostic improvement, not proof of legacy-level speed or a total-time profile.

## Hot region fusion follow-up

The experimental runtime now fuses two profiled source regions into one guarded
Tier 2 SSA activation, retaining GPR/FLAGS/XMM and retirement state over internal
edges. Publication, admission and SMC guards cover both snapshots. See
[the fusion contract and tests](ir-fusion.md) for bounds, diagnostics and results.

## XP/compiler investigation and fixed work (2026-09-20)

This iteration accepts only same-image XP cold-start milestones and fixed CPU
workloads. Application/game performance is outside this acceptance run.
`tests/ir/performance/xp_compare.mjs` uses three alternating fresh-VM pairs,
diagnostics off, RAM disk overlays, identical core/options, and the first
800×600×32 mode as its explicit milestone (not proof of desktop idle).
`fixed_work.mjs` compares exact retired work and final architectural state after
Tier-2 warm-up and checks that at least 95% actually retires through cached IR.
Its four throughput ratios use a geometric mean ≥1.0 and a per-workload floor of
0.9; these gates were set before collecting results.

All-call compiler timers identified repeated interference construction as the
main regular lowering cost. Incremental bitset interference preserves the old
conservative graph exactly; randomized reference-graph tests check that invariant.
HIR liveness now has a separate work cap. Machine allocation already charges its
fixpoint and graph work. CFG merge rewrites independent phi tuples in one arena
scan, preserving verification after each pass. The previously observed ~230-second
machine stall has not reproduced; these measurements do not prove its cause.

Additional changes include native RAM RMW continuation with code-alias exits,
matching overlap snapshots, bounded helper-free four-source extension, selective
XMM operand synchronization/reload, finite packed-single SIMD arithmetic with
special-value/task-fault fallback, real/VM86 MOV-segment continuation, and CLI
continuation after privilege checks. Protected-mode segment walks stay terminal.

A larger cache (768 vs 256) did not materially improve the tested cold start;
the default stays 256. The experimental cold capacity setter and eviction count
allow reproduction without increasing the shared Wasm table pool. Hotness hints
compare the full current entry after replacement/compaction before bypassing the
scheduler tree lookup; they do not change heat or promotion thresholds.

An unrestricted four-source trial regressed XP into repeated BIOS interrupt/IRET
execution. It was stopped and excluded from acceptance. Extended traces with
helpers are now rejected before publication; existing working two-source traces
remain installed. This is a real remaining hidden-state/observer correctness gap,
not a successful optimization or completed IR-13 gate.

The initial three-run paired result was IR 38.434 s / 40.049 mIPS versus legacy
19.420 s / 115.991 mIPS to the display milestone: **not accepted**. After the finite
SIMD path, an intermediate fixed-work three-run result had a 1.0617 geometric
mean (integer .9384, RAM .9009, indirect regions 1.5566, SSE .9657).
Final measurements must be rerun after the remaining changes; intermediate figures
are not final acceptance claims. See the generated paired JSON/JSONL reports in
`build/` for exact work, state comparisons, medians and pass flags.

### Paired results before the scheduler/lookup follow-up

Same production core, recording/diagnostics off, no concurrent build/test jobs:

| Work | IR median | Legacy median | IR / legacy |
| --- | ---: | ---: | ---: |
| XP first 800×600×32 | 32.560 s | 18.751 s | 1.736× time |
| XP throughput to milestone | 47.550 mIPS | 119.157 mIPS | 0.399× |
| Fixed integer | 174.464 mIPS | 181.404 mIPS | 0.962× |
| Fixed RAM RMW | 101.371 mIPS | 111.880 mIPS | 0.906× |
| Fixed indirect regions | 107.323 mIPS | 67.640 mIPS | 1.587× |
| Fixed SSE registers | 49.082 mIPS | 50.437 mIPS | 0.973× |

The fixed-work geometric mean is **1.077×**, with >99.9999% of each IR workload
retired through cached IR and matching final architectural states. It passes its
stated gate. XP **fails** its gate; compared with the initial IR median, milestone
time fell by about 15.3%, but it is still 73.6% slower than its paired legacy.
IR-13 performance acceptance and IR-14 default switching remain incomplete.

Raw records: `build/ir13-xp-final-summary.json` and
`build/ir13-fixed-final.jsonl`. These headless Node measurements do not establish
browser/Worker XP throughput; the browser matrix verifies integration separately.

### Scheduler and missing-target follow-up

The complementary V8 profile (`build/xp-final-cpu-summary.json`) attributed about
19.2% of sampled wall time to admission/dispatch, 19.1% to interpretation, 15.9%
to compilation, and 10.3% to scheduling. These are stack-grouped sampling
intervals, including idle time elsewhere in the denominator, not OS CPU time.
The diagnostic run retired 78.6% through cached IR, averaging 15.08 guest
instructions per activation. It still performed 57.1 million full checks and
34.3 million missing-entry lookups. Short entries and observing exits remain
substantial, even after improving generated-code arithmetic.

The scheduler now records interpreted entries, Tier-1 promotion candidates and
Tier-2 entries with a witnessed hot exit that can still attempt fusion. Completed
traces and terminal Tier-2 entries no longer continually replace unpublished
entries in the 128-entry hot ring. Visit/link statistics remain independent of
this heat policy. Existing heat thresholds, cold publication and budgets remain.

A separate 64-slot negative target cache remembers exact absent entry keys.
Publication and record compaction clear it; it cannot authorize guest execution
or suppress a newly published entry. The cache matrix explicitly checks repeated
misses, publication after a miss, and retirement. Automatic promotion, bounded
four-source fusion, SMC, restore and eviction also pass with the new policy.

Remaining work is not just increasing region limits: observer-bearing extensions
beyond two sources need a correctness fix, I/O and clock helpers still terminate
activations, and uncovered PCs still incur interpretation/compilation costs.
Port callbacks and the timestamp clock are real state observers; removing their
state synchronization or byte validation without a proven contract would change
behavior. The earlier long optimizer stall remains unreproduced, rather than
being declared fixed merely because regular machine passes are now short.

### Latest acceptance after the lookup changes

Three alternating fresh-VM pairs, diagnostics off and no concurrent build/test:

| Work | IR median | Legacy median | IR / legacy |
| --- | ---: | ---: | ---: |
| XP first 800×600×32 | 31.943 s | 18.627 s | 1.715× time |
| XP throughput to milestone | 48.258 mIPS | 117.771 mIPS | 0.410× |
| Fixed integer | 172.874 mIPS | 182.406 mIPS | 0.948× |
| Fixed RAM RMW | 101.261 mIPS | 112.077 mIPS | 0.903× |
| Fixed indirect regions | 106.092 mIPS | 66.665 mIPS | 1.591× |
| Fixed SSE registers | 49.215 mIPS | 50.140 mIPS | 0.982× |

Fixed-work geometric mean: **1.0754×**, passing the stated aggregate/per-case
floor, with exact retirement, matching architectural state and >99.9999% cached
IR coverage. This does not mean every workload beats legacy. XP remains **FAIL**;
the first display milestone is about 71.5% slower. Compared with the iteration's
initial 38.434 s IR median, it is about 16.9% shorter, not legacy parity.
Negative hints avoided 19.3–19.9 million tree lookups in each IR cold start.

Raw records: `build/ir13-xp-lookup-summary.json` and
`build/ir13-fixed-lookup.jsonl`. Both harnesses now return nonzero on a failed
performance gate. The cold-boot harness measures a common milestone, not exact
guest instruction work: timer/polling behavior changes retired instruction totals.
Browser main-thread/Worker integration, automatic compilation, diagnostics
conservation and production/debug/release cache matrices pass. These functional
browser tests are not browser XP speed measurements. IR-13 remains incomplete.
