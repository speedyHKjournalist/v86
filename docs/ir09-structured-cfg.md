# IR-09 structured CFG emission

## Goal

The IR backend already lowers reachable guest control flow to MIR, but the Wasm
emitter historically executed every MIR CFG through a generic pc-local dispatcher:

```text
loop
  if pc == block0 { ... set pc; br loop }
  if pc == block1 { ... set pc; br loop }
  ...
```

That representation is general, but it adds a local write, a block-id comparison
chain and dispatcher branches on every internal guest edge. IR-13 budget
measurements showed that removing outer activation frequency alone asymptotes well
below the paired legacy backend, so IR-09 now begins replacing the internal
dispatcher where the MIR graph has a directly provable structured shape.

## First structured shape

The first fast path accepts only a single external entry with a preheader jump and
one natural loop:

```text
entry -> header -> ... -> tail
                    ^       |
                    +-------+
```

The linear loop chain may end in either:

- an unconditional jump back to the header; or
- one conditional split where exactly one arm returns to the header and the other
  reaches an exit block.

The conditional backedge arm may contain one trampoline block. This covers the
existing x86 frontend shapes for `Jcc self` and `LOOP self`, where the
single-instruction lifter creates taken/not-taken exit blocks before CFG grafting.

Every MIR block must be accounted for by the structured plan. Multi-arm merges,
extra side regions, irreducible graphs and any shape outside this proof fall back
to the existing generic dispatcher.

## Semantic boundaries retained

Structured emission changes only the Wasm control representation. It preserves:

- per-block budget polls and exact recovery StateMaps;
- dynamic retirement-count accounting;
- edge parallel-copy schedules, including scratch locals for cycles;
- RAM loop-cache reset points;
- memory/helper/fault exits;
- entry ABI and invalid initial-state rejection;
- publication, cache admission, SMC and snapshot rules.

The generic dispatcher remains the correctness fallback.

## Diagnostics

Every emitted Artifact records:

- whether structured CFG was selected;
- the number of direct structured backedges;
- the number of generic dispatcher edges.

Published cache records expose the same metadata through the experimental
entry-scoped diagnostic API. Global publication counters are also included in
`get_jit_info()` so later XP/application acceptance can measure real structured
coverage rather than extrapolating from a synthetic loop.

The CFG corpus requires unconditional self-jump, conditional self-jump and LOOP
self-loop fixtures to select structured emission. A multi-arm merge fixture is
kept on the generic fallback. The cache lifecycle test separately verifies that a
published self-loop advertises one structured backedge and zero generic dispatch
edges.

## IR-13 measurement

The execution-budget matrix hard-requires its target Tier-2 loop to be structured.
The same 128/256/512/1024/2048/4096 fresh-VM matrix is then rerun without changing
the runtime default budget. This isolates the benefit of internal direct Wasm
control flow from the already-measured outer activation cost.

This is an IR-09 backend backfill discovered through IR-13 acceptance. It does not
mark IR-09 or IR-13 complete and does not enable IR-14.
