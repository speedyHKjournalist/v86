# IR timing attribution and exit diagnostics

Diagnostics are opt-in. They do not turn on the existing performance recorder,
disable legacy links, or change guest compilation budgets. Changing diagnostic
policy invalidates compiled artifacts and pending publications so one session
cannot mix instrumented and ordinary modules. With diagnostics off, generated
Wasm has no diagnostic imports, marker stores or timing branches; the cache dispatcher selects a separate uninstrumented specialization. A policy
check remains before selecting that path.

## Public API

On a loaded V86 instance (the same calls work through CPU Worker RPC):

```js
await vm.stop();
await vm.configure_ir_diagnostics(128); // 0 disables; powers of two, 1..65536
await vm.run();
// Later: returns a copied snapshot, or a Promise in Worker mode.
const report = (await vm.get_jit_info()).ir.diagnostics;
```

Configuration returns false inside a CPU activation/batch and rejects invalid
periods. Each successful configuration starts a new session and clears diagnostic
counters. Reset/restore invalidate code normally; diagnostics remain a host policy,
not guest snapshot state. Old asynchronous publication reports cannot affect a new
session. Period 1 is useful for correctness tests, not normal performance runs.

## Measurements

A xorshift generator independently selects entire CPU batches with probability
`1 / sample_period`. Selected batches use nested, **exclusive** stage timing:

- `dispatch`: residual dispatcher/chaining/counter work;
- `scheduler`: heat collection, source selection and publication scheduling;
- `admission`: lookup, context/mapping checks and entry metadata;
- `byte_validation`: cached source byte/mapping validation;
- `source_capture`: full read-only recapture used by cache admission;
- `fetch`: architectural initial instruction translation;
- `generated`: generated IR execution excluding timed children, including native
  RAM accesses/guards, SSA computations, entry loads, edge copies and budget polls;
- `state_write`: emitted architectural state/count materialization;
- `state_reload`: explicit helper continuation reloads (not every entry load);
- `memory_slow`: emitted low-level memory/RMW/XMM adapter calls;
- `helper`: other emitted semantic/check/helper calls;
- `interpreter`, `legacy`: execution through those paths;
- `compile`: synchronous automatic IR compilation within sampled CPU work.

These sampled times sum to `sampled_batch_ms`. `cpu_batch_ms` times all CPU batches;
it excludes time outside CPU batches, device scheduling and idle host waits.
`estimated_ms = sampled_ms * sample_period` is an estimate, **not** measured total
CPU time. Random finite sampling and timing perturbation affect it.

The `compiler` object independently measures all automatic pipeline and source
capture calls and compiler lift/pass/lower/machine/emit phases. Phases also cover
explicit compilation and retries; do not add the pipeline parent to its children.
Each compiler phase also reports `max_ms`, `max_pc`, and `max_tier`. PC/tier
identify the selected entry in capture/pipeline and each concrete compiler attempt.
`compiler_breakdown` partitions the same phase samples by tier, artifact kind
(`ordinary`, `shared`, `fused`), and input HIR shape (`single`, `multi`). Unknown
kind/shape remain explicit before lifting or on failed attempts; they are not
silently attributed to single-block Tier 1. Calls and time across buckets conserve
the original phase total. HIR shape is measured before passes, not after merging.
The optional buckets are omitted when diagnostics are off or an older core does
not support them. Lowering is split into HIR allocation, state plans,
proofs and final verification; machine passes have separate timings. Parent and
child phases overlap and must not be summed.

`publication.wall_ms` is latency from starting Wasm instantiation through publish
settlement, including event-loop waits, failures and cancellation. It overlaps CPU
work and is not browser compiler CPU time; it must not be added to the CPU totals.

Timing includes observer overhead. Configuration calibrates an empty timing scope
(`empty_scope_sampled_ms` and full `empty_scope_wall_ms`). This is a reference,
not an exact correction for cross-module calls. Near-floor tiny scopes cannot
establish precise cost rankings. The report script marks average scope times
below five times the empty-scope sampled cost. Compare profiling-off/on runs and
different sampling periods before treating a ranking as an optimization decision.

## Complete event counters

With diagnostics enabled, every dispatched IR activation contributes one exit and
its wrapping-delta retired instruction count; counters accumulate in u64 and are
exported as JavaScript numbers. Reasons distinguish normal completion, execution
budget, fused-epoch revocation, known faults, scalar store completion, writes to
captured code/aliases, RMW completion, vector-memory completion, helper transfer,
yield/invalidation, entry guards and interrupt-shadow completion.

`helper_control_or_fault` is deliberately ambiguous: the existing generic helper
outcome `ControlTransferred` includes both delivered faults and genuine transfers.
It is not reported as a proven fault. `helper_invalidated` similarly reports the
helper ABI outcome, not proof of an actual SMC event. `unclassified` is retained
explicitly for any exit not covered by a marker. A native CMPXCHG8B completion is
classified with RMW; vector memory includes successful native vector stores.

`admission` separates absent targets, context rejection, stale code, capture
fallback, fetch faults, lost owners and unavailable/stale secondary translations.
`capture` is a path event, not an additional terminal rejection. Thus admission
fields must not simply be summed as mutually exclusive failures.

`chain_stops` distinguishes no continuation request, exhausted CPU budget, halt,
control-flag changes, target admission failure and the 64-successor cap. These
explain why a normal region exit does not necessarily remain in an IR chain.

The full exit counts and retired steps equal `ir_activations` and `ir_steps`.
Interpreter/legacy counts track retirement in their dispatch paths; counts are not
time shares. Helper-retired instructions inside an IR activation count as IR.
Sampled hotspots store entry PC, CR3, reason, tier, fused status, steps and inclusive
activation time. The fixed 512-slot table replaces colliding entries and reports
replacements; it is a bounded sample, not an exhaustive top-N profile. Global exit
counts do not lose events on collisions. `helper_exits` groups terminal helper
outcomes by semantic family (segment, port, REP, far control, flags, descriptor,
CPU control, FP, halt, invalid, other); it does not count successful continuing
calls. `interpreter_hotspots` is a separate 256-slot sampled table of linear PC,
CR3, physical PC, batches, steps and inclusive time. Collisions replace entries;
it locates remaining interpreted work without claiming exhaustive coverage.

`control_exits` subdivides terminal CPU-control exits into RDTSC, CPUID, CR reads,
CR writes and CLTS; STI-check exits are also reported separately. These are subsets
of the existing helper families, not additional activations. `missing_entries`
partitions absent-target admissions into unseen, heating, ready, pending and failed.
`discovery_latency` reports total/count/maximum milliseconds from the first retained
hot observation to publication, separately for each tier. It is bounded-history
latency, not the first-ever execution of a PC; eviction/invalidation loses history.
No discovery clock reads occur with diagnostics disabled.

The XP runner emits interval retirement, IR coverage, activations per million
retired instructions and full checks per million, labeled by the current display
mode. These display phases are observable milestones, not OS-internal boot stages.

## Reproduction

Run one process at a time, with no concurrent build/test load. The XP runner uses
the disk read-only with an in-memory write overlay:

```sh
IR_BOOT_MS=60000 IR_DIAGNOSTICS=0 node tests/ir/performance/xp_boot.mjs /path/to/xp.img ir > build/xp-diag-off.jsonl
IR_BOOT_MS=60000 IR_DIAGNOSTICS=128 node tests/ir/performance/xp_boot.mjs /path/to/xp.img ir > build/xp-diag-128.jsonl
IR_BOOT_MS=60000 IR_DIAGNOSTICS=512 node tests/ir/performance/xp_boot.mjs /path/to/xp.img ir > build/xp-diag-512.jsonl
node tests/ir/performance/diagnostics_report.mjs build/xp-diag-128.jsonl > build/xp-diag-summary.json
```

The report checks count/time conservation and ranks stages and exits. The headless
runner does not validate XP rendering in a browser Worker; browser integration is
covered separately through the same public API.

`IR_BOOT_MS` is checked when the event loop yields. A synchronous compilation can
overrun it; the runner records `requested_ms`, actual `ms` and `overrun_ms`. The
report warns about large overruns and poor sampled-time extrapolation.

`make ir-diagnostic-tests` covers exact guest state with profiling off/on, absence
of emitted hooks when off, timer/count conservation, precise faults, slow stores,
MMIO callback rejection of reconfiguration, copied reports and late publication
cancellation. The automatic suite with diagnostics enabled exercises fusion,
SMC, reset/restore and cache pressure. Browser scenarios test main-thread/Worker
RPC, generated timing/exits and safe enable/disable.

## XP observations, 2026-09-20

Sequential headless runs used `windowsxp_multidisk_C_4G.img`, 2 GiB RAM, 16 MiB
VRAM and x87 fast-math/cache, with the same production core. No build/test jobs
ran alongside them, but the shared host had other application/background load.
These are diagnostic observations, not a controlled browser/legacy speed claim.

| Mode | Actual elapsed | Avg mIPS | Synchronous pipeline | Lowering | Machine optimization |
| --- | ---: | ---: | ---: | ---: | ---: |
| Off | 60.17 s | 19.67 | unmeasured | unmeasured | unmeasured |
| Period 128 | 280.46 s | 3.59 | 244.51 s | 12.08 s | 229.74 s |
| Period 512 | 60.05 s | 26.30 | 22.30 s | 16.10 s | 2.56 s |

Logs are `build/xp-diag-{off,128,512}.jsonl`; reports are generated with the command
above. These initial logs predate the explicit requested/overrun fields; all three
requested 60 seconds. The period-128 run stopped reporting after about 50 seconds
until a synchronous machine-optimization stall returned. All-call timing captured
it; batch sampling missed most of it. The second run did not reproduce that stall.
The data does not yet identify the responsible machine pass or region.

In the period-512 run, synchronous compilation consumed 22.30 of the measured
54.29 seconds inside CPU batches (41.1% of batch wall time); lowering accounted for
16.10 seconds. This makes compilation cost a concrete next investigation, alongside
region-exit frequency. These are elapsed-time scopes, not operating-system CPU
usage measurements. Publication latency is separate and must not be added.

That run retired 67.0% of guest instructions in cached IR, averaging 8.44 instructions
per activation. Complete exit counts were 56.48% normal, 27.48% helper-invalidated
ABI outcome, 11.49% RMW completion and 3.87% interrupt-shadow completion. These
are event shares, not time shares; the helper outcome alone does not establish SMC.
Unclassified exits and instrumentation errors were zero.

Only 37 of 18,248 batches were sampled at period 512. Extrapolated sampled time was
2.15 times measured batch time, and many per-call scopes were near the empty-scope
floor. Consequently, admission/dispatch/state-write percentages are not reliable
whole-run cost rankings yet. The enabled XP run being faster than the disabled run
also cannot establish negative profiler overhead: guest progress, compilation and
host scheduling differed.

A separate warmed two-region fusion microbenchmark, run sequentially with identical
128-instruction activations, measured 995.8 mIPS off and 890.6 mIPS at period 512
(about 10.6% lower throughput). This is a single-pair observer-cost check, not an XP
overhead estimate. Keep diagnostics off for normal use and recheck multiple periods
and repeated matched workloads before deciding on small optimizations.
