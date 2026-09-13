# IR-11 continuation: bounded LICM and bit-exact SIMD rewrites

Base: `372ccdc42cc8cb66c61c283295ab7ab673b73f2f` on `ir`.

This is a partial implementation of IR-11, not completion of IR-00–IR-14.
It supplements the earlier `IR-11: 未实现` entry in `ir-progress.md` with two
implemented, independently switchable Tier 2 passes. Production ISA coverage,
the default backend and the legacy retirement gate are unchanged.

## Integration

`PassConfig::licm` and `PassConfig::simd` select the passes. Both default to true
in the generic optimization pipeline. `compile_inner` masks both off for Tier 1;
only an optimized Tier 2 request enables them. `optimize: false` and the existing
`rounds` bound still apply. No new JavaScript option or serialized CPU field is
introduced. Tests exercise the actual `compile_cpu_cfg_region` entry and assert
that Tier 1 does not run either pass, Tier 2 does, and each pass can be disabled.

The pipeline remains bounded to at most eight rounds. SIMD rewriting follows
CFG simplification and precedes GVN. LICM follows GVN and precedes StateMap-aware
DCE. Statistics are returned in `CompiledArtifact::passes`:
`licm_hoisted`, `licm_loops`, `simd_eliminated`, `simd_composed`, and
`simd_overwritten_lanes`. LICM statistics count transformations; an expression
moved through two nested loops contributes two motions, not one unique value.

## Loop-invariant code motion

Natural loops are found from dominance backedges and all latches are united per
header. The reverse predecessor walk stops at the header. A candidate requires
one existing, unconditional preheader; no critical edge is split and no new
block or parameter is introduced. Public-entry headers, side entrances,
irreducible cycles and conditional preheaders are conservatively excluded.

Nested loops are processed inner first. Blocks are scanned in dominance order,
not arena allocation order. An expression can move only when every input is
already available at the end of the preheader. Loop parameters remain variant;
the existing trivial-phi pass may separately prove a parameter redundant.

The whitelist contains only total, pure integer/address-offset/vector value
operations with one result and no observation or recovery metadata. In
particular, `LinearOffset` is wrapping offset arithmetic, **not** permission,
segment, bounds or translation validation. Guest loads/stores, RMW tickets,
address checks, CPU reads, helpers, division, polls and architectural recovery
points do not move. An operation being unordered is not sufficient evidence
that it is safe to speculate.

Instruction ownership and block schedules change together. SSA identities,
terminators, CFG edges, effect tokens and StateMaps do not change. Dispatcher
budget checks therefore remain at the same positions, including zero-trip
loops and exits before the first body execution.

## SIMD simplification

The pass performs bit-level operations only; it makes no floating-point,
MXCSR, exception or memory alias assumptions.

- Remove identity/projection byte shuffles. Normalize selectors for identical
  shuffle sources, and compose a shuffle selecting from one earlier shuffle.
- Remove `And(x, x)` and `Or(x, x)`.
- Forward a matching 32/64-bit extract after replace; eliminate reinsertion of
  a lane extracted from the same vector; bypass a previous write to the same
  width and lane when overwritten.

A 16-bit extract after insertion deliberately remains: forwarding the original
i32 would lose the required truncation. Differently sized, overlapping writes
also remain. Rewrites preserve other uses of an earlier vector. Aliases are
applied to instruction arguments, branch arguments/conditions and every
recovery-only value, including complete XMM StateMaps. CPU observations,
SSE guards, memory operations and helper boundaries are not removed or moved.

## Resource limits and rejection

Each pass defaults to 262,144 work units per invocation and caps the region at
64 blocks, 8,192 instruction slots, 16,384 value slots, 8,192 state slots and
1,024 helpers. Work charges cover discovery/candidate/operand visits and the
recovery rewrite/retention sweeps; the arena caps bound structural analysis.
These are logical work bounds, not wall-clock guarantees.

Each pass verifies its input, transforms a private candidate, verifies the
result, and only then commits. Budget exhaustion or verification failure leaves
the caller's region unchanged. This transaction is per pass; it is not a claim
that the entire pre-existing multi-pass pipeline rolls back earlier passes.
Compile failures continue through the existing rejection/fallback handling.

## Reproducing validation

From a clean checkout with the normal Rust/Node/make toolchain:

```sh
node tests/ir/advanced.mjs
# Full native and Wasm-core regression after the above generated dispatch tables:
env RUSTFLAGS="-D warnings" make ir-backend-tests
# Requires ndisasm from NASM:
node tests/ir/decode/oracle.mjs
```

The focused entry always recompiles Rust fixtures before executing emitted Wasm;
it does not treat a pre-existing build directory as evidence. CI additionally
runs CPU CFG/memory/REP/SIMD and online compile/cache/tiering/backend regressions.
It does not require proprietary OS images or claim to validate Windows XP.

The local continuation added 12 Rust tests; all 139 native tests and the Wasm
core harness passed with Rust 1.98.1 and Node 22.16.0. New execution oracles:

| Oracle | Executions | Independently checked behavior |
|---|---:|---|
| LICM | 13,014 | wrapping integer arithmetic, zero-trip loops, all tested dispatcher budget exits, FLAGS and recovery PC, full observed-state equality |
| SIMD | 8,560 | byte selectors, lane insertion/extraction, 16-bit truncation, overlapping widths, all eight XMM registers and CPU state |

The LICM oracle interprets the fixture CFG directly, rather than reusing HIR
semantics. The SIMD oracle uses byte arrays and BigInt. Its CPU-entry import
stubs are intentionally limited to a pure-value fixture; those tests alone do
not establish real CPU helper/lifecycle correctness. Separate runtime tests
cover that integration. Further cases test nested loops, permuted block IDs,
multiple latches/entries, unsupported metadata and budget failure after staged
modifications.

## Large differential fixture lifetime

The packed-integer and shuffle differential drivers previously retained every
compiled Wasm module and instance before executing their first case. On the local
Node 22 environment this exhausted Wasm code space. They now use a 32-pair LRU
fixture cache, without removing cases, assertions, interpreter comparisons or
CPU imports. A separate test checks reuse, eviction, invalid inputs and import
isolation. Capacity bounds retained pairs, not the engine's immediate native
code reclamation.

The local selected matrix comprised 39 CPU differential/runtime commands. The
initial run passed 37 and aborted the two eager SIMD drivers; both subsequently
passed completely in debug and release after the fixture-lifetime correction.
The selection includes CFG, scalar memory, system/control instructions, SIMD,
real CPU entry, live compilation, cache invalidation, automatic Tier 2 and public
backend integration. It is not the complete IR-13 workload/browser/OS matrix.
The standalone decoder oracle still requires `ndisasm`; it was not available
locally and is explicitly run in CI with the NASM package.

## Still outstanding

Proof-carrying RAM access reuse, guest load/store forwarding, memory LICM,
induction-variable/range optimization and general loop restructuring remain
unimplemented. This change does not complete the remaining ISA/MMX/x87/FP work,
MIR graph/stack scheduling, the shared version/link graph, or XP/application and
performance acceptance. Legacy compilation is not retired, coverage entries
are not relabelled as complete, and no game-loading or benchmark speedup is
claimed from these correctness tests.
