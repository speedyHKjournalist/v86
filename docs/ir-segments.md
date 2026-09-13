# Experimental segment transfers and far pointer loads

MOV r/m,Sreg (8C), MOV Sreg,r/m (8E), LES/LDS/LSS/LFS/LGS now lower through
native HIR operand accesses and a terminal descriptor adapter. This extends the
cold experimental CPU path; production Tier 1/Tier 2 still use legacy emission.

## Operand and descriptor ordering

8C reads a typed I16 segment selector. A word register destination preserves its
upper half; a dword register destination zero-extends the selector. Memory forms
write only two bytes for either operand size. Register forms can continue inside
the region; memory stores use the existing successful commit exit.

8E reads only the low word of its register or memory source, regardless of operand
size. Far loads first read the complete 16/32-bit offset, then read the selector
word, then validate and load the destination segment. The target GPR is captured
as SSA data and written only after descriptor validation succeeds. This preserves
both operand fault priority and GPR/segment aliasing, including LSS with ESP.

The selector address of a far pointer is the original resolved linear address
plus the offset width in bytes. `LinearOffset` explicitly types that addition as
(LinearAddress, I32) -> LinearAddress with wrapping u32 arithmetic. It does not
repeat address16 truncation or segment resolution, and supplies no RAM permission
proof. The two GuestLoad operations have independent guards and slow paths. An
MMIO callback between them may change the selector page mapping; the second load
must observe the new mapping. No full-pointer preflight is introduced.

`ir_load_segment(selector, segment, register, value, bytes)` is a CpuExit adapter.
A zero byte width denotes 8E; widths two/four denote far-load GPR results. It
calls the pinned CPU `switch_seg`, preserves its descriptor checks, cache changes,
accessed-bit write and delivered exceptions, and updates the destination GPR only
on success. It then commits one instruction and returns Invalidated. Failures
return ControlTransferred without committing or restoring old CPU state. Caller
ownership and the pinned post-delivery descriptor-write abort follow
[the terminal helper contract](ir-system-stack.md).

Invalid selector fields (8C fields 6/7, 8E CS/6/7) and register-only far-load
encodings stop compilation. They are not newly implemented runtime #UD paths.
These subencoding constraints are finer than the current coarse catalogue and
must remain part of the pending complete acceptance matrix. State-changing
forms end the supplied linear region; standalone CPU-independent emission rejects
these instructions. Online interrupt-shadow/scheduling behavior is still pending.

## Evidence

`make ir-segment-tests` generates 1,168 fixtures in both optimized and unoptimized
forms. Each has a preceding INC to verify state and completed-prefix accounting;
8C fixtures also verify continuation or the conservative memory-store exit.

- 9,344 protected-mode comparisons and 3,264 confirmed native operand data paths.
  Modes, operand widths, all valid segment fields, all GPR destinations and four
  memory addressing forms include ESP/SIB, address16 BP+SI and FS overrides.
- 816 source/descriptor MMIO observation comparisons and 1,800 null, nonpresent,
  execute-only, out-of-range and privilege-invalid selector cases.
- 3,104 real ring3 operand faults through a separate TSS stack, including first
  offset reads, selector-tail reads and read-only selector stores. No descriptor
  adapter executes after an operand fault.
- 1,952 page/address16 boundary cases and 640 MMIO-driven selector-page remaps.
- 624 descriptor read/accessed-bit write #PF cases, preserving old GPRs and full
  delivered exception state. Expected caught baseline panic messages appear for
  the existing accessed-bit write unwrap after fault delivery.
- 612 real-mode and 1,224 VM86 comparisons with IOPL zero/three and selectors
  zero/A000/FFFF. Comparisons retain complete FLAGS, segment caches and memory.

Rust negative checks verify CPU-only use, invalid forms, the two independent
access widths, single source segment resolution and LinearOffset operand types.
The suite compares GPRs, FLAGS, last_op1, IP, CR2, CPL, stack/mode state, selector
caches, memory and device observations against the pinned CPU interpreter.

The catalogue gains four CpuMemoryHIR forms and 28 CpuStateHIR forms. CpuStateHIR
includes CPU-only selector reads and terminal descriptor changes; it is not a
claim that each instruction is a standalone native operation. Production Pending
remains 3,728. Full ISA, online tiers/regions/invalidation, OS/performance acceptance
and legacy emitter retirement remain incomplete.
