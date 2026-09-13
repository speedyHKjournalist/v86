# IR-11: conservative pure LICM

This is a partial IR-11 implementation, not completion of IR-00 through IR-14.
Production remains on the legacy backend; ISA coverage and acceptance gates are
unchanged. No XP compatibility or application speedup is claimed.

## Pipeline

`runtime::compile` runs LICM after the existing HIR passes and before independent
MIR lowering for optimized Tier 2 requests. `optimize = false`, `passes.gvn = false`
or `passes.rounds = 0` disables this pure-dataflow family. Tier 1 does not pay the
LICM analysis cost. All three compilation entry points share this policy.
`CompiledArtifact.passes.loop_hoisted` counts instruction motions (an instruction
may move through more than one enclosing loop). The direct pass API additionally
returns loop and work counts and accepts an explicit work budget.

## Safety boundary

- Natural loops are built from dominance backedges, merging all latches of a
  header. A loop needs an existing, unique, unconditional preheader. External
  entry headers, side entries and irreducible cycles without such a header are
  not transformed. Inner loops are processed first.
- The allowlist admits total scalar integer and pure vector SSA operations.
  It excludes CPU reads, memory, segment/address resolution, permission checks,
  helpers, division, SSE checks, polls, and instructions carrying recovery or
  commit metadata. `!op.ordered()` alone is deliberately insufficient.
- An operand must have a definition outside the loop that dominates the
  preheader, or have already been hoisted there. Block parameters within the
  loop are not guessed invariant; the preceding trivial-phi pass can prove them.
- Values, StateMaps, CFG edges, effect order, instruction accounting and budget
  locations retain their identities. Only instruction ownership and scheduling
  change. Verification runs before transformation and before commit.
- Transformation uses an owned staging copy. A rejected input, verifier failure
  or work-budget exhaustion leaves the input unchanged. Arena caps bound region
  size; the explicit work counter covers loop discovery and candidate/operand
  visits, not wall-clock time or the existing verifier's internal operations.

## Tests

```sh
mkdir -p build
make src/rust/gen/interpreter.rs src/rust/gen/interpreter0f.rs \
     src/rust/gen/jit.rs src/rust/gen/jit0f.rs \
     src/rust/gen/analyzer.rs src/rust/gen/analyzer0f.rs
env RUSTFLAGS="-D warnings" cargo test
node tests/ir/wasm/run.mjs
node tests/ir/wasm/licm.mjs
```

The native tests cover dependent invariants, loop-carried SSA, CPU observations,
work-budget rollback, invalid input, self loops, multiple latches, nested loops,
external entries and conditional/multiple preheaders. They emit optimized and
unoptimized Wasm for eight execution budgets. The Node test compares complete
snapshot prefixes for zero/nonzero iterations, integer overflow and invalid
entry indices, and checks full-budget results against a separate arithmetic
oracle. The focused `IR core` workflow runs these checks from a clean checkout.
A configured test is not a passing test; consult the commit's actual CI result.

## Remaining IR-11 and overall scope

This pass does not implement proof-based load reuse, store forwarding, loop
unrolling, induction-variable strength reduction, or general SIMD peepholes.
It does not add preheaders, change CFG structure, or hoist memory/FP observations.
The remaining ISA, MIR graph allocation/scheduling, version/link graph, complete
system/performance acceptance, and legacy retirement remain as tracked in
[the implementation status](ir-progress.md). Do not mark the default gate green
or erase production `Pending` entries on the basis of this change.
