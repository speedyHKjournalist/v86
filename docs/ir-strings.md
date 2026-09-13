# Experimental string instructions

The non-REP forms of MOVS, CMPS, STOS, LODS and SCAS now lower into native HIR
memory accesses, integer comparisons and SSA pointer updates. Byte/word/dword
operand width, address16/address32 and DF are independent. REP/REPE/REPNE now use a separate [terminal batch frontend](ir-rep.md). Non-REP INS/OUTS are covered by the separate [I/O contract](ir-io.md).
This is an incremental IR-07 implementation, not completed string coverage or a
production backend switch.

## Single-iteration access and commit contract

For instructions with a destination, resolve ES before resolving any source
segment. MOVS/CMPS/LODS use DS or the selected source override. STOS ignores source
overrides. The pinned SCAS wrapper passes ES as its fixed source-segment argument;
its shared CPU implementation therefore checks ES again, and does not validate
DS or the override. Null-segment differential tests found and corrected an initial
mistake that treated this wrapper argument as the ordinary source override.

MOVS and CMPS read source data before accessing destination data. There is no
whole-instruction destination preflight for these non-REP forms. MOVS then writes
the captured element; CMPS reads the destination and produces source-minus-dest
FLAGS. SCAS compares AL/AX/EAX against ES:(E)DI. LODS writes the selected accumulator
fragment and STOS writes the selected accumulator fragment to ES:(E)DI.

Each data access uses its own existing TLB guard and CPU slow path. A source MMIO
callback can remap or unmap the destination, including invalidating a previously
warm destination translation. The later destination access observes the new
mapping and its own fault location. Different linear pointers are not assumed to
be physically disjoint. A single MOVS element is fully read before its store,
including sub-element overlap; no bulk-copy substitution is introduced.

SI/DI or ESI/EDI advance by plus/minus the element size after successful execution.
Address16 wraps each low word while preserving the high half. ECX is unchanged.
LODS preserves untouched accumulator bits; comparisons retain all defined FLAGS
and last_op1 provenance. Pointer arithmetic does not change FLAGS.

Stores carry the old pointers in their fault map and the advanced pointers in
the successful commit map. The pure SSA updates do not become architectural
writes before the store succeeds. MOVS/STOS exit after the committing store,
including when it aliases following instruction bytes. Read-only forms can
continue inside the linear region. Earlier completed instructions remain visible
to faults/devices and are counted exactly once.

## Executable evidence

`make ir-string-tests` generates 420 fixtures, each optimized and unoptimized,
covering both code modes, all three element widths, both address widths, five
families, and default/all six segment overrides. A preceding INC establishes
live GPR/FLAGS state; a following MOV DL tests continuation without overwriting
the string instruction's comparison flags. Store forms must exit before it.

- 3,360 CPU comparisons and 1,680 confirmed native operand data paths, with
  independent DF/pointer-wrap and unchanged-ECX checks.
- 1,200 zero/equal/sign/borrow/overflow operand patterns with complete FLAGS.
- 240 source/destination MMIO observation comparisons and 96 source-callback
  destination remaps or delayed destination faults.
- 264 source/destination/read-only #PF cases; simultaneous missing operands
  independently verify source-first fault priority and exact CR2.
- 2,040 null-segment cases, including ES precedence and ignored SCAS overrides.
- 720 page/address16 boundary and pointer-wrap cases, plus 112 real ring3
  second-page read/write faults through a separate kernel TSS stack.
- 72 physical-alias/overlap MOVS cases and 48 writes aliasing following code.
- 120 real-mode and 120 VM86 cases using real-mode-style segment bases.

Rust checks reject CPU-independent emission, LOCK and unimplemented repeat forms;
inspect segment/data effect order; and verify distinct old/advanced
pointer commit maps. CPU comparisons include GPRs, full FLAGS, last_op1, IP, CR2,
CPL, code/stack mode, segment state and memory/fault frames.

## Separate REP batches

The [bounded REP engine](ir-rep-engine.md) now supplies explicit outcomes and
element progress. The [REP HIR adapter](ir-rep.md) now materializes progress and
accounts for final completion; online scheduling remains pending.

The shared CPU string implementation has additional behavior that cannot be
obtained by blindly looping this single-iteration region. It checks zero count
before segments; its address32 fast path validates destination translation before
source translation; it batches to page boundaries and can re-enter at previous_ip.
CMPS/SCAS update comparison FLAGS at the pinned batch/termination points, which
also matters when a later iteration faults. Slow execution and unaligned MOVS
have separate re-entry behavior. These contracts are handled by the separate REP adapter and its bounded
progress/accounting tests.

The coarse catalogue records 30 CpuStringHIR forms for the non-REP capability
only. F2/F3 have their own encoding rows and are counted separately as CpuRepHelper.
Neither count claims full prefix/mode acceptance. Production Pending remains 3,728. Online scheduling,
IR regions/tiers/invalidation, complete ISA, OS/performance acceptance and legacy
emitter retirement remain incomplete.
