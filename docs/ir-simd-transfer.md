# Experimental XMM half, scalar and duplicate transfers

The IR frontend now covers these additional transfers:

| Encodings | Behavior |
|---|---|
| 0F 12, 66 0F 12 | MOVLPS/MOVLPD memory loads; 0F 12 register MOVHLPS |
| 0F 16, 66 0F 16 | MOVHPS/MOVHPD memory loads; 0F 16 register MOVLHPS |
| 0F 13, 66 0F 13 | Low-qword stores |
| 0F 17, 66 0F 17 | High-qword stores |
| F2 0F 12 | MOVDDUP |
| F3 0F 12, F3 0F 16 | MOVSLDUP/MOVSHDUP |
| 66 0F 6E, 66 0F 7E | MOVD between XMM and GPR/memory |
| F3 0F 7E, 66 0F D6 | MOVQ with the XMM register destination's high qword cleared |

These 15 encoding records add 48 valid coarse forms, bringing experimental
CpuSimdHIR to 428. The invalid register forms of MOVLPD/MOVHPD and the four
half-store encodings remain BaselineUD. MMX conversion forms are not included.
Production Pending remains 3,728.

## State and native operations

Register MOVHLPS takes the source high qword into the target low qword; the same
opcode's memory form takes an eight-byte source into the low qword. Both preserve
the target high qword. MOVLHPS and the high-half memory loads preserve the target
low qword and replace its high qword with the source low qword.

MOVDDUP reads/copies a low qword twice. MOVSLDUP repeats source dwords 0 and 2;
MOVSHDUP repeats source dwords 1 and 3. MOVD into XMM copies exactly 32 bits and
clears the other 96; MOVD out writes the complete 32-bit GPR even with a 16-bit
execution default. MOVQ register destinations clear their high qword, including
source/target aliases. No float conversion or FLAGS operation is performed.

Pure register paths use VectorShuffle, integer lane extract/replace and explicit
zero vectors. MOVD participates in the existing GPR SSA mappings, so a later
instruction can consume the value without a CPU state round trip. Native memory
loads use the exact 4/8/16-byte load and retain V128 SSA. Neither path calls a
legacy emitter or a generic per-instruction interpreter helper.

## Memory contracts

XmmTransferLoad carries a typed TransferOp and old-target V128, and requires the
complete matching XMM BeforeInstruction map. TransferOp fixes the source width:
eight bytes for low/high/duplicate qwords, sixteen for duplicate dwords. The
native path combines load64_zero or v128.load with the fixed byte permutation.

`ir_xmm_transfer_load(address, register, operation_id)` validates the operation,
reads the entire source with safe_read64s/128s, then samples the target. Half
loads retain the other half as changed by a source callback. Duplicate loads
replace all lanes after the read. Cold success commits once and exits with
outcome 4; faults retain CPU-owned state and return 2 without a result or commit.
MOVD and MOVQ zero-extending memory loads reuse the existing XmmLoad adapter.

XmmStore now has an explicit source lane. Lane 1 is legal only for eight-byte
stores; lane 0 retains existing 4/8/16-byte behavior. The native eight-byte store
extracts that qword. `ir_xmm_store(address, register, bytes, lane)` captures the
source vector before any write callbacks and uses the selected qword. The
pre-instruction and committed StateMaps still identify the full source vector;
every store terminates the current region. Slow success/fault exits do not replace
callback state with an old commit map. Original wide-write preflight, partial
writes and late unwrap-aborts are preserved.

SSE guards and their exact post-ModRM PC precede EA evaluation. The baseline
EM/TS, OSFXSR and unaligned-access policies remain unchanged. CPU wide-read MMIO
sign extension and cross-page decomposition remain part of the comparison.

## Evidence

`make ir-simd-transfer-tests` generates 5,664 optimized/unoptimized fixture pairs:
both execution defaults and address widths, all legal register destinations and
sources, and default/all segment overrides. Per debug/release CPU the suite passes
9,024 independent/CPU scenarios and 1,536 random transfer/GPR scenarios. It
requires zero slow adapter calls in 6,720 warmed native RAM entries.

Further checks cover 144 EM/TS guards, 150 alignment/boundary/fault cases,
75 segment faults, 72 OSFXSR/real/VM86 cases and 30 exact 4/8/16-byte access extents
with an absent following page. There are 60 MMIO/wide accesses, 15 callback state
mutation cases, 13 partial faults/aborts and 15 early exception observations per
build. Callbacks change all target lanes so the retained half and captured store
source are checked explicitly.

Eight mixed-transfer executions per build combine GPR-to-XMM, MOVHLPS, a half
load, MOVQ, duplication and a high-half store across native and cold completions.
Two faults check dirty XMM state, and two XMM-to-GPR-to-XMM chains check the shared
SSA mappings. Comparisons retain complete XMM/GPR/FLAGS state, PCs, segments,
frames, memory, callback observations and instruction counts. Verifier tests
reject invalid lane/width combinations and target/state mismatches.

Focused evidence is `build/ir-simd-transfer-suite.log`; the additional exact-width
cases are in `build/ir-simd-transfer-diff.log`. This increment does not establish
complete SIMD/MMX/x87, host fallback, full MIR/regions, online Tier integration,
XP compatibility, performance acceptance or legacy retirement.

The final full connected regression suite passed in
`build/ir-simd-transfer-full-suite.log`, including 83 warnings-as-errors Rust
tests, every existing IR/reference target and all SIMD suites after the store
ABI gained its explicit lane argument.
