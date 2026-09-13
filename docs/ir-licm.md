# IR-11: bounded natural-loop analysis and transactional LICM

This is a partial IR-11 implementation, not completion of IR-00–IR-14. The
production backend remains legacy; experimental coverage and retirement gates
are unchanged. See [current status](ir-progress.md) and the original
[implementation plan](v86-ir-implementation-plan.md).

## Placement and contracts

`compile_inner` runs LICM after the existing bounded dataflow pipeline and before
HIR-to-owned-MIR lowering, only for an optimized Tier 2 request with nonzero pass
rounds. Tier 1, `optimize = false`, and `rounds = 0` skip loop discovery and motion.
All three compile APIs share this gate. `CompiledArtifact.loop_passes` records
loops seen/changed, moves and charged work; it is not a runtime timing metric.

Natural loops are discovered from dominance backedges, unioning the latches of
each header. Inner loops are processed before their containing loops. Motion
requires an existing, unique outside predecessor with an unconditional edge to
the header. No preheader, edge, parameter, terminator or recovery point is created
or removed. Loops with external entry points or without this proof are skipped.
Irreducible cycles are not converted to reducible control flow.

Only an explicit whitelist of total integer/address and pure V128 value
operations is eligible. Operands must be defined outside the loop and dominate
the preheader, or have already been moved there. Instruction traversal follows
dominance rather than block-allocation order. A chain may move through multiple
nested preheaders; a value dependent on an outer induction parameter stops at
the inner preheader. Constants may execute on a zero-trip path because these
operations have no architectural side effects or traps.

CPU state reads, guest memory, segment checks, RMW tickets, proof values,
helpers, division, SSE checks, floating-point types, StateMap observations,
commits and budget polls are never speculated. An operation being unordered is
not sufficient evidence of speculatability. The pass preserves guest retirement,
FLAGS provenance, exception ownership and existing recovery points.

The pass verifies the input, transforms a private candidate, verifies the result
and commits only after success. A work/move limit hit midway through a dependency
chain leaves the caller's original graph byte-for-debug-byte unchanged. No result
of a rejected optimization may be published by the compile entry.

## Bounds

At most 64 blocks, 8,192 instruction/state/helper entries and 16,384 SSA values
are accepted. Default transformation limits are 1,048,576 work units and 4,096
moves; explicit configurations are capped at 16,777,216 units and 8,192 moves.
Moving a value through two nested preheaders counts as two moves. Dominance,
cloning and verification are independently arena-bounded: charged work is not a
claim that every allocation or verifier operation consumes one budget unit.

## Validation and reproduction

Generate the normal Rust instruction tables first on a clean checkout:

```sh
make src/rust/gen/jit.rs src/rust/gen/jit0f.rs \
     src/rust/gen/interpreter.rs src/rust/gen/interpreter0f.rs \
     src/rust/gen/analyzer.rs src/rust/gen/analyzer0f.rs
RUSTFLAGS="-D warnings" cargo test ir::passes::licm
node tests/ir/wasm/licm.mjs
```

`cargo test` plus `node tests/ir/wasm/run.mjs` includes the same checks in the
standard fixture/test flow. Do not run fixture generation concurrently with a
consumer of the same output files. `make ir-tests` also runs the existing decoder,
builder and CPU-register differential checks. The dedicated IR workflow adds
memory, control-flow, SIMD, live publication, cache and tier-scheduling checks.

Eight Rust tests exercise dependency chains, reversed block allocation,
state/effect preservation, invalid input, partial-work rollback, multiple latches,
external/irreducible entries, nested induction variables and the real compile
API's tier/optimization gates. The generated Wasm oracle performs 45,900 executions
across three optimization modes, ten execution budgets, five trip counts, two
FLAGS patterns, boundary inputs and a deterministic random corpus. The 4,590
completed executions are checked against an independent BigInt arithmetic model;
all modes also compare the entire observed state window and its write sentinels.

These tests demonstrate the specified transformation and recovery behavior. They
do not establish a speedup, complete ISA coverage, full memory-proof safety, XP
boot compatibility or eligibility to retire legacy. Remaining IR-11 work includes
proof-based memory reuse/forwarding, CFG-changing loop transforms and further SIMD
optimization; the original system and performance gates remain mandatory.
