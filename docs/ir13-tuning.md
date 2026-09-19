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

Further tuning remains separate: safe IR-to-IR continuation/link consumption,
Tier-1/Tier-2 region/budget policy, compiler-stage timing, and controlled XP/
application benchmarks.
