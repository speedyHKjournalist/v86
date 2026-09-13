# Explicit MIR helper call sites

Ownership update: [owned MIR](ir-mir-owned.md) supersedes the retained-HIR lifecycle described in this stage report. Canonical checks now run at transaction sealing; the resulting read-only machine artifact owns its types/plans and is emitted after HIR is released. Historical test results below remain stage-specific.

Lowering now creates a CallPlan for generic helper instructions. The plan records
the recovery state, target-specific observation mode, argument values, typed
result staging order, caller-owned fault delivery, accepted exit outcomes and
optional normal outcome. Shared helper signatures and outcome ownership are
legalized separately from the per-site plan. The emitter consumes both records.

CPU calls with BeforeInstruction or RepProgress recovery maps expose decoded
next PC before entering the helper. Other CPU calls and standalone calls expose
the captured state. A caller-owned fault restores the planned recovery map and
invokes the planned void delivery adapter exactly once. Helper-owned exits return
without restoring cached SSA over authoritative CPU state.

Multi-results are popped into typed scratch locals in reverse result order.
Only an accepted normal outcome assigns these staged values to allocated SSA
slots. This preserves recovery values when normal results reuse snapshot slots.
Terminal CPU and REP calls have no normal outcome; unexpected outcomes trap.
The existing explicit scratch-ABI requirement for V128 helpers and rejection of
continuing state-mutating helpers without reloads remain in force.

Emission validates HIR, the shared helper table and every call plan before
writing Wasm. Table entries are re-legalized from helper descriptors, and plans
are compared with canonical lowering. Live calls require plans; unused arena
records whose helper is no longer referenced do not require a callable import.
This is consistency validation against the current HIR contract, not an
independent semantic proof. MIR dumps show the call plans with their state/value
definitions.

Three focused warnings-as-errors tests cover caller/helper/no-fault ownership,
optimized and unoptimized calls, CPU versus standalone observations, typed
multi-result pop order, CPUID/REP terminal outcomes, thirteen unsafe table/site
mutations, missing vectors and an orphaned call record. Evidence is
`build/ir-mir-call-contracts.log`. Existing helper, I/O, REP and system execution
suites exercise state observations, fault ownership and actual Wasm calls.

MIR still shares HIR value/state/CFG arenas and allocation. Independent MIR
instructions and CFG, explicit state materialization operations, dynamic
accounting, further optimization and production Tier integration remain open.
ISA coverage and production Pending are unchanged. Full IR-00–IR-14 acceptance
is not established by this migration.

The full connected regression matrix passed in one make invocation in
`build/ir-mir-call-full-suite.log`, including 99 warnings-as-errors Rust tests,
42 helper ABI executions, mixed-width result/phi tests, CPU/REP outcome suites,
all memory/SIMD targets and the final FLAGS-observer diagnostic. Feature
compilation passed in `build/ir-mir-call-check.log`; catalogue, normal production
export isolation and whitespace checks pass. No production source changed after
this verification.


Control dispatch subsequently moved into a [lowered MIR CFG](ir-mir-control.md),
including entry selection, branch conditions, budget recovery and typed scheduled
edge copies. The graph preserves HIR topology; independent graph transforms,
standalone MIR verification and the shared instruction/state arenas remain work
in progress.
