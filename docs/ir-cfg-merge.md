# Straight-block merging with preserved budget polls

`PassConfig.merge` enables a bounded HIR pass before phi/fold/GVN/DCE. It is on
in the default optimized configuration and can be disabled independently. It
merges an unconditional edge only when the target has one incoming edge, is not
an external entry or the source itself, and has a recovery map and an effect
parameter. Joins, loop headers with multiple predecessors and CPU-owned terminal
store/helper boundaries remain intact. At most 63 merges are possible in the
64-block region budget; instruction/value capacity stops further merging.

Target parameters are substituted by edge arguments, including all StateMap
references. A new effectful `PollBudget` precedes the moved instructions at the
former target entry. Its input is the incoming effect, its output replaces the
target effect parameter, and its recovery map is the removed block's entry map.
Instruction owners, block parameters, successors and external entry IDs are
remapped when compacting the block arena. Dead value/instruction arena records
remain valid under the existing unused-slot policy. The pass verifies its result
before later dataflow transformations.

MIR control now owns explicit `Poll { recovery, cost: 1 }` records indexed by
instruction ID. Canonical verification rejects missing or changed states/costs.
Wasm uses the same remaining-work local and state materializer for dispatcher
checks and internal polls. An exhausted poll writes its exact snapshot and exits;
a successful poll consumes one unit and continues in the current block. Pure
arithmetic may share values across the old edge, while the ordered poll and its
recovery-only values cannot disappear through DCE or cross an open RMW/partial
store transaction.

This removes dispatch branches and edge-copy work without changing the number
or position of execution-budget checks. Work cost remains separate from guest
retirement. Loop/count, exception and CPU-owned completion contracts are unchanged.
Polls are compilation-local budget checks; online interrupts, device scheduling
and Tier integration still require the runtime work packages.

The structural tests cover loops, diamonds, backward-numbered targets, terminal
stores, fixed-point behavior and the disabled pass. Every former entry recovery
is retained exactly once as an entry or poll. Negative tests cover missing poll
states/effects, nondominating recovery values and stale MIR recovery/costs. Existing
RMW tests reject an inserted poll between ticket acquisition and commit.

The bytecode execution suite compares 35,328 results with the actual interpreter
and additionally checks 17,664 optimized/unoptimized exits for exact GPR/FLAGS/
XMM/memory/PC/previous-IP/count equality at all eight budgets. Sixteen second-loop
scalar/vector page faults retain precise commit counts and exception frames.
No online or XP speed improvement is claimed from these compiler tests.

The complete connected matrix passed in `build/ir-merge-full-suite.log`, including
115 warnings-as-errors Rust tests, all existing IR/independent-reference targets
and final FLAGS observers. Feature compilation passed in `build/ir-merge-check.log`;
catalogue, production export isolation and whitespace checks pass. Compiler source
was unchanged during the full verification run.
