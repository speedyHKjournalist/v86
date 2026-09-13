# Dominator value numbering and constant CFG pruning

The optimized HIR pipeline now selects constant conditional branches, removes
unreachable blocks and reuses pure expressions from dominating definitions.
`PassConfig.prune` controls branch selection/reachability cleanup independently;
`PassConfig.gvn` controls both local and dominator-based value numbering. Both are
enabled by default. Existing round limits and region budgets still apply.

GVN visits strict dominators before dominated blocks, even when arena order is
reversed. An expression key includes opcode, canonical SSA operands and result
type. A previous definition may replace a later one only when its block dominates
the use; same-block definitions must already have been visited. Sibling branches
and independent external entries cannot supply each other's values. Canonical
aliases are applied to instruction/edge arguments and every StateMap field in a
single final rewrite, including count bases and recovery-only FLAGS/vector values.

An explicit whitelist includes scalar arithmetic/conversions/bit operations,
linear offsets and pure vector kernels/lane operations. CPU GPR/FLAGS/XMM/segment/
stack-mode reads, ordered operations, guest memory and helper calls are excluded.
Instructions carrying observation maps are also excluded. The pass eliminates
redundant computations; it does not move operations or speculate a faulting read,
and does not perform LICM, memory forwarding or address-proof reuse.

Pruning replaces a conditional edge when its i1 condition is constant, or when
both targets and argument lists are identical. Reachability begins at every
external entry. If blocks become unreachable, the pass compacts block,
instruction, value and StateMap arenas and remaps definitions, effect arguments,
all observations/commits, dynamic next PCs, count bases, REP progress and exits.
Unused helper descriptors may remain, but dead calls receive no lowered ABI.
A valid branch that never executes an unadapted helper can thus compile after
pruning. Input HIR must still satisfy the verifier; pruning is not a repair pass
for an invalid graph.

A selected path retains its original blocks/recovery points. Later straight-block
merging retains polls for every removed boundary, so constant branch selection
and GVN preserve exact execution-budget exits. No surviving fault, partial store,
helper observation or retirement point is bypassed.

Three focused tests cover non-topological block order, i32/i64 identities, sibling
negative cases, independent entries, CPU-state reads, constant true/false and
identical-edge branches, root retention, arena compaction and dead helper removal.
The independent diamond execution oracle performs 3,072 Wasm runs across overflow
inputs, both arms and eight budgets. The bytecode suite now performs 39,936 actual
CPU comparisons (184,768 interpreter steps), 19,968 exact optimized/unoptimized
budget comparisons, sixteen second-iteration scalar/vector faults and sixteen
constant branches that skip absent-page reads. The latter explicitly verify
retirement counts and compare with the interpreter.

This advances the IR-10 foundation. Full Tier 1/ISA coverage, remaining Tier 2
passes, proof-based memory/loop optimization, online lifecycle/scheduling,
XP/performance acceptance and default-backend retirement remain unfinished.

The full connected regression matrix passed in `build/ir-dataflow-full-suite.log`,
including 118 warnings-as-errors Rust tests, all IR/independent-reference execution
targets, all SIMD families and final FLAGS observers. Feature compilation passed
in `build/ir-dataflow-check.log`; catalogue, production export isolation and
whitespace checks pass. Compiler source remained unchanged during that full run.
