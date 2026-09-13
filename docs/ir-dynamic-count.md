# SSA count bases and executable CPU loops

StateMap now carries an optional i32 count_base. Its completed-work value is
count_base plus committed_instructions, with i32 wrapping; absent bases retain
the existing static snapshot behavior. This separates a dynamic loop/region base
from fixed offsets within one lifted instruction sequence. A region builder must
start the entry-relative base at zero and advance it only for completed guest
work. The existing byte frontend still emits static counts; automatic lifting
of guest CFGs with counter phis is not yet integrated.

## Dataflow and commit phases

The base is a real SSA use. StateMap value enumeration includes it in dominance
checking, recovery liveness and local allocation; value replacement during GVN
and phi simplification rewrites it, and DCE retains count-only definitions.
The verifier requires i32 and availability at every live state use. Store before
and commit maps must share the same base. Partial-store instruction identities
also include the base; existing static offset/PC commit checks remain in force.

MIR Count records both base and offset. Materialization computes their sum for
absolute standalone counts or CPU delta accounting. CPU emission subtracts the
last accounted sum from the new sum and adds the delta to the current CPU
counter, then remembers the new sum. It therefore preserves noncached counter
adjustments by observers and does not double-count when an after-instruction
map uses old_base + 1 and the next loop header uses new_base + 0.

## CPU cycle admission

Lowered control flow records whether every live budget, instruction-observer,
commit and terminal state has a dynamic base. Unused state-arena records do not
participate. CPU cycles are admitted only with this complete dynamic-count
contract. Static or mixed-count CPU cycles remain rejected; entry-prologue and
budget-recovery restrictions are unchanged. The admission flag and count plans
are protected by canonical MIR consistency checks.

This verifies the structural count contract, not a proof that an arbitrary
user-built arithmetic recurrence equals the retired guest instruction count.
The region builder owns that semantic recurrence, just as it owns register/FLAGS
semantics. General bytecode-to-region lifting, online scheduler/interrupt reentry
and workload acceptance remain work in progress.

## Executed checks

Three focused warnings-as-errors tests pass in `build/ir-dynamic-count-contracts.log`.
They generate optimized and unoptimized CPU loop modules for eight budgets,
check counter phi allocation, reject wrong types/non-dominating bases and mixed
static cycles, protect the lowered admission/count records, reject changing a
store's count base, and retain/rewrite count-only values through GVN/DCE.

The independent dispatcher model runs 7,786 executions with 17,604 before/after
helper observations and 106 bounded reentries. Cases include zero work, maximal
input trip counts cut by budgets, accumulator/CS/counter wrapping, and independent
observer adjustments to the noncached CPU counter. A completed body is observed
as old_base + 1 before passing new_base through the backedge. Budget/header exits
observe the same sum without counting it twice.

Caller-owned faults restore the current loop state, deliver once and do not
commit the failing body. Helper-owned transfer/yield/invalidation outcomes retain
authoritative register, FLAGS, PC and counter mutations. Repeated bounded entries
start a fresh relative base while continuing the global count and guest values.
These use the real CPU-global Wasm ABI with controlled helper imports; they are
not an XP boot or integrated online guest-loop workload. `make ir-tests` includes
the Rust fixtures and JS execution oracle.

No ISA forms changed. Production Pending remains 3,728, and complete MIR/runtime,
remaining ISA, host fallback, XP/performance acceptance and legacy retirement
are still required for full IR-00–IR-14 completion.


The complete connected regression matrix passed in one make invocation in
`build/ir-dynamic-count-full-suite.log`, including 111 warnings-as-errors Rust
tests, the new dynamic CPU-loop oracle, all existing IR/independent-reference
targets, all SIMD families and final FLAGS observers. Feature compilation passed
in `build/ir-dynamic-count-check.log`; catalogue, normal production export
isolation and whitespace checks pass. No production source changed after this
verification.

Reachable direct x86 bytecode now uses this contract through the separate
[CFG frontend](ir-cfg-frontend.md), including actual interpreter comparisons and
faults after a completed loop iteration. Online scheduling remains separate.
