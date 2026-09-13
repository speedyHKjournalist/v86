# IR-10: executable-edge integer constant propagation

Base: `d1b587a2ac1dd4aef7bb8aa33816683df711d053` on `ir`, after PR #29.
This continues IR-10; IR-00--IR-14 are not complete. Production coverage gates,
legacy defaults, CPU ABI, x87/MMX semantics and guest snapshots are unchanged.

## Implementation and contract

`src/rust/ir/passes/sccp.rs` propagates integer constants through block parameters
using an executable-edge, block-granular worklist. Its lattice is Unknown,
Constant, Overdefined. Distinct predecessor literals with equal values can meet
at a constant parameter, exposing arithmetic/FLAGS expressions and conditional
branches to simplification. Different values widen to Overdefined, including a
loop backedge that changes an initially constant induction variable.

Both outcomes of a conditional are tracked separately, even when they target
the same block with different arguments. Every region entry is an independent
root; externally supplied parameters are never specialized from internal edges.
Unresolved reachable conditions conservatively enable both edges before edits
are accepted. Arena order is not treated as a dominance or execution order.

Only integer pure-result instructions are replaced, in place, retaining their
SSA IDs. Parameters are not removed or replaced by instructions: their original
values remain available to entry StateMaps before the first instruction. Memory,
CPU-state reads, helpers, effects, FP/vector values, proofs and RMW tickets are
opaque. No reachable polling or memory/helper operation is moved, hoisted or
eliminated from value facts. Proven-unreachable blocks can be removed.

Branch changes and pure-result constants are installed on a private region
clone, then unreachable blocks are compacted with the existing CFG pass and the
candidate is verified. Any budget/verification error leaves the caller's region
unchanged. Work accounting bounds arenas, use edges, rescans, propagation and the
final state traversal; verifier/CFG routines retain their separate structural
limits. This is not a claim of a new global bound on all compiler allocations.

The pass runs only when `fold`, `phis` and `prune` are all enabled. It reports
`sccp_constants` and `sccp_parameters`; selected branches/unreachable blocks feed
the existing aggregate counters. The latter parameter counter describes facts,
not removed parameters, and can count the same surviving parameter in multiple
pipeline rounds. There is no new public configuration switch.

Literal folding and conditional propagation share `evaluate_integer`, retaining
the existing HIR integer widths, signed comparisons and Wasm shift-count masks.
This change does not claim general FLAGS liveness or state-sync elimination.

## Tests

Native regressions cover lattice laws, equal/different phi inputs, parallel
edges, forced dead predecessors, non-topological block allocation, stable and
changing loop-carried values, independent entry parameters, pass disabling,
transactional budget exhaustion, malformed input, helpers and DCE tombstones.
A delayed predecessor explicitly tests revocation of optimistic phi/branch facts.

The native suite generates 144 Wasm modules: eight CFGs, six execution budgets,
and three modes (unoptimized, SCCP only, full optimization pipeline). The Node
runner requires all fixtures and compares complete StateMaps at budget exits.
It also checks independent arithmetic/CMP FLAGS, loop counts, dynamic EIP and
count bases, invalid entries, and state-boundary canaries. Existing helper,
scalar, decoder and actual CPU differential suites remain enabled unchanged.

Another 480 typed-phi modules cover I1/I8/I16/I32/I64, twelve integer operations,
and four boundary input pairs, before and after SCCP. The JavaScript BigInt
oracle checks signedness, overflow and shift masks independently of the shared
Rust evaluator. In total the new suite requires 624 modules, performs 9,600
normal executions and checks 25,920 invalid-entry calls.

```sh
env RUSTFLAGS="-D warnings" cargo test
node tests/ir/wasm/sccp.mjs
# The standard CI runner imports the new suite:
node tests/ir/wasm/run.mjs
```

## Observed validation

The editing container has Node but no Rust toolchain or repository clone access.
Local checks were Node syntax and whitespace; actual Rust/Wasm/CPU execution
was performed by the repository's existing GitHub Actions workflows.

[IR-core run 34745721650](https://github.com/speedyHKjournalist/v86/actions/runs/34745721650)
passed completely at `a9ab08ec0e64b4114d7cb221fb9c766f153c7771`:

- 171 native tests under `RUSTFLAGS="-D warnings"`, including all 11 new tests.
- 144 CFG modules / 8,640 executions and 480 typed-phi modules / 960 independent
  BigInt executions. Invalid-entry checks and complete StateMaps also passed.
- Existing 78,888 scalar execution comparisons and 80 cross-block FLAGS/budget
  regressions, 3,840 real CPU LICM comparisons, 43,008 reachable bytecode CFG
  comparisons and 21,504 exact optimized/unoptimized budget exits passed.
- 32 second-iteration scalar/vector page-fault comparisons and 16 branches
  skipping absent-page reads passed.
- 48 guarded-read RAM differentials, 22 real fault cases, 18 MMIO/callback
  remapping differentials, 60 merged-block budget exits and CPL guards passed.
- Generated-source checks and whitespace checks passed.

The final follow-up only formats the native test source and records these
results; it does not modify compiler logic or test semantics. Current-head CI
is authoritative for that follow-up, rather than retroactively attributing the
completed run above to a later SHA.

The separate general CI reported rustfmt differences in the new test source;
the follow-up applies its exact formatting changes without weakening checks.
Its initial ESLint job reported 1,048 errors in unchanged files. Complete
repository CI is not asserted green. No workflow or lint rule is changed here.

## Remaining work

Shared decoding, mutable MIR graphs/allocation, remaining ISA and strict FP,
version/link invalidation, full browser/Worker/XP/application and performance
acceptance, and IR-14 legacy-emitter retirement remain incomplete. No game
speedup, XP compatibility, production readiness or complete IR-10 result is
claimed by this increment.
