# Experimental CMPXCHG8B

CMPXCHG8B (0F C7 /1 memory) now has an ordered, terminal CompareExchange8B HIR
operation. Its input is a resolved linear address and effect; its pre-instruction
StateMap supplies the implicit GPR/FLAGS state. Verification rejects subsequent
instructions, a mismatched exit, an after-instruction fault map, or an invalid
signature. Standalone, nonterminal and register-form requests remain compile
stops; the catalogue separately retains the baseline register-form #UD fact.

Operand-size prefixes do not narrow the eight-byte access or implicit registers.
Address-size and segment overrides affect EA only; the eight-byte linear tail does
not wrap at 16 bits. LOCK uses the same audited, unshared, single-owner CPU ABI as
the existing RMW family. This does not implement shared-memory/SMP atomics.

## Native RAM and CPU-owned slow path

Lowering emits a native i64 load, comparison with EDX:EAX, and either a store of
ECX:EBX or updates to EDX:EAX. Mismatch performs no memory write. The RAM guard
checks a valid TLB entry, write permission, CPL3 accessibility, absence of MMIO and
code-page flags, and an eight-byte span wholly inside one page. The guard/use is
uninterrupted. Existing scalar memory accesses now share that guard emitter.
Both success and mismatch clear ZF laziness, set raw/architectural ZF from the
comparison, commit once, and exit at decoded next PC.

Cold translations, cross-page accesses, MMIO and code-page guards call the named
ir_cmpxchg8b slow adapter. The adapter uses the existing CPU memory primitives:

1. Preflight all eight bytes for writes, before any data read. An initial #PF
   returns outcome 2 with no completed CMPXCHG8B count.
2. Read the qword, then capture EDX:EAX. A source callback can therefore change
   the comparison registers. A post-preflight read fault retains the baseline
   delivered-exception-then-unwrap-abort behavior.
3. On match, set raw ZF before reading ECX:EBX and writing the qword. Replacement
   halves are captured together before the first store callback. Stores translate
   afresh, so callback remaps and late write faults retain CPU authority. On
   mismatch, clear raw ZF and copy the loaded halves to EAX/EDX without a write.
4. Clear flags_changed.ZF only after the memory/register update completes, then
   commit once and return outcome 4. No stale StateMap is restored after a fault,
   abort or callback. Late cross-page store failures preserve the first write.

The original interpreter body is unchanged. The old JIT emitter uses an
unconditional RMW writeback, including mismatch; source inspection therefore
rules it out as an unconditional MMIO oracle for this instruction. These tests
use the pinned interpreter semantics plus independent RAM-result checks.

## ZF backing state and pinned MMIO read behavior

FlagState now retains both raw_zero and zero_is_lazy as I1 SSA sources. The latter
comes from ReadFlagChanges at entry. Lazy ALU/shift/multiply/BCD results set it;
SAHF, bit scans and POPCNT clear it. Zero-count shifts and flag-preserving
operations retain it. Both sources participate in StateMap liveness, replacement,
verification and allocation. CPU materialization uses flags_changed=zero_is_lazy<<6
and the existing synthetic last_result=1-computed_ZF, last_op_size=31; all other
arithmetic flags are materialized. The frontend maintains raw_ZF=computed_ZF when
zero_is_lazy is false. Generic standalone states may omit both backing sources.

This is observable beyond internal fields: CMPXCHG8B sets raw ZF before a store
callback and clears its changed bit afterward. With an initially materialized
ZF=0, the callback sees ZF=1; with lazy computed ZF=0, it still sees 0. A diagnostic
reproduced an IR-exit mismatch in both builds before this fix
(`build/ir-cmpxchg8b-flags-probe.log`). The permanent raw_zero.mjs suite checks
IR exit followed by interpreter CMPXCHG8B callbacks, including both lazy states.
No ZF mismatch is masked as undefined behavior.

Another pinned behavior comes from memory::read64s: a same-page non-SVGA MMIO
read sign-extends the low dword before OR-ing the high dword. If low bit 31 is set,
the resulting high half is FFFFFFFF even though both read callbacks still occur.
The cross-page safe_read64s path zero-extends the individual halves. The slow
adapter retains this distinction; native RAM reads retain all 64 actual bits.
Tests cover mismatches and matches induced by this behavior, without changing the
shared CPU memory implementation or treating it as a hardware claim.

## Evidence and remaining scope

`make ir-cmpxchg8b-tests` generates 3,584 fixtures: default/operand/address widths,
all eight EA register patterns, default/all segment overrides, no/LOCK/F2/F3
prefixes, and NOP or INC EDI preceding the instruction. Each has optimized and
unoptimized Wasm artifacts, tested against debug and release CPUs. The suite
compares registers, arithmetic/raw/lazy ZF, last_op1, PCs, exception state/frames,
written bytes, alternate mappings, callbacks and completed instruction counts.
Native entry checks count calls to the slow adapter and require zero for warmed,
ordinary RAM. TLB code-bit injection checks guard rejection; it is not an online
IR dependency-invalidation test.

Per build, the run passed 21,504 RAM/result scenarios with 21,504 native entries;
60 boundaries, 24 preflight faults, 10 segment faults, four real-mode cases,
eight code-bit guards, 20 CPL/WP cases and 48 address16 tails; 48 MMIO/lazy-ZF
sequences, 16 callback register changes, four remaps, 24 late fault/abort sequences
and 20 signed-low read cases. The complete IR suite passed 72 warnings-as-errors
Rust tests and all existing differential suites (`build/ir-cmpxchg8b-suite.log`).

The new family contributes four CpuMemoryHIR forms (622 total). Production
Pending remains 3,728. Full MIR/region lowering, online tier publication and
invalidation, remaining ISA, XP/performance acceptance and legacy retirement
remain incomplete.
