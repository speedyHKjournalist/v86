# IR helper contract status

The metadata, verifier and explicit outcome Wasm ABI are implemented. The generic
legacy helper registry is not migrated. The first audited CPU segment and memory
adapters now execute through the separate [CPU ABI](ir-memory.md). Unadapted descriptors
remain compile errors; executable adapters must explicitly declare their ABI.

Descriptors carry input/result types, state read/write sets, memory effects,
fault/control-transfer/yield/translation/code-invalidation/I/O properties, and
exception ownership. Unknown helpers default to conservative effects and never
enter GVN/DCE. They require an explicit audit before use by an executable adapter.

The tested outcome policy is:

| Outcome | Required action |
|---|---|
| Normal | Continue under the helper's state write contract |
| FaultNeedsDelivery | Restore the appropriate fault/partial-progress map and deliver exactly once; only caller-owned faults permit this outcome |
| ControlTransferred | Exit without restoring a pre-call snapshot |
| Yield / Invalidated | Exit without restoring a pre-call snapshot; pre-call materialization and helper-maintained progress are prerequisites |

Helpers capable of observing, dispatching, yielding or invalidating require state
synchronization before the call. Their post-transfer CPU state is authoritative.
Tests validate policy decisions and 42 actual Wasm executions with instrumented
callees, optimized and unoptimized. These generic ABI fixtures do not test real guest delivery; separate CPU memory
tests exercise real #PF/#GP, division tests exercise #DE, and the REP suites
now verify partial-progress/final-commit exits.

Remaining work includes a complete helper registry, remaining ABI/scratch layouts,
normal-path state reloads, FP/state audits and the full guest exception matrix.
F80 remains a distinct IR type. There is no f64 approximation or new x87 execution
path in this change. The WasmBuilder v128 helper test uses a Wasm-to-Wasm callee;
vector values are never passed to a JS function.

## Executable outcome ABI

`HelperAbi::Outcome` maps i32-backed and I64 arguments to typed Wasm imports returning
`(outcome: i32, result...)`. Stable outcome tags are 0 through 4 in the table's
order (Yield=3, Invalidated=4). The normal results are masked to their declared
HIR widths. Unknown tags and contract-forbidden outcomes trap immediately.
This ABI currently supports I1/I8/I16/I32, not F80 or vector helper data.

Lowering records structural import signatures and the allowed cold exits in
`MirRegion::helpers`. Conflicting import signatures are compile errors. A
caller-owned fault requires an explicit `() -> ()` delivery adapter, which
consumes pending fault metadata after snapshot restoration. The generated
function exits immediately after delivery, even if the adapter returns to Wasm.
Helper-owned delivery, yield and invalidation return without restoring the old
map. Caller-owned fault maps cannot resume after the faulting instruction.

A call materializes GPR/FLAGS/EIP/committed state before entering its adapter.
Result values first enter temporary Wasm locals; SSA result assignment occurs
only after a Normal outcome. This preserves fault snapshots when register
allocation reuses snapshot slots for normal results. Local-count statistics
include these temporary slots and edge-copy staging slots.

The adapter must explicitly guarantee `normal_preserves_state` for cached
architectural state. This permits authoritative exceptional-state changes while
preventing continuation with stale SSA after a normal state-writing call.
Normal-path reloads are still pending. Extended XMM/x87 StateMaps remain rejected. REP progress maps now require exact
aliases of their complete GPR mappings, which the current materializer restores. Real adapters must also audit
implicit/lazy CPU state (including FLAGS provenance), fault metadata and CPU
memory layout before being registered in the live runtime.


Native checked division uses `ir_divide_fault`, a CPU-owned ()->() delivery adapter
that performs no arithmetic or architectural result writes. Generated guards
materialize the pre-instruction StateMap, call it once, then exit. Generic outcome
helpers now also stage mixed I32/I64 returns with matching typed temporaries.


## ENTER memory compatibility

ENTER uses the normal memory adapters with explicit per-access state and no
whole-instruction helper. Nested faults carry `trap_after_fault` after CPU-owned
single delivery; final write faults exit normally. The narrow
`ir_memory_write_unmasked_word` adapter preserves the pinned ENTER16 word MMIO
payload and does not alter guest registers. See [ir-enter.md](ir-enter.md) for
release/debug distinctions and the exact tested contract.


## Terminal CPU state adapters

`HelperAbi::CpuExit` now supports audited state-changing operations that end the
region. POPF/segment POP operands are read in HIR; their adapters own state changes
and the successful instruction commit. Only ControlTransferred/Invalidated exits
are legal, and the caller never restores stale state. The ordinary returning
helper reload requirement remains in force. See [ir-system-stack.md](ir-system-stack.md)
for the exact contract, real exception/IRQ evidence and compatibility limits.


## Port permissions and device observations

Scalar IN/OUT and non-REP INS/OUTS use explicit terminal adapters. OUTS checks
permissions before native source memory access; INS checks permissions and the
write range before its port read, then performs a fresh safe write. See
[ir-io.md](ir-io.md) for contracts and real TSS/MMIO fault evidence.

BeforeInstruction CPU helper calls now expose decoded next IP even for normal
returning adapters. previous_ip retains the fault PC. Standalone helper calls and
caller-owned fault restoration retain their explicit StateMap behavior.


## REP terminal batches

`HelperAbi::CpuRep` accepts CPU-owned Fault/Yield/Complete exits through outcomes
2/3/4. Only completion commits the REP instruction. Partial element work is
reported separately through the per-entry runtime result, and stale SSA state
never resumes after the call. See [ir-rep.md](ir-rep.md) for StateMap validation,
configuration, accounting and remaining online reentry requirements.

## CPU identification and model-specific state

CPUID/RDTSC/RDMSR/WRMSR use separate terminal CpuExit adapters. Permission
failure is delivered once before the semantic body and returns ControlTransferred;
success commits once and returns Invalidated. No old SSA state is restored. CPUID
policy, MSR tables, APIC assertions and timestamp interpolation stay CPU-owned.
The deterministic clock and TSC-state accessors exist only in the test harness
and ir-test-hooks build. See [ir-cpu-info.md](ir-cpu-info.md).

## Terminal execution-context and halt transitions

SYSENTER/SYSEXIT own the new CS/SS caches, CPL, widths, EIP/ESP and fetch-context
invalidation; their guarded bodies have no further guest-fault path. HLT owns
halt state, timer/halt notifications and immediate IRQ delivery before the adapter
commits. CLI uses the CPU's explicit fault-free success predicate; CLTS/WBINVD
check CPL before their bodies. All six use CpuExit without stale-state restoration.
See [ir-cpu-system.md](ir-cpu-system.md) for ownership, VME baseline limits and
target-fetch/observer tests. Online scheduling and generation publication remain
unconnected, and STI's next-instruction shadow is not implemented by these helpers.

## Control/debug registers and partial PDPTE loading

MOV CR/DR has four named terminal adapters with two explicit register-index
arguments. Permission precedes register validity and DR4/5 alias checks. The
adapters distinguish audited delivered-fault guards from successful bodies; they
never infer a fault from EIP. CR0/CR3/CR4 retain CPU-owned translation/cache and
PDPTE changes, including partial state on a host abort. No successful instruction
commit occurs after an abort. The exact debug/release policies, physical MMIO
observations and warmed mapping tests are recorded in
[ir-control-regs.md](ir-control-regs.md). Online IR dependency invalidation remains
unconnected even though these bodies perform existing CPU TLB maintenance.

## Descriptor tables, machine-status word and INVLPG

SGDT/SIDT/LGDT/LIDT use named staged CPU-memory adapters after explicit HIR
EA/segment resolution. Six-byte stores preflight the full destination and read
each table field at its original write point; loads commit only after both reads.
Late store faults retain the baseline dispatch-then-unwrap-abort policy. SMSW/LMSW
and INVLPG have explicit widths/permission guards; INVLPG does not read target
memory. Invalid register forms deliver #UD directly. All paths use terminal
CpuExit state ownership. See [ir-descriptor.md](ir-descriptor.md) for exact
ordering, MMIO remaps and mapping tests. These accesses are semantic-helper
coverage, not a new native HIR memory family or online IR invalidation integration.

## Task and local-descriptor registers

SLDT/STR/LLDT/LTR use named terminal adapters. Mode checks precede privilege
and operand access, after HIR segment resolution for memory forms. LLDT returns
its existing explicit read-fault status; LTR now has load_tr_checked while the
legacy load_tr wrapper retains its void ABI. TR cache updates still precede the
busy-byte write, whose fault retains CPU state and the baseline unwrap abort.
Independent pinned interpreter bodies validate this shared-CPU refactor in
[ir-task-regs.md](ir-task-regs.md). VERR/VERW and other task-switch machinery are
not implemented by this instruction family.

## Selector queries and late destination writes

LAR/LSL use explicit terminal query adapters. Source #PF returns without a
destination write; descriptor #PF preserves the baseline's later write of the
saved destination, including word writes into the post-fault ESP high half.
The saved destination is captured after a memory-source callback. See
[ir-selector-query.md](ir-selector-query.md). VERR/VERW now use separate terminal
query adapters. Their source mode/word-read guards precede the early
flags_changed.ZF clear, and descriptor faults return outcome 2 immediately.
FlagState.raw_zero and CPU state materialization retain the backing bit observed
by that early clear, including across IR-to-interpreter and IR-to-IR transitions.
`make ir-verr-tests` covers permissions, faults, MMIO and backing-state transitions;
`make ir-flags-observer-tests` retains the original eight-case baseline diagnostic.

## Conditional qword exchange

CMPXCHG8B has a terminal HIR intrinsic with native RAM lowering and an
ir_cmpxchg8b slow adapter. Initial preflight faults return 2; successful match
or mismatch commits once and returns 4. Post-preflight read/write faults retain
the original delivered-exception/unwrap abort and partial state. Implicit
comparison and replacement registers are captured after operand callbacks.
ZF backing value and laziness are distinct StateMap sources. See
[ir-cmpxchg8b.md](ir-cmpxchg8b.md) for the exact timing and MMIO read policy.

## XMM transfer cold completions

SseCheck faults before EA resolution. XmmLoad returns V128 SSA only on the native
RAM path; ir_xmm_load commits and exits on cold success, or exits without a
commit on #PF. ir_xmm_store likewise owns successful or faulted slow-path state
and captures the source before callbacks. V128 values never pass through JS;
helpers access explicitly materialized CPU XMM storage. Generic V128 helper
signatures remain rejected. See [ir-simd-moves.md](ir-simd-moves.md).


## Packed XMM integer cold completions

XmmBinary returns vector SSA on guarded RAM. ir_xmm_binary takes integer address,
destination register, validated PackedOp ID and explicit read width; it reads the
entire source before
sampling the destination XMM, applies explicit packed semantics and commits once
on success (outcome 4). A source fault returns 2 without a destination result or
commit. Both outcomes exit with CPU-owned state; callback changes are not replaced
by pre-call SSA. No v128 crosses JavaScript. See [ir-simd-integer.md](ir-simd-integer.md).


For UNPCKLPS/UNPCKLPD that read is exactly eight bytes; integer unpacking and
variable shifts still read sixteen, including a zero low-qword count. The
verifier and CPU adapter validate this distinction. Immediate packed shifts are
pure vector nodes after SseCheck and have no execution helper. See
[ir-simd-permute.md](ir-simd-permute.md).


## XMM shuffle cold completions

XmmShuffle has a fixed sixteen-byte read and a complete pre-instruction XMM map.
`ir_xmm_shuffle(address, register, operation_id, immediate)` validates its integer
arguments, reads the entire source and then samples the target. It commits once
and exits on success (4), or retains CPU fault state without committing (2).
PSHUF is source-only; SHUFPS/PD can observe callback-modified target lanes. See
[ir-simd-shuffle.md](ir-simd-shuffle.md).


## Half-vector/duplicate transfer completions and store lanes

XmmTransferLoad and `ir_xmm_transfer_load(address, register, operation_id)` use an
operation-defined eight- or sixteen-byte source read, then sample preserved target
lanes. Cold success commits/exits (4); a read fault exits without a result/commit
(2). MOVD/MOVQ zero-filling loads reuse XmmLoad. XmmStore now carries a source lane,
and `ir_xmm_store(address, register, bytes, lane)` captures the full source before
callbacks; lane 1 is legal only for an eight-byte store. See
[ir-simd-transfer.md](ir-simd-transfer.md).
