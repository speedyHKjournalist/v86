# IR-11: bounded loop motion and exact SIMD rewrites

Status: implemented subset, 2026-09-13. This does **not** complete IR-00–IR-14,
production ISA coverage, XP acceptance, performance acceptance or legacy retirement.
The active status is [ir-progress.md](ir-progress.md).

## Pure-expression LICM

`src/rust/ir/passes/licm.rs` discovers natural loops using dominance and backedges,
unions multiple latches, and processes inner loops first. It requires an existing,
unconditional, unique preheader. It excludes externally enterable headers, multiple
entry regions, irreducible cases and conditional preheaders. It does not split edges
or create blocks, because those operations would change dispatch-budget boundaries.

Only an explicit whitelist of total SSA expressions is eligible: scalar integer
arithmetic/bit operations, selects, conversions and exact integer-vector operations.
An unordered operation is not automatically pure: CPU observations, memory operations,
RMW, helpers, divisions, segment/linear-address operations, SSE guards and budget polls
are not moved. State/commit/fault metadata also disqualifies an instruction.

Operands must already be available at the preheader, or have been hoisted earlier in
dominance/instruction order. Stable instruction/value IDs and StateMaps are retained.
The pass stages a clone and verifies before and after transforming it; a failed
verification or exhausted work budget leaves the caller's region unchanged. Arena
caps bound CFG work separately from the optimizer's visit budget. The default work
limit is 1,000,000 visits. This is not a memory-alias or permission proof.

The immutable compilation entry invokes LICM after the ordinary HIR passes and before
MIR lowering, only when optimization is enabled, the request is Tier 2, and pass rounds
are nonzero. Tier 1 and no-opt requests do not pay for loop discovery. Results are
reported as `CompiledArtifact.passes.loop_hoisted`. The count measures motions, so one
instruction can be counted twice when leaving nested loops. Publication keys,
physical dependencies, entry mappings and cache ownership are unchanged.

## Exact SIMD simplification

`src/rust/ir/passes/simd_rewrite.rs` runs in the existing `PassConfig.fold` phase;
`PassStats.simd_rewritten` records its rewrites. No new public backend defaults or
configuration fields are introduced. Its default work budget is 262,144 visits/cost
units, with fixed region caps and transactional verification.

Implemented rules:

- Same-input integer AND/OR, selected min/max and unsigned average identities.
- Identity and duplicate-input shuffles; one-layer shuffle composition when the
  result can still be represented by at most two input vectors.
- Extract after lane replacement, bypassing writes to another lane and forwarding
  the inserted scalar for matching 32/64-bit lanes.
- Replace with a matching lane extract; bypass an overwritten insert while keeping
  it live whenever a recovery StateMap or another use still needs its value.

A matching 16-bit extract is deliberately **not** replaced by its input i32: the
extract zero-extends and the original scalar may contain nonzero high bits. Shuffle
composition with more than two source vectors is rejected. No floating-point
reassociation, NaN relaxation, guest-memory forwarding or load speculation is used.

Alias rewriting includes StateMaps, flags, dynamic retirement/EIP values, XMM/x87 and
REP state, and edge arguments. The pass verifies the resulting dominance and types
before committing. Independent models test bit patterns that also represent NaNs;
these are preserved as bits, not interpreted as relaxed floating-point arithmetic.

## Bounded differential-fixture loading

The existing SIMD integer suite initially exhausted Wasm code-space allocation while
retaining all optimized/unoptimized fixture modules. `fixture_cache.mjs` replaces
that eager retention with a 32-case-pair LRU. Integer, shuffle and lane scripts retain
their original cases, models and assertions. This is a **test fixture** cache, not a
change to the guest JIT's runtime cache, invalidation or reclamation policy.

Only fixtures without start, element or data sections are accepted, so re-instantiation
does not run a start function or initialize imported memory/tables. Optional
`--expose-gc` lets constrained CI explicitly collect unreachable compiled code.
The cache's unit test checks LRU behavior, retained caller-held instances, copied
statistics, index/capacity errors, missing/corrupt files and initializer rejection.

## Verification performed in this implementation session

All results below are local executions against the changed source, not inferred from
previous progress reports. Rust was 1.98.1 and Node was 22.16.0 in the Linux workspace.

| Check | Observed result |
|---|---|
| `RUSTFLAGS="-D warnings" cargo test` | 139 passed, 0 failed |
| `node tests/ir/wasm/run.mjs` | Existing runner and new oracles passed |
| LICM independent loop/budget model | 12,336 Wasm executions passed |
| SIMD independent byte/lane model | 38,912 Wasm executions passed |
| WasmBuilder dummy-output execution | Passed |
| Generated IR decoder consistency | Passed |
| Cache and auto-tier tests | Passed for debug, release and IR-runtime-without-test-hooks kernels |
| Public backend Node tests | Passed for debug-cache and IR-runtime kernels |
| Legacy publication/eviction coexistence | 900 iterations each in debug and release cache tests |
| SIMD integer full CPU differential suite | Passed in debug and release; 48,576 ordinary scenarios per kernel plus boundary/fault cases |
| SIMD shuffle full CPU differential suite | Passed in debug and release; 41,120 ordinary scenarios per kernel plus boundary/fault cases |
| SIMD lane full CPU differential suite | Passed in debug and release; 18,064 ordinary scenarios and 65,556 exhaustive sign-mask cases per kernel plus other cases |
| Fixture LRU unit test | Passed, also included in the Wasm runner |

The CPU suites include optimized/unoptimized execution, MMIO callbacks, page and
segment faults, permissions, state materialization, dirty-XMM recovery and completion/
resume paths. Deliberate trap/abort cases can print expected Rust panic messages;
the suites still assert the expected architectural result and exit successfully.

The independent NDISASM decoder oracle could not run locally because `ndisasm` was
missing. Native decoder tests and generated-file checks passed; the new CI installs
`nasm` and runs the external oracle. CI results must be read from the actual workflow.
No new real-browser/Worker, XP, game-loading or performance result is claimed here.
The complete all-ISA differential matrix was not rerun in this session.

### Reproduction

Install the repository's normal build prerequisites, plus `nasm` (including
`ndisasm`), a native Rust toolchain with the `wasm32-unknown-unknown` target, Node,
Clang and LLD. From the repository root:

```sh
mkdir -p build
make src/rust/gen/interpreter.rs src/rust/gen/interpreter0f.rs \
     src/rust/gen/jit.rs src/rust/gen/jit0f.rs \
     src/rust/gen/analyzer.rs src/rust/gen/analyzer0f.rs
make ir-generated-check
env RUSTFLAGS="-D warnings" cargo test
node tests/ir/wasm/run.mjs
node tests/rust/verify-wasmgen-dummy-output.js
node tests/ir/decode/oracle.mjs
make ir-cache-tests ir-auto-tests ir-backend-integration-tests
make build/v86-ir-test.wasm build/v86-ir-test-release.wasm
node --expose-gc tests/ir/differential/simd_integer.mjs
node --expose-gc tests/ir/differential/simd_shuffle.mjs
node --expose-gc tests/ir/differential/simd_lane.mjs
```

`.github/workflows/ir-core.yml` executes this scoped regression set and retains logs.
It does not turn the deliberately incomplete `ir-default-gate` into a passing check.

## Remaining acceptance work

Proof-based memory reuse/store forwarding, further loop optimization and compile-time/
code-size/performance evaluation remain. Full ISA (including outstanding x87/MMX/FP
and system forms), general MIR transformations/allocation scheduling, complete runtime
link/version policy, XP/applications and the original performance thresholds must be
completed before changing production coverage or retiring the legacy emitter.
