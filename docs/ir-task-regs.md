# Experimental task and local-descriptor registers

SLDT/STR and LLDT/LTR (0F 00 groups 0–3) now lower to explicit EA/segment HIR
and named terminal CpuExit adapters. Memory operands use CPU safe-memory
primitives; this is semantic-helper coverage. Register sources/destinations have
explicit indices and widths. LOCK, standalone and nonterminal use are rejected.
VERR/VERW are implemented separately in [selector queries](ir-selector-query.md);
full task switching remains open.

## Permission, data and state order

Real mode and VM86 deliver #UD before privilege or operand data checks. In protected
mode, LLDT/LTR require CPL zero; SLDT/STR do not. Memory-form segment resolution
occurs before these instruction-body checks, retaining the interpreter's ordering.
Permission and operand faults are dispatched once and return ControlTransferred
(2), without committing the failed instruction.

SLDT/STR register forms write a word for operand16 and a zero-extended selector
for operand32. Memory forms always write two bytes. LLDT/LTR always read the word
source, then perform the existing descriptor lookup. Prefix operand size does not
change that source width. The preceding instruction's GPR and FLAGS changes are
materialized before the helper, including source/destination overlap with INC EAX.

LLDT preserves its null-selector behavior and descriptor validation policy.
Successful loads update LDTR selector, base and effective limit. LTR preserves
available 16/32-bit TSS handling, changes the cached TSS width and TR fields, then
translates and writes the descriptor busy byte. The busy write can use a mapping
different from the descriptor read after an MMIO callback remaps the page.

The CPU's load_tr now delegates to load_tr_checked and discards its result, keeping
the existing void caller ABI. The checked core replaces only the early read-fault
return with a Result and adds Ok on completion. Descriptor validation, TR cache
updates and the busy-write unwrap retain their original order. IR uses the explicit
result to distinguish read faults from completion. A busy-write fault occurs after
the TR updates, dispatches #PF and then host-aborts through unwrap; the new TR state
is preserved and the adapter never commits the instruction as complete.

Null/out-of-table LTR, invalid descriptor type/presence and other pinned validation
panics remain host aborts, not newly synthesized hardware faults. TI selectors
retain the debug GDT assertion and release LDT lookup behavior. Descriptor reads
retain the baseline single first-address translation followed by a physical
eight-byte read. With an unaligned table base, the physical descriptor tail and
the separately translated busy-byte write can therefore reach different pages.
These are explicit v86 policies, not expanded task-switch or architectural support.

All successful adapters commit once and return Invalidated (4). Exit preserves
CPU-owned TR/LDTR, descriptor writes and exception state; no old snapshot is
restored after the helper. Online IR scheduling, publication and invalidation
remain unconnected.

## Independent reference and evidence

`tests/ir/differential/build_task_reference.py` builds isolated debug/release CPUs.
Their interpreter load_tr and load_ldt bodies come from fixed commit
`8ee73e538daaab15411344d39a1f271e778ac7f3`; IR adapters and checked/current bodies
come from the worktree. The reference is built without editing the checkout.
`build/task-reference.json` records the source, function hashes, Wasm hashes and
scope. Other CPU infrastructure remains shared/current; this is not a complete
historical CPU binary or an independent hardware specification.

`make ir-task-regs-tests` generates 1,440 fixtures with both default/operand/address
widths, all four groups, eight GPR operands, default/all memory segment overrides,
and ignored F2/F3 prefixes. Each debug/release reference CPU checks both optimized
and unoptimized IR artifacts. Per build:

- 1,440 ordinary width/operand/prefix and cache/busy-bit comparisons.
- 128 mode/privilege cases, 128 operand faults and 160 segment-before-mode cases.
- 160 null/outside-table policies and a 640-case descriptor type/presence/system/
  granularity matrix across all default/operand/address widths. Each width slice
  runs in a fresh CPU; the main suite also repeats the canonical 80-case subset.
- Eight pinned TI-selector assertion/release lookup cases.
- 32 complete operand/descriptor/busy-byte MMIO sequences.
- 48 descriptor read/readonly cases and 16 busy-write #PF/host-abort cases that
  verify already-updated TR state.
- 32 descriptor callback remaps and 32 physical-tail/separate busy-write mappings.

State comparison includes all GPRs, FLAGS/last_op1, instruction PCs, CR/mode state,
all eight segment selectors/bases/limits, TSS width, descriptor bytes, operand bytes
and exception-stack bytes. MMIO observations include TR/LDTR caches at every access.
Independent assertions check selector widths, cache fields, effective limits,
TSS width, busy bytes and fault/partial-state outcomes. Expected validation/unwrap
panics are caught and checked rather than counted as test failures. Repeated
terminal host aborts are split across fresh CPU instances: a panic does not provide
a reusable, unwound Wasm activation. The complete attribute matrix is retained.

The catalogue adds 32 CpuTaskRegHelper forms; production Pending remains 3,728.
Remaining ISA/system semantics, full MIR/regions, online tiers/lifecycle, advanced
passes, XP/performance acceptance and legacy retirement remain incomplete.

## Isolating expected host traps

A later compiler build exposed stack exhaustion in the combined null/type matrix.
Measuring the test reference's exported `__stack_pointer` showed that caught host
panics retained their linear-memory frames: the probe observed losses of 112,
12,544, 112, 128, 12,560 and 128 bytes in six consecutive calls. A fresh CPU for
only each matrix shard was insufficient when compiler-generated frames grew.

The reference builder now exports that mutable global only in its dedicated test
artifacts and records it in `build/task-reference.json`. After a caught Wasm
RuntimeError, the harness restores the host stack to its value before that ended
activation, before beginning the next independent comparison. Normal returns must
already have the same stack pointer. CPU registers, guest memory, fault frames,
callback observations and partial completion remain available for their original
checks; no production helper, fault behavior or state-reset policy is changed.
The pinned load_tr/load_ldt body hashes remain unchanged.

The diagnostic is `build/ir-task-stack-probe.log`. The repaired complete suite
passes in `build/ir-task-stack-fixed.log`: 582 debug and 558 release stack repairs
in the main matrix, and 222 per build in each type shard, with observed maximum
frames of 24,400 debug / 24,640 release bytes in the main matrix. Fresh CPU shards
are retained in addition to per-activation stack isolation.
