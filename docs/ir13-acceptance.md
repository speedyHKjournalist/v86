# IR-13 acceptance matrix

IR-13 is an acceptance and end-to-end tuning stage. It is **not complete** merely
because the IR-10 through IR-12 unit/differential suites are green.

The first automated IR-13 gate is:

```sh
sh tools/ir13-smoke-tests.sh
```

It intentionally reuses production-facing public APIs and existing host/device
tests instead of introducing a smaller synthetic-only harness.

## Automated cells in this gate

| Host / path | IR core | Coverage |
|---|---|---|
| Node main thread | invariant debug + experimental release | backend selection, Tier 1/2, SMC, interpreter fallback, save/restore/restart, backend-crossing snapshots, invalid configuration, zero legacy generation |
| Chromium main thread | experimental release | same public backend scenario through browser loading and normal CPU dispatch |
| Chromium dedicated CPU Worker | experimental release | public backend scenario, Worker ownership, Tier 1/2, SMC, snapshots, zero legacy generation |
| Chromium dedicated CPU Worker + device transport | experimental release | DOM keyboard make/break + mouse movement through Worker into guest PS/2, VGA/canvas, virtio graphics backpressure, disk/filesystem RPC, SB16 PCM, save/restore, rejected-restore recovery, main-thread/Worker snapshot interchange |
| Chromium AudioWorklet + CPU Worker | experimental release | AudioWorklet construction plus save/restore while the Worker CPU uses the IR backend |
| Node cold/warm diagnostic | experimental release, paired policies | same-core legacy/IR load + boot, first compile/Tier 1/Tier 2 publication, short warm instruction-rate window and recorder/codegen counters; no CI speed threshold |

The Worker/device and AudioWorklet pages are parameterized. Their default invocation
continues to exercise the existing legacy production core; adding
`?jit_backend=ir` selects `build/v86-ir-runtime.wasm` and requires the public
runtime to report IR enabled with legacy generation disabled and zero legacy
compile requests.

## Why this belongs to IR-13 and also feeds IR-09

This matrix validates the completed backend/cache machinery in the environments
where production failures can arise: browser module loading, Worker RPC, device
callbacks, graphics batching, audio transport and snapshot generation changes.
Failures here should be traced back to the owning earlier package rather than being
papered over in IR-13. In particular:

- decode/lowering misses go back to IR-02 and IR-05 through IR-08;
- StateMap/helper/exception mismatches go back to IR-03/IR-04;
- Tier-1 execution, structuring or exit failures go back to IR-09;
- publication, invalidation and stale callbacks go back to IR-12.

The gate therefore extends IR-09 system-level evidence, but it does not declare
IR-09 complete while ISA/Tier-1 coverage remains incomplete.

## Remaining IR-13 completion cells

This initial gate does **not** satisfy the full IR-13 completion condition. Still
required are:

- Windows XP boot under the online IR backend;
- fixed application/game loading scenarios;
- controlled release cold-start versus warm-run measurements on fixed OS/application workloads (the CI smoke records instrumentation only);
- compile/lift/pass/lower/emit/install timing and code-size/local/StateMap metrics;
- paired Everest 5.50 Queen/PhotoWorxx/ZLib/AES runs;
- workload regression thresholds and failure minimization artifacts;
- completion-driven backfills for remaining IR-02 through IR-09 ISA and Tier-1 gaps.

IR-14 remains blocked until those acceptance cells are complete, production
coverage has no unsupported baseline forms, IR Tier 1/Tier 2 can become the
default, and the legacy emitter can be removed from the production path.
