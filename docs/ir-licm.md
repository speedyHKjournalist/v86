# IR-11: conservative Tier 2 LICM

This increment does **not** complete IR-00–IR-14. The default backend and legacy
emitter remain unchanged. Current status is in [ir-progress.md](ir-progress.md);
prior implementation reports are preserved in [ir-progress-history.md](ir-progress-history.md).

## Compiler contract

`src/rust/ir/passes/licm.rs` operates on verified HIR before owned MIR lowering.
It is called by the immutable CPU compile API only when all three conditions hold:
`config.optimize`, `request.tier == Tier::Two`, and `config.passes.rounds != 0`.
Tier 1, optimization-off, and zero-round diagnostic requests do not pay for LICM.
`PassStats.loop_hoisted` counts motions, so one value leaving two nested loops
contributes twice. No new CPU serialization or browser ABI is introduced.

Natural loops are recognized through dominator-confirmed backedges, with all
latches for one header unioned. A loop needs one existing unconditional preheader
that dominates its header. External-entry headers, additional external entries,
irreducible side entries and conditional preheaders are not transformed. Loops
are processed inside-out; block order follows dominance rather than arena IDs.
A moved value becomes available to dependent expressions in the same scan.

Only a positive whitelist of total scalar and integer-vector SSA operations may
move. Operands must be loop invariant and dominate the preheader. An instruction
carrying a recovery map, commit map or fault policy cannot move. CPU initialization
reads are not pure; in particular, `Op::ordered() == false` is not a purity proof.
Memory accesses, MMU/segment/SSE checks, helpers, divides, polls and physical
address proofs remain in place. Recovery state, edge arguments and terminators
are preserved. The final verifier checks updated instruction ownership/schedules.

The pass stages changes in a cloned region and commits only after final
verification. Budget or verification failure leaves the caller's region intact.
The work budget accounts for discovery and candidate/operand visits, not the
whole compiler's wall-clock cost; arena caps separately limit the input size.
A region with no eligible loops returns before cloning and re-verification.

No entry splitting, induction-variable optimization, load hoisting, memory
forwarding or new alias/permission proof is claimed. A hot region whose loop
header is its external entry is deliberately skipped. Consequently these tests
are not evidence of a measured XP/game speedup or a universal loop optimization.

## Focused tests

From the repository root, after installing the repository build prerequisites:

```sh
make ir-licm-tests
```

This runs the native LICM/compile-API tests, independent standalone Wasm oracle,
and a test that compiles IR inside the live CPU Wasm before comparing execution
with the baseline interpreter. The standalone oracle also runs through the
existing `make ir-tests` Wasm harness.

The native tests cover transitive invariants, loop-carried values, multiple
latches, nested/self loops, external/irreducible entries, non-topological arena
order, whitelist exclusions, failed-budget rollback and compile-API switches.
A malformed body `ReadGpr` is rejected without weakening the verifier. The first
version of the test fixture accidentally used that invalid form; three failing
tests were corrected, and rejection is now explicitly regression-tested.

The standalone JS oracle uses independent BigInt arithmetic and exact block
budget recovery. It checks zero-trip loops, overflow, FLAGS/lazy operands,
recovery EIP and memory sentinels. The live test checks a real invariant IMUL,
zero-trip guard and invariant arithmetic next to a faultable load. It compares
GPR/FLAGS/XMM state, CS-relative instruction pointers, retirement-count wrapping,
exact opt-on/opt-off budget exits and the complete second-iteration #PF frame.

## Large SIMD regression resource bound

The existing packed-integer and shuffle suites eagerly retained 66,240 and
58,800 compiled Wasm modules respectively, before constructing instances. This
run failed with V8 `Commit wasm code space Allocation failed`. The scripts now
use `bounded_instances.mjs`: an LRU of at most 64 case pairs, with periodic GC of
evicted modules. Normal invocation re-executes Node with `--expose-gc` when needed
and propagates any child failure. A failed construction is not cached.

All original case indices and independent models remain unchanged: 33,120
packed-integer cases and 29,400 shuffle cases, each with optimized/unoptimized
artifacts and debug/release CPU comparisons. No cases, MMIO checks, exception
checks or randomized/boundary scenarios were dropped to make the run fit.
`bounded_instances_test.mjs` verifies the cache contract separately. This is test
infrastructure; production cache sizing and CPU runtime behavior are unchanged.

## Validation on 2026-09-13

The local source checkpoint was `6658af1f7c8cd50c182b34dab040f04c56e2f244`, plus
this increment. Rust 1.98.1, Node 22.16.0, Clang 17 and NASM 2.16.01 were used.
These are newly executed results, not inherited claims from earlier reports.

| Check | Observed result |
|---|---|
| `RUSTFLAGS="-D warnings" cargo test --offline` | 137 passed, 0 failed |
| Native LICM subset | 10 passed, including real compiler API and disabled switches |
| Independent LICM Wasm oracle | 41,664 executions passed |
| Live LICM CPU/interpreter oracle | Per debug/release build: 1,728 comparisons, 864 exact opt-on/off exits, 4 second-iteration #PF cases passed |
| Existing reachable CFG oracle | 39,936 CPU comparisons / 184,768 interpreter steps; 19,968 exact budget comparisons; 16 second-iteration faults and 16 absent-arm skips passed |
| Live compiler | Debug/release snapshots, mapping/byte validation, exception frames, cancellation and restore passed |
| Publication/cache | Debug, release and experimental-only builds passed; includes the existing 900 legacy-publication/eviction stress |
| Automatic tiers | Three builds passed promotion, failed-upgrade suppression, reset/restore, bounded eviction and zero legacy-publisher requests |
| Extended CPU scripts | 37 script invocations passed after the two bounded-module SIMD reruns; original failures were V8 code-space exhaustion |
| Public backend | Node debug and experimental-only backend scenarios passed |
| Build isolation | Default `v86.wasm` has no `ir_*` exports; experimental-only release has no `ir_test_*` or `jit_test_*` exports |
| Coverage | Catalogue check passed; 3,728 production `Pending` remain |
| Default-switch gate | Rejected incomplete coverage as expected; not a pass or a removed gate |

The broader CPU-script matrix and the two bounded SIMD reruns have separate logs
in the PR validation record. Initial failed logs are retained alongside reruns.
NDISASM decoder-boundary comparison was not executed locally because that binary
was absent; CI explicitly installs NASM/NDISASM and runs the oracle. The pinned
historical REP/task reference builds, a fresh real-browser/Worker matrix, XP boot,
game/application loads and performance acceptance were **not** completed here.
C/production linker warnings were present; the warning-as-error claim above
applies specifically to the native Rust test invocation.

`.github/workflows/ir-core.yml` runs the native/Wasm oracles, focused live LICM,
CFG/live/cache/auto/backend regressions and the two large SIMD suites. It retains
diagnostics. It is a validation workflow, not a claim that every original IR-13
system/performance acceptance condition has been satisfied.
