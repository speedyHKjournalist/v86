# Hot region fusion and retained state

The experimental IR runtime can fuse up to four compatible immutable captured
regions into one Tier 2 SSA graph and Wasm function. This is enabled with the IR
backend; it does not change the default legacy backend.

## Selection and execution

The cache records a dominant completed exit (origin instruction, target CPU entry,
saturating heat) for each unfused artifact. Selection requires at least eight
observations at both published sources, compatible CS base and operand mode,
and existing region budgets. Tier 2 promotion tries fusion; an unfused Tier 2 can
also make one later attempt when its peer becomes hot. A failed attempt leaves
the working compiled region available. There are at most four source snapshots,
four predicted edges, 128 decoded instructions and 64 blocks before optimization.

Direct edges inside the union become SSA branches. A profiled indirect edge tests
the actual computed guest target; a miss exits with its original recovery map.
The graph carries GPRs, the full FLAGS/lazy backing tuple, XMM values, retirement
count and memory-effect ordering through block parameters. Successful internal
transitions do not materialize and reload CPU state. Memory/helper observation,
fault recovery, budget exits and final exits retain their existing contracts.

There is one externally guarded cold entry per fused artifact. The other original
entry remains separately compiled. This is bounded trace fusion, **not** a new
register ABI between arbitrary independently compiled Wasm functions, and not
unrestricted multi-entry fusion. x87 state continues to use CPU-owned helpers.

## Invalidation and precise recovery

All source snapshots participate in publication validation, byte/mapping entry
checks, physical code-page watches and generated store guards. A write to either
source, including a physical alias, invalidates or exits the fused artifact.
Pending publication and queued jobs carry the union of dependencies. Snapshot
restore discards these runtime artifacts.

A fused activation snapshots the continuation epoch using the typed
`ir_admission_epoch_address` import. Every instruction recovery/budget poll also
checks that epoch. Observing helpers revoke it before calling out; even a callback
that writes raw code bytes without cache notification therefore exits before the
next instruction. The exit materializes that location's carried state. Epoch
saturation rejects the activation. Existing interrupt-shadow handling still
controls where a poll may exit.

## Diagnostics and tests

`get_jit_info().ir` exposes `fusion_attempts`, `fusion_budget_stops`,
`fusion_unsupported_stops`, `fusion_invalid_stops`, `fused_publications`,
`fused_hits`, `fused_guest_steps`, and `fusion_enabled`. Counters are cumulative
32-bit values. The cold diagnostic `ir_cache_set_fusion(0|1)` supports A/B runs;
disabling retires fused records and rejects pending/queued fused publication.
`ir_cache_entry_stat(linear, cs_base, mode, 10)` reports the number of captured
sources for a published entry.

Run `make ir-fusion-tests ir-auto-tests`. The focused differential has 1,536
interpreter comparisons per test core: 16/32-bit CS-relative execution, optimized
and unoptimized graphs, GPR/FLAGS/XMM, wrapping retirement counts, warm/cold RAM,
stores, prediction misses, callback code mutation, and a fault in the second
source. Extra store checks cover both source pages and a virtual alias of the
peer. The automatic suite requires repeated fused transitions, exact retirement,
peer-write invalidation, restore, and delayed publication cancellation. It runs
with debug/release invariants, production, and the portable no-SIMD core.

For an isolated XP cold-start A/B, use the same core and disk, separate processes,
and no concurrent builds/tests:

```sh
IR_BOOT_MS=60000 IR_FUSION=0 node tests/ir/performance/xp_boot.mjs /path/to/xp.img ir build/v86-ir-runtime.wasm
IR_BOOT_MS=60000 IR_FUSION=1 node tests/ir/performance/xp_boot.mjs /path/to/xp.img ir build/v86-ir-runtime.wasm
```

The benchmark uses a RAM overlay for disk writes. Its average instruction rate
covers startup and idle time; VGA mode changes are milestones, not proof that
the desktop is ready. A single pair of runs is not a stable performance median.

## Local measurements (2026-09-19)

Sequential runs, same production core, recording disabled, no concurrent build
or test load. These are individual runs rather than medians.

| Workload | Fusion off | Fusion on |
| --- | ---: | ---: |
| Two-region indirect loop, 1.5 s warmup + 3 s timed | 39.09 mIPS | 1016.90 mIPS |
| Guest instructions / activation in that loop | 2 | 128 |
| XP cold start, 60 s average | 29.77 mIPS | 29.96 mIPS |
| XP first 800×600×32 mode | 48.115 s | 48.072 s |
| XP IR activations | 167,541,017 | 132,732,137 |

The loop verifies exact final retirement against its register updates. Reproduce
with `node tests/ir/performance/fusion.mjs 0` and `... 1` in separate processes.
Its large gain demonstrates elimination of short-region round trips; it does not
predict OS boot performance.

XP published 184 fused artifacts and retired 479,102,367 guest instructions in
37,883,491 fused activations (about 12.65 instructions per activation). Fusion
reduced total IR activations by about 20.8%, but the 0.6% average-rate difference
and essentially unchanged VGA milestone do **not** establish a material XP speedup.
The source union also increased capture fallbacks from 6,830 to 152,649. Remaining
work includes measuring helper/slow-exit costs and finding safe ways to extend
useful traces across current observation boundaries and overlapping snapshots.
Do not describe this change as restoring legacy-level XP performance.

Raw local logs: `build/xp-boot-fusion-off.jsonl`,
`build/xp-boot-fusion-on.jsonl`, `build/ir-fusion-loop-off.json`,
`build/ir-fusion-loop-on.json`. The XP disk was
`windowsxp/windowsxp_multidisk_C_4G.img`, 2 GiB guest RAM, 16 MiB VGA RAM,
x87 fast math/cache on; this headless test has no graphical renderer or Worker.

The same-core legacy control run averaged **154.68 mIPS** over 60 s and reached
800×600×32 at **19.562 s** (`build/xp-boot-fusion-legacy.jsonl`). All three were
separate sequential runs. This confirms that the XP performance gap remains;
retaining state over short synthetic transitions alone does not close it.

## Bounded extensions (2026-09-20)

Compatible overlapping byte windows are allowed; mismatched bytes/mappings and
middle-of-instruction targets are rejected. Hot exits of fused entries continue
to accumulate heat, allowing one witnessed peer (including its existing sources)
to be added at a time. Four sources, four predictions, eight physical code pages,
and the existing HIR/MIR work limits bound publication.

Extensions beyond two sources currently require a helper-free graph. XP exposed
an interrupt/observer-path regression when arbitrary larger traces were enabled;
that form is rejected before publication and the working shorter trace remains.
This restriction is a remaining optimization gap, not an IR-13 completion claim.
The automatic and differential suites verify four-source helper-free cycles,
carried state, bounded retirement and invalidation of the fourth source.

Known physical-page writes retire affected artifacts and revoke fused execution;
unaffected byte certificates can survive. Host observers/batches still revoke all
byte certificates, and each admission independently checks current TLB identity.
Raw writes, remapping, secondary pages and accessed-bit aliases remain covered by
the cache differential matrix.

## Audited helper extensions (2026-09-20)

Three/four-source graphs now accept code-preserving CPU helpers (including CLI)
and audited register SSE operand forms, rather than rejecting every graph that
contains a helper. The normal ring-0 non-VM86 or real-mode CLI check observes only
backing privilege bits; it keeps GPR/arithmetic state in SSA. Other privilege paths
materialize precise state and revoke admission before the check. SSE still uses
its CR0 guard and selective operand/destination synchronization. Every fault path
exits with authoritative CPU state.

The native fixtures and CPU differential add 192 three/four-source comparisons:
16/32-bit CS-relative loops, CLI/SQRTSS, opt on/off, six budgets, counter wrap,
normal repeated transitions, #GP and #NM. Existing memory/alias/callback cases
remain required. This is not unrestricted helper fusion: STI shadows, generic
host callbacks, descriptor/memory observers still reject extensions beyond two
sources. The earlier unrestricted XP failure is not claimed resolved.
