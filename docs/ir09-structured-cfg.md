# IR-09 structured CFG emission

## Goal

The IR backend lowers reachable guest control flow to MIR, but the original Wasm
backend executed every MIR CFG through a generic pc-local dispatcher:

```text
loop
  if pc == block0 { ... set pc; br loop }
  if pc == block1 { ... set pc; br loop }
  ...
```

That representation is the correctness fallback, but it adds pc-local writes,
block-id comparisons and dispatcher branches on every internal edge. IR-13
measurements showed that this overhead is material for hot reducible loops, so
IR-09 now emits directly structured Wasm for graph shapes that can be proven
without a general relooper.

## Structured subset

The first subset accepts one single-entry natural loop with a synthetic preheader,
a linear loop body and one backedge. The loop tail may be unconditional or
conditional.

The second subset extends that proof in two directions.

### Linear exit/backedge arms

A conditional loop may now have an arbitrary finite Jump-only chain on either
side before the arm reaches the loop header or an Exit:

```text
entry -> header -> ... -> tail
                    ^       | \
                    |       |  -> epilogue -> ... -> Exit
                    +-------+
```

This covers loops whose not-taken path remains inside the immutable region and
executes an epilogue before returning to the outer CPU dispatcher.

### Acyclic diamond with a join

One conditional branch may split into two Jump-only arms, reconverge at one join,
then continue through a shared linear tail:

```text
             -> left  ->
entry -> test             join -> ... -> Exit
             -> right ->
```

Either arm may be empty when the branch edge targets the join directly. Edge
parallel-copy schedules execute on the same logical edge as in the generic
dispatcher, so join parameters retain normal SSA/phi semantics.

The detector requires every MIR block in the region to be owned exactly once by
the selected plan. It rejects nested branches, extra side regions, cycles in a
diamond, multiple loop backedges and any graph outside these proofs. Those cases
continue through the generic pc-local dispatcher.

Relative targets remain architectural-width sensitive. In 16-bit mode a branch
near a high 32-bit EIP may wrap outside the immutable snapshot; such a target is
not treated as an internal structured edge.

## Semantic boundaries retained

Structured emission changes only the Wasm control representation. It preserves:

- every per-block execution-budget poll and recovery StateMap;
- dynamic retirement-count accounting;
- edge parallel-copy schedules, including scratch locals for cycles;
- RAM loop-cache reset points;
- memory/helper/fault exits and partial-completion rules;
- entry ABI and invalid initial-state rejection;
- publication, cache admission, SMC, reset/restore and snapshot rules.

The generic dispatcher remains the correctness fallback.

## Diagnostics

Every emitted Artifact records:

- whether structured CFG was selected;
- the number of direct structured backedges;
- the number of MIR control edges emitted directly;
- the number of edges left to the generic dispatcher.

Published cache records expose the same metadata through the experimental
entry-scoped diagnostic API. Global publication counters also include cumulative
structured-edge coverage so later XP/application acceptance can measure real
structured use instead of extrapolating from synthetic loops.

The CFG corpus requires:

- unconditional, conditional and LOOP self-loops to select structured emission
  when their architectural target remains inside the snapshot;
- multi-block natural loops to stay structured;
- a conditional loop with an in-region epilogue to stay structured;
- simple diamonds, including memory-bearing arms, to stay structured;
- a nested-branch graph to remain on the generic fallback;
- high-EIP 16-bit wrapped targets not to be misclassified as internal edges.

The cache lifecycle test separately verifies that a published self-loop advertises
one backedge, nonzero structured-edge coverage and zero generic dispatch edges.

## IR-13 evidence

PR #46 removed the internal pc-local dispatcher from the synthetic target loop.
IR-core run 362 kept the same 128/256/512/1024/2048/4096 fresh-VM matrix and
reported all samples as structured with one backedge and zero generic edges.
Within that hosted run, 1024 and larger budgets exceeded the paired legacy
synthetic-loop throughput. These values are diagnostic only; they do not establish
XP or application performance.

PR #47 expands coverage while preserving the same matrix and fallback policy. It
does not change the runtime default execution budget.

This remains an IR-09 backend backfill discovered through IR-13 acceptance. It
does not mark IR-09 or IR-13 complete and does not enable IR-14.
