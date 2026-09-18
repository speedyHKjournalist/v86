# IR-11: guarded ordinary-RAM forwarding

This document describes the ordinary-RAM forwarding component of the completed
IR-11 optimizer package. IR-00–IR-14 as a whole remains incomplete. This work
does not change ISA coverage, the default backend, CPU/snapshot ABI, live
publication/invalidation, or the legacy-retirement gate. No application speedup
or Windows XP acceptance is claimed. See [IR-11 completion](ir11-completion.md)
for the package-level boundary.

## Implementation

`MirRegion::forward_ram_reads(work_limit)` operates after HIR destruction on the
owned, legalized machine plans. It selects consecutive compatible scalar reads
inside one actual MIR block, allowing only pure machine values, canonical segment
address checks and the continuation of the existing one-unit budget poll between
them. Each read must have identical SSA address (or identical segment base/null
check/offset tuple), width and canonical ordinary-read slow-path contract.

Supported widths are 1, 2 and 4 bytes. In addition to repeated loads, an exact
same-address/same-width scalar store with a successful architectural commit may
seed the immediately following load chain. The store is never removed: only a
later load result may be supplied from the value that was actually written on the
native RAM path. A proven-disjoint committed scalar store may also leave an
earlier load cache live; disjointness is limited to constant byte ranges whose
physical-page offsets cannot overlap even under virtual-page aliasing. The pass
does not guess pointer aliases, merge different widths, or carry this
intra-block certificate across a control-flow edge. RMW, vector memory, general
checks and unknown/helper effects remain barriers. Loop reuse uses a separate
fault-preserving certificate described in [IR-11 completion](ir11-completion.md).

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
2. A successful native ordinary load may store its loaded value and set validity.
   A committed scalar store may also set the cache to the value just written,
   but only after the native same-page writable-RAM guard and physical write have
   succeeded. The existing immutable code-page alias check still runs before any
   following guest instruction can consume that cache; an alias forces return.
3. `Reuse` can bypass a later same-address/same-width scalar load only while
   that runtime bit is valid.
4. A slow load or store clears validity **before** state materialization, page
   walking or callbacks. Slow store success already returns from the IR entry,
   and slow load success never grants validity. MMIO therefore retains its
   ordered callback behavior, including callbacks that change mappings.

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

Native tests cover repeated-load and store-to-load certificates at 8/16/32 bits,
width/address/segment barriers, a non-forwardable RMW, checks, budget polls,
separate locals, HIR destruction, invalid certificates, no cross-block reuse, and
atomic budget failures. Compiler entry tests cover linear/CFG APIs, both tiers,
disabled optimization and zero-round diagnostics. The scalar-store CPU fixture
also executes a native store followed by a same-address load while preserving the
existing slow-path exit, code-alias exit and post-store fault checks.

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

## Remaining roadmap scope

The proof lattice intentionally remains conservative: unproved address
relations are `MayAlias`, loop memory reuse is read-only, and stores/RMW/vector
memory or unknown helpers disable loop caching. Broader speculative alias
analysis is not required by the IR-11 completion boundary. Complete ISA
semantics, IR-12 lifecycle/linking, IR-13 system/performance acceptance and
IR-14 legacy retirement remain outstanding. See [LICM](ir-licm.md),
[IR-11 completion](ir11-completion.md) and the
[implementation status](ir-progress.md).
