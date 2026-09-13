# IR-10/IR-11: exact canonicalization and conservative LICM

Status: **partial implementation**, 2026-09-13. This does not complete IR-00–IR-14,
full ISA coverage, independent MIR graph optimization, XP acceptance or legacy retirement.
The original scope and gates in `v86-ir-implementation-plan.md` remain unchanged.

## Pipeline and controls

`passes::run` now runs exact canonicalization before GVN and LICM before DCE.
`PassConfig::canonicalize` and `PassConfig::licm` are independently disableable and
true in the optimization configuration by default. `IrConfig::optimize = false`
still bypasses the entire optimization pipeline. The existing automatic Tier 1
policy disables optimization; automatic Tier 2 enables it. This change does not
switch the default CPU backend or enable automatic IR globally.

Compiler statistics expose `identities`, `identity_constants`, `vector_rewrites`,
`loop_hoisted` and `loop_work`. Loop hoists count motions: the same instruction can
move out of an inner loop and then out of its enclosing loop. These are compiler
statistics, not new fields in the serialized CPU or snapshot ABI.

## Loop-invariant code motion

`src/rust/ir/passes/licm.rs` finds dominated backedges, unions all latches of a
natural-loop header, and processes nested loops inner-first. It requires a single
existing outside predecessor with an unconditional edge to the header. Independent
external entries, irreducible cycles, and non-dedicated preheaders are skipped.
No edge splitting, loop rotation, unrolling or new entry construction is performed.

Candidate blocks are visited in dominance order, not arena allocation order.
Operands must already be available outside the loop and dominate the preheader.
Moving definitions updates their ownership, allowing dependent invariant chains.
Stable value/instruction IDs, effect schedules, CFG edges and recovery StateMaps
are retained. A five-expression invariant chain and nested scalar/vector motions
are checked in structural tests and executable Wasm fixtures.

The whitelist contains total pure scalar/bit-vector expressions only. CPU reads,
memory access, address operations, helpers, permission/SSE checks, division, polls,
commits and fault-bearing nodes are not moved. `Op::ordered() == false` alone is
not a sufficient proof of purity. Moving a whitelisted expression through a
zero-trip loop is permitted because it has no externally observable effect.

Both new passes stage changes in a clone, verify before and after, and commit only
on success. Work exhaustion or failed verification leaves the input unchanged.
Each invocation uses a default work budget of 1,000,000. Arena caps are 64 blocks,
8,192 instructions, 16,384 values, 8,192 states and 1,024 helpers. The work counter
covers discovery/candidate visits (canonicalization also conservatively charges
16 byte-lane visits per instruction); verifier, dominance and clone costs are
bounded separately by those fixed caps, not included in the work statistic.
This compile-work budget is unrelated to the guest execution/poll budget.

## Exact identities and SIMD

`src/rust/ir/passes/canonicalize.rs` implements width-aware integer identities,
commutative operand canonicalization, same-arm/constant selects and lossless
extract/truncate-extend simplifications. Existing all-literal folding keeps its
own pass and statistics. Alias replacement includes StateMap-only uses, dynamic
counts, flags, vector state and branch arguments; ordered or observed nodes remain.

SIMD rewrites compose one shuffle level only when the result needs at most two
source vectors, remove identity shuffles and exact idempotent packed operations,
and simplify same-width lane extraction/replacement. Three-source compositions
and mixed-width forwarding are retained conservatively. In particular,
`extract16(replace16(v, x))` cannot alias the original unmasked i32: insertion
truncates and extraction zero-extends. Tests exercise nonzero high halves.
No floating-point reassociation, CPU-read commoning or memory forwarding is added.

## Reproduction

Install the repository's normal Rust/Wasm, Clang, Node and NASM prerequisites.
NASM supplies both `nasm` and the independent decoder oracle `ndisasm`.

```sh
# New structural tests, real compile-API integration, and independent executors.
cargo test ir::passes::
node tests/ir/wasm/licm.mjs
node tests/ir/wasm/canonicalize.mjs

# Existing full native/emission/register entry point (includes the new executors).
make ir-tests

# Selected CPU/runtime regression targets.
make ir-memory-tests ir-stack-tests ir-cfg-tests ir-rep-tests
make ir-live-tests ir-cache-tests ir-auto-tests ir-backend-integration-tests
make ir-simd-integer-tests ir-simd-shuffle-tests ir-simd-lane-tests
```

The fixtures cover 69,120 independent CPU loop executions (unoptimized, LICM-only
and full pipeline), 5,400 integer executions against a BigInt model and 7,280
SIMD executions against a byte/lane model. These are **81,800 executions**, not
81,800 distinct ISA instructions or a performance benchmark. Structural tests
include multiple latches, nested/self/irreducible loops, external roots, polls,
metadata exclusions, exact budget failures and compilation with optimization off.

Local validation used Rust 1.98.1 and Node 22.16.0. All 140 native tests passed;
the existing standalone Wasm suite and the new 81,800 executions passed. The
experimental CPU Wasm and production/test kernels were rebuilt from the edited
source. Memory, stack, CFG, REP, live compilation, cache, automatic tiering,
backend integration and the remaining selected integer/system/SIMD targets were
also executed successfully. These results are not XP or real-browser acceptance.

Two local limits are explicitly outstanding: the external decoder check could
not start without `ndisasm`; the very large existing SIMD integer and shuffle
matrices aborted with V8 `Commit wasm code space Allocation failed`. Neither is
recorded as passing. The local kernel exposes `vm.max_map_count = 65530`; the
SIMD integer harness attempts to retain 66,240 modules. This is a resource-limit
suspect, not proof that the full matrices would pass elsewhere. The dedicated
CI workflow runs these targets with NASM installed and preserves failures.

## Still required

IR-11 still needs proof-based memory reuse, load/store forwarding, more SIMD
transformations and broader loop optimization. The wider roadmap still requires
remaining ISA/helper contracts (including x87/MMX/FP), general MIR graph work,
production region/link/version management, full system/XP/application/performance
acceptance and only then default migration and removal of the legacy emitter.
No coverage row, acceptance threshold or legacy retirement gate is relaxed here.
