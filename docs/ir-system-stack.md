# Experimental FLAGS and segment stacks

PUSHF/POPF and PUSH/POP ES/SS/DS/FS/GS, plus PUSH CS, now use HIR operand
accesses and explicit CPU-state exit adapters. Operand size, stack size and
segment descriptor state remain independent. This extends the cold experimental
CPU frontend; it does not switch production tiers or complete system instructions.

## Native accesses and precise commit order

PUSHF assembles its value from the six arithmetic SSA bits and the system source.
The word form stores the low 16 bits. The dword form clears VM/RF using the
pinned FCFFFF image mask. Segment pushes read a typed I16 selector through
`ReadSegment`; operand32 reserves four stack bytes but still writes only two.
Untouched upper bytes are neither checked nor written. Every push has distinct
fault and commit maps and exits after its successful store.

POPF and segment pops read the full operand width using the existing RAM/MMU
path before entering their state-changing adapter. Segment POP32 therefore checks
and reads four bytes even though its selector is only the low word. Source faults
precede flags or descriptor changes and do not advance the stack pointer.

PUSHF/POPF first call the audited `ir_flags_stack_check`: VM86 with IOPL below
three delivers #GP before any stack data access. This ordinary outcome adapter
preserves cached state on success and owns fault delivery.

`ir_pop_flags` advances SP/ESP, applies the pinned `update_eflags` privilege and
reserved-bit rules, commits the instruction once, then handles pending IRQs if
IF changed from zero to one. The 16-bit form retains the old high flag bits.
`ir_pop_segment` calls `switch_seg`, returns immediately after a delivered failure,
and only on success adjusts the stack and commits. In particular, POP SS uses
the newly loaded stack width for this final adjustment, matching the baseline.
Descriptor lookup and accessed-bit writes remain CPU-owned operations.

## Terminal CPU helper ABI

`HelperAbi::CpuExit` is distinct from a normal returning helper. Its caller
materializes the BeforeInstruction map, including completed-prefix accounting,
and exposes decoded next PC before the call. The adapter returns no SSA data:

- `ControlTransferred` (2): failure/transfer state is already authoritative;
  the adapter has not committed this instruction.
- `Invalidated` (4): the adapter has committed the instruction and owns all
  resulting CPU state, including an IRQ handler entered after successful POPF.

Both outcomes exit without restoring the old snapshot. Normal, Yield and other
codes violate this terminal contract and trap rather than silently continuing.
A successful adapter increments the real guest counter once; the caller accounts
only for the prefix. This contract is for cold CPU entry outside legacy `in_jit`.

Descriptor validation requires helper-owned exceptions, no data results and the
appropriate transfer/invalidation effects. HIR verification requires the call to
be the last instruction in its block, with a matching pre-instruction terminal
map. The standalone emitter rejects this CPU-specific ABI. Normal helpers that
change cached state still require explicit reload support and remain unsupported.
The generic existing outcome ABI and its result staging continue unchanged.

This preserves the pinned descriptor-access failure behavior: a missing descriptor
page delivers #PF and returns failure; the accessed-bit system-write path can
then abort through its existing `unwrap()` after #PF delivery. The adapter does
not catch that abort or restore stale state. Expected caught baseline panic
messages appear in those differential cases. Full architectural interrupt-shadow
and online scheduling acceptance are not established by these cold-entry tests.

## Evidence

`make ir-system-stack-tests` generates 52 optimized/unoptimized fixtures, each
with a preceding INC. Push fixtures include a following INC that must not execute
after the committing store. Pop fixtures end at their state-changing helper.

- 864 protected-mode comparisons, 432 confirmed native data-memory paths and
  416 SP/ESP wrap / stack-width-change cases. Native-path claims concern operand
  data accesses; privilege and descriptor adapters may still run.
- 208 stack/descriptor MMIO observation cases and 480 null/invalid/nonpresent/
  privilege-invalid selector cases with exact #GP/#SS/#NP state comparisons.
- 104 real ring3 stack #PF cases through a separate kernel TSS stack, and 64
  POPF CPL/IOPL masking cases.
- 44 page-boundary cases distinguishing word-only PUSH from full-width POP.
- 80 descriptor #PF cases, including the pinned accessed-bit write abort after
  delivery; CPU registers, FLAGS, segment caches, memory and fault frames match.
- 156 real-mode cases and 32 VM86 cases covering permission precedence, IOPL3
  success and retained RF/VIF/VIP behavior.
- 16 immediate IF-enable IRQ deliveries, including ring3 transitions whose
  interrupt frame saves the already-advanced user ESP and completed instruction.
- 24 injected terminal-helper outcomes proving that CPU state/counters are not
  overwritten, and that Normal/invalid returns trap. Rust negative checks cover
  misplaced calls, invalid selectors, wrong resume maps and CPU-only emission.

The catalogue gains 52 CpuStackHIR forms (156 total). Production Pending remains
3,728; the full IR ISA, online runtime/tiers, OS/performance matrix and legacy
emitter retirement remain incomplete.
