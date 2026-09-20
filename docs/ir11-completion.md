# IR-11 completion: proof-based memory and loop optimization

IR-11 is complete within the boundary defined by
[`v86-ir-implementation-plan.md`](v86-ir-implementation-plan.md): proof-based
address reuse, controlled LICM/forwarding/SIMD optimization, and negative tests
that preserve exception and callback order. This does **not** complete IR-00–IR-14,
change the production default from legacy, claim full ISA coverage, or retire the
legacy emitter.

## Completed optimization boundary

IR-11 now consists of five conservative layers:

1. **Pure SSA LICM.** Natural loops, multiple latches and nested loops use the
   existing transactional LICM pass. Only total SSA calculations move. CPU
   observations, memory, helpers, checks and recovery-bearing operations do not.
2. **Integer SIMD simplification.** Exact SSA/byte-provenance peepholes cover the
   bounded shuffle, identity and lane cases documented in
   [`ir-simd-peephole.md`](ir-simd-peephole.md).
3. **Proof-based scalar RAM forwarding.** Owned MIR classifies relevant address
   pairs as `Exact`, `Disjoint` or `MayAlias`. Exact identity can forward.
   A committed native store with a proven-disjoint constant byte range may stay
   between a source load and its later reuse. Any unproved relation is
   `MayAlias` and remains a barrier.
4. **Fault-preserving memory LICM.** A loop-invariant ordinary scalar load is
   cached across loop iterations without moving its first dynamic access. The
   original load/guard/fault point remains in the loop; only a successful native
   RAM access sets the cache-valid bit. A unique unconditional preheader clears
   the cache on every loop entry.
5. **RAM guard reuse.** Within a block, an exact canonical address may reuse a
   preceding successful translation/range/permission proof when its width is no
   larger and required permissions are covered. Data loads/stores still execute.
   This separate certificate excludes value-forwarded/loop-cached accesses,
   RMW tickets, vector memory and intervening unknown effects. Slow paths clear
   validity before callbacks; no proof survives a new entry invocation.

The fourth rule deliberately differs from textbook physical load hoisting.
Moving a possibly faulting x86 load into a preheader could introduce a #PF/#GP
on a zero-trip or previously non-faulting path. Keeping the first access in place
while reusing its value on later iterations obtains the loop-invariant reuse
without changing the first observable fault point.

## MIR proof ownership

The lowering boundary retains the defining block and defining instruction of
each SSA value as owned MIR provenance. HIR is still dropped before machine
optimization/emission; proof construction resolves `ValueId` through that owned
definition map rather than indexing the instruction-plan arena directly.

The RAM optimizer derives deterministic certificates from:

- the owned dispatcher CFG and its predecessor/dominator relation;
- canonical segment-address plans;
- value-definition provenance and pure machine value programs;
- scalar RAM guard width and slow-path contract;
- helper/effect/memory classifications already fixed by lowering.

Certificates are bounded and transactional. The verifier independently re-derives
the intra-block forwarding, RAM guard reuse and loop-cache plans. Missing resets,
invalid slots, forged reuse sites or plans inconsistent with current MIR are
rejected before emission.

## Alias proof

The current proof lattice is intentionally small:

- `Exact`: identical canonical address identity and width;
- `Disjoint`: constant byte ranges under the same canonical segment base (or
  absolute constant linear values) have disjoint low-12-bit byte offsets.
  Native page translation preserves those offsets, so the proof remains valid
  even when distinct virtual pages alias the same physical page;
- `MayAlias`: every relation not proven by the two rules above.

No pointer-range speculation, profile assumption or host address comparison is
used. `MayAlias` never enables forwarding.

For a proven-disjoint committed scalar store, the previous load cache may remain
valid only on the native continuation. A slow store does not continue in the IR
frame, so an MMIO/page-walk callback cannot reach a later reuse with that stale
cache.

## Loop-cache safety contract

A loop cache is emitted only for a natural loop with a unique unconditional
preheader. The candidate address must be loop invariant according to owned MIR
provenance: defined outside the loop, a literal, or a pure machine expression
whose dependencies are invariant.

The current memory-LICM class is intentionally read-only. A loop containing any
guest store, RMW, vector memory operation, unknown helper, or non-segment effect
is rejected as a whole. Canonical segment resolution and one-unit budget polls
may remain in place.

Runtime rules provide the second half of the proof:

- cache validity starts false and is reset in the preheader;
- a cache is set only after the normal same-page native readable-RAM guard and
  physical load succeed;
- the original segment check remains in its original instruction order;
- every slow guest-memory path clears **all** loop caches before page walking or
  MMIO callbacks;
- if the budget poll observes exhaustion, state is materialized and the frame
  returns; a new frame starts with invalid caches;
- no cache is shared across compiled-entry invocations.

These rules rely on the existing non-shared, synchronous single-CPU Wasm ABI.
A future shared-memory or concurrently mutating CPU ABI must invalidate this
proof and re-justify the optimization.

## Interaction with ordinary forwarding

Loop certificates are planned first. Ordinary intra-block forwarding is then
re-derived while treating loop-cached loads as barriers, so one Wasm cache cannot
accidentally satisfy two independently scoped proofs.

The existing intra-block cache remains a separate pair of Wasm locals. Each
loop-invariant alias class receives its own validity/value pair. Nested loops are
processed inner-first and an instruction belongs to at most one loop-cache scope.

## Validation

The IR-core workflow continues to run the complete native Rust suite, executable
Wasm oracles, CPU LICM/CFG differentials, per-pass IR-10 differential, guarded
RAM forwarding/MMIO recovery, SIMD state recovery and store-continuation tests.

IR-11-specific native coverage additionally checks:

- exact reuse across a proven-disjoint constant-range store;
- overlapping ranges remaining `MayAlias`;
- loop-invariant RAM cache creation and preheader reset;
- guest stores disabling loop caching;
- independent MIR/Wasm emission after HIR destruction;
- forged loop-cache certificates being rejected.

The forwarding CPU corpus covers RAM, page crossings, #PF/#GP,
MMIO/remapping callbacks, supervisor guards, code-page aliases and budget exits.
It also emits cache-disabled/cache-enabled loop modules and compares ordinary
RAM, first-load #PF, repeated MMIO, MMIO-to-RAM remapping, callback unmapping and
exact budget exits against the instruction-step interpreter. The LICM/CFG corpus
continues to cover later-iteration faults and recovery ordering.

## What IR-11 completion does not mean

IR-11 completion is an optimizer-package milestone. It does not imply that the
full IR compiler is production complete. Remaining roadmap work includes the
unfinished ISA/state/helper coverage in earlier packages, IR-12 shared
version/link/publication policy, IR-13 operating-system/application/performance
acceptance, and IR-14 default-backend/legacy-emitter retirement.
