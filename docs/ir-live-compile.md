# Live CPU code capture and experimental compilation

The experimental CPU Wasm can now execute the IR compiler itself. Explicit calls
capture the running CPU's entry context and RAM code, compile through the existing
linear/CFG HIR, optimization, MIR and Wasm pipeline, and return an unpublished
artifact handle. Offline Rust fixture generation is no longer the only way to
obtain a CPU IR module. The separate [explicit cache API](ir-cache.md) can now
consume this result and enable normal CPU dispatch. The separate [automatic policy](ir-auto.md)
is opt-in; the default remains legacy.

## Read-only capture

`runtime::snapshot` captures 1–1,920 requested bytes. It preserves the baseline's
visible TLB mapping when a usable entry already exists. A cold lookup performs a
separate read-only RAM walk: non-paged addressing, ordinary paging, PSE, PAE,
cached PDPTEs and the baseline's PSE gate for large PAE pages. The unmasked non-PAE
CR3 low-bit behavior is deliberately retained. Supervisor/user read permissions
are checked without demanding write permission for code.

The existing `translate_address_read_no_side_effects` is not used: despite its
name, its page-table reads can invoke MMIO and it updates profiler miss counters.
The capture walker never invokes those readers, fills a TLB, updates accessed or
dirty bits, changes CR2/PC or delivers a guest fault. Non-RAM code/page-table reads,
missing pages and unsupported upper/NX bits in PAE entries produce compile-stop
errors. It follows the existing CPU model rather than adding missing paging modes.
The requested range is conservative: even bytes later found unreachable by CFG
lifting must be readable if included in that range.

`ImmutableCodeSnapshot` now has ordered `CodeMapping` records for every covered
linear page, including 32-bit address wrap. Physical `CodeDependency` records are
unique, so two linear pages may alias one physical page. The compiler rejects
missing/reordered/unaligned mappings, mappings without dependencies and unused or
duplicate dependencies. `CompiledArtifact::current` requires mapping identity as
well as publication key, dependency versions and entry contract. Identical bytes
at a different physical page do not silently change the artifact's dependencies.

## Experimental Wasm API

`make build/v86-ir-runtime.wasm` builds an `ir-experimental` release CPU with these
exports and without differential test hooks. Automatic IR compilation requires opt-in.

- `ir_compile_live(length, tier, optimize, cfg, budget, rep_budget) -> i64`: captures
  the current entry and explicitly compiles it. Tier is 1 or 2; optimize/cfg are
  0 or 1. Returns a nonzero opaque handle, or zero on failure. Every attempt replaces
  the previous result, including failed attempts. It refuses halted, prefixed or
  legacy JIT contexts. The tier argument feeds the current compiler request; it
  does not implement online tier selection or promotion.
- `ir_live_info(handle, field) -> i32`: fields 0/1 are byte pointer/length, 2/3 are
  mapping/dependency counts, 4/5/6 are linear IP/guest EIP/default width, 7 is guest
  byte count and 8 is the Wasm local count. Invalid handle/field returns zero.
- `ir_live_mapping(handle, index, physical) -> i32`: physical=0 selects the linear
  page and physical=1 selects its physical page; other selectors return zero.
- `ir_live_validate(handle) -> i32`: checks context, generation, observed write
  versions and an exact fresh code/mapping capture without guest side effects.
- `ir_live_release(handle) -> i32`: releases only the matching result; an old handle
  cannot release a replacement. `ir_live_error()` returns the last compile error.

JS receives i64 handles as BigInt and i32 metadata as signed Number; normalize
unsigned metadata with `>>> 0`. Handles are scoped to the original CPU Wasm
instance. A caller must retain that instance identity, copy bytes synchronously
before any replacement/release/reset, and revalidate against that same instance
after asynchronous browser instantiation and immediately before a cold call.
Getting bytes does not imply that the artifact is currently admissible.

Error codes are 1 invalid arguments, 2 invalid/active CPU context, 3 unreadable
mapping, 4 non-RAM source, 5 unsupported paging, 6 unsupported lowering, 7 compiler
budget, 8 invalid IR and 9 exhausted identity space. Job serials never wrap. Reset,
`rust_init` and global JIT cache clear drop the result and advance a non-wrapping
VM generation. None of this metadata is added to guest snapshots.

## Dependency and ownership limits

One bounded result is retained at a time. Its dependency versions are local to
that task: existing legacy dirty-page notifications increment versions for its
physical source pages. Notifications for unrelated pages do not invalidate it.
The fresh byte comparison also detects writes that bypass those notifications,
without modifying TLB code tags merely to capture code. An unnotified change that
restores exactly the same bytes is not a complete write-history observation;
notified same-byte writes do invalidate the result. This is not the full shared
physical-page version registry required by online IR caching.

The live artifact's slot fields are zero: this API alone performs no reservation
or installation. The separate explicit cache consumes the artifact, assigns slot
ownership, registers code watches and reclaims after cold execution returns. The
guarded ABI still requires CPU-owned state and matching context. Full link graphs,
cancellation scheduling and production IR tier selection remain to be connected.
Live validation is not a license to retain or execute a stale table slot.

## Validation

`make ir-live-tests` checks native mapping contracts and, in each Node-hosted debug/release
CPU build, 44 actual in-Wasm compilations against the interpreter, eight exact
data page faults, and 19 paging/capture cases. These cover cold/warm translations,
CR3 low bits, PSE/PAE, cached PDPTEs, user code, cross-page/noncontiguous/aliased code,
high-address wrap, missing pages and MMIO page tables/code. CPU fields, RAM tables,
the complete TLB and MMIO observations are compared before and after capture and
revalidation. Lifecycle checks cover raw/notified/secondary/unrelated writes,
unchanged bytes with changed mapping identity, asynchronous instantiation, replaced
handles, restore/reset, release, failure and a real legacy JIT callback.

A separate experimental-only release check compiles, asynchronously instantiates
and executes a CFG with a terminal store for both tier requests and optimization
settings. It verifies independently expected values, IP and wrapped retirement
counts while asserting that no test hooks are exported. These are explicit cold
entries; they do not establish online IR, XP or workload performance acceptance.
