# Lowered dispatcher CFG and parallel copies

Ownership update: [owned MIR](ir-mir-owned.md) supersedes the retained-HIR lifecycle described in this stage report. Canonical checks now run at transaction sealing; the resulting read-only machine artifact owns its types/plans and is emitted after HIR is released. Historical test results below remain stage-specific.

MIR now owns a ControlFlow record with entry selection, blocks, ordered instruction
references, block-budget recovery maps, dispatcher costs and terminal actions.
Terminal actions are Exit, Jump or Branch. Branch conditions use allocated local
slots, and edges contain typed, executable local-copy schedules. Wasm emission
uses this graph for dispatch, branching, edge copies, polling and exits; it no
longer walks HIR blocks or entries or computes the HIR CFG.

Lowering currently preserves the HIR block topology. Instruction bodies and
state/value definitions remain shared, and allocation still computes liveness
from HIR, including recovery references. This is an executable lowered control
representation, not completion of independent MIR optimization or the complete
IR-03/IR-09 work packages.

## Parallel-copy scheduling

Lowering resolves each edge's block arguments to typed source and destination
local slots and drops Effect arguments. It rejects duplicate destinations and
type mismatches. Identity copies require no operation. A destination can be
written as soon as its old value is no longer needed by another pending copy.
If the remaining assignments form a cycle, lowering saves one source to typed
scratch and redirects all uses of that old source, including fanout, to scratch.
Scheduling then continues until all simultaneous assignments are resolved.

This uses no scratch for acyclic copies and one scratch per nontrivial cycle.
Source-use counts avoid repeated nested dependency searches. The emitter encodes
Save and Move steps in order and releases scratch at the end of that edge.
Copies execute within the selected branch arm, so critical edges do not write
the other arm's destinations. I32, I64/RmwTicket and V128 scratch use the matching
Wasm local class. No guest observer, budget poll or recovery point occurs inside
the copy schedule.

## Recovery and checks

Entry index selection and invalid-entry returns are preserved. Each block still
costs one dispatcher work unit; this is not dynamic guest instruction accounting.
Budget exhaustion at a non-entry block materializes its recovery map after the
incoming copies. The normal terminal Exit uses its separately planned StateId.
CPU cyclic graphs remain rejected because current CPU recovery maps use static
commit counts. Standalone loops remain executable. Unsupported entry parameters,
internal entry predecessors and missing non-entry budget maps are checked using
the lowered graph.

Before emission, control plans are compared against canonical lowering of the
verified HIR and current allocation. This rejects stale topology, instruction
order, condition slots, recovery/cost policy and corrupted copy schedules. It
is a migration consistency check, not yet a standalone MIR verifier that permits
independent graph transformations. MIR dump includes lowered entries, per-block
recovery/cost and complete edge schedules.

## Evidence and remaining scope

The symbolic scheduling oracle enumerates every source mapping for one through
six destinations for I32, I64 and V128: 150,207 mappings. It compares execution
of the schedule with simultaneous assignment from an untouched input, including
identity, fanout, chains and disjoint cycles. Explicit cases check scratch counts,
mixed local classes, invalid slots/types and duplicate destinations.

Graph tests check multi-entry loops, cycle rejection under the CPU ABI and twelve
unsafe graph/copy mutations plus stale HIR edges. Another 108 modules cover all
27 three-source mappings with two branch arms, optimization enabled/disabled and
normal/budget exits. The independent JS oracle executes 648 cases and checks all
GPRs, FLAGS, recovery-only flag operands, PCs and commit state. The existing loop,
wide arithmetic and SIMD suites exercise real i32/i64/v128 copies through this
backend. `make ir-tests` runs the new Rust and JS tests.

Independent MIR instructions/verification/transforms, explicit materialization,
entry splitting, structured CFG optimization, operand-stack scheduling, dynamic
CPU accounting and online Tier integration remain open. ISA coverage and the
3,728 production Pending forms are unchanged. Full IR-00–IR-14 acceptance remains
unmet.


The complete connected regression matrix passed in one make invocation in
`build/ir-mir-control-full-suite.log`, including 102 warnings-as-errors Rust
tests, the new 648 Wasm executions, existing i32/i64/v128 copy fixtures, all
CPU/independent-reference suites and the final FLAGS-observer diagnostic.
Feature compilation passed in `build/ir-mir-control-check.log`; catalogue,
normal production export isolation and whitespace checks pass. No production
source changed after this verification.


Supported scalar/vector instruction bodies subsequently moved to explicit
[value programs and selected packed kernels](ir-mir-values.md). The emitter
no longer selects HIR opcodes; shared value/state definitions, independent graph
transforms and explicit materialization remain work in progress.


CPU cycle support subsequently gained [SSA dynamic count bases](ir-dynamic-count.md).
Graphs with complete live dynamic-count maps can now execute under the CPU ABI;
static/mixed-count cycles remain rejected. Automatic guest-CFG lifting and online
scheduler/interrupt integration remain incomplete.

The [reachable bytecode frontend](ir-cfg-frontend.md) now supplies guest-derived
loops and diamonds to this same dispatcher/copy lowering, with typed frame and
dynamic count parameters. It does not add independent MIR graph transforms.

Explicit HIR budget polls now lower into canonical `ControlFlow.polls` records,
with recovery StateId and unit work cost. The emitter handles them through the
same remaining-work local and state materializer as dispatcher checks.
[Straight-block merging](ir-cfg-merge.md) preserves former entry polls, allowing
fewer dispatch edges with identical budget exits. This is an HIR transformation;
it does not complete independent MIR graph optimization or online scheduling.
