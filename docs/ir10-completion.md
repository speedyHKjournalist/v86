# IR-10 completion: Tier 2 basic dataflow optimization

IR-10 is complete against the work-package acceptance line in
`docs/v86-ir-implementation-plan.md`:

> FLAGS / DCE / GVN / copy / CFG / helper-state passes; positive, negative and
> per-pass differential validation.

This document defines the completed boundary. It does not move IR-11 memory,
loop or SIMD work into IR-10.

## Completed pass families

### CFG

The Tier-2 pipeline has bounded CFG cleanup with constant-branch selection,
unreachable-block pruning, straight-line merge with exact PollBudget recovery,
and trivial-phi elimination. CFG analyses are recomputed inside the passes that
consume them; stale dominance/reachability facts are not reused across mutation.

### Constant and copy propagation

Constant/scalar canonicalization and SSA alias propagation are separate controls.
`PassConfig.copy` runs the bounded copy/identity pass and reports
`PassStats.copied`. `PassConfig.fold` owns constant identities/folding.
Copy rewrites instruction operands, StateMaps, branch conditions and edge
arguments atomically and rejects ordered/stateful candidates.

### FLAGS demand

The complete lazy-FLAGS provenance carried by StateMap feeds a bounded CPU-only
liveness certificate. Exact backing allows unused concrete CF/PF/AF/ZF/SF/OF
SSA to disappear while recovery still writes the baseline lazy representation.
`PassConfig.flags` controls this Tier-2 optimization.

Unsupported or special backing forms are not guessed. They canonicalize at the
recovery boundary and remain semantically valid. This conservative fallback is
part of the completed pass contract rather than an IR-10 failure.

### Snapshot-aware DCE

DCE treats StateMaps, entry recovery, exits, effect/ordered instructions and CFG
conditions as roots. It removes only pure definitions that are no longer needed
by architectural recovery or execution.

### GVN/CSE

GVN is dominance-aware and effect constrained. It reuses only equivalent pure
expressions with a dominating representative and does not cross memory/helper
observations by assumption.

### Helper-state trimming

Lowering derives an audited helper-state certificate. A pre-call CPU StateMap
observation may be omitted only for helpers proven state-independent and safe:

- exact `Effects::pure()` metadata;
- `ExceptionOwner::CannotFault`;
- nonterminal Outcome ABI with `normal_preserves_state=true`;
- no fault delivery and no exit outcome.

State-reading, state-writing, faulting, terminal or otherwise conservative
helpers remain full barriers. The helper-state certificate is enabled before the
FLAGS/CPU liveness certificate so the latter can safely choose the trimmed root
set. Standalone emission is unchanged.

## Independent controls and counters

The basic Tier-2 families have independent controls or existing dedicated pass
entry points. The completion work adds explicit controls for copy, FLAGS
liveness and helper-state trimming:

- `PassConfig.copy` / `PassStats.copied`;
- `PassConfig.flags` / `PassStats.cpu_values_elided`;
- `PassConfig.helper_state` / `PassStats.helper_states_elided`.

Existing CFG, DCE, GVN, phi and fold controls/counters remain unchanged.

## Completion validation

`tests/ir/semantics/ir10.rs` generates a dedicated single-pass corpus. Each
fixture enables only the intended family and requires its expected counter to
fire. Negative cases cover stateful copy candidates, a state-reading helper and
a special FLAGS form that must fall back to canonical recovery.

`tests/ir/differential/ir10.mjs` executes baseline versus single-pass Wasm for:

- copy only;
- DCE only;
- GVN only;
- CFG prune only;
- FLAGS liveness only;
- helper-state trimming only.

The differential compares final architectural state. The CPU cases also compare
raw EFLAGS, `flags_changed`, `last_result`, `last_op1`, `last_op_size`,
EIP and retirement count.

The IR-core workflow permanently runs this per-pass differential in addition to
the existing CFG/fault, extended FLAGS, RAM, SIMD and store-continuation gates.

## Boundary with IR-11 and later work

IR-10 completion does **not** mean the whole IR project is complete.

The following remain outside this work package:

- proof-based TLB/RAM address reuse and load/store forwarding;
- LICM, induction-variable and strength-reduction work;
- advanced SIMD/lane/loop optimization;
- broader helper/MMU dirty-state specialization beyond the audited conservative
  helper-state contract;
- remaining ISA/system/application coverage and performance acceptance;
- production default migration and legacy-emitter retirement.

Those stay tracked under IR-11 through IR-14. The production default backend
remains legacy and the existing coverage/default gates are unchanged.
