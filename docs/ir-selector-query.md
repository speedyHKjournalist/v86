# Experimental LAR/LSL descriptor queries

LAR/LSL (0F 02/03) now lower to explicit EA/segment HIR and named terminal CPU
query adapters. Sources are always words; destinations use the decoded operand
width. Register source/destination aliases and memory-source callbacks retain the
baseline read order. LOCK, standalone and nonterminal artifacts are rejected.

## Query and fault semantics

Memory-form segment resolution precedes the instruction's mode check. Real mode
and VM86 deliver #UD before reading source data. A source #PF returns without a
destination write. After a successful source read, the adapter captures the old
destination and performs descriptor lookup using the CPU selector/descriptor
primitives. The capture occurs after a source MMIO callback, as in the interpreter.

Null/out-of-table or disallowed descriptors clear ZF and retain the old destination;
successful queries set ZF and return access rights or the effective limit. The
type, DPL/RPL/CPL and conforming-code rules follow the original LAR/LSL bodies.
Present-bit handling is retained rather than imposing a new check. Other FLAGS
and last_op1 provenance retain their existing values.

Descriptor #PF differs from source #PF: the original instruction wrappers still
write the captured old destination after exception delivery. The IR adapter does
the same and then returns ControlTransferred (2) without a successful instruction
commit. This includes ESP. For operand16, the word write retains the high half of
the post-fault CPU register, not the pre-instruction high half. For example, with
old ESP=12348000 and new exception ESP=0008FFE8, the subsequent word write produces
00088000; the dword form restores 12348000. The exception stack bytes remain where
the CPU wrote them. This records a pinned v86 behavior, not a new hardware claim.

Successful and unsuccessful non-fault queries commit once and return Invalidated
(4). BeforeInstruction state supplies preceding GPR/FLAGS changes and exact PCs;
the terminal exit never restores a stale pre-call snapshot over CPU-owned results.
These are semantic query helpers, without arbitrary interpreter dispatch or old
JIT emitter calls. Their query policy is separate from the original lar/lsl bodies,
while descriptor and exception primitives are shared.

## Evidence

`make ir-selector-query-tests` generates 1,920 fixtures across default/operand/
address widths, both queries, all eight destination GPRs, all register sources
and default/all segment overrides for memory. INC EAX precedes each query to make
aliases and FLAGS provenance observable. Both optimized and unoptimized artifacts
run against debug and release CPU interpreters. Per build:

- 1,920 ordinary alias/width/address/override cases.
- 64 real/VM86 #UD cases and 320 null/out-of-table unsuccessful queries.
- 8,192 independent type/DPL/RPL/CPL/present/conforming result checks.
- 32 source #PF and 64 descriptor #PF cases with exact destination/ESP behavior.
- 80 segment-before-mode cases, 64 operand/descriptor MMIO sequences, 32
  post-source old-destination captures and 64 LDT lookups.

State comparison covers GPRs, FLAGS/last_op1, PCs, control/mode/segment state,
descriptor-table state, operand bytes and precise exception-stack bytes. Device
callbacks compare GPRs, FLAGS and IP. The independent result model uses accepted
descriptor types and explicit privilege rules rather than the adapter bit masks.

## VERR/VERW and raw ZF backing state

VERR/VERW (0F 00 /4 and /5) use four named terminal CPU adapters for word
register/memory operands. Operand-size prefixes do not widen the selector. HIR
segment resolution precedes the protected/VM86 guard; source data is read only
after that guard succeeds. Source #PF retains the architectural pre-query FLAGS.
After the source read, the adapter clears flags_changed.ZF **before** descriptor
lookup, matching the original CPU bodies. Descriptor #PF and descriptor MMIO
therefore observe the raw ZF backing bit, not the computed preceding ZF. No
post-fault destination write exists for these instructions.

The frontend now carries FlagState.raw_zero as a distinct I1 SSA source, initialized
by ReadRawFlags. SAHF, BSF/BSR and POPCNT replace it; currently supported lazy ALU,
shift, multiply, BCD and compare operations preserve it. StateMap value enumeration,
verification, replacement, DCE and allocation include this source. CPU materialization
stores raw_zero in flags.ZF and preserves whether ZF was lazy using the paired
zero_is_lazy source: last_result=1-computed_ZF, last_op_size=31,
flags_changed=zero_is_lazy<<6. [CMPXCHG8B](ir-cmpxchg8b.md) explains why the
lazy marker itself is observable. CPU getzf then returns the computed ZF;
clearing its changed bit exposes raw_zero. Other arithmetic bits are materialized,
and last_op1 provenance remains separately preserved. This is a canonical backing
representation, not an exact copy of every original lazy-result field. Standalone
ABI flags remain fully architectural; hand-built maps may omit both backing sources.

VERR requires a non-system readable segment, with privilege checks except for
conforming executable segments. VERW requires a non-system writable data segment
and both CPL/RPL privilege checks. The pinned present-bit behavior is retained.
Invalid selectors/descriptors clear ZF without fault; valid ones set it. Both
non-fault results commit once and exit with outcome 4. Delivered faults return 2
without a successful query commit or stale restoration. Original CPU verr/verw
bodies are unchanged and serve as the interpreter oracle.

`make ir-verr-tests` generates 720 ordinary fixtures across mode/operand/address
widths, both operations, eight register sources, default/all segment overrides and
F2/F3 prefixes. Per debug/release build, optimized and unoptimized IR pass:

- 720 ordinary cases, 32 mode checks and 160 invalid selectors.
- 8,192 independent type/DPL/RPL/CPL/present/permission checks.
- 16 source faults, 64 descriptor faults with raw ZF in the exception frame,
  and 80 segment-before-mode checks.
- 64 operand/descriptor MMIO raw-ZF observations and 32 LDT queries.
- 8,640 transitions using 36 prefix sequences, initial raw/lazy ZF combinations,
  five operand seeds and both VERR/VERW. These compare fused IR, IR-to-interpreter
  exits, and IR-to-IR-to-interpreter exits, including a CPU slow-load callback.
  Arithmetic FLAGS, raw ZF, last_op1, registers, PCs, callbacks and exception frames
  are checked without a ZF mask. Raw writers followed by lazy INC exercise source
  replacement and optimized StateMap liveness.

The original eight-case interpreter-only diagnostic remains available through
`make ir-flags-observer-tests`. It records computed ZF=0/raw ZF=1 saving 1, and
computed ZF=1/raw ZF=0 saving 0, in debug and release builds.

The catalogue now contains 32 CpuSelectorQueryHelper forms (16 LAR/LSL plus
16 VERR/VERW). Production Pending remains 3,728. Remaining ISA/system semantics,
full MIR/regions, online tiers/lifecycle, advanced passes, XP/performance acceptance
and legacy retirement remain open.
