# MIR value programs and packed kernel selection

Ownership update: [owned MIR](ir-mir-owned.md) supersedes the retained-HIR lifecycle described in this stage report. Canonical checks now run at transaction sealing; the resulting read-only machine artifact owns its types/plans and is emitted after HIR is released. Historical test results below remain stage-specific.

Lowering now selects every supported scalar and vector value instruction into a
ValuePlan. The Wasm instruction dispatcher consumes value, memory, effect or call
plans; it no longer matches HIR opcodes. Value programs contain explicit inputs,
i32/i64 constants, machine-width scalar operations, selected reads, shuffle/lane
opcodes and selected packed kernels, followed by assignment to the result slot.

## Scalar selection and state reads

Narrow signed comparison and arithmetic right shift explicitly sign-extend their
operands in i32 operations. I64 arithmetic/comparisons use their i64 instructions;
comparisons produce an i32 condition. Extend, truncate, extract and insert lower
to explicit shifts, masks, wraps and signed/unsigned conversions. Result masks
for I1/I8/I16 are part of the program, rather than selected after emission from
the HIR result type. Select records Wasm's value/value/condition operand order.
LinearOffset selects wrapping i32 addition.

Read programs record access width, address binding and CPU versus standalone
behavior. Register, FLAGS and flag-operand addresses bind to StateLayout. Segment,
stack-width, XMM and flags_changed reads carry fixed CPU global offsets. CPU
ReadFlags calls get_eflags with its explicit void-to-i32 signature; standalone
ReadFlags loads the provided FLAGS slot. Standalone flags_changed is zero.
CPU-only reads declare their target requirement in the plan. Native guest memory
still uses the separate permission-checked memory plans.

## Packed kernels

The same PackedPlan now drives pure vector instructions and native vector-memory
combines. Lowering fixes unpack shuffle lanes, direct opcode and operand order,
variable-shift maximum/opcode/sign-fill policy, signed/unsigned widening multiply
opcodes, even-dword multiply selection and SAD reduction/shuffle operations.
PANDN/ANDNPS/ANDNPD explicitly reverse the operands to preserve source & !dest.
Variable shifts still inspect the full unsigned low qword; oversized logical
counts produce zero and oversized arithmetic counts use the planned sign fill.

The packed emitter encodes selected kernels without matching PackedOp or deriving
guest widths. Bitmask, lane extraction/replacement and ordinary shuffle opcodes
are selected directly in value programs. CPU cold-memory adapters and their
sampling, callback, fault and completion ownership remain unchanged.

## Validation

Before byte emission, plans are compared with canonical lowering and their
machine stacks are type-checked. Stack typing consumes selected operations and
value types without inspecting HIR opcodes. It checks argument order/types,
conversions, select arms/condition, matching CPU/standalone read result types,
valid SIMD lane/opcode combinations, valid value references and a single result of
the required machine type. Canonical checks additionally protect semantic masks,
signedness, CPU requirements and packed-kernel policy. This does not yet provide
an independent verifier for the complete MIR graph, state effects or transforms.

Three focused warnings-as-errors tests in `build/ir-mir-value-contracts.log`
cover 60 scalar width/operator combinations, 57 packed operations with register
and memory sources under both optimization settings (228 combinations), nine
invalid machine stacks, five stale/unsafe scalar contracts and six corrupted
register/memory shift kernels. Focused scalar programs produce 120 modules.
The independent BigInt oracle executes 17,280 cases across narrow/32/64-bit
boundaries, signs, wraparound, shifts and comparisons, while checking preserved
registers, FLAGS and recovery-only state. `make ir-tests` includes this oracle.

Value/state arenas and local liveness still originate in HIR. Explicit state
materialization plans, a fully independent MIR instruction/CFG lifecycle,
dynamic CPU accounting, host SIMD fallback, remaining ISA and online Tier/runtime
integration remain open. No ISA forms changed and production Pending remains
3,728. Full IR-00–IR-14 acceptance has not been reached.


The complete connected regression matrix passed in one make invocation in
`build/ir-mir-value-full-suite.log`, including 105 warnings-as-errors Rust tests,
17,280 new scalar Wasm executions, the prior CFG/call/typed-ABI fixtures, all
CPU/independent-reference targets, all SIMD families and final FLAGS observers.
Feature compilation passed in `build/ir-mir-value-check.log`; catalogue, normal
production export isolation and whitespace checks pass. No production source
changed after this verification.


StateMap writes subsequently moved into [explicit materialization plans](ir-mir-state.md),
using the same typed machine-expression encoding. HIR-derived liveness/canonical
checks, independent MIR lifecycle and dynamic CPU accounting remain incomplete.
