# MIR state materialization plans

Ownership update: [owned MIR](ir-mir-owned.md) supersedes the retained-HIR lifecycle described in this stage report. Canonical checks now run at transaction sealing; the resulting read-only machine artifact owns its types/plans and is emitted after HIR is released. Historical test results below remain stage-specific.

Lowering now converts StateMaps into explicit StatePlans. A plan contains ordered
CPU and standalone writes, a separate count phase, the decoded-next-PC observer
write and the CPU target requirement. Writes carry a symbolic or fixed address,
i32/v128 store class and a typed machine expression. The emitter consumes these
plans at normal exits, budget exits, memory/check observers, helper calls, fault
restoration and RMW completion. It no longer inspects HIR StateMap fields.

## Ordered writes

Plans first write the eight GPRs, then any XMM slots, then optional last_op1.
FLAGS reconstruction preserves system bits and supplies CF/PF/AF/ZF/SF/OF at their
explicit bit positions. Standalone writes use computed architectural ZF. CPU
writes retain raw ZF when present, derive last_result as 1 XOR computed ZF,
set last_op_size to 31 and encode zero_is_lazy in flags_changed bit 6. If raw
backing is absent, the plan omits the synthetic result/size writes and clears
flags_changed. Missing last_op1 leaves that CPU slot untouched.

CPU previous_ip receives the instruction PC plus the current CS base. EIP uses
the SSA next_value when present, otherwise next_pc for AfterInstruction or the
instruction PC for BeforeInstruction/RepProgress. CPU EIP adds the current CS
base with i32 wrapping. Standalone addresses bind to StateLayout and omit this
CPU translation. The separately planned decoded-next write exposes next_pc plus
CS after state/count materialization at CPU operand observers, preserving the
fault PC in previous_ip.

## Count phases and RMW

Standalone counts remain absolute snapshot values. CPU counts use a Delta plan:
add snapshot minus already-accounted to the current CPU counter, then update the
local already-accounted snapshot. Loading the current CPU counter preserves
changes owned by intervening CPU helpers. Values and counts can select different
StatePlans at an observation point. RMW slow writes select committed register
and FLAGS writes, pre-write count, and the committed state's decoded-next write.
Final completion then materializes the committed count once. The emitter no
longer clones and edits a HIR StateMap to manufacture this intermediate phase.

These remain static snapshot counts with within-entry delta tracking. Dynamic
guest-loop accounting, scheduler budgets and online reentry are not completed by
this migration, and cyclic CPU CFGs remain rejected.

## Verification and evidence

Before emission, each StatePlan is checked against canonical lowering. Every
write expression is independently machine-stack typed using the value-program
checker; it must produce exactly the store's i32 or v128 value. MIR dumps include
ordered materialization writes and count modes. Canonical checking protects
semantic ordering, omitted writes, PC choice, raw/lazy FLAGS and count phases;
this is not yet an independent proof system for arbitrary MIR state transforms.

Three focused warnings-as-errors tests pass in `build/ir-mir-state-contracts.log`.
They cover four PC modes, presence/absence of XMM and FLAGS backing, optimization,
CPU/standalone requirements, three RMW widths, twelve unsafe state-plan mutations
and stale HIR state. Forty-eight emitted modules run through an independent
memory-observation oracle for 3,456 executions, checking GPR/XMM permutation,
FLAGS backing, optional provenance writes, CS/count wrapping, decoded PC modes
and repeated entry. Existing helper and MMIO fault suites remain the actual CPU
oracle for intermediate observers and restoration.

HIR still provides value/state definitions, liveness and canonical migration
checks. Independent MIR graph/instruction lifecycle, dynamic accounting,
remaining ISA, host fallback and online Tier/runtime integration remain open.
ISA coverage and production Pending (3,728) are unchanged. Full IR-00–IR-14
acceptance remains unmet.


The complete connected regression matrix passed in one make invocation in
`build/ir-mir-state-full-suite.log`, including 108 warnings-as-errors Rust tests,
3,456 new state observation executions, all existing IR/independent-reference
targets and final FLAGS observers. Actual CPU suites exercise memory/helper
observations, RMW phases, callback mutations, partial faults and vector exits.
Feature compilation passed in `build/ir-mir-state-check.log`; catalogue, normal
production export isolation and whitespace checks pass. No production source
changed after verification.


CPU cycle support subsequently gained [SSA dynamic count bases](ir-dynamic-count.md).
Graphs with complete live dynamic-count maps can now execute under the CPU ABI;
static/mixed-count cycles remain rejected. Automatic guest-CFG lifting and online
scheduler/interrupt integration remain incomplete.
