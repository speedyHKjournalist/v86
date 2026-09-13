# IR-11: conservative loop-invariant code motion

This is a partial IR-11 implementation, not completion of IR-00 through IR-14.
It extends the `ir` branch at `372ccdc42cc8cb66c61c283295ab7ab673b73f2f`.
The project-wide acceptance requirements in `v86-ir-implementation-plan.md`
and the remaining work recorded in `ir-progress.md` are unchanged.

## Integration

`runtime::compile::compile_cpu_cfg_region` invokes HIR LICM after the existing
scalar passes and before owned-MIR lowering, only when all three conditions hold:

- `IrConfig.optimize` is true;
- the request is `Tier::Two`;
- `PassConfig.rounds` is nonzero.

Tier One and explicitly disabled optimization do not perform this pass.
`PassStats.loop_hoisted` reports motions: a value moved through two nested
preheaders is counted twice. No public backend defaults, ISA coverage statuses,
CPU publication contracts or legacy-retirement gates are changed.

## Safety boundary

`src/rust/ir/passes/licm.rs` discovers dominance backedges, unions the latches of
each natural loop and visits inner loops first. It requires an existing unique,
unconditional preheader. External entries, side entries, irreducible cycles and
conditional preheaders are not transformed. No CFG edges or block parameters
are rewritten.

Only a whitelist of total, pure, single-result SSA expressions can move. Every
input must already be available at the preheader; moving a definition makes its
dependent expressions available later in the same dominance-ordered scan.
Arena allocation order is not assumed to be execution order.

CPU observations, loads/stores, permission checks, division, RMW operations,
helpers and budget polls never move. Instructions with state, commit or fault
metadata are excluded even if their opcode is otherwise eligible. In particular,
`!Op::ordered()` alone is not a purity proof. The existing pure vector operations
are eligible, but this does not implement a new SIMD peephole pass or additional
SIMD instructions.

The input is verified, all changes are staged on a clone, and the resulting
region is verified before replacement. Failure leaves the caller's region
unchanged, including recovery maps. Discovery and candidate/operand visits have
a default work limit of 1,000,000. Fixed arena caps bound accepted region size:
64 blocks, 8,192 instructions, 16,384 values, 8,192 states and 1,024 helpers.
The counter is not a wall-clock compilation-time guarantee.

## Validation and reproduction

A clean checkout needs Node, Rust and the generated CPU dispatch sources. NASM
provides `ndisasm` for the existing decoder oracle in the complete core workflow.
Run from the repository root:

```sh
make src/rust/gen/interpreter.rs src/rust/gen/interpreter0f.rs \
  src/rust/gen/jit.rs src/rust/gen/jit0f.rs \
  src/rust/gen/analyzer.rs src/rust/gen/analyzer0f.rs
make ir-generated-check
env RUSTFLAGS="-D warnings" cargo test
node tests/ir/wasm/run.mjs
node tests/ir/wasm/licm.mjs
node tests/ir/wasm/licm_safety.mjs
node tests/rust/verify-wasmgen-dummy-output.js
node tests/ir/decode/oracle.mjs
cargo fmt -- --check
```

Rust tests emit both reference and optimized modules; running only the JavaScript
scripts without first running the Rust tests does not create the fixtures.
`.github/workflows/ir-core.yml` installs dependencies, generates sources, runs
these checks and preserves diagnostics with read-only repository permissions.

On commit `36a49f646258299f5efd4cafea529236cc3d9689`, GitHub Actions run
`34733393867` verified 141 Rust tests and 56,280 LICM Wasm executions:

- 41,664 executions use an independent modular-arithmetic and exact CFG-budget
  oracle, not only comparison against another compiler output;
- 14,616 executions exercise explicit polls, zero-trip loops, integer overflow,
  recovery-only values, CPU layout canaries and full return/state equivalence;
  2,480 of these reached a completed-loop arithmetic oracle.

The scripts assert that both completed loops and intermediate recovery sites
are actually reached, preventing a vacuous all-early-exit result. Structural
Rust tests cover nested loops, multiple latches, external entries, non-topological
arena numbering, metadata exclusions, dead arena definitions, atomic failure and
real CPU compile-request tier gating. The cited run's final decoder oracle was
blocked by missing `ndisasm`; the workflow now installs NASM explicitly. Consult
the latest PR checks for the result of the dependency and formatting fixes.

## Still open

Proof-based memory reuse, load/store forwarding, other loop optimizations and
SIMD peephole optimization remain outside this patch. So do remaining ISA,
complete version/link management, system and Windows XP acceptance, performance
acceptance and legacy retirement. No application speedup or XP compatibility is
claimed from these compiler fixtures. Full project CI and IR acceptance are
separate from the focused LICM test results.
