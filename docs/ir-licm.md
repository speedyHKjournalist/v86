# IR-11 continuation: conservative loop-invariant code motion

This is an implementation-status addendum to `ir-progress.md` after commit
`372ccdc42cc8cb66c61c283295ab7ab673b73f2f`. IR-11 now has a first implemented
loop optimization; the overall IR-00–IR-14 request remains incomplete.
The acceptance criteria in `v86-ir-implementation-plan.md` are unchanged.

## Integration

`runtime::compile::compile_inner` runs LICM after HIR simplification and before
owned MIR lowering, only when `optimize` is true, the requested tier is `Tier::Two`,
and pass rounds are nonzero. Tier 1 and optimization-disabled compilation keep
their previous paths. `CompiledArtifact.passes.loop_hoisted` reports motions;
a value leaving two nested loops contributes two motions. No public JS option,
CPU layout, snapshot format, cache/publication key or default backend is changed.

## Legality and bounds

The pass discovers natural loops using dominance backedges, unions every latch
of a header, and visits nested loops from the inside out. It requires a unique
existing unconditional preheader, with no external entry or side entry inside
the loop. It does not split edges, invent preheaders, or change any CFG edge or
execution-budget checkpoint. Irreducible/unsupported shapes are left alone.

Only an explicit whitelist of total scalar and packed-vector SSA expressions
may move. Every operand must already be outside the loop and dominate the
preheader; earlier motions in the dependency-ordered scan can satisfy this
condition for later expressions. Values and instruction IDs are retained.
CPU observations, memory operations, address checks, division, helpers, SSE
checks, polls, state-bearing instructions and commit metadata never move.
`!Op::ordered()` alone is deliberately NOT accepted as a purity proof.

The operation is transactional: verify the input, transform a private clone,
verify the result, then replace the caller's region. Errors, including work
budget exhaustion after partial planning, do not change the caller's region.
The discovery/candidate/operand work allowance is 1,000,000; region arena caps
are 64 blocks, 8,192 instructions, 16,384 values, 8,192 states and 1,024 helpers.
This is a work/size budget, not a wall-clock latency guarantee. A failed pass
rejects that compilation request; it does not publish a partly rewritten region.

## Tests

Ten targeted Rust tests cover transitive invariants, variant loop state,
self-loops, nested loops, multiple latches, external/irreducible entries,
conditional preheaders, verifier rejection, failure atomicity, memory effect
ordering, SSE guard preservation, and actual CompileRequest tier gating.
An intentionally invalid body GPR-read fixture is rejected by the existing
verifier; the verifier was not relaxed to accommodate the tests.

`tests/ir/wasm/licm.mjs` independently computes expected arithmetic and budget
exits with BigInt. It runs 25,248 executions over 16 emitted modules, including
zero iterations, integer overflow, large iteration counts with bounded work,
FLAGS/EIP/count restoration and invalid entry rejection. It is included by the
normal Wasm test suite, not just an optional standalone script.

The existing dynamic-count helper fixture now also performs LICM in its
optimized variant. Its independent oracle covers 7,786 CPU-ABI loop executions,
17,604 observing helper snapshots and 106 bounded reentries, including caller-
owned fault delivery and callee-owned outcomes. These are instrumented helper
ABI tests, not a claim of complete Windows XP exception or OS validation.

From a clean checkout, generate the shared tables before native compilation:

```sh
make src/rust/gen/interpreter.rs src/rust/gen/interpreter0f.rs \
     src/rust/gen/jit.rs src/rust/gen/jit0f.rs \
     src/rust/gen/analyzer.rs src/rust/gen/analyzer0f.rs
env RUSTFLAGS="-D warnings" cargo test
node tests/ir/wasm/run.mjs
node tests/rust/verify-wasmgen-dummy-output.js
```

The focused loop test is `cargo test ir::passes::licm`, followed by
`node tests/ir/wasm/licm.mjs`. CI separately installs NASM for the decoder oracle
and runs live/cache/automatic-tier/backend and selected CPU differential tests.
A configured CI job is not a passing test until its run has completed.

## Still not complete

This pass does not implement proof-based load reuse, store-to-load forwarding,
load hoisting, alias analysis, loop strength reduction, induction optimization,
automatic vectorization, MIR graph rewriting, or a profitability model.
It does not fill the remaining ISA/MMX/x87/FP-control gaps, complete shared
version/link management, pass XP/application/performance acceptance, or retire
legacy code. `ir-default-gate` must continue to reject incomplete production
coverage. No application speedup, including game loading or FPS, is claimed.
