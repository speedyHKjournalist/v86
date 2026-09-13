# Experimental stack lowering

The cold CPU IR frontend supports register and immediate PUSH, register POP,
FF /6 register/memory PUSH, 8F /0 register/memory POP, PUSHA/POPA and LEAVE.
This is an IR-06 subset; ENTER, flags/segment stack forms, interrupts and online
Tier integration are still pending. Near CALL/RET now uses the same stack path;
see [ir-control.md](ir-control.md). Experimental coverage labels 100 coarse forms
`CpuStackHIR`; production coverage remains Pending.

The frontend builds native SSA pointer arithmetic and uses the existing guarded
RAM / exact MMU-MMIO memory path. `ReadStack32` reads the CPU's stack-width input,
independently of decoded operand and effective-address widths. A 16-bit stack
wraps SP while retaining ESP's high half. Effective stack offsets are masked
before adding SS; a 32-bit stack uses the full pointer. Address-size prefixes
apply to explicit memory operands, not to the stack-width input.

PUSH captures the source before decrementing the pointer, including PUSH ESP and
memory operands using ESP. Its fault map retains the old pointer; only the store's
success commit map contains the new pointer. Memory-source reads occur before
stack writes, so source faults and device side effects have baseline ordering.

POP register reads from the old stack pointer. The decoded destination is then
written after the appropriate adjustment. The pinned baseline has a specific
encoding distinction: opcode 5C reads directly into SP/ESP without the intermediate
increment, whereas 8F /0 register uses pop16/pop32 followed by a register write.
For operand16 with stack32, a carry out of bit 15 therefore changes the preserved
high half for 8F /0 but not 5C. Both encodings are tested across that boundary.
This preserves the fixed repository baseline; it is not a new architectural claim.

POP memory resolves its effective address using the temporarily incremented
pointer, then reads the old stack and writes the destination before committing
ESP. Device reads/writes observe old ESP. A separate `PopAddress` operation models
the baseline's null-segment path: its fault map contains the temporary ESP, and
`ir_pop_address` applies the baseline's inverse stack adjustment even after #GP
has been delivered. This deliberately matches `instr16_8F_0_mem` and
`instr32_8F_0_mem` rather than silently changing their exception-frame behavior.
The already-delivered exception exits without restoring a stale snapshot.

As with other current stores, PUSH and memory POP end the region after a successful
write. A register POP can continue through the region. All state maps participate
in verification, DCE roots and local liveness; no interpreter helper performs the
normal stack pointer arithmetic or register update.

## Multiple stack accesses

PUSHA and POPA first issue ordered `GuestCheck` for the baseline's contiguous
16/32-byte range. The adapter uses `readable_or_pagefault` or
`writable_or_pagefault`, preserving the first/last page lookup and CR2, page-table
side effects and CPU-owned fault delivery. It performs no data read or device
callback. This check is not a reusable translation proof: each later access runs
its own RAM guard or slow translation, including after MMIO remaps a page.

PUSHA preserves its original SP/ESP value for the saved-pointer slot. Each of its
first seven writes uses `PartialStore`, with a StateMap describing progress before
that access. The final `GuestStore` commits the whole instruction and exits.
Intermediate successful writes do not exit, even if they invalidate code; only
operations of that same decoded instruction can follow them. The verifier rejects
an unfinished sequence, a different guest PC/count, an intervening helper/load or
a mismatched final commit. No subsequent guest instruction executes after PUSHA's
writes. This is a multi-access instruction boundary, not a transaction rollback.

POPA updates registers and the pointer after each successful read, so subsequent
device callbacks observe the baseline's partial register state. It skips the
saved SP slot without a read. LEAVE uses SS width to select BP/EBP for its stack
address, retains old ESP/EBP on access failure, and commits both only after reading.
Neither operation delegates register arithmetic to a CPU instruction helper.

## Verification

`make ir-stack-tests` generates 120 fixtures and compares actual CPU execution
against exact interpreter steps:

- 960 normal cases across 16/32-bit decoding, operand widths, stack widths,
  optimized/unoptimized IR and cold/warm translation.
- 480 instrumented warm paths with no slow data-memory calls; PUSHA/POPA still
  call the permission preflight adapter.
- 128 SP-wrap/high-ESP cases, including PUSH ESP and both POP SP encodings.
- 100 real fault cases covering source/destination/stack #PF, null-segment POP
  resolution, and ring3 PUSH write faults through a real TSS onto a separate
  kernel stack. Fault frames, GPRs, EIP, FLAGS, CPL and memory are compared.
- 128 MMIO comparisons covering callback order, value, FLAGS, EIP and old-ESP
  observation, including POP [ESP]'s post-increment destination address.

Additional multi-stack checks cover 72 real preflight/LEAVE faults (including
ring3 PUSHA first/last-page permission failures before any device write), 16
skipped-SP device cases, 144 stack wrap/high-half cases, 8 self-alias PUSHA exits,
and 32 device remaps of later stack accesses. MMIO comparisons include all GPRs.
Negative verifier tests reject incomplete or mismatched partial-store sequences.

The fixed 5C/8F high-half difference was exposed by the differential matrix and
is now preserved explicitly. Broader stack/control instructions and complete
privilege/segment/task-switch coverage remain part of the full implementation plan.


## ENTER extension

ENTER now uses the same memory path with interleaved frame-chain loads and
partial stores. Its pinned ordering, release-only oracle, post-fault host abort
policy and unmasked word device payload are documented separately in
[ir-enter.md](ir-enter.md). Historical ENTER-pending statements above describe
the earlier PUSH/POP/PUSHA/POPA/LEAVE milestone.


## FLAGS and segment stacks

PUSHF/POPF and segment PUSH/POP are now supported under the separate
[system stack contract](ir-system-stack.md). Their privilege checks, asymmetric
word/dword accesses and state-changing exits are tested against real CPU steps.
Earlier pending statements describe prior milestones, not the current catalogue.
