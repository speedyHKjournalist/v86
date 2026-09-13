# Experimental ENTER contract

ENTER (C8 iw ib) is lowered to native SSA stack/frame arithmetic and the existing
CPU memory path, in 16/32-bit operand and stack modes. The second immediate is
masked with 31 and the bounded access sequence is unrolled. Address and segment
prefixes do not change SS stack/frame addressing. This is a cold experimental
artifact; production tiers and the complete IR goal remain unfinished.

## Pinned baseline ordering

The implementation follows `src/rust/cpu/misc_instr.rs::enter16/enter32`, including
its unusual ordering. `frame_temp` is the full original ESP minus the operand
byte width, even for a 16-bit stack. If nesting is nonzero, frame-chain reads and
pushes occur first, followed by a push of `frame_temp`. The final write stores the
old BP/EBP at `frame_temp` (masked for SS16 addressing), potentially overwriting
an earlier nested push. Only then does BP/EBP become the new frame pointer and
the current stack pointer decrease by the allocation size plus one operand.

Each successful nested push advances ESP/SP before the next access. Each access
has its own pre-access StateMap: later faults and MMIO callbacks observe exactly
that progress. Frame-chain pointer arithmetic uses a private SSA temporary;
guest BP is not changed during the chain walk. No permission preflight or access
is introduced for the allocation range: the allocation itself is pointer arithmetic.
Every later memory access rechecks current translation, including after a device
callback remaps a page. Source/frame aliasing follows the actual access order.

Nested writes use `PartialStore`; the final `GuestStore` alone commits and exits.
The verifier now permits same-instruction `GuestLoad` between partial writes,
while retaining instruction identity, effect order and mandatory final commit
checks. It still rejects helpers, polls, unrelated ordered operations and crossing
the instruction boundary. This also allows ENTER to finish after writing through
its own code alias, while exiting before the next potentially overwritten opcode.

## Explicit baseline compatibility policies

Two baseline behaviors are represented explicitly instead of hidden in an
instruction interpreter helper:

1. Nested reads/pushes use `unwrap()` in the pinned CPU. A page fault is delivered
   before this aborts the host Wasm call. The HIR instruction's `trap_after_fault`
   policy preserves that abort after the ordinary memory adapter has delivered
   the fault. It does not redeliver or restore stale state. The final write uses
   ordinary fault exit. The verifier limits this policy to GuestLoad/PartialStore.
   The contract preserves the abort and CPU state, not the Rust panic message.
2. ENTER16's frame push passes the full untruncated frame pointer to `safe_write16`.
   RAM stores its low word, but same-page MMIO can observe upper bits in the
   high-byte callback value. `unmasked_word_store` is restricted to a PartialStore
   of two bytes with an I32 payload. The native store truncates to a word; the
   narrow `ir_memory_write_unmasked_word` adapter preserves the baseline device
   payload and fault order. Other word stores retain their canonical I16 data.

Both policy fields survive optimization and appear in HIR dumps. No optimizer
currently reorders or eliminates these effectful memory operations.

The pinned debug CPU asserts the raw frame-push value is already a word, so it
can abort even for otherwise successful high-ESP ENTER16. The complete ENTER
oracle therefore uses `build/v86-ir-test-release.wasm`, matching production CPU
semantics. This does not establish debug-assertion equivalence for native RAM
ENTER16. The production release module remains separate and exports no test hooks.
Fixing these pre-existing CPU behaviors would require an explicit baseline change;
the IR migration does not silently apply one.

## Verification

`make ir-enter-tests` builds the dedicated release CPU oracle and generates 200
fixtures, each with optimized/unoptimized IR, a preceding INC and a following INC.
The following instruction must not execute after the committing ENTER store.

- 1,600 ordinary comparisons against exact CPU steps and an independent pointer
  model; 800 warm native paths with no slow data-memory imports.
- 384 SP/BP wrap and overlapping frame cases, including high halves of SS16
  registers, and 16 self-alias exits before a modified following instruction.
- 192 device read/write state comparisons and 32 callback-driven mapping changes.
- 48 real ring3 faults through a separate TSS kernel stack: initial/final writes,
  first/later chain reads and pushes, and a final write fault after a device remap.
  Saved ESP is checked against the exact number of completed pushes. Thirty-two
  cases also compare the pinned post-delivery host abort.
- Rust checks all 256 raw nesting immediates, fault-policy restrictions, invalid
  unmasked payloads and reads illegally crossing a partial instruction boundary.

Expected panic diagnostics in the test log come from the caught baseline unwrap
faults; the test still requires the exact fault frame and final CPU state. The
catalogue gains four CpuStackHIR forms (104 total). Production Pending is 3,728.
FLAGS/segment stack operations, the rest of the guest ISA, online IR tiers and
OS/performance acceptance remain incomplete.
