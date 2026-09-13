# Owned MIR and checked lowering lifetime

The emitted `MirRegion` no longer owns or borrows a HIR `Region`. It owns machine
value types, allocated locals, helper ABIs, memory/effect/call/value plans, the
dispatcher graph and materialization plans. `runtime::compile` explicitly drops
HIR before MIR optimization and Wasm emission. Dumps also consume only owned MIR.

## Lowering boundary

`lower_draft` creates a transient transaction borrowing validated HIR. Its
`finish` method checks every existing canonical plan contract, machine value types
and the local allocation before transferring the owned machine data into
`MirRegion`. The draft cannot be passed to the emitter. Construction is restricted
to the compiler implementation; its HIR borrow prevents the original graph from
changing during the transaction through ordinary Rust references.

The result exposes shared references through `Deref`, with no mutable dereference
and no public constructor. This permits multiple emissions and inspection after
the source HIR is destroyed. Plan vectors contain no interior mutable objects.
The emitter still checks target-specific CPU/import/layout/budget restrictions.
It no longer re-runs HIR verification or repeats canonical lowering on every
emission. Existing malformed-plan tests now assert rejection by `finish`, before
an emittable artifact can exist. Added tests reject corrupted value types, changed
local assignments that would alias live values, and missing local types.

This is an ownership and construction invariant, not a claim that arbitrary
mutable MIR graphs can be independently verified. SSA definitions and liveness
are still derived from HIR during lowering; new operations, arbitrary graph edits,
state-write motion and allocator changes require additional MIR verification.
Sealing currently recomputes allocation to check the mapping, adding bounded
lowering work. No compile-time or peak-memory speedup is claimed from this change.

## MIR constant folding

`MirRegion::fold_constants` rewrites literal postfix machine expressions using
explicit i32/i64 modulo arithmetic, bit operations, masked shifts, comparisons,
leading/trailing zeros, population counts, width conversions and literal selects.
It also folds newly adjacent literals in nested expressions. It does not propagate
values across SSA definitions, move CPU reads, erase reads multiplied by zero,
touch helper/effect plans, alter state materialization, or change CFG/budget polls.

Every replacement is stack-checked against the owned type arena. Definitions,
local mappings and all non-value plans are unchanged. Replacements are collected
before committing them, so failure leaves the original artifact intact. Work is
bounded to 1,000,000 input steps per invocation; the rewrite only removes steps
and is idempotent. This is a first controlled transformation on owned MIR, not
completion of MIR graph, stack scheduling or proof-based memory optimization.

All three compile-request paths (standalone, cold CPU and CPU CFG) run this pass
when `IrConfig.optimize` is true and record its count in `CompiledArtifact.mir_folds`.
Automatic Tier 1 remains unoptimized; automatic Tier 2 uses the same optimized
compile-request path. The public backend options retain their existing semantics.

## Evidence

`make ir-mir-owned-tests` includes HIR destruction followed by repeated emission,
real narrowed-register MIR rewriting, unchanged recovery/control plans, idempotence,
invalid types/allocation, preserved observations and compile-request integration.

The independent JS BigInt oracle checks 6,384 literal programs both before and
after rewriting (12,768 actual Wasm results), including overflow, signedness,
zero-bit counts, width conversion and shift counts beyond 32/64. The aggregate
literal fixture shrinks from 94,451 to 64,986 bytes. Another 36 actual emitted MIR
executions check AL/AH/AX updates, preserved GPRs/FLAGS and precise PC/counts after
HIR destruction. These fixture sizes are not application performance results.

The remaining full-plan gates are unchanged: incomplete ISA, broader MIR and
memory/loop optimizations, runtime linking, OS/application/performance acceptance
and production-default legacy retirement. See [the implementation status](ir-progress.md).
