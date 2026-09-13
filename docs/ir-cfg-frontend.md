# Reachable guest bytecode CFG frontend

`frontend::region::lift_cpu_cfg` and `runtime::compile::compile_cpu_cfg_region`
compile reachable direct guest control flow into the existing HIR/MIR pipeline.
The latter accepts the same immutable code/dependency snapshot and publication
key as the linear compiler. Neither API installs an online Tier entry.

The frontend discovers instructions from offset zero, following decoded branch
exits and fallthroughs inside the supplied snapshot. Bytes skipped by a direct
jump are not decoded. Out-of-snapshot and dynamic targets stay explicit exits.
Overlapping instruction streams, reachable unsupported/truncated instructions,
and exhausted budgets reject compilation. This is one entry with per-instruction
blocks, not general multi-entry region selection. The default optimization pipeline now
[merges straight blocks while preserving budget polls](ir-cfg-merge.md).
The limits are 128 decoded instructions, 1,920 snapshot bytes, 64 HIR blocks,
8,192 instructions and 16,384 values, followed by existing lowering/emission
limits. Conditional branch leaf blocks count toward the block limit.

Each guest instruction reuses the audited single-instruction lifter. The graft
checks its entry initialization contract, remaps values, blocks, helper IDs and
all observation/commit states, and replaces initial GPR/FLAGS/XMM reads with
incoming typed SSA parameters. Entry-only selector/stack-mode reads are captured
in the prologue; guest operations that can change that state terminate through
CPU adapters. Normal memory adapters retain their existing state-preservation
contract. An effect parameter connects each path, including conditional leaves.

The frame carries all GPRs, arithmetic FLAGS, system flags, last_op1, raw/lazy ZF,
the active XMM file, effect and an i32 relative retirement count. Every internal
successor adds the fragment's committed offset to that count. Recovery states
refer to the incoming dynamic base; completed leaf states preserve their static
offset. These representations agree at budget boundaries and faults. Dispatcher
work remains one unit per HIR block, including the entry prologue and branch
leaves; it is distinct from retired guest instructions.

Stores/RMW commits, generic CPU-owned helper calls and CMPXCHG8B retain terminal
completion. This conservatively avoids continuing past a possible code write or
mode change. XMM cold-load adapters may exit after CPU-owned completion while
the native RAM path continues; all inherited vector state remains in snapshots.
No speculative memory/code fetches or new device operations are introduced.

`make ir-cfg-tests` generates optimized/unoptimized modules at eight execution
budgets, two CPU modes and wrapping/nonwrapping EIPs. Fourteen byte programs cover
integer/FLAGS loops, LOOP/JCXZ, self-jumps, a diamond, partial-register prefixes,
selector reads, unreachable invalid bytes, RAM/XMM state and terminal stores and
CPUID. 35,328 executions compare the resulting state with 173,056 actual CPU
interpreter steps. Explicit self-loop retirement oracles independently check the
count at budget exits. Sixteen second-iteration scalar/vector #PF cases verify
five committed instructions before the sixth faults, including dirty GPR/FLAGS/
XMM, nonzero CS base, exception frames and global counter wrapping. Rust tests
also reject overlap/truncation/budget errors and stale dependency versions.

This adds automatic direct CFG lifting to the previously hand-built dynamic
count fixtures. It does not complete online scheduling/interrupts, publication,
invalidation, ISA coverage, independent MIR graph transformations, OS acceptance
or IR-00–IR-14. Production Pending remains 3,728 and production uses legacy JIT.

The connected regression matrix passed in `build/ir-cfg-full-suite.log`, including
113 warnings-as-errors Rust tests, all existing IR and independent-reference
execution targets and final FLAGS observers. `build/ir-cfg-check.log` records
successful experimental Wasm compilation. Catalogue, production export isolation
and whitespace checks pass. No compiler source changed during these checks.

The [dataflow passes](ir-dataflow.md) now reuse dominating pure values and prune
constant branches. The expanded bytecode suite covers seventeen programs and
39,936 CPU comparisons, including constant branches over absent-page reads.
