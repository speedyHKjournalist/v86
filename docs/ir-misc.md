# Experimental scalar, FLAGS, BCD and implicit-address operations

The frontend now lowers CBW/CWDE, CWD/CDQ, SAHF/LAHF, SALC, CLD/STD,
DAA/DAS, AAA/AAS, AAD/AAM, moffs MOV (A0–A3) and XLAT (D7). Register arithmetic
uses native typed SSA and Wasm scalars. AAM uses the existing guarded Divide
operation; implicit-address accesses use the existing CPU memory path. There is
no whole-instruction interpreter helper. Production tiers still use legacy JIT.

## Register and FLAGS contracts

- CBW/CWDE sign-extend AL/AX into AX/EAX; CWD/CDQ replicate AX/EAX's sign into
  DX/EDX. Narrow writes preserve the untouched register high halves.
- SAHF replaces CF/PF/AF/ZF/SF from AH and applies the pinned FLAGS reserved-bit
  mask/default without changing OF. LAHF composes the low flag byte from SSA
  sources and writes AH while preserving AL and upper EAX.
- SALC writes either 0 or 255 to AL from CF. CLD/STD update the system DF source
  while retaining all arithmetic flag sources.
- DAA/DAS evaluate both adjustment decisions from the old AL and input CF/AF;
  DAS additionally handles the low-digit subtraction borrow. PF/ZF/SF describe
  the resulting AL, and CF/AF describe the decimal adjustments.
- AAA/AAS account for carry/borrow through the full AX update before the extra
  AH adjustment and low-nibble AL mask. Only CF/AF are replaced, matching the
  pinned behavior for the other undefined bits.
- AAD accepts every immediate base, including zero. The wide mathematical sum
  is at most 65,280, so the pinned carry test above 65,535 is always false for
  decoded byte operands. Native byte arithmetic computes its low result; AX is
  zero-extended from AL. CF/AF/OF are cleared and PF/ZF/SF describe AL.
- AAM accepts all nonzero immediate bases, writes quotient to AH and remainder
  to AL, and clears CF/AF/OF with PF/ZF/SF from AL. A zero base calls only the CPU
  #DE delivery adapter, once, before any result or flags commit. Earlier completed
  instructions remain counted. Real ring3/TSS fault frames are tested.

All these operations retain the existing `last_op1` provenance, except for
preceding arithmetic instructions in composed test regions that update it normally.
Recovery and continuation therefore retain the established shift/flag contract.

### DAA/DAS undefined OF policy

The plan's section 5.1 permits a stable undefined-bit policy and limits differential
masking to actually undefined bits. DAA/DAS preserve the input OF SSA value.
The pinned interpreter instead retains its physical flag-register OF; that can
be stale while incoming OF is lazy. The new policy is independent of incidental
materialization and matches the baseline with materialized input flags.

The independent oracle checks the exact preserve-input policy. CPU differential
comparison masks **only OF for DAA/DAS** (and the two composed chains containing
them); it checks every other flag, register, PC and retained operand exactly.
No flags are masked for the other scalar operations or for AAM faults. This is
an explicit undefined-bit policy, not a claim of byte-for-byte lazy-storage identity.

## Implicit-address memory

Moffs uses the address-size immediate and the selected segment (DS by default),
independently of operand width. XLAT adds unsigned AL to BX/EBX, wraps according
to address size, then adds the segment base modulo 32 bits. The selected byte is
read before AL changes. Tests include 16-bit index wrap and 32-bit overflow from
FFFFFFFF, nonzero segment bases and FS overrides.

Loads/stores have explicit fault maps and use the existing native TLB guard or
CPU-owned safe memory adapter. A store has a separate successful commit map and
exits under the current conservative invalidation policy. Null-segment, missing,
readonly and cross-page faults preserve the pre-access state; MMIO sees fully
decoded IP and the correct register/FLAGS state. Fault and device ordering are
not bypassed for direct addresses.

## Evidence

`make ir-misc-tests` generates 1,246 fixtures (1,050 scalar and 196 memory), each
optimized/unoptimized, plus nine composed regions. It verifies:

- 88,032 scalar cases against both an independent arithmetic/bit oracle and
  exact CPU interpreter steps, including materialized and lazy incoming FLAGS.
- 3,292,160 exhaustive BCD, conversion and flag-transfer cases: all AL inputs
  and CF/AF/OF combinations for DAA/DAS; all AX values with both AF values for
  AAA/AAS; every nonzero AAM base × AL; all AX values at AAD bases 0/10/255;
  all word sign-extension inputs; and all AH bytes × arithmetic flag patterns.
- 378 composed arithmetic/BCD/FLAGS regions, including completed-prefix #DE
  accounting and OF-preservation policy checks.
- 3,136 moffs/XLAT comparisons, 1,184 instrumented native paths, 1,048 actual
  memory/segment faults, and 392 MMIO observation comparisons.
- 84 ordinary AAM zero-base faults and 16 real ring3 faults through a TSS.
- ABI checks requiring CPU access for AAM/memory while allowing standalone
  lowering/emission for the pure scalar subset.

The catalogue gains 28 NativeHIR, 14 CpuMemoryHIR and two CpuArithmeticHIR forms.
Production Pending remains 3,728; full guest ISA, production IR tiers, online
publication/invalidation and OS/performance acceptance remain unfinished.
