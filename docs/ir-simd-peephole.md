# IR-11: bounded integer SIMD simplification

This documents the bounded integer-SIMD simplification component of IR-11.
IR-11 is now complete at the package boundary in `ir11-completion.md`; the
full IR-00–IR-14 request remains open. No ISA coverage classification, CPU ABI,
default backend or legacy-retirement gate is changed.

## Implementation

`passes::simd::run` is part of the existing `PassConfig.fold` stage, before GVN
and DCE. It runs only in an enabled optimization pipeline with nonzero rounds;
`fold: false` disables it without adding a new public configuration field.
`PassStats.simd_eliminated` and `simd_shuffled` report the transformations.
LICM remains independently restricted to optimized Tier 2 as documented before.

Rules operate on exact SSA values, not on register names or guessed memory
contents. Identity/duplicate-input shuffles and two-level shuffles are reduced
using per-byte source provenance. A composition requiring three distinct source
vectors is rejected because one Wasm shuffle has only two vector operands.

Integer rules include idempotent AND/OR/min/max/average, proven-zero bitwise and
wrapping arithmetic identities, and redundant lane extraction/replacement.
PANDN uses x86 operand order `(~destination) & source`. A 16-bit extraction after
a replacement cannot be replaced by its original 32-bit scalar, because its
truncation remains semantically necessary; that negative case is tested.

## Safety and resource limits

The pass verifies its input and modifies a private clone. Aliases are applied to
all uses, including flags provenance, XMM snapshots, dynamic counts, branch
arguments and recovery values. The candidate is verified before publication.
Failures leave the original arenas unchanged. Instructions with observation,
commit, trap or store metadata are never removed or rewritten by a rule.

CPU reads, memory accesses, helper calls, SSE guards, floating-point arithmetic,
x87 and MMX state operations are not simplification candidates. Eliminating a
pure value does not eliminate an earlier architectural check or fault point.

Limits are 64 blocks, 8,192 instructions, 16,384 values, 8,192 StateMaps and a
separate default budget of 1,000,000 work units. Alias traversal, candidate visits
and reference rewriting consume the work budget. Initial verification/CFG work
is bounded by arena limits, not claimed as an exact instruction budget.
Scalar-only regions return before CFG analysis and cloning.

## Verification

```sh
RUSTFLAGS="-D warnings" cargo test ir::passes::simd::tests
node tests/ir/wasm/simd_peephole.mjs
```

The four native tests cover positive/negative rules, pinned observations, the
disable switch, verifier rejection, atomic budget exhaustion and generation of
real Wasm modules. The Node suite is imported by the normal Wasm test runner and
therefore runs in IR CI after native fixture generation.

The independent byte oracle executes 49,680 cases: 20 expression families,
276 boundary/random inputs, three FLAGS patterns and three compilation modes
(unoptimized, this pass alone, and the whole optimization pipeline). It checks
GPR results, every XMM snapshot, FLAGS provenance, EIP, retirement-counter wrap
and the complete observed CPU-state memory. The fixture host uses deterministic
CPU-ABI stubs; this is a compiler-semantic execution test, not an OS boot test.

All 20 fixture modules were smaller with the complete optimization pipeline
than without optimization. That result includes existing DCE/GVN and is not a
measurement of this pass alone, guest throughput or application/game speedup.

## Remaining scope

IR-11's proof-carrying RAM reuse and fault-preserving loop cache are documented
separately in `ir11-completion.md`. Additional speculative vector/loop
transformations are not part of the completed conservative boundary. Remaining
ISA and full x87/MMX/FP semantics, general MIR scheduling, version/link
management, XP/application acceptance, performance measurement and legacy-JIT
retirement remain incomplete. The production coverage gate stays closed until
its separate criteria are met.
