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

The editing container has Node but no Rust toolchain or repository clone access.
Only Node syntax checks were available locally when preparing this change.
Actual results for the initial implementation at `1b66da3` are recorded in
[IR-core run 34745211129](https://github.com/speedyHKjournalist/v86/actions/runs/34745211129):
169 native tests passed under `-D warnings`, all 144 CFG modules and 8,640 new
executions passed, as did the existing 3,840 CPU LICM comparisons, 43,008 CFG
comparisons, 21,504 exact budget exits, real page faults and guarded RAM/MMIO
callback recovery. The additional typed-phi suite must pass on the final PR
head; the initial run does not validate tests added afterward.

The separate CI initially reported rustfmt differences in the new Rust files;
these were corrected without weakening the check. Its ESLint job reported
1,048 errors in unchanged files. Complete repository CI is not asserted green.

## Remaining work

Shared decoding, mutable MIR graphs/allocation, remaining ISA and strict FP,
version/link invalidation, full browser/Worker/XP/application and performance
acceptance, and IR-14 legacy-emitter retirement remain incomplete. No game
speedup, XP compatibility, production readiness or complete IR-10 result is
claimed by this increment.
