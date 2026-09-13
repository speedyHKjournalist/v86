# LICM CPU-ABI recovery validation

These tests extend the experimental IR-11 optimization's acceptance coverage.
They do not establish complete ISA support, Windows XP compatibility, a speedup,
or readiness to remove the legacy compiler.

## Covered behavior

`src/rust/ir/passes/licm/cpu_recovery.rs` emits unoptimized and LICM-optimized
owned-MIR modules with the real CPU Wasm ABI, rather than the standalone test
state layout. Its scalar loops carry i64 values; its SIMD loops carry v128
values. The use block intentionally precedes its dominating header in the HIR
arena. The optimized invariants move to the preheader, while the accumulator
and induction variables remain loop-carried.

The corpus crosses eight execution budgets with both explicit-poll settings and
both scalar/SIMD settings. HIR is released before emission. Recovery maps carry
a dynamic instruction count and, for SIMD, all eight XMM registers. A separate
JavaScript block-budget interpreter and BigInt arithmetic model check:

- Zero-trip, finite and budget-limited loops, including unsigned overflow.
- Exact normal and early-exit EIP, including wrapping CS-base addition.
- All GPR outputs, dynamic instruction-counter overflow, FLAGS and the lazy
  flag operand. SIMD cases check all eight XMM recovery values.
- Equality of the surrounding CPU-state/guard words before and after LICM.

`licm_cpu.mjs` performs 97,280 executions across deterministic boundary and
pseudorandom operand cases. It supplements, rather than replaces, the existing
standalone LICM oracle and the decoder/CPU differential matrices.

Additional Rust tests cover aggregate metadata bounds across many individually
small helper descriptors, oversized snapshots, edge arguments and helper names,
atomic work-budget failure, and rejection of non-entry CPU initialization reads.
No verifier restriction or helper ABI is weakened for the tests.

## Reproduce

After generating the repository's instruction tables:

```sh
RUSTFLAGS='-D warnings' cargo test licm
node tests/ir/wasm/licm_cpu.mjs
```

The CPU-ABI oracle is also imported by `tests/ir/wasm/run.mjs`; a full native
`cargo test` generates its fixtures before that runner executes. Fixtures are
written under `build/ir-licm-cpu` and are not source-controlled.
