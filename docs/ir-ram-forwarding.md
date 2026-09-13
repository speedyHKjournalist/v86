# IR-11: guarded ordinary-RAM read forwarding

This is an incremental optimization, not completion of IR-11 or IR-00–IR-14.
It does not change ISA coverage, the default backend, CPU/snapshot ABI, live
publication/invalidation, or the legacy-retirement gate. No application speedup
or Windows XP acceptance is claimed.

## Implementation

`MirRegion::forward_ram_reads(work_limit)` operates after HIR destruction on the
owned, legalized machine plans. It selects consecutive compatible scalar reads
inside one actual MIR block, allowing only pure machine values, canonical segment
address checks and the continuation of the existing one-unit budget poll between
them. Each read must have identical SSA address (or identical segment base/null
check/offset tuple), width and canonical ordinary-read slow-path contract.

Supported widths are 1, 2 and 4 bytes. The pass does not guess pointer aliases,
remove stores, forward a store, move memory out of a loop, merge different widths,
or carry a certificate across a control-flow edge. RMW, vector memory, general
checks, CPU observations and unknown/helper effects terminate a chain.

The immutable compiler enables it only for optimized Tier 2 with nonzero pass
rounds, after existing machine constant folding. `passes.ram_forwarded` counts
statically selected reuse sites, not dynamic cache hits or eliminated guest
instructions. Tier 1, disabled optimization and zero-round diagnostics retain
the original emitter behavior. `PassConfig` and the external configuration ABI
are unchanged. MIR dumps expose the per-instruction certificate.

## Static certificate and dynamic guard

The private certificate records `Begin` and the preceding `Reuse` instruction.
It is derived deterministically from the complete machine schedule; the lowering
boundary rejects a forged, partial, out-of-order or incorrectly sized certificate.
Planning is bounded and read-only until successful installation. A failed work
budget leaves any previously installed certificate intact. Existing arenas,
SSA uses, effect order, recovery maps and allocated SSA locals are not rewritten.

A certificate alone never proves that a guest read is ordinary RAM. The emitter
allocates separate i32 validity/data locals only when a chain exists:

1. `Begin` clears validity on every visit, including repeated loop entries.
2. Only the original successful native RAM guard followed by the actual load
   stores a value and sets validity. Read permissions, privilege, same-page
   access and the existing TLB/RAM conditions are checked before certification.
3. `Reuse` can bypass a duplicate read only while that runtime bit is valid.
4. A slow path clears validity **before** state materialization, page walking or
   callbacks. Slow success never grants validity. MMIO therefore remains an
   ordered sequence of reads, including callbacks that change mappings.

The cache uses dedicated Wasm locals rather than an earlier SSA local whose
physical slot may already have been reused by the allocator. Each chain starts
with a fresh validity check; no certificate is trusted across blocks or calls.

Segment checks remain at their original positions, including null-segment faults.
A canonical segment-address check has no memory/mapping effect on its successful
continuation. A one-unit `PollBudget` also stays in place: continuation changes
only the Wasm budget local, while its observation/materialization arm **returns**
from the entry. It cannot observe an asynchronous event and then resume with a
stale cache. A future resumable poll or callback-bearing segment check must become
a barrier or explicitly invalidate the cache.

This reasoning uses the existing non-shared, synchronous single-threaded CPU ABI.
It is not a memory-model claim for concurrent guests, shared Wasm memory or new
asynchronous memory mutation. Those changes require revisiting this optimization.

## Reproduce

```sh
tools/ir-forwarding-tests.sh
```

The script uses the normal Rust/Wasm, Node.js, C compiler and NASM prerequisites
and the repository's small CPU fixture, not a guest OS image.

Native tests cover width/address/segment barriers, a non-forwardable RMW, checks,
budget polls, separate locals, HIR destruction, invalid certificates, no cross-block
reuse, and atomic budget failures. Compiler entry tests cover linear/CFG APIs,
both tiers, disabled optimization and zero-round diagnostics.

The actual-CPU corpus compares optimized/unoptimized modules against the
interpreter, including cold/warm TLBs, page crossings, true #PF/#GP, FLAGS/EIP/CR2,
exception frames and retirement counts. Device reads change their value on every
callback; additional callbacks remap the address to RAM or remove its mapping.
A CPL3/supervisor-TLB guard and merged-block budget exits are checked explicitly.
The test reports counts from the executed corpus rather than treating configured
checks as passed acceptance.

The broader CFG regression also contains a loop with a confirmed nonzero LICM
motion count followed by a later faulting load. Pure motion and memory forwarding
must preserve the original exception point, stack frame and retirement count.

## Still open

General proof-carrying alias/effect analysis, store-to-load forwarding, memory
LICM, broader loop/SIMD optimization, complete ISA semantics and the roadmap's
system/performance acceptance and legacy retirement remain outstanding. See
[LICM](ir-licm.md) and the [implementation status](ir-progress.md).
