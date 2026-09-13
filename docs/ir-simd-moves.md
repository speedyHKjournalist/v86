# Experimental XMM SSA and SIMD transfers

IR now supports XMM values as V128 SSA, typed Wasm locals, edge copies and CPU
StateMaps. The first implemented family is MOVUPS/MOVUPD, MOVAPS/MOVAPD,
MOVDQA/MOVDQU and MOVSS/MOVSD, each in load/store register and memory forms.
These are 16 encoding records and 64 coarse experimental CpuSimdHIR forms.

## Vector state and scalar lanes

The frontend initializes all eight XMM mappings when it first encounters an XMM
operation. Earlier integer-only snapshots have no cached vector state. Register
moves and successful native RAM loads remain SSA; vectors are written to CPU
state only at observations, faults and exits. V128 values participate in state
liveness, local reuse, replacement, DCE and block-edge parallel copies. The
backend uses existing WasmBuilder SIMD encoding, without old JitContext SIMD
caches or per-operation scratch memory.

Packed register transfers copy all 128 bits. Register MOVSS replaces only the
low dword and MOVSD replaces only the low qword, preserving the destination high
bits. Memory MOVSS/MOVSD loads zero the remaining high bits. Stores write only
their 4/8/16-byte payload. Integer lane extract/replace operations carry explicit
width and lane bounds; no float conversion touches NaN payloads or signed zero.

Verifier and lowering reject partial/incorrect XMM StateMaps, invalid vector
lane types, non-CPU XMM layouts and unadapted V128 helper signatures. V128 never
crosses the JS helper ABI. Generic vector helper calls remain rejected until an
explicit non-JS or scratch ABI is supplied. F80 state lowering remains unsupported.

## Guard order and memory paths

Every covered instruction checks CR0.EM (#UD), then CR0.TS (#NM), before effective
address/segment resolution. The guard StateMap's decoded PC ends immediately
after ModRM, matching the interpreter before SIB/displacement reads. Memory
observers see the full decoded next PC. CR4.OSFXSR=0 retains the pinned CPU's
no-fault behavior. The original CPU functions do not implement the documented
alignment #GP for MOVAPS/MOVAPD/MOVDQA; the IR deliberately preserves their
unaligned behavior rather than adding a new check.

XmmLoad returns a V128 SSA value on a valid, permitted, same-page RAM TLB path.
It uses v128.load, load32_zero or load64_zero directly. Cold translations,
MMIO and page crossings materialize the pre-instruction state and call
ir_xmm_load. The CPU adapter reads the full value before updating the destination,
commits that instruction once, and exits at its next PC (outcome 4). A delivered
fault returns 2 without a successful instruction commit. Following instructions
in the compiled region are not executed after this cold completion exit; a new
entry or interpreter continuation starts at the committed next PC. This retains
callback-modified GPR/XMM state without continuing with stale SSA.

XmmStore has a mandatory completed StateMap and ends the current region. Ordinary
RAM uses a native vector or scalar-lane store, then materializes that commit.
The store guard additionally excludes read-only and code-marked TLB entries.
The CPU slow adapter captures the source vector before writing and preserves
callbacks and CPU-owned fault state; it never restores a stale commit map.
Existing safe_write64/128 preflight, staged writes and late unwrap-abort behavior
are retained, including an already completed first write.

CPU wide-read primitives are unchanged. Their same-page qword MMIO sign extension
and cross-page decomposition can affect MOVSD and some crossing 128-bit loads;
these pinned differences are compared against the interpreter, not masked out.

## Evidence and remaining IR-08 work

`make ir-simd-move-tests` generates 7,680 optimized/unoptimized fixture pairs over
both execution defaults and address sizes, all move encodings, all XMM register
aliases, and default/all segment overrides for memory. Operand patterns include
zeros, signed zero, infinities, NaN payloads and denormals as exact raw bits.

Per debug/release CPU, the initial matrix checks 11,264 independent/CPU scenarios
and requires zero slow-adapter calls in 7,168 warmed native RAM entries. Further
checks cover 192 EM/TS priority cases, 160 unaligned/boundary/fault cases, 80 null
segments, 96 OSFXSR/real/VM86 cases, 64 MMIO/wide reads, 16 callback state changes,
14 partial faults/aborts and 16 pre-EA exception observations. Vector chains check
SSA retention, cold completion exits and resumed stores. Coalesced V128 parameters
exercise parallel edge-copy cycles; a source #PF after two register transfers
checks that dirty vector values are materialized before CPU exception delivery.

This is the first IR-08 instruction family, not full MMX/SSE/x87 support. Vector
arithmetic, floating-point controls/exceptions, MMX/x87 aliasing, F80 helpers,
SIMD capability selection/fallback, full MIR/regions and online runtime integration
remain incomplete. No XP compatibility or performance claim follows from these
transfer tests; production Pending remains 3,728.


Packed integer arithmetic and PS/PD bitwise aliases have subsequently been added;
see [ir-simd-integer.md](ir-simd-integer.md) for their separate coverage and tests.
