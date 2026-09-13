# Experimental MASKMOVDQU

The XMM register form `66 0F F7 /r` adds two address-size catalogue forms,
bringing experimental CpuSimdHIR to 450. The ModRM reg field supplies data;
the r/m field supplies the byte mask. The destination is the segment-prefixed
DS:(E)DI address. The memory ModRM form remains BaselineUD; MMX MASKMOVQ remains
outside this increment. Production Pending is still 3,728.

## Ordering and access contract

The pinned CPU first performs the SSE guard, resolves the segment and initial
DI/EDI offset, and checks that the **entire sixteen-byte range is writable**.
This preflight applies even when the mask is zero or only selects bytes in the
first page. After preflight completes, it samples both XMM registers once.
It then writes each selected byte in increasing address order. Only bit 7 of
each mask byte controls selection. A 16-bit address size truncates initial DI;
it does not wrap each successive byte at the 64 KiB boundary.

The ordered XmmMaskedStore node carries the linear address, source V128, mask
V128 and effect. Its BeforeInstruction StateMap must identify both vectors, and
an explicit successful commit map advances the count exactly once. The verifier
checks registers, types, matching state values, commit identity/count and the
terminal-region boundary.

On ordinary writable RAM within one page, the emitter checks the same complete
range and uses a native byte sign mask plus conditional byte stores. There are
no runtime helper calls or old JIT emitters on this path. All guest SSA values
remain available until the terminal committed state is materialized.

Cold, cross-page, MMIO, protected and code-page accesses enter
`ir_xmm_masked_store(address, source_register, mask_register)`. It calls the CPU
write preflight before sampling source/mask, then performs the selected byte
writes with safe_write8. Callback changes during preflight therefore affect the
sampled data and mask. Write callbacks cannot change the already captured source,
mask or linear address, although their changes to CPU registers survive the exit.
Success commits once and returns 4; preflight faults return 2 without a commit.
A write fault after preflight retains the pinned CPU's partial writes, exception
state and unwrap-abort behavior. The backend must not restore an old SSA map over
any CPU-owned completion, callback change or fault state.

The existing legacy masked-store emitter rereads CPU bytes while issuing writes;
it is not an unconditional oracle for mutating MMIO callbacks. This increment
follows the pinned interpreter's single sampling after preflight, without changing
the production legacy emitter. No floating-point arithmetic, FLAGS update or MMX
transition is introduced.

## Validation

`make ir-simd-masked-tests` generates 1,792 optimized/unoptimized fixture pairs:
both execution defaults, both address sizes, every source/mask alias and all
segment choices. Six representative masks produce 21,504 CPU/independent-model
scenarios per debug/release build and 21,504 warmed native entries with zero slow
calls. An additional 128 random cases compare arbitrary source and mask bits.

A focused exhaustive loop checks all 65,536 masks against a scalar byte-selection
model, the CPU interpreter and both IR optimization settings. Full visible state,
memory output, instruction counts and absence of slow calls are checked.
Guard, unaligned/boundary, segment, real/VM86 and exact sixteen-byte range cases
retain full exception frames and callback state. Twenty-seven further cases check
zero/sparse/full masks across absent, read-only and supervisor following pages,
and initial DI truncation versus successive-byte address progression.

MMIO checks cover zero, single-lane, alternating and full masks at four offsets,
requiring exactly the selected byte write events in increasing address order.
A callback overwrites source, mask, EDI and unrelated state to verify captured
inputs and CPU-owned output. A later-page remap produces a partial write and the
baseline host abort. Six page-table callback cases change source/mask/EDI during
preflight and verify both success and subsequent page-fault state, including a
mask that starts at zero and becomes nonzero during the check.

Eight dirty-vector chains combine XOR, PINSRW and the masked store, then reenter
at PMOVMSKB. They check native, cold and MMIO completion; two additional faults
verify the prior changed XMM is visible during exception delivery. Comparisons
include GPR/XMM/FLAGS, PCs, control/segment state, memory, frames, events and counts.

Initial focused evidence is `build/ir-simd-masked-suite.log`; added page-table
callback evidence is `build/ir-simd-masked-walk.log`. Complete ISA and MIR, online
Tier integration/invalidation, host SIMD fallback, XP/performance acceptance and
legacy retirement remain unfinished.

The final complete regression matrix passed in one make invocation
(`build/ir-simd-masked-full-suite.log`), including 87 warnings-as-errors Rust tests,
all existing IR/independent-reference targets and the expanded twenty-four MMIO
write-event checks. Experimental Wasm compilation passed in
`build/ir-simd-masked-check.log`; catalogue attribution, normal Wasm exports and
whitespace checks also passed.
