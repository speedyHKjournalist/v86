# Checked arithmetic and compare/exchange plans

Ownership update: [owned MIR](ir-mir-owned.md) supersedes the retained-HIR lifecycle described in this stage report. Canonical checks now run at transaction sealing; the resulting read-only machine artifact owns its types/plans and is emitted after HIR is released. Historical test results below remain stage-specific.

MIR effect plans now include explicit checked-division and CPU-register
compare/exchange plans. Lowering selects guest-specific ranges, exceptional input
pairs, implicit register/global slots, RAM policy and adapter ABI. The emitter
consumes these plans and no longer selects those facts from Divide or
CompareExchange8B HIR opcodes.

## Division

A Division plan records dividend/divisor SSA values, quotient/remainder results,
signedness, the legal quotient interval, the pre-instruction recovery map and a
void ir_divide_fault call. Signed division also records the i64 MIN/-1 input pair
that must raise guest #DE before Wasm's signed-division instruction could trap.
Division by zero is rejected before executing either signed or unsigned division.

Lowering fixes signed intervals to [-128,127], [-32768,32767] or
[-2147483648,2147483647], and unsigned maxima to 255, 65535 or 4294967295.
The emitter checks the staged quotient against that interval, sends invalid
results through the planned fault edge, then computes the remainder. Both
results are staged until all possible faults have been checked, preserving
recovery values when the allocator reuses SSA slots. Output narrowing and the
existing instruction-level FLAGS policy remain unchanged. Memory operand faults
still precede divide checks through ordered HIR/effect execution.

## CMPXCHG8B

The CompareExchange plan records its resolved address, recovery state and full
eight-byte writable RAM guard. It fixes CPU globals for expected EDX:EAX and
replacement ECX:EBX, raw FLAGS and flags_changed, the ZF mask/shift and the counter
increment. Operand-size prefixes do not narrow this operation. LOCK retains the
existing audited single-owner, unshared-memory ABI.

The native path materializes the planned input state, checks RAM eligibility,
loads the qword and reads the planned expected register pair. A match stores the
planned replacement pair; a mismatch updates the expected-pair slots and does
not write guest memory. It then records the comparison in raw ZF, clears ZF
laziness, increments the counter once and exits. These native steps have no
intervening device observer or fault after the guard.

The slow edge uses the explicit ir_cmpxchg8b signature and address argument,
accepts only outcomes 2/4 and returns without restoring cached SSA. The existing
CPU adapter owns complete write preflight, read-before-register-sampling, raw-ZF
callback timing, partial faults/host aborts and final commit. This migration does
not replace those semantics with unconditional RMW writeback or use the legacy
emitter as an MMIO oracle. See [the instruction contract](ir-cmpxchg8b.md).

## Checks and remaining scope

The arithmetic plans share EffectPlan consistency verification, runtime import
signature validation, CPU import protection and MIR dump. Sixteen mutation cases
reject missing host-trap guards, broadened quotient ranges, wrong signedness,
wrong results/ABIs, narrowed RAM checks, incorrect implicit registers/FLAGS slots,
wrong counts and wrong exit outcomes before byte emission.

Focused tests cover all three division widths, both signs, register/memory forms
and both optimization settings (24 combinations), plus execution defaults,
operand-size prefixes, LOCK and optimization for CMPXCHG8B (16 combinations).
Warnings-as-errors evidence is `build/ir-mir-arithmetic-contracts.log`; the existing
CPU, independent-model and fault suites remain the execution oracle.

MIR still shares HIR value/state/CFG arenas and its allocator. These executed
plans remove more guest-specific policy from emission but do not establish the
independent MIR instruction/CFG representation, explicit materialization
operations, dynamic budget/commit accounting
or production Tier integration. Full IR-00–IR-14 acceptance remains open.
Generic helper observation and outcome policy subsequently moved into
[call-site plans](ir-mir-calls.md).

The complete connected regression matrix passed in
`build/ir-mir-arithmetic-full-suite.log`, including 96 warnings-as-errors Rust
tests and all existing IR/independent-reference targets. Division faults and
CMPXCHG8B native/MMIO/callback/ZF paths execute through the new plans. Feature
compilation passed in `build/ir-mir-arithmetic-check.log`; generated catalogue,
normal export isolation and whitespace checks pass.


Control dispatch subsequently moved into a [lowered MIR CFG](ir-mir-control.md),
including entry selection, branch conditions, budget recovery and typed scheduled
edge copies. The graph preserves HIR topology; independent graph transforms,
standalone MIR verification and the shared instruction/state arenas remain work
in progress.
