# IR-10: bounded scalar SSA canonicalization

Base: `18cf28745553e4015c8911058de6ed2aedf8b28a` on `ir`.
This is incremental work toward IR-00--IR-14, not completion of the plan.
Production coverage gates, the default legacy backend, Tier 1 policy, guest
state layout, snapshots and floating-point behavior are unchanged.

## Implementation

`src/rust/ir/passes/scalar.rs` runs under the existing `PassConfig::fold` switch,
before literal folding, SIMD simplification, branch pruning, GVN and DCE. The
pipeline reports `scalar_aliases` and `scalar_constants` separately. Disabling
folding also disables this pass; no cosmetic public option is introduced.

The pass handles I1/I8/I16/I32/I64 identities with dynamic inputs: neutral and
absorbing integer operands, same-value comparisons/bitwise operations, known or
identical select arms, exact truncate/extend round trips, and exact inverse
insert/extract operations. Shift counts follow the existing HIR's Wasm 32/64-bit
mask, not an 8/16-bit guest-width mask. Lossy round trips and different bitfields
are not aliases. Floating-point, vector, address/proof and RMW-ticket types are
excluded.

A dependency worklist handles instructions independently of block and arena
allocation order. Phi parameters remain opaque roots: this is not SCCP, a phi
constant lattice or a new alias analysis. Replacements are either an existing
operand of exactly the same type (which already dominates every replaced use)
or an in-place integer constant retaining its original SSA ID. No instruction,
CFG edge, effect or recovery point is moved by the pass.

All edits are planned before mutation. The work budget includes arena scans,
SSA use edges and the final reference traversal. Exhaustion leaves the input
unchanged, including exhaustion after some rewrites have already been planned.
The existing verifier runs before planning and the pipeline verifies the result;
the pass budget does not replace the compiler's separate region/verifier bounds.

The shared reference visitor updates instruction operands, edge arguments,
conditions, entry/fault/commit/exit StateMaps, GPRs, arithmetic and backing FLAGS,
dynamic EIP, dynamic instruction-count bases and REP progress. Ordered producers
remain present even when their numerical result becomes unused; the existing
DCE retains their memory/helper/check/recovery obligations.

## Regression coverage added

The native tests include atomic budget exhaustion, reordered instruction arenas,
non-topological block allocation, complete scalar StateMap references, bitfield
round trips, ignored load results, pass disabling and unused arena slots. A
lifted `xor eax,eax; jnz ...` fixture checks that newly exposed constant FLAGS
feed the existing branch-pruning pass without changing budget recovery.

Native tests generate 188 scalar cases at five widths, before and after
optimization (376 modules), plus eight CFG/budget modules. The Node runner
requires the complete manifest: missing fixtures fail rather than being skipped.
It compares optimized and unoptimized state, checks values against an independent
BigInt expression oracle, checks state-boundary canaries, and reuses memory to
avoid allocating a new guest memory for every test case.

```sh
# Uses the repository's existing generated-source/toolchain prerequisites:
env RUSTFLAGS="-D warnings" cargo test
node tests/ir/wasm/scalar.mjs
# Also imported by the existing CI entry point:
node tests/ir/wasm/run.mjs
```

The following command validates only the arithmetic identities. It deliberately
does not claim to compile Rust or execute compiler-generated Wasm:

```sh
node tests/ir/wasm/scalar.mjs --oracle-only
```

At preparation time, Node syntax checks and **39,444 independent arithmetic
identity checks passed**. The editing container has no Rust toolchain and could
not clone the repository over its network; native compilation, emitted-Wasm
execution and CPU differentials were **not run locally**. The existing IR-core
workflow is unchanged and includes the new native tests and imported Node suite.
Its result must be checked separately; defining tests is not a passing result.

## Remaining plan work

IR-10 remains partial: this change is not general inter-block FLAGS liveness or
state-synchronization elimination. It does not complete shared decoding, MIR
graph transformation/allocation, remaining ISA and strict x87/MMX coverage,
shared version/link management, browser/Worker and Windows XP/application
acceptance, performance acceptance, or IR-14 legacy-emitter retirement. No speedup
or Windows XP compatibility result is claimed.
