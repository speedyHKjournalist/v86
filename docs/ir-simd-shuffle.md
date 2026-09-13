# Experimental immediate XMM shuffles

The IR frontend now supports PSHUFD, PSHUFLW, PSHUFHW, SHUFPS and SHUFPD in
register and memory forms. These five encoding records add 20 coarse forms;
experimental CpuSimdHIR totals 380 forms. Production Pending remains 3,728.

## Lane semantics and SSA

PSHUFD selects four source dwords. PSHUFLW selects four low source words and
copies the source high half; PSHUFHW selects four high source words and copies
the source low half. Neither operation preserves that half from the old target.
SHUFPS selects two old-target dwords and then two source dwords. SHUFPD selects
one old-target qword and one source qword, using only the low two immediate bits.
All operations preserve payload bits and FLAGS; they perform no float conversion.

ShuffleOp explicitly describes these five semantics. Register lifting creates a
VectorShuffle whose byte indices are fixed at compilation from the decoded imm8.
The existing typed verifier checks the two V128 inputs, V128 output and byte-index
bounds. Native Wasm emits one byte-shuffle instruction; it does not call old JIT
emitters or the interpreter.

Memory lifting creates XmmShuffle with operation, imm8, destination register,
linear source address, old-target V128 and effect token. Its BeforeInstruction
StateMap must contain all eight XMM values and identify the same old target.
A permitted, same-page RAM source uses v128.load and a byte shuffle, retaining
the result as SSA for following instructions. All five memory forms read exactly
sixteen bytes, even when the immediate selects only the first source element.

## Cold completion and exceptions

SseCheck runs before effective-address decoding and before the immediate byte is
consumed, retaining the pinned post-ModRM observation PC. Ordinary memory
observations use the completely decoded next PC. The existing EM/#UD, TS/#NM,
OSFXSR and alignment policies are preserved.

`ir_xmm_shuffle(address, register, operation_id, immediate) -> outcome` uses only
integer arguments and an integer result. The adapter validates the operation and
imm8, performs the CPU's full safe_read128s, then samples the current destination.
SHUFPS/SHUFPD therefore see changes made by source MMIO callbacks to the target;
PSHUF variants replace the target from the captured source while preserving
changes to other CPU state. Native vectors never cross a JavaScript helper ABI.

Cold success writes the result, commits one instruction and returns outcome 4.
The backend exits at the decoded next PC without restoring stale SSA. A source
fault returns outcome 2, retaining the delivered exception and callback state,
without a destination result or a successful instruction commit. Original CPU
wide-read decomposition and MMIO sign-extension behavior are unchanged.

## Differential evidence

`make ir-simd-shuffle-tests` generates 29,400 optimized/unoptimized fixture pairs.
Every imm8 is tested in both execution defaults and address sizes with XMM0 as
target and self, distinct-register and default-memory sources. Six control bytes
also cover every destination/source alias and all segment overrides. Word lanes
in the exhaustive source data are distinguishable; random vectors and raw NaN,
sign and boundary payloads provide additional coverage.

Per debug/release CPU, the suite checks 41,120 ordinary scenarios against an
independent lane model, with zero slow adapter calls in 23,440 warmed RAM entries.
It adds 3,840 boundary/random/alias scenarios, 360 EM/TS priority cases, 300
alignment/boundary/fault cases, 150 segment faults, 180 OSFXSR/real/VM86 cases,
120 warmed permission checks, 60 full-read checks with an absent upper page,
120 MMIO/wide reads, 30 callback mutations, 30 late source faults, 30 early fault
observations and 60 callback remaps. Eight mixed-shuffle executions check native
SSA continuation and cold-completion reentry; two source faults check dirty-XMM
materialization before CPU exception delivery.

Comparisons include complete XMM/GPR/FLAGS state, raw/lazy flag sources, PCs,
segments, exception frames, memory, callbacks and instruction counts. SHUFPD's
ignored high control bits are covered by the full imm8 enumeration. The initial
focused result is `build/ir-simd-shuffle-suite.log`; the final full suite also
uses the strengthened distinguishable-word input patterns.

This is another IR-08 family. Remaining SIMD transfers, MMX/x87 aliasing, floating
arithmetic and controls, F80, host SIMD fallback, complete MIR/regions, online Tier
publication/invalidation and XP/performance acceptance remain incomplete.

Final strengthened shuffle evidence is in `build/ir-simd-shuffle-resume-suite.log`.
The full connected target matrix completed in two segments after fixing an
expected-host-panic stack-isolation problem in the task test harness;
[ir-validation.md](ir-validation.md) records both runs and the diagnostic evidence.
