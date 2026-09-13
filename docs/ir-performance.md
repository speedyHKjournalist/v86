# IR performance status

Performance acceptance is pending. The IR compiler is not integrated into the
live CPU tiers, so existing JIT tier benchmark output must not be attributed to IR.

`ir-baseline.json` records the pinned baseline, host/tool versions, artifact hash,
and initial A/A samples for integer flags, RAM reads, packed SSE and x87 addition.
Both sides of those measurements used the same baseline Wasm. Other work was
active, so the samples document variability rather than a controlled speedup.

The next performance work must separately measure decode/lift/pass/legalize/emit,
Wasm validation/instantiation, peak live locals and metadata, cold first-run cost,
hot execution, invalidation/recompilation, and XP/Everest/WZ/RHO/game workloads.
Paired runs need the same JS/Wasm build pair, CPU mode, feature policy, browser,
image, guest memory, device configuration and scene. No Core 2 Duo comparison or
end-to-end application improvement has been established.
