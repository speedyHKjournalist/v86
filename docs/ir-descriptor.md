# Experimental descriptor tables, machine-status word and INVLPG

SGDT/SIDT, LGDT/LIDT, SMSW/LMSW and INVLPG (0F 01 groups 0–4, 6 and 7) now
lower to explicit EA/segment HIR followed by named terminal CpuExit adapters.
Compound data accesses use staged CPU safe-memory primitives. This is semantic
helper coverage, not a claim of new native GuestLoad/GuestStore lowering or RAM
proof optimization. No arbitrary interpreter dispatch or old JIT emitter is used.

The HIR computes the effective offset and resolves the selected segment before
calling a memory-form adapter. Thus a null segment faults before its later CPL
check or data access. Register forms have no EA. SGDT/SIDT/LGDT/LIDT/INVLPG
register forms explicitly deliver #UD regardless of CPL; the decoder still rejects
unknown group 5. LOCK, standalone and nonterminal linear-artifact use are rejected.

## Compound operands and observations

SGDT/SIDT check all six destination bytes for write permission before any store.
They then write the two-byte limit and four-byte base using fresh safe accesses.
Each field is read from the CPU at its original write point. In particular, a
first-write MMIO callback that changes the table base affects the second write.
Operand16 stores mask the base to 24 bits and still write all four base bytes;
operand32 stores retain all 32 bits.

A fault during preflight returns ControlTransferred (2) without writes. The pinned
CPU uses unwrap on the two subsequent safe writes; if a callback invalidates the
preflight and a later store faults, exception delivery is followed by a host panic.
The adapter preserves that policy in debug and release, including completed bytes,
CPU-delivered state and no successful instruction commit. It does not silently
turn that host abort into ordinary completion or restore an old snapshot.

LGDT/LIDT require CPL zero before data reads, then read a two-byte limit and a
four-byte base. Only after both succeed are either table fields changed. The base
is masked to 24 or 32 bits according to operand size. A second-read fault leaves
the old table fields intact. No earlier translation is reused across a device
callback. The second field is at linear address +2, without address16 rewrapping.

SMSW memory writes exactly two bytes at either operand width. Register SMSW writes
the low word for operand16 and the full baseline CR0 value for operand32. LMSW
requires CPL zero before reading the word operand; it retains the CPU's low-four-bit
update, sticky protected-mode PE bit, forced ET and set_cr0 side effects. Successful
real-mode LMSW can enter protected mode. These retain v86's existing policies.

INVLPG requires CPL zero, invalidates the computed linear page (including a global
entry) and resets the CPU fetch cache through its existing semantic function.
It neither translates nor reads the target data address. Neighboring cached
translations remain valid. An unmapped or MMIO target is consequently not itself
a data-access fault; segment resolution and privilege can still fail first.

## State and completion

The BeforeInstruction map materializes prior GPR/FLAGS/last_op1 changes, prefix
instruction accounting and exact previous/next PCs. Successfully completed
adapters increment instruction_counter once and return Invalidated (4). Guest
faults return (2), and host aborts never reach the commit. Every path ends the
artifact with CPU state authoritative. Successful stores cannot continue executing
potentially overwritten instructions inside that artifact.

Existing CPU INVLPG/code-cache maintenance is retained. Online IR generation,
dependency invalidation, publication and safe reentry are still unconnected;
these helpers do not establish those lifecycle protocols.

## Verification

`make ir-descriptor-tests` generates 2,520 fixtures with a preceding INC EAX, both
default/operand/address widths, all seven groups, eight register operands,
default/all segment overrides for memory, and ignored F2/F3 prefixes. Optimized
and unoptimized Wasm are compared with the separate original instruction bodies
in debug and release CPUs. The adapters share CPU memory/exception primitives;
they do not call the old SGDT/SIDT/LGDT/LIDT whole-instruction functions.

Per build the suite passes:

- 5,040 ordinary descriptor/system-word comparisons with independent result checks.
- 280 CPL/VM86/invalid-register cases and 560 segment-before-helper failures.
- 448 page-boundary/preflight/permission cases, including INVLPG with absent data.
- 168 linear tails across address16 boundaries and 48 table-base masking cases.
- 56 full MMIO sequences, 32 callback mapping changes, 32 late faults/aborts and
  16 delayed base-field reads after a first-write callback changes the CPU table.
- 224 real-mode operations, including LMSW protected-mode entry and register #UD.
- 16 warmed/global INVLPG target invalidations with adjacent-entry retention.

State comparison includes complete GPRs, FLAGS/last_op1, instruction PCs, control
and mode state, segment state, both descriptor tables, exact operand bytes and
exception-stack bytes. Device sequences compare GPRs, FLAGS, IP and table fields
at every access. Expected unwrap panics are caught and verified, not suite failures.

The catalogue adds 56 CpuDescriptorHelper forms, including the 20 coarse explicit
invalid-register #UD forms that the opcode table does not label reg_ud. This does
not alter the catalogue's 102 BaselineUD count or 3,728 production Pending count.
Full remaining ISA/system semantics, MIR/regions, online tiers/scheduling/lifecycle,
advanced passes, XP/application/performance acceptance and legacy retirement remain
incomplete.
