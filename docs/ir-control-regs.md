# Experimental control/debug register transfers

MOV from/to CR and DR (0F 20–23) now lower to four named terminal CpuExit helpers.
The HIR supplies explicit GPR and control/debug-register indices. Every transfer
uses the full 32-bit GPR, independently of the instruction's 16/32-bit default or
66 prefix. ModRM.mod is ignored exactly as in the opcode catalogue: there is no
SIB, displacement, segment resolution or data operand memory access, even for
ModRM bytes ordinarily interpreted as memory operands. The eight catalogue forms
retain this fixed register-only classification and independent address sizes.

LOCK, standalone execution and a following instruction in the same linear
artifact are rejected. The BeforeInstruction map materializes all preceding
GPR/FLAGS/last_op1 updates, completed prefix accounting and exact previous/next
PCs. All calls are specific audited semantic operations; no arbitrary interpreter
dispatch or legacy emitter is used.

## Faults, commits and CPU authority

CPL zero is checked before register validity or alias checks. Failed permission
delivers #GP(0) once and returns ControlTransferred (2). Invalid CR indices retain
the CPU body's debug abort and release #UD behavior. DR4/DR5 alias DR6/DR7 unless
CR4.DE is set, in which case the CPU body delivers #UD. CR4 reserved-bit writes
retain the body's #GP guard. All delivered-fault cases return (2) without a
successful instruction commit; no EIP comparison is used to infer success.

Successful bodies increment instruction_counter once and return Invalidated (4).
The artifact exits without restoring stale GPR, control/debug state, translation
state or a pre-call snapshot. Read destinations can include ESP and the preceding
INC's destination; writes observe the materialized full-width source value.

CR0/CR3/CR4 preserve existing set_cr0/set_cr3/PDPTE/TLB behavior. CR0 forces ET,
updates protected_mode and the baseline CS access byte, and flushes on its existing
PG/WP changes. CR3 preserves the baseline low-bit handling and global-TLB policy.
CR4 retains the exact existing flush and PDPTE-reload conditions. These are CPU
policies, not a replacement architectural model or expanded CPU feature support.

The following existing host-abort policies are preserved:

- CR0 PG without PE panics in both debug and release before storing CR0.
- Non-PAE CR3 cache-control low bits survive masking and trigger a debug assertion;
  release retains them and performs the existing clear_tlb path.
- PDPTE loading can update earlier cached entries before a later debug assertion.
  CR0 may already be stored, while CR3/CR4 assignments following the load have not
  occurred. An abort never reaches the adapter's final instruction commit.
- PDPTE high/reserved bits keep their debug assertions and release behavior;
  ignored bits are masked at the original point. Absent-entry conditional checks
  are also preserved.

PDPTE loads read physical memory, including MMIO, in CPU order. Each callback sees
the CPU state and already-loaded entries at that point. No guest translation proof
is substituted for these physical accesses. Existing CPU TLB maintenance does
not connect the experimental IR dependency graph, async generation validation or
online publication/invalidation protocol.

## Evidence

`make ir-control-regs-tests` generates 5,120 fixtures and both optimized/unoptimized
artifacts. Every GPR, every CR/DR index, both default modes, all four ModRM.mod
values and seven prefix combinations are included. All mod values use the empty
prefix; additional prefixes use mod=3. Decoder assertions verify no EA or extra
bytes are consumed. The initial INC EBX makes FLAGS and source/destination overlap
observable. Rust negative checks reject LOCK, standalone and nonterminal use.

Each debug/release CPU suite passes:

- 7,680 ordinary full-width transfers, with independent read/DR-write assertions.
- 1,792 CPL/VM86 cases, 64 DR4/5 DE faults and 128 invalid CR abort/#UD cases.
- 64 individual CR4 valid/reserved-bit cases and 168 real-mode transfers.
- 14 warmed TLB checks: CR3 new roots/global mappings, CR4 flush/retention and
  CR0 WP/PG changes. Independent expected data values distinguish old, new and
  untranslated physical mappings.
- 60 RAM/MMIO PDPTE reloads through CR0/CR3/CR4, including each partial-abort slot.
- 96 present/absent PDPTE reserved, ignored and address-bit cases.
- 18 CR0 PG-without-PE and non-PAE CR3 low-bit cases.
- Four subsequent instruction-fetch #PF cases after CR3 successfully commits
  a new address space that no longer maps the old instruction page.

State comparison covers GPRs, FLAGS/last_op1, instruction/previous PCs, CR/DR,
all four PDPTE entries, protected mode, CPL, execution widths, cached state flags,
segment state and exact exception-stack bytes. PDPTE device reads compare complete
callback state and order. Expected host panics are caught and checked; their log
messages are not failures. Most comparisons share the CPU semantic body and prove
integration consistency; targeted independent assertions establish the stated
masking, state-transition and mapping expectations.

Production Pending remains 3,728. Remaining ISA/system operations, STI shadow,
full MIR/regions, online tiers/scheduling/publication/invalidation, advanced passes,
XP/application/performance acceptance and legacy retirement remain incomplete.
