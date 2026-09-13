# IR-11: bounded SSA loop-invariant code motion

This implements one part of IR-11, not the complete IR-00–IR-14 migration.
Baseline: `372ccdc42cc8cb66c61c283295ab7ab673b73f2f` on `ir`.

## Placement and contract

`runtime/compile.rs` runs LICM after the existing HIR passes and before lowering,
only for optimized Tier 2 requests. All compiler entry APIs share this path.
Tier 1 and `optimize = false` do not run LICM. `CompiledArtifact.licm` records
loop/move counts, work consumed, and a budget-exhaustion reason where applicable.
Existing public `PassConfig` and `IrConfig` layouts are unchanged.

`analysis/loops.rs` finds backedges whose target dominates their source, unions
latches by header, and walks predecessors without passing the header. Nested
natural loops are processed inner-first. A loop is eligible only if it has no
external entry and has one existing outside predecessor ending in an
unconditional branch to its header. No edge is split and no new block is made.
Irreducible cycles without a dominating header cannot become natural-loop
candidates. Independent eligible natural subloops may still be analyzed.

An instruction may move only when it is in an explicit, total-operation
whitelist, has one result, and has no StateMap, commit map or special store/fault
policy. This includes integer arithmetic, bit operations and pure vector SSA
operations; it excludes CPU reads, loads/stores, RMW, guards, division, helpers
and budget polls. `!op.ordered()` alone is deliberately not the eligibility test.

Every operand's definition must be outside the loop and dominate the preheader.
Within a pass, earlier hoists make invariant producer chains available. Blocks
are visited in dominance order, not arena allocation order. Appending at the end
of the preheader preserves its existing observations and keeps moved producer
instructions before their moved consumers.

Instruction/result IDs, edge arguments, StateMaps, entry recovery maps and effect
chains do not change. The verifier validates the original and the candidate.
HIR can still be dropped after lowering, before Wasm emission.

## Bounded and transactional

Default transformation limits are 1,048,576 work units and 4,096 moves. A value
moved through two nested preheaders counts as two moves. Hard arena limits are
64 blocks, 8,192 instructions, 16,384 values, and 8,192 states/helpers. Dominance
and verifier cost is bounded separately by these limits; the work counter is
not a wall-clock or total-compilation-time measurement.

The pass modifies a candidate clone and commits only after verification. Budget
exhaustion returns zero committed moves with `budget_exhausted = true`, retaining
the original HIR so compilation can continue. Invalid IR returns an error rather
than being silently accepted. No partial hoist escapes on failure.

This first implementation favors auditable correctness over maximal compile-time
or memory efficiency. It does not synthesize preheaders, speculate memory,
forward stores, reuse address-translation proofs, optimize FP exceptions, or
claim a complete cost/profitability model. Large regions skip LICM under the hard
limits instead of making region compilation fail for an optimization budget.

## Tests

Thirteen new Rust tests cover invariant and loop-carried values, reversed block
allocation, idempotence, rollback after planned moves, invalid-IR rejection,
conditional preheaders, multi-entry/irreducible graphs, multiple latches, nested
and self loops, duplicate/dead edges, pure vector dataflow, helper/poll snapshots,
and actual x86 CFG compilation with Tier/optimization gating.

The Node execution harness builds 478 boundary/seeded-random inputs. The pure
loop is executed with LICM off/on and ten budgets (9,560 executions). An observed
loop is executed with six budgets and five helper outcome modes (28,680
executions). The harness compares restored CPU state and memory, callback order
and observed StateMaps; BigInt arithmetic provides an independent integer oracle.
Fault delivery occurs once, and callee-authoritative state must survive yield,
invalidation or transfer without a stale epilogue restore. These are controlled
helper-ABI fixtures, not a replacement for real MMU/SMC/OS acceptance tests.

After generating the repository's ordinary Rust dispatch tables:

```sh
make src/rust/gen/jit.rs src/rust/gen/jit0f.rs \
     src/rust/gen/interpreter.rs src/rust/gen/interpreter0f.rs \
     src/rust/gen/analyzer.rs src/rust/gen/analyzer0f.rs
RUSTFLAGS="-D warnings" RUST_TEST_THREADS=4 cargo test
node tests/ir/wasm/run.mjs
node tests/rust/verify-wasmgen-dummy-output.js
cargo check --features ir-experimental
node gen/generate_ir_decoder.js --check
```

For focused iteration:

```sh
cargo test ir::passes::licm
node tests/ir/wasm/licm.mjs
```

The standard Wasm test runner imports the new harness; tests are not hidden in a
standalone command. The dedicated CI workflow additionally runs the decoder
oracle with NASM/ndisasm and selected CPU differential/cache/backend regressions.
See [current progress](ir-progress.md) for actual validation results and checks
that remain unexecuted. No XP or performance acceptance is implied.
