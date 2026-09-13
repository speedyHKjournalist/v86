# Bounded pure-SSA LICM (IR-11, partial)

This implements loop-invariant code motion, not completion of IR-11 or IR-00–IR-14.
Production still defaults to the legacy backend. ISA coverage, memory proofs and
system/performance release gates are unchanged.

## Integration

`passes::run` runs LICM after its existing bounded cleanup rounds and then, when
motion occurred, repeats enabled GVN/DCE cleanup. `PassConfig.licm` defaults to
`true`; setting it to `false` preserves the old pass selection. `rounds: 0` still
disables all transformation. The runtime only enters this pipeline when
`IrConfig.optimize` is true (the automatic scheduler's Tier 2). There is no new
interpreter fallback, default-backend change, CPU ABI change or snapshot format.

`passes::licm::run` also accepts an independent `LicmConfig { max_work }`; its
default is 1,000,000 accounted work units, not a wall-clock time limit. Fixed
block/instruction/value/state/helper caps bound admitted compiler arenas. Work
exhaustion is a compilation error rather than partial publication.

`PassStats.loops`, `hoisted` and `loop_work` expose the pass statistics. `loops`
counts discovered natural loops, including loops without an eligible preheader.
`hoisted` counts motions: the same instruction can leave two nested loops.

## Safety boundary

A dominator-qualified backedge identifies a natural loop; all latches with the
same header are unioned. The pass requires a unique existing unconditional
preheader and rejects side entries and loops containing external entries. It
processes inner loops first and preserves the CFG, value IDs, instruction IDs,
recovery maps, guest instruction accounting and effect chains.

Only explicitly whitelisted total scalar and vector SSA operations may move.
Each operand must already dominate the preheader or come from an earlier planned
motion. CPU reads are not speculative even though they are not ordered. Memory
accesses/checks, division, helpers, SSE checks, polls and operations with attached
recovery/commit metadata never move. No RAM, alias, permission or MMIO proof is
inferred. In particular, this does not implement load forwarding or elimination.

The input is verified first. Changes are staged on a clone and committed only
after post-transform verification. Invalid input, budget exhaustion or failed
verification leaves the caller's entire region unchanged. This transaction is
specific to the LICM API; it does not retroactively make all earlier passes
transactional.

## Reproduction

```sh
make ir-licm-tests
# Full native and emitted-Wasm semantic coverage:
env RUSTFLAGS="-D warnings" cargo test
node tests/ir/wasm/run.mjs
node tests/rust/verify-wasmgen-dummy-output.js
# Requires NASM/ndisasm, independently of the compiler:
node tests/ir/decode/oracle.mjs
```

The dedicated target generates the baseline Rust dispatch tables on a clean
checkout. The nine Rust regressions cover invariant chains, loop-carried values,
scalar/vector lowering, CPU reads, attached recovery, guest loads, helper effects,
conditional/ambiguous preheaders, external/irreducible loops, nested/multi-latch
loops, budget failure atomicity and configuration gates.

The execution oracle runs 31,104 cases: unoptimized, LICM-only and full-pipeline
modules; scalar and vector CPU-ABI variants; nine execution budgets; overflow,
zero-trip and bounded very-long-loop inputs. It independently computes accumulator,
remaining work, recovery PC and committed count, and compares complete 4 KiB state
images. This is not an XP boot or game benchmark and does not establish a speedup
on the user's applications.
