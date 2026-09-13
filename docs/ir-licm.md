# IR-11: bounded pure-value loop-invariant code motion

This is a **partial IR-11 implementation**, not completion of IR-00–IR-14.
It supplements the baseline status in [ir-progress.md](ir-progress.md). No ISA
coverage entry, production default, public CPU state layout or legacy retirement
gate is changed.

## Runtime integration

`runtime/compile.rs` invokes `passes::licm::run` after the existing verified HIR
pipeline and before HIR-to-MIR lowering, only when all of the following hold:

- `IrConfig.optimize` is enabled;
- the compile request is `Tier::Two`;
- `PassConfig.rounds` is nonzero.

Tier 1 and disabled-optimization diagnostic runs keep their previous behavior.
The actual CPU-entry and CFG-entry compiler APIs share this implementation.
`CompiledArtifact.passes.loop_hoisted` reports instruction motions, not unique
instructions: a value can move out of multiple nested loops. No extra public
`PassConfig` field is required, preserving existing struct initializers.

## Safety contract

Natural loops are recognized through domination backedges. All latches of one
header are unioned. Inner loops are processed before outer loops. A candidate
requires an existing, unique, unconditional preheader; external-entry headers,
multiple-entry loops and conditional preheaders are conservatively skipped.
No CFG edge is split and no synthetic entry or new execution-budget boundary is
created.

Only explicitly whitelisted, total SSA arithmetic/conversion/bit operations and
pure packed-integer vector operations may move. Every operand must already be
available at the preheader or be produced by an earlier hoisted instruction.
CPU initialization reads are **not** classified as pure merely because their
`Op::ordered()` flag is false. Memory accesses, segment resolution, permission
checks, RMW operations, helpers, division, SSE checks, polls and instructions
with recovery/commit metadata do not move.

Instruction IDs and result definitions stay stable. Only scheduled instruction
lists and instruction ownership change. StateMaps, flags provenance, dynamic
retirement bases, exception ownership, effects and branch arguments are retained.
The transformation runs on a clone and verifies it before replacing the caller's
region. A budget failure or verifier error therefore leaves the original region
unchanged, including after partial candidate planning.

Arena caps are 64 blocks, 8,192 instructions, 16,384 values, 8,192 StateMaps and
1,024 helper descriptors. A separate default budget of 1,000,000 work units bounds
loop discovery and candidate/operand visits. The verifier and initial CFG work
are subject to the arena caps, not charged as exact work units. Regions without
an eligible loop return before cloning.

## Reproduction

With the repository's normal Rust/Wasm, Node.js, C compiler and NASM prerequisites:

```sh
tools/ir-licm-tests.sh
```

This builds the test CPU, generates native-test Wasm fixtures, executes the
standalone Wasm oracle and compares actual CPU entries against interpretation.
The new `IR core` workflow also runs all native regressions, the existing Wasm
and decoder oracles, and existing CFG differential tests. Its token is read-only.
Temporary compiler-bootstrap workflows are not part of the final implementation.

## Tests

The LICM module has 12 focused Rust tests covering transitive invariants,
loop-carried values, self loops, multiple latches, nested loops, independent
entries, irreducible control flow, conditional preheaders, metadata-bearing
values, budget rollback, invalid IR and the real immutable compiler API. An
exhaustive test enumerates reachable three-block CFGs with one or two distinct
successors and all nonempty entry sets, checking verification and idempotence.

`tests/ir/wasm/licm.mjs` runs 12,288 standalone Wasm executions across eight
budgets. It checks zero-trip loops, modulo-2^32 arithmetic, poll boundaries,
FLAGS/provenance, unrelated-state sentinels and exact optimized/unoptimized
recovery. Invalid entry indices must leave the state untouched.

`tests/ir/differential/licm.mjs` runs 3,840 real CPU-entry comparisons against
32,160 interpreter steps for the register-loop fixture. It compares Tier 1 and
Tier 2 recovery, lazy/non-lazy flags, overflow and wrapping retirement counters.
It does not substitute for an operating-system or application benchmark.

## Still outstanding

This does not implement proof-carrying memory reuse, load elimination,
store-to-load forwarding, memory LICM, new SIMD peepholes, synthetic preheaders,
full ISA coverage, the complete link/version graph, general MIR restructuring,
XP/workload acceptance, measured application speedups or legacy-JIT retirement.
The production coverage gate must remain closed until its independent criteria
are met. No completion claim or speedup claim should be inferred from the test
counts or the existence of this pass.
