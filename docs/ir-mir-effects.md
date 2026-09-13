# Explicit MIR observation and guarded-call plans

Ownership update: [owned MIR](ir-mir-owned.md) supersedes the retained-HIR lifecycle described in this stage report. Canonical checks now run at transaction sealing; the resulting read-only machine artifact owns its types/plans and is emitted after HIR is released. Historical test results below remain stage-specific.

Lowering now produces EffectPlan entries for SegmentAddress, PopAddress,
GuestCheck, SseCheck and RmwStore. The emitter dispatches these plans directly;
it no longer chooses their adapter names, signatures, segment globals, guard
conditions or observation phases from HIR opcodes.

## Address and check plans

An address plan fixes the segment-null byte and segment-base global, the offset
SSA value, the result, the BeforeInstruction StateMap and the complete packed
result call. The non-null path adds the segment base directly. The null path
materializes the planned state and calls ir_segment_address or ir_pop_address;
the latter retains its explicit stack operand width. Packed-result decoding
keeps the plan's fault/host-abort policy. The POP memory temporary-SP observation
is represented by the selected HIR recovery state and remains unchanged.

A check plan has an optional fixed-global mask condition, recovery state,
complete call and explicit success/fault outcomes. GuestCheck always calls its
range/permission adapter, continues only on outcome 0 and exits on outcome 2.
SseCheck uses CR0 mask 12; when either EM or TS is set, its adapter must transfer
control with outcome 2. Its call branch cannot continue with cached SSA. This
retains the post-ModRM/pre-EA or pre-immediate exception observation point.
Unexpected outcomes trap rather than silently continuing.

## RMW observation and commit

RmwCommit carries the physical/CPU ticket, scalar width and value, the typed
`ir_rmw_write(i64, i32, i32) -> ()` call and the final commit StateMap. Its explicit
Observation references two phases: register/FLAGS values from the successful
commit map, and instruction count from the before-write map. The slow call thus
exposes completed ALU results before a device write, without counting the
instruction before that write completes. This phase selection previously lived
inside the emitter.

The native ticket selects a physical byte/word/dword store. The CPU ticket selects
materialization of the planned observation followed by the CPU write adapter.
Both successful paths materialize the final commit and return. Existing CPU
partial-write/late-fault host behavior remains owned by the adapter; there is no
new result code or recovery that could overwrite callback/fault state.

## Consistency, imports and dump

Emission verifies the HIR and both memory/effect plan vectors against canonical
lowering before it emits bytes. The check rejects altered observation phases,
ticket ABI, access widths, segment globals, guard masks, call arguments and
outcome policies, as well as missing/stale plans. This consistency check does not
independently prove guest semantics; the execution suites remain required.

Effect calls join memory and generic helper calls in import signature validation.
A central CPU_IMPORTS list prevents generic helpers from shadowing all twenty-two
built-in adapters, including the newer XMM arithmetic/shuffle/transfer/word/masked
adapters. MIR dump now includes the full HIR definitions and recovery maps before
the memory/effect plans, so their SSA and state references can be inspected.

Three focused tests cover all RMW scalar widths and their two commit phases,
segment/POP addresses, PUSHA/POPA widths and read/write checks, SSE guards,
eleven unsafe plan mutations, missing plans, every reserved CPU import and an
effect/helper signature conflict. They run through `make ir-tests`; focused
warnings-as-errors evidence is `build/ir-mir-effect-contracts.log`.

## Remaining work

MIR still shares the HIR value/state/CFG arenas and local allocator. These
structured plans migrate concrete execution policy into lowering; they do not
complete the independent MIR instruction/CFG representation. CMPXCHG8B and
division have subsequently moved to [arithmetic plans](ir-mir-arithmetic.md).
Generic helper observation policy subsequently moved into [call-site plans](ir-mir-calls.md).
Other arithmetic/control operations, explicit
materialization operations, dynamic budget/commit accounting and later MIR
optimization remain to be completed before production Tier migration.

The complete connected regression suite passed in
`build/ir-mir-effect-full-suite.log`, including 93 warnings-as-errors Rust tests
and every existing IR/independent-reference target. The planned paths execute
under real CPU RAM/MMIO, callbacks, partial completion, faults and SSE guard
conditions. Feature compilation passed in `build/ir-mir-effect-check.log`;
generated catalogue, production export isolation and whitespace checks pass.


Control dispatch subsequently moved into a [lowered MIR CFG](ir-mir-control.md),
including entry selection, branch conditions, budget recovery and typed scheduled
edge copies. The graph preserves HIR topology; independent graph transforms,
standalone MIR verification and the shared instruction/state arenas remain work
in progress.
