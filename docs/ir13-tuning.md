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
