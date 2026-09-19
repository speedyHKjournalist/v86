# IR performance status

Performance acceptance is still pending. The experimental IR compiler now participates
in live CPU dispatch, supports automatic Tier 1/Tier 2 publication, and is selectable
through the public `jit_backend: "ir"` API. Production default remains legacy, and no
Windows XP, application, game, or Everest performance claim has been established.

`ir-baseline.json` records the pinned pre-IR baseline, host/tool versions, artifact
hash, and initial A/A samples for integer flags, RAM reads, packed SSE and x87
addition. Both sides of those measurements used the same baseline Wasm. Other work
was active, so the samples document variability rather than a controlled speedup.

IR-13 adds a diagnostic cold/warm smoke:

```sh
node tests/ir/performance/smoke.mjs
```

The smoke runs legacy and IR policy on the **same experimental release core** and the
same tiny protected-mode workload. It records core load and guest boot wall time,
time to first legacy compile or IR Tier 1/Tier 2 publication, a short post-promotion
instruction-rate window, IR publication/cache counts, and available performance
recorder code-generation/publication counters. CI stores the output as
`build/ir13-performance-smoke.json`.

There is intentionally **no CI performance threshold** for this smoke. Hosted-runner
timing is noisy, the tiny loop is not an application benchmark, and the recorder
changes observation behavior. Its role is to keep cold and warm phases explicit and
to detect missing/zero instrumentation before controlled workload measurements.

The remaining performance work must separately measure decode/lift/pass/legalize/emit,
Wasm validation/instantiation, peak live locals and metadata, cold first-run cost,
hot execution, invalidation/recompilation, and XP/Everest/WZ/RHO/game workloads.
Paired application runs need the same JS/Wasm build pair, CPU mode, feature policy,
browser, image, guest memory, device configuration and scene, with repeated samples
and reported variability.

No Core 2 Duo comparison or end-to-end application improvement has been established.
