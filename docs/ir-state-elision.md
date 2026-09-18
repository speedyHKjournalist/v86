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
- terminal region exits are allowed;
- GPR, XMM and split FLAGS origins are propagated through CFG block parameters to
  a fixed point;
- any conflicting predecessor, unknown origin or changed value remains materialized.

The analysis recognizes entry sources for GPRs, XMM registers and FLAGS provenance,
but this increment only elides exact GPR/XMM and `last_op1` backing writes. Full
EFLAGS, `last_result`, `last_op_size` and `flags_changed` writes remain mandatory:
StateMap currently keeps only the ZF lazy-provenance bit, so it cannot prove the
complete lazy-FLAGS backing representation unchanged. EIP, previous-IP and
retirement-count materialization are also never removed.

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

- multi-block JECXZ/MOV control flow with entry-equivalent GPR/last_op1 sources;
- arithmetic FLAGS backing that must always remain materialized in this increment;
- memory-containing regions disabling the optimization;
- bounded/atomic enabling.

The existing reachable-CFG CPU differential now emits the optimized modules with
state-write elision enabled. It compares optimized and unoptimized exact budget
exits, interpreter state, FLAGS/XMM state, previous IP, retirement counts and the
existing fault cases.

## Still open

Full lazy-FLAGS provenance and arithmetic FLAGS write elision, general backing-state
dataflow across resumable MMU/helper observations, partial FLAGS demand/liveness,
dirty-state merging after callbacks, broader cross-block
state synchronization, memory LICM, complete ISA coverage, system/application
acceptance and IR-14 retirement remain open.
