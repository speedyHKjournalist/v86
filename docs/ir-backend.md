# Public experimental IR backend

`jit_backend: "ir"` selects automatic IR compilation and disables legacy code
generation. Unsupported instructions continue through the existing interpreter.
The default is still `"legacy"`; this option does not satisfy the IR-14 default
cutover or the complete Windows XP and application acceptance gates.

Build the experimental core with `make build/v86-ir-runtime.wasm`. Pass its URL
as `wasm_path` in a browser, Node, or CPU Worker configuration:

```js
const emulator = new V86({
    wasm_path: "build/v86-ir-runtime.wasm",
    jit_backend: "ir",
    ir_region_budget: {
        hot_threshold: 16,
        promotion_threshold: 64,
        max_source_bytes: 192,
        execution_budget: 256,
        rep_iterations: 64,
    },
    // BIOS, disks, display and other normal V86 options go here.
    // cpu_worker: true,
    // cpu_worker_url: "build/cpu-worker.js",
});
emulator.add_listener("emulator-error", error => console.error(error));
emulator.add_listener("emulator-loaded", async () => {
    console.log(await emulator.get_jit_info());
});
```

The budget object is optional; the example shows every default. Heat thresholds
accept integers 1–1,000,000; source bytes accept 15–960; execution and REP budgets
accept 1–4,096. Tier 2 doubles the byte window with a 960-byte cap. The independent
instruction, CFG, cache and per-frame compiler bounds in [ir-auto.md](ir-auto.md)
still apply. Unknown budget keys, invalid numbers and a budget supplied for the
legacy backend are rejected. No JS-to-Wasm integer truncation is used to validate
these options.

`disable_jit: true` overrides either selected compiler: IR scheduling and legacy
generation are disabled. Selecting IR still requires an IR-capable core. A
missing IR core, unsupported backend or invalid budget emits `emulator-error`
before `emulator-loaded` or guest autostart, in either execution mode.

Backend selection is a constructor policy. It is not serialized in guest
snapshots and is not a public runtime switching API. Reset/restart and snapshot
restore keep the destination VM's policy while invalidating compiled code.
The existing low-level IR test/explicit compilation interfaces are separate;
changing them can override the constructor's automatic policy.

`get_jit_info()` returns a copy immediately for a main-thread CPU and a Promise
for a CPU Worker. It reports the selected backend, actual legacy generation
switch, IR availability/enabled state, effective budgets, legacy publication
requests, automatic compilation/promotion counters and IR cache entries/hits.
Legacy requests count calls into the JS publication bridge, including failures;
they are not a timing metric. IR counters wrap at 2^32 and continue across cache
clear/reset/restore. No guest RAM, mutable CPU state, or Wasm table is exposed by
the Worker RPC. Performance recordings include the actual selected backend as
`metadata.jit_backend`, alongside the existing core SHA-256 when available.

The remaining plan options (`ir_opt_level`, `ir_verify`, `ir_dump`, pass disable
lists and selectable stats modes) are not wired into this public policy yet.
Tier 1 currently uses unoptimized lowering, and Tier 2 uses the existing standard
pass pipeline; optimization and verifier controls must gain actual semantics
before they become public options.

Validation entry points:

* `make ir-backend-integration-tests`: shared public API scenarios on invariant
  debug and pure experimental release cores.
* `make ir-backend-browser-tests`: the same scenarios in Chromium's main thread
  and a real dedicated CPU Worker, using an isolated browser profile.
* `make ir-backend-tests`: the pre-existing standalone Wasm emitter fixtures;
  this is distinct from public backend selection.
