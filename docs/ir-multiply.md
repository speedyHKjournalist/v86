# Experimental multiply and divide lowering

The frontend supports MUL/IMUL/DIV/IDIV F6/F7 groups 4–7, two-operand IMUL 0F AF,
and immediate IMUL 69/6B. Implicit operands cover 8/16/32 bits; explicit IMUL
covers 16/32 bits, with immediate sign extension and register/memory sources.
Source values are captured before destination updates, including AL/AH/AX/EAX,
DX/EDX and source/destination aliases. Explicit memory addressing can be 16/32-bit.

MUL/IMUL sign- or zero-extend both operands into ordinary I64 HIR and multiply
natively. Low/high extraction updates AX or DX:AX / EDX:EAX, or the explicit IMUL
destination. Comparing the full product with an extended truncated result computes
CF/OF. Other FLAGS follow the pinned CPU's undefined result-based policy, including
the retained `last_op1` AF source described in [ir-shifts.md](ir-shifts.md).
Multiplication does not overwrite that retained operand.

## Checked divide and exception ownership

`Divide { bits, signed }` is an ordered HIR operation with two I64 operands, typed
quotient/remainder outputs and an explicit pre-instruction StateMap. Verification
accepts only guest widths 8/16/32 and correctly typed values. CPU lowering requires
a before-instruction map. Standalone emission refuses guest division without the
CPU fault ABI.

The Wasm backend checks a zero divisor and the signed i64 MIN / -1 pair before
executing native division. It then checks the quotient's signed/unsigned guest
range before assigning either architectural result. Quotient and remainder are
staged until all fault checks have passed. No Wasm division trap is used to signal
a guest exception, and neither destination register is partially committed on #DE.

A failed check materializes the fault map and calls `ir_divide_fault` exactly once.
This adapter performs no arithmetic or destination writes; it only invokes the
CPU's real #DE delivery. IR then exits without restoring stale state. Previous
instructions remain committed, while the failing divide does not increment the
instruction count. Successful DIV/IDIV preserve FLAGS and their retained operand.

Memory-source segment/page checks and the actual read precede divide checks, so
#PF/#GP wins over a potential #DE and MMIO sees the old architectural state.
Memory sources use ordinary read permission, never a write/RMW path. The current
entry model is still cold CPU execution outside legacy `in_jit`, not online tier
publication.

## i64 compiler support

The scalar backend now accepts I64 arithmetic, comparisons, shifts, select,
extension/truncation, extraction/insertion, locals and parallel edge copies.
I64 and affine RMW tickets have distinct IR types even though both use Wasm i64
locals. General outcome helpers use structural signatures for mixed I32/I64
arguments/results and stage each return value in a correctly typed temporary.
StateMap architectural GPR and FLAGS fields remain I32/I1; these changes do not
claim guest SIMD/FPU support. CPU CFG loops still require dynamic commit accounting
before they can be enabled.

## Verification

`make ir-multiply-tests` compares optimized/unoptimized generated artifacts against
an independent BigInt arithmetic model and exact CPU instruction execution:

- 129,500 successful multiply/divide executions and 16,900 exactly-once #DE cases,
  covering zero, signed minima, quotient bounds, random values, register aliases,
  both decode widths and all operand widths.
- 29,280 instrumented warm memory-source executions with no slow read import.
- 688 source #PF/#GP precedence cases, 240 MMIO read comparisons and 240
  lazy-FLAGS source paths.
- 24 ring3 #DE deliveries through a real TSS, including nonzero CS bases and
  saved logical fault EIP / original user ESP.
- 244 completed-prefix instruction counts and fault snapshots, ensuring successful
  earlier work survives a later divide failure.
- Twelve standalone i64 loop phi-copy and mixed-width helper ABI executions,
  plus negative divide type and recovery-map checks.

The catalogue adds `CpuArithmeticHIR` for register DIV/IDIV because real exception
delivery requires the CPU ABI. Memory forms use `CpuMemoryHIR`; register multiply
forms use `NativeHIR`. Production Pending remains 3,728. Full integer/guest ISA,
production IR tiers, optimization and OS/performance acceptance are still pending.
