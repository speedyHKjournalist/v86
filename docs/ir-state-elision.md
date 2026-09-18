# IR-10: conservative CPU state-write elision

This is an incremental Tier 2 optimization. It does not complete IR-10 or
IR-00–IR-14, does not change ISA coverage or the production default backend, and
does not retire the legacy emitter.

## Scope

The pass removes CPU backing-state stores at recovery/exit materialization points
only when the value is proven equal to the CPU value that existed at IR entry.

The proof is intentionally narrow:

- exactly one external IR entry;
- no resumable memory, helper, commit or exception observation in the region;
- `PollBudget` recovery is allowed because its materialization arm returns;
- guarded `SseCheck` is also allowed: if it has to observe CPU state, that arm
  materializes state and returns from the IR entry instead of resuming;
- terminal region exits are allowed;
- GPR, XMM and split FLAGS origins are propagated through CFG block parameters to
  a fixed point;
- any conflicting predecessor, unknown origin or changed value remains materialized.

The analysis now carries the complete entry lazy-FLAGS backing needed by v86:
raw EFLAGS, the full `flags_changed` mask, `last_result`, `last_op1` and
`last_op_size`, in addition to the six semantic arithmetic flags and system
flags. When every one of those sources is still exactly entry-equivalent, Tier 2
may leave the original lazy backing untouched instead of canonicalizing it.
Any changed semantic flag or backing component disables that FLAGS elision and
falls back to the existing full materialization path. EIP, previous-IP and
retirement-count materialization are never removed.

## Why this is safe

IR normally keeps architectural values in SSA locals while CPU backing memory can
remain stale until a recovery point or exit. A write can therefore be omitted
only when the SSA value is exactly the same value already present in CPU backing.

The implementation does not assume that an arbitrary helper or MMU callback
preserves backing state. If any instruction has a state/commit observation other
than a terminal budget poll, the whole region receives an all-false certificate
and emission is unchanged.

This restriction also avoids the harder case where a slow path materializes
state, returns normally, and later control flow would need to reason about two
possible CPU-backing histories. That case remains future work.

## Lowering and verification

Lowering derives an immutable per-StateMap/per-write certificate while HIR and
CFG edge arguments are still available. The owned MIR stores the certificate in
disabled form. Draft verification recomputes the certificate and rejects stale
or forged data.

Optimized Tier 2 may enable the already-verified certificate with a bounded,
transactional operation. Tier 1, standalone emission, disabled optimization and
zero-round configurations retain the original behavior.

`passes.state_writes_elided` reports statically skipped CPU stores. It is not a
dynamic execution count or performance claim.

## Validation

Native tests cover:

- multi-block JECXZ/MOV control flow with entry-equivalent GPR and complete
  lazy-FLAGS backing sources;
- an SSE loop where loop-carried XMM0 remains materialized while unchanged XMM1
  can be elided across the guarded SseCheck;
- changed arithmetic FLAGS that must fall back to canonical materialization;
- a real CPU pure-loop differential that requires raw EFLAGS, `flags_changed`,
  `last_result`, `last_op1` and `last_op_size` to remain bit-for-bit equal
  to their entry backing values;
- memory-containing regions disabling the optimization;
- bounded/atomic enabling.

The existing reachable-CFG CPU differential now emits the optimized modules with
state-write elision enabled. It compares optimized and unoptimized exact budget
exits, interpreter state, FLAGS/XMM state, previous IP, retirement counts and the
existing fault cases.

## Still open

Partial FLAGS demand/liveness, selective materialization after changed flags,
general backing-state dataflow across resumable MMU/helper observations,
dirty-state merging after callbacks, broader cross-block state synchronization,
memory LICM, complete ISA coverage, system/application acceptance and IR-14
retirement remain open.
