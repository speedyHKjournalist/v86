# Experimental XMM word insertion, extraction and sign masks

| Encoding | Behavior |
|---|---|
| 0F 50, 66 0F 50 | MOVMSKPS/MOVMSKPD: four/two sign bits into a full 32-bit GPR |
| 66 0F D7 | PMOVMSKB: sixteen byte sign bits into a full 32-bit GPR |
| 66 0F C4 | PINSRW: GPR low word or exact memory word into XMM lane imm8 & 7 |
| 66 0F C5 | PEXTRW: zero-extended XMM word imm8 & 7 into a full 32-bit GPR |
| 0F 2B, 66 0F 2B, 66 0F E7 | MOVNTPS/MOVNTPD/MOVNTDQ: sixteen-byte memory stores |
| F2 0F F0 | LDDQU: sixteen-byte unaligned memory load |

Nine encoding records add twenty valid coarse forms, bringing experimental
CpuSimdHIR to 448. Masks and PEXTRW are register-only; the non-temporal stores
and LDDQU are memory-only. Their invalid counterparts remain BaselineUD. MMX
forms are excluded. Production Pending remains 3,728.

## SSA and runtime contracts

VectorBitmask accepts 8/32/64-bit lane widths and returns I32 using the Wasm
integer SIMD bitmask operations. VectorExtract and VectorReplace now also accept
16-bit lanes. Unsigned extraction zeroes the high GPR bits, including under a
16-bit execution default. Register PINSRW consumes GPR SSA and preserves the
other seven XMM words. All imm8 high bits are ignored through an explicit & 7;
these operations do not convert floating-point values or change FLAGS.

XmmInsertWord is an ordered node with address, matching old-target V128 and
effect inputs, V128/effect results, and a full XMM BeforeInstruction StateMap.
Its native path guards exactly two bytes, loads an unsigned word, and inserts
it into the selected lane. The verifier rejects invalid lanes and destination/
StateMap mismatches. Cold execution uses
`ir_xmm_insert_word(address, register, lane)`: safe_read16 completes before the
target is sampled, so callback changes to the seven preserved words survive.
Success writes once, increments the instruction count once, returns 4 and exits;
a fault returns 2 without a result or count increment. CPU-owned fault and
callback state must not be overwritten by the old SSA map.

The four memory-only transfers use the established XmmLoad/XmmStore contracts.
The baseline models no separate non-temporal cache effect; its alignment #GP
checks are TODOs, so this migration retains its ordinary unaligned sixteen-byte
access behavior. Stores capture the source before write callbacks and terminate
the region. Read widths, write preflight, wide MMIO decomposition, partial faults
and late wide-write unwrap-aborts remain those of the pinned CPU. SSE guards
still precede immediate and EA processing at the baseline post-ModRM PC.

## Evidence

`make ir-simd-lane-tests` generates 13,944 optimized/unoptimized fixture pairs.
Both execution defaults, both address widths, every legal register alias and
all segment choices are exercised. PINSRW/PEXTRW cover every imm8 value for
representative aliases and all eight lanes plus ignored-high-bit cases elsewhere.
The independent scalar JS model does not use the IR lane mappings or Wasm SIMD.

Per debug/release CPU, the suite passes 18,064 independent/CPU scenarios, 832
random scenarios and 8,240 native RAM entries requiring zero slow calls. It
exhausts all 65,536 byte sign masks, sixteen dword masks and four qword masks,
checking full GPR output, unchanged XMM values, FLAGS and instruction counts.

Additional checks pass sixty EM/TS priority cases, fifty unaligned/boundary/
fault cases, twenty-five segment faults and thirty OSFXSR/real/VM86 cases.
Ten accesses end exactly at the page boundary with the next page absent,
including PINSRW's two-byte extent. Twenty MMIO accesses, five callback mutation
cases, five partial faults/aborts and five pre-EA guard observations compare
CPU state and callback ordering. Callbacks overwrite all destination lanes and
unrelated XMM/GPR state to detect stale materialization.

Eight mixed chains per CPU build combine register insertion, memory insertion,
word extraction, a byte mask and a non-temporal store. They check native
continuation, cold/device completion and reentry; two fault exits check the
already changed XMM before exception delivery. Full state comparisons include
GPRs, all XMMs, logical/raw/lazy FLAGS, PCs, segments, control registers, memory,
exception frames, callback events and committed instruction counts.

Focused evidence: `build/ir-simd-lane-suite.log`. Complete SIMD/MMX/x87 coverage,
host feature fallback, full MIR/regions, online Tier publication/invalidation,
XP/performance acceptance and legacy retirement remain unfinished.

The full connected suite also passed in `build/ir-simd-lane-full-suite.log`,
including 85 warnings-as-errors Rust tests and every existing IR/reference target.
The final experimental Wasm compilation passed in `build/ir-simd-lane-check.log`.
