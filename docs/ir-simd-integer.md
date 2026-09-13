# Experimental packed XMM integer operations

The first packed IR increment covered 38 integer operations plus the eight
ANDPS/ANDPD, ANDNPS/ANDNPD, ORPS/ORPD and XORPS/XORPD encoding aliases.
That increment added 184 coarse experimental forms, bringing CpuSimdHIR to 248.
Packing/unpacking and shift coverage was subsequently added; current coverage
and its tests are recorded in [ir-simd-permute.md](ir-simd-permute.md).
Production Pending remains 3,728; this does not enable the IR production backend.

| Family | Implemented instructions |
|---|---|
| Wrapping arithmetic | PADDB/W/D/Q, PSUBB/W/D/Q |
| Saturating arithmetic | PADDSB/W, PADDUSB/W, PSUBSB/W, PSUBUSB/W |
| Comparison | PCMPGTB/W/D, PCMPEQB/W/D |
| Min/max and average | PMINUB, PMAXUB, PMINSW, PMAXSW, PAVGB/W |
| Multiply and reduction | PMULLW, PMULHUW, PMULHW, PMULUDQ, PMADDWD, PSADBW |
| Bitwise | PAND, PANDN, POR, PXOR and PS/PD aliases |

## SSA and native Wasm

`PackedOp` is an explicit typed operation enum. `VectorBinary` takes destination
and source V128 SSA values and produces a V128 result without changing FLAGS.
The native backend emits Wasm SIMD directly, using the generic WasmBuilder;
it does not call legacy JIT emitters or consult their SIMD caches.

Most operations map to one SIMD opcode. PMULHUW/PMULHW extend-multiply each
half and shuffle the high words. PMULUDQ selects even dwords, zero-extends them
and multiplies qwords. PSADBW computes unsigned byte differences, pairwise sums
and separate qword results. PMADDWD uses the signed word dot-product instruction,
including the wrapping -32768 * -32768 pair. PANDN and the floating logical
aliases reverse the Wasm andnot inputs to implement source & !destination.
No floating conversion, NaN canonicalization or MXCSR operation is involved.

## Memory and fault contract

`XmmBinary` takes a linear address, the old destination V128 and an effect token,
and returns V128 plus an effect token. Its BeforeInstruction StateMap must
contain all eight XMM values and identify the same destination SSA value.
The shared SSE guard preserves EM/#UD then TS/#NM priority before EA decoding,
including the baseline post-ModRM observation PC. As with the CPU baseline,
OSFXSR=0 and an unaligned operand do not add new faults.

A valid, permitted, same-page RAM TLB entry uses v128.load and native SIMD, then
continues with the result in SSA. Read-only pages remain eligible for reads.
User mode cannot reuse a supervisor-only TLB entry. Page crossings, cold entries,
MMIO and permission failures materialize the BeforeInstruction state and call
`ir_xmm_binary(address, register, operation_id, bytes)`; the ABI uses only integer
arguments and an integer outcome.

The adapter validates the operation ID, reads the complete source with the
existing safe_read128s primitive, then samples the destination XMM. This order
matters when a device callback changes destination state. It applies the explicit
packed semantics, writes that XMM, commits one instruction and returns outcome 4.
The backend exits without restoring its old SSA snapshot. A source #PF returns
outcome 2 without writing a result or committing the faulted instruction;
callback changes and the delivered CPU exception state remain authoritative.

The CPU's original wide read decomposition and same-page qword MMIO sign
extension remain unchanged. Device tests compare these details, observer state
and remapping behavior against the original CPU operation, rather than replacing
them with a mathematically idealized memory read. A cold successful operation
ends the current invocation at its decoded next PC; separately compiled
continuation or the interpreter resumes subsequent instructions.

## Validation

`make ir-simd-integer-tests` generates 22,080 optimized/unoptimized fixture pairs:
46 encodings, both execution defaults and address widths, every XMM destination,
all register source aliases and default/all segment overrides. Each debug/release
CPU is checked against the emitted modules and an independent BigInt lane model.

Per build, the suite passes:

- 32,384 ordinary scenarios, including 20,608 warmed native RAM entries with no
  slow adapter calls, and 5,888 independent boundary/random/alias scenarios.
- 552 EM/TS priority cases, 460 unaligned/boundary/fault cases, 230 segment faults,
  276 OSFXSR/real/VM86 cases and 184 warmed page-permission cases.
- 184 MMIO/wide-read cases, 46 callback state mutations, 46 late source faults,
  46 pre-EA exception observations and 92 callback remap/destination sampling cases.
- Eight packed SSA/slow-completion/resume chains and two dirty-XMM fault exits.

Comparisons include all XMM and GPR state, architectural and raw/lazy FLAGS,
PCs, segment state, exception frames, memory, callback observations and completed
instruction counts. Invalid operation IDs, destination/state mismatches and
unsupported MMX/LOCK forms are rejected. The independent model covers signed and
unsigned saturation, byte/word boundaries, 64-bit carries, multiplication
truncation, PMADDWD overflow, SAD grouping and exact logical payload bits.

The original evidence above covers this arithmetic family. Additional packed
shifts and packing/unpacking are now implemented; general shuffles,
MMX/x87 aliasing, floating arithmetic and controls, F80, SIMD feature fallback,
full MIR/regions, online Tier integration and XP/performance acceptance still
require implementation and validation.
