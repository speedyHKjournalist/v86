# Experimental packed XMM packing, unpacking and shifts

IR-08 now includes PUNPCKLBW/LWD/LDQ/LQDQ and HBW/HWD/HDQ/HQDQ,
PACKSSWB/PACKUSWB/PACKSSDW, PSRLW/D/Q, PSRAW/D and PSLLW/D/Q. UNPCKLPS/LPD
and UNPCKHPS/HPD reuse the corresponding pure interleave operation. The 19
packed operations and four PS/PD encodings add 92 experimental forms.

The immediate forms cover 66 0F 71 /2,/4,/6; 66 0F 72 /2,/4,/6; and
66 0F 73 /2,/3,/6,/7, including PSRLDQ/PSLLDQ. These add 20 valid register
forms. CpuSimdHIR totals 360 coarse forms. Invalid memory encodings remain
BaselineUD; MMX forms remain unsupported by this frontend. Production Pending
remains 3,728 and the production backend remains legacy.

## Semantics and native selection

Interleaving alternates destination and source elements from the specified low
or high half. Native Wasm uses a byte shuffle. Saturating pack operations narrow
signed words/dwords; PACKUSWB clamps signed inputs to unsigned bytes, so negative
words become zero. The result concatenates the narrowed destination then source.
Native Wasm uses the corresponding signed/unsigned saturating narrow opcode.

Variable shifts use the complete unsigned low 64 bits of the source as their
count; the high 64 bits do not affect the result. Native selection extracts the
low qword and checks the full count before converting it to i32. Logical shifts
at or above the lane width produce zero; arithmetic right shifts clamp to the
sign-filling count. Directly using Wasm's masked shift count would be incorrect.
The CPU adapter implements the same behavior with explicit lane arithmetic.

Immediate shifts preserve the complete unsigned imm8. Ordinary lane shifts feed
an explicit vector count to VectorBinary. PSRLDQ/PSLLDQ shift the entire vector
by bytes through a typed VectorShuffle, using a zero second input. Counts at or
above 16 bytes produce zero. The verifier validates both vector inputs, the
V128 result and every shuffle index. All immediate operations execute SseCheck,
even at count zero; they never use a per-instruction CPU execution helper.

## Exact source read extent

The baseline CPU reads only eight bytes for UNPCKLPS and UNPCKLPD memory forms.
It reads sixteen bytes for the integer unpack operations, high PS/PD unpacks,
packs and variable shifts, including zero counts or operations using only the
low half of the value. This distinction affects page faults and MMIO observations.

XmmBinary therefore carries an explicit `bytes` field. Verifier permits eight
bytes only for low dword/qword interleaving; all other packed operations require
sixteen. The frontend selects eight only for the two low PS/PD encodings. Native
RAM uses load64_zero or v128.load, with a guard covering exactly that width.

The integer-only ABI is now
`ir_xmm_binary(address, register, operation_id, bytes) -> outcome`. The adapter
validates the width/operation combination and uses the original safe_read64s or
safe_read128s. It samples the destination after the complete read and retains
CPU-owned callback/fault state. Eight-byte MMIO reads retain the baseline qword
sign-extension behavior. Cold success commits once and exits; a source fault
neither writes a vector result nor commits the faulted instruction.

This read-width distinction was exposed by differential testing, then preserved
with explicit absent-upper-page regressions. The original failed run is retained
in `build/ir-packed-permute-suite.log`; the corrected run and added regressions
are in `build/ir-packed-permute-fixed.log` and `build/ir-packed-permute-diff.log`.

## Evidence

`make ir-simd-integer-tests` now generates 33,120 fixture pairs across 69 encoded
operations, both execution defaults and address widths, every XMM destination,
all register aliases and all segment override forms. Each debug/release CPU passes
48,576 ordinary comparisons and 8,832 boundary/random/alias comparisons against
native Wasm and the independent BigInt model. There are 30,912 warmed native RAM
entries with no slow adapter call.

Additional checks per build cover 1,344 full-u64 count/high-half cases, 276 signed
narrowing boundaries, 828 EM/TS guards, 690 alignment/boundary/fault cases, 345
segment faults, 414 OSFXSR/real/VM86 cases, 276 warmed permission checks, 276
MMIO/wide reads, 69 callback mutations, 69 late source faults, 69 pre-EA fault
observations and 138 remapping cases. The 138 absent-upper-page cases verify that
low PS/PD unpacks can finish after exactly eight bytes, while integer unpacks and
zero-count variable shifts still require the upper bytes. Existing vector-chain
checks retain cold completion/reentry and dirty-XMM fault coverage.

`make ir-simd-immediate-tests` generates 14,720 fixture pairs. Every imm8 is checked
for XMM0 in both execution defaults/address sizes, and boundary counts are checked
for every other destination. Per build, 1,440 additional random vectors, 540 EM/TS
guards, 270 OSFXSR/real/VM86 cases and 90 pre-immediate exception observations pass.
Zero-count guards preserve the post-ModRM PC before the immediate byte is consumed.

The independent model uses BigInt lane narrowing and shifts rather than Wasm
instructions or the Rust packed calculator. Comparisons include full XMM/GPR,
FLAGS backing and lazy sources, PC, segments, frames, memory, callback events and
instruction counts. These tests do not establish full SIMD/MMX/x87, host fallback,
complete MIR/regions, online Tier integration or OS/performance acceptance.

The final full regression log is `build/ir-packed-permute-full-suite.log`.
It includes 79 warnings-as-errors Rust tests and all connected legacy/IR baseline
suites. The immediate suite additionally passes eight mixed byte-shuffle, unpack,
shift, pack, load and store chain executions per build, plus two dirty-XMM source
fault exits. These cover native continuation, cold completion and reentry after
an eight-byte source operation with callback-modified destination state.
