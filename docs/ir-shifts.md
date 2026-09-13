# Experimental integer shifts and rotates

The IR frontend now lowers C0/C1, D0–D3 groups 0–7, including the group-6 SHL
alias, and SHLD/SHRD (0F A4/A5/AC/AD). Register forms use pure SSA; memory forms
use the existing affine RMW ticket. Operand widths 8/16/32, immediate/one/CL
counts, byte register aliases and 16/32-bit explicit addressing are covered.
SHLD/SHRD have 16/32-bit operands. LOCK remains a compile stop.

Count inputs are captured before destination updates, including CL/CH/ECX aliases
and double shifts whose source is ECX. Counts are masked by 31. Small RCL/RCR
counts are additionally reduced modulo 9/17; their zero effective counts retain
all flags. ROL/ROR first test the masked count, so a nonzero multiple of 8/16
still updates CF/OF, matching the pinned CPU implementation. Every pure Wasm
shift uses defined modulo-32 behavior, and explicit selects handle boundaries
where an x86 width or carry ring differs from that behavior.

Memory forms perform write-permission checks, a read, and a write even for count
zero. Permission checks for both pages precede any device read. Arithmetic and
FLAGS computation happen in SSA between the paired ticket operations. Slow writes
publish the resulting FLAGS to device callbacks; success commits and exits, and
already-delivered faults exit with the old state intact. The whole instruction is
never delegated to an interpreter or arithmetic helper.

## Pinned FLAGS provenance

The baseline's undefined shift AF depends on its retained `last_op1`: lazy AF
reconstructs a second addition operand from the new result and that old operand.
A materialized FLAGS word alone is therefore insufficient to preserve subsequent
behavior. `FlagState.last_op1` is an optional I32 SSA source, and the integer
frontend always supplies it through `ReadFlagOperand`. ADD/ADC/SUB/SBB/CMP,
INC/DEC and NEG update it with their first operand; logic/rotate/shift preserve it.
StateMap values, dominance/type checks, replacements, DCE roots and local liveness
include this source. CPU exits and observer snapshots write the actual `last_op1`
global alongside materialized FLAGS. This also fixes an IR arithmetic exit followed
by an interpreter shift, even when the arithmetic itself did not use undefined AF.

The standalone artifact ABI has an explicit, nonoverlapping `StateLayout.flag_operand`
word (offset 44 in tests). CPU artifacts use the real global. Manually constructed
StateMaps can omit this source only when their contract does not own it; frontend
snapshots never omit it. The field is neither a scratch local nor an external pointer.

Other undefined flag policies also follow the pinned baseline: SHR computes OF
from the original sign for nonzero counts; ROL/ROR/RCL/RCR compute it from final
carry/sign bits; SHLD32 clears OF for counts greater than one, while SHLD16 retains
its CF/result-sign formula. Counts greater than a 16-bit double-shift width follow
the baseline's concatenated-operand behavior. These are compatibility policies,
not additional architectural guarantees for undefined x86 flags.

## Verification

`make ir-shift-tests` generates 1,632 fixtures (920 register, 712 memory), each
with optimized and unoptimized Wasm. The CPU differential covers:

- 746,688 ordinary executions, all 256 CL byte inputs for register forms,
  operand/address/decode widths, carry inputs and aliasing. A separate BigInt
  bit-serial model validates register results and FLAGS; it uses one-bit steps
  instead of the emitter's variable-shift formulas.
- 50,208 warm memory executions with no slow RMW imports.
- 2,976 real memory/segment faults, including read-only and second-page faults
  before any device read, even when the masked count is zero.
- 672 MMIO callback comparisons and 672 slow-memory lazy-FLAGS cases. Callback
  order, values, all GPRs, FLAGS and sequential EIP are compared.
- 4,032 arithmetic-to-shift sequences both inside IR and across an IR-to-interpreter
  exit, including 8/16/32-bit and high-byte arithmetic provenance.
- Standalone provenance execution and negative provenance-type/layout checks.

Production coverage remains Pending until runtime, complete mode coverage and
acceptance gates pass. Wide multiply/divide, bit operations and the remaining
integer instructions are still part of IR-05; these shifts do not complete the
full IR-00–IR-14 request or enable production IR tiers.
