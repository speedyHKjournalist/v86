# IR-11: bounded pure LICM and bit-exact SIMD simplification

This is a partial implementation of IR-11, not completion of IR-00–IR-14.
The production default, ISA coverage gate, CPU layout, snapshot format, devices and
graphics proxy protocols are unchanged. The initial IR source is commit
`372ccdc42cc8cb66c61c283295ab7ab673b73f2f`; semantic reference tests continue to pin
`8ee73e538daaab15411344d39a1f271e778ac7f3`.

## LICM contract

`passes::licm::run` verifies the input, discovers natural loops from dominance
backedges, and groups all latches of each header. A loop must have an existing
unique, reachable, dominating, unconditional preheader. External entries, secondary
entries, irreducible cycles and conditional preheaders are rejected conservatively.
No block, edge, block parameter, budget poll or recovery map is added or deleted.

Inner loops are visited before outer loops. Instructions are considered in
dominance/definition order, not arena allocation order. An operand must already
be available in the preheader, or be the result of a previously selected invariant.
A loop-carried parameter is never treated as invariant merely because its type
matches an outside value. Stable instruction/value IDs preserve recovery-only uses.

Eligibility is an explicit whitelist of total, pure scalar and packed-vector
operations. `!ordered()` alone is insufficient: CPU register/flag/segment/XMM reads
also remain in place. Guest loads/stores, address guards, helpers, divide, SSE checks,
RMW tickets, StateMaps and polls are not moved. Integer overflow is wrapping, as in
the original HIR. Zero-trip loops may compute an unused pure value, but cannot gain
a guest-visible access, exception, state update or retired instruction.

The pass rejects regions above 64 blocks, 8,192 instruction slots, 16,384 value slots
or 4,096 StateMaps. Defaults allow 262,144 charged work units and 256 moves. A move
through each nested preheader is counted separately. Work units count selected
traversals, not wall-clock time or every allocator/verifier operation; structural
limits bound the other work. A verified clone is committed only after success, so
budget exhaustion or verifier failure leaves the input unchanged.

## SIMD contract

The SIMD pass handles only bit identities and byte selection: `AND(x,x)`, `OR(x,x)`,
identity shuffles, and composition of nested `VectorShuffle` instructions. Each
output byte is resolved through at most one child level per pass, with bounded
rounds. Composition is accepted only when at most two distinct vectors actually
contribute bytes. Four independently contributing sources remain uncombined.
Duplicate inputs and swapped source order are canonicalized without assuming an
identity mask in the original source numbering.

Aliases rewrite instruction inputs, CFG edge arguments, exit state, entry state,
XMM state, flag state, dynamic counts and other recovery-only values using the
existing shared rewrite routine. No CPU observation, memory access, SSE guard,
helper or floating-point arithmetic is eliminated or reordered.

## Pipeline and controls

`PassConfig::simd` and `PassConfig::licm` are independent Boolean switches, enabled
by default in an enabled optimization pipeline. Existing isolated-pass tests set
both false. Global `IrConfig::optimize = false` bypasses all passes. SIMD runs in
bounded canonicalization rounds. LICM runs once after those rounds; GVN and DCE
cleanup are repeated only when code was moved and those passes are enabled.

The common runtime compiler entry retains the upstream `run`/`run_tier2` split.
Only optimized `Tier::Two` requests through the three compilation APIs use
`run_tier2`; ordinary `run` performs no LICM. Zero rounds do not run LICM.
`PassStats::licm_loops`, `licm_hoisted` and `licm_work` preserve upstream statistics,
while `simd_simplified` counts the new rewrites. No new
JavaScript public option, memory-proof claim, or performance guarantee is implied.

## Reproducible tests

Run the focused tests from the repository root:

```sh
python3 tests/ir/run_regressions.py --optimizations-only
```

Run all 42 listed IR/CPU/publication Make targets with an installed Wasm toolchain,
NASM, the normal build dependencies and a full Git checkout:

```sh
python3 tests/ir/run_regressions.py
```

The full runner fails early when the pinned pre-refactor commit is unavailable.
It does not replace the reference implementation with current code or silently skip
REP/task-register oracles. CPU feature variants build serially because their Cargo
outputs share paths before Make copies the isolated artifacts. The two CI jobs
preserve logs even on failure; this does not imply unobserved CI has passed.

Large existing SIMD corpora are sharded into separate Node processes rather than
keeping tens of thousands of compiled modules live at once. Original artifact
indices are preserved. Every original case appears in one shard, apart from the
shared chain fixture, which is repeated deliberately. Each shard still runs debug
and release CPU builds, reference and optimized code, independent expected values,
fault priorities, memory permissions, MMIO observations, partial faults and resume
checks. Child failure or a malformed shard token fails the parent run. A partition
unit test checks exhaustive coverage and index preservation.

## Reconciliation with concurrent upstream work

While this change was being validated, `ir` advanced to
`00da882962df72b4c39e8dd28f9f5a3bb1ac8639` and incorporated another LICM
implementation. The final integration **retains that implementation**, its public
`Config`/`Stats` API, original tests, Wasm oracle, `run_tier2` entry and runtime
compiler selection. It does not overwrite that concurrent work with the initial
implementation from this branch.

The nine additional LICM tests now live in `licm/extended_tests.rs`, with their
own `ir-licm-extended` fixtures and `wasm/licm_extended.mjs` model. The four SIMD
tests remain new. A per-pass LICM opt-out and optional GVN/DCE cleanup extend the
existing Tier 2 pipeline; the SIMD pass runs during scalar canonicalization.

## Validation recorded before reconciliation

Local compiler: Rust 1.98.1; Wasm execution: Node 22.16.0, Linux x86-64. Source and
public toolchain were obtained through a temporary repository CI artifact; that
bootstrap workflow is not included in this change. No user-local working tree was
modified. Logs describe checks actually executed, not promised future results.

- 140 native Rust tests passed with `RUSTFLAGS="-D warnings"`.
- The 13 new Rust optimization tests cover dependent and nested loops, multiple
  latches, secondary entries, CPU/memory observations, budget rollback, state
  preservation, tier/optimization opt-outs, shuffle identities and four-source
  composition rejection.
- 7,680 emitted-Wasm LICM executions matched an independent CFG/budget model,
  including zero iterations, wrapping arithmetic and exact recovery PCs.
- 9,088 emitted-Wasm SIMD executions matched an independent byte model across
  71 graphs and 64 input seeds, with full XMM/GPR/FLAGS/recovery checks.
- The existing standalone Wasm execution matrix and Wasm encoding verification
  passed. Debug/release test-hook and release runtime IR kernels built successfully.

The local CPU runner completed 43 build/test invocations. Its first two large
packed-integer/shuffle runs hit the host Wasm code-space limit rather than a
semantic assertion; they were rerun successfully with the exhaustive sharding fix:
33,120 packed-integer corpus cases and 29,400 shuffle corpus cases, each with
reference/optimized code and debug/release CPUs. Cache and automatic compilation
checks passed for debug cache-test, release cache-test and release runtime builds;
public backend checks passed for debug cache-test and release runtime builds.
The shared-decoder/legacy-analyzer comparison also passed 2,670,035 cases.

The separate disassembler oracle could not run locally because `ndisasm` was not
installed; CI installs the NASM package that supplies it. The full-history REP and
task-register reference builders, real browser/Worker matrix, XP boot, application
workloads and performance acceptance are not claimed as locally validated here.

Proof-based memory reuse, forwarding, induction-variable/strength transforms,
remaining ISA/MIR/link management, production-default migration and legacy emitter
retirement remain open. No speedup or XP/game compatibility claim follows from
synthetic optimization correctness tests.

## Validation after reconciliation

The integrated source passed all **153 native Rust tests** and the complete
standalone Wasm execution runner. The optimization subset contains 26 tests:
13 preserved upstream LICM tests and 13 additional LICM/SIMD tests. The preserved
LICM oracle passed 21,600 executions; the additional CFG/budget oracle passed
7,680 executions; the SIMD byte oracle passed 9,088 executions. Moving the added
tests to a separate module was followed by rerunning the 26-test subset and all
three optimization oracles.

The integrated release runtime kernel was rebuilt, and `live_runtime`, `cache`,
`auto` and `backend` differential runners passed against that exact kernel.
The earlier large-corpus counts remain results of the pre-reconciliation revision;
they are not silently relabeled as a full CPU-matrix rerun of this final merge.
The final full CI matrix is a separate check on the PR commit.
