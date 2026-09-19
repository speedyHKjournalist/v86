# IR-12 completion: region, cache, link and publication lifecycle

IR-12 closes the runtime lifecycle package defined by the implementation plan:
bounded region selection, generation/dependency invalidation, transactional
publication, safe tier replacement, compilation cancellation, shared slot
ownership and validated link lookup. It does not make IR the default backend or
claim the IR-13 operating-system/application/performance matrix.

## Ownership and generations

Every IR compilation receives a non-wrapping publication identity scoped to the
current VM generation. Reset, restore and cache-clear advance the generation and
retire pending/live artifacts. A table slot has a separate owner identity, so a
late browser callback cannot publish or release a slot that has since been reused.

IR reservations share the existing bounded Wasm table pool but never enter legacy
entry/link tables. Physical source pages are registered as code dependencies.
Dirty-page notification retires dependent live, cached and scheduler records.
Reclamation happens only at a cold/quiescent point after any active guest frame
has returned.

## Transactional publication

Publication remains a three-phase transaction:

1. reserve a slot for a unique owner;
2. immediately before `table.set`, revalidate VM generation, source bytes,
   physical mappings and entry contract;
3. after installation, revalidate again before making the record Published.

Pending/validated entries cannot execute. Cancellation, reset, SMC, mapping
change or browser failure retires the owner. Failed replacement does not remove a
working published entry. Old asynchronous completions are rejected by identity.

## Region and tier scheduling

The automatic scheduler remains bounded: at most 128 heat records, one in-flight
automatic publication, and one candidate scan per outer CPU frame. Tier 1 uses a
small decoder-selected region; Tier 2 uses the larger verified optimization
pipeline. Unsupported or unchanged failed inputs are suppressed until source or
mapping identity changes.

Published automatic entries use deterministic cold-point LRU-style eviction by a
monotonic use stamp, with publication identity as a tie breaker. Explicitly
published entries are not automatic eviction victims. Eviction resets historical
heat so inactive entries do not immediately recompile.

## Validated link graph

IR-12 adds a cache-owned link-target lookup for compiled exits. A link lookup is
only a hint: it identifies a currently published target for the exact
`CpuEntryKey`, then rechecks VM generation, immutable source bytes and physical mapping identity. Cached-TLB visibility remains part of the normal execution admission, so a graph lookup cannot create architectural A-bit effects merely to discover a target.

The lookup never performs an unchecked `call_indirect`, never compiles or
publishes while guest locals are live, and never retains a cache lock across guest
activation. Actual execution continues through the normal entry/admission path.
Missing or stale targets count as link misses and stale records are retired.

This provides the shared version/link graph needed by the runtime without making
cross-region linking a second, weaker admission protocol.

## Invalidation graph

The runtime dependency graph now has explicit edges from:

- VM generation to live jobs and cached records;
- physical code pages to live jobs, cached records and scheduler failure history;
- publication identity to table-slot ownership;
- entry key to Tier 1/Tier 2 replacement and automatic heat;
- cached record to validated link lookup.

SMC, host writes routed through the JIT dirty path, reset/restore, page mapping
changes observed by fresh capture, active code/PTE aliases and slot reuse all
invalidate at or before the next admission. Same-byte notified writes still
invalidate by dependency history.

## Recovery and pressure

No cache/JIT mutex survives a guest activation. Synchronous I/O callbacks that
dirty or clear code retire the active owner but cannot reclaim its table slot
until the frame returns. Zero-retirement entries are retired to prevent repeated
admission from starving interpretation/device progress.

IR records and legacy modules share the finite table pool. Automatic IR eviction
can reclaim only automatic IR entries; explicit IR reservations survive that
policy. The deterministic use stamp replaces first-match eviction so repeated
pressure has a stable victim rule.

## Acceptance

The IR-core workflow now runs the lifecycle matrix as a required IR-12 gate:

```sh
make ir-live-tests ir-cache-tests ir-auto-tests
```

The matrix covers debug, release and experimental-only runtime builds and includes
generation reset/restore, same-byte SMC, raw byte changes, mapping changes,
secondary-page dependencies, active self-modification, pending/duplicate/forged
publication, slot ABA reuse, browser failure and out-of-order completion,
32-entry IR capacity pressure, 900 legacy publications, automatic Tier 1/Tier 2
promotion, failed-upgrade retention, one-task cancellation, bounded heat,
automatic eviction, synchronous I/O invalidation and validated link lookup.

The link-specific regression verifies that a current target is found, an absent
entry misses, and a source write makes the old target un-linkable before
reclamation.

## Remaining roadmap

IR-12 completion is a runtime lifecycle milestone. IR-13 still requires the full
release/browser/host/OS/application/performance matrix and tuning. Earlier
IR-02–IR-09 coverage gaps remain prerequisites for broad production workloads.
IR-14 still requires IR to become the production default and removal of legacy
emission from production builds.
