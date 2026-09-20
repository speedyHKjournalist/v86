# Public experimental IR backend

`jit_backend: "ir"` selects automatic IR compilation and disables legacy code
generation. Unsupported instructions continue through the existing interpreter.
The default is still `"legacy"`; this option does not satisfy the IR-14 default
cutover or the complete Windows XP and application acceptance gates.

The stock `index.html` also accepts `?jit_backend=ir` (or append
`&jit_backend=ir` to an existing URL). Manual **Start Emulation** preserves the
selection in the rebuilt URL, and both main-thread and Worker starts load
`build/v86-ir-runtime.wasm`. Without the parameter, the page keeps its normal
legacy core. Build the page, Worker, and experimental cores before using this:

```sh
make build/v86_all.js build/cpu-worker.js build/v86-ir-runtime.wasm build/v86-ir-runtime-fallback.wasm
```

Reload the page after rebuilding; an already-running VM keeps its constructor
policy. Selecting IR in the debug page currently also uses the experimental
release runtime core.

Build the experimental core with `make build/v86-ir-runtime.wasm`. Pass its URL
as `wasm_path` in a browser, Node, or CPU Worker configuration:

```js
const emulator = new V86({
    wasm_path: "build/v86-ir-runtime.wasm",
    jit_backend: "ir",
    ir_opt_level: 2,
    ir_passes_disabled: [],
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

`ir_opt_level` accepts 0 (no optional optimization), 1 (Tier-1 canonicalization
at both tiers), or 2 (the default Tier-1/Tier-2 pipeline). Heat/promotion and
correctness checks still operate at every level. Level 0 may hit compilation
budgets earlier because simplification is disabled; normal interpreter fallback
still applies. These controls govern automatic compilation; explicit test compile
APIs keep their own parameters.

`ir_passes_disabled` is a list of unique pass names: `prune`, `merge`, `phis`,
`copy`, `fold`, `flags`, `helper_state`, `gvn`, `dce`, `licm`, `mir_fold`, `stack`,
`allocation`, `state_elision`, `ram_loop`, `ram_forward`, `ram_guard`. Disabling a
pass does not enable it at a lower optimization level. `allocation` controls the
optional post-lowering allocation pass, not the required initial typed locals.
Loop caching does not implicitly enable a disabled forwarding pass. Unknown or
duplicate names, invalid levels and IR controls on a legacy backend are rejected
before autostart. All verifier, StateMap, fault and cache admission checks remain.

Both options pass through the real Worker initialization path, appear as copied
effective policy in `get_jit_info()`, and survive reset/restore as destination
policy. They require rebuilding the experimental (including fallback) core.
The stock page accepts and preserves, for example,
`&jit_backend=ir&ir_opt_level=1&ir_passes_disabled=gvn,ram_forward` across manual
Start Emulation. Constructor arrays use the ordinary JavaScript list syntax.

`ir_verify` accepts `off`, `debug` (default), or `every_pass`. Necessary lowering,
import, recovery, RAM-proof and publication checks remain enabled in every mode.
`debug` additionally runs the independent owned-MIR verifier at compiler boundaries
in assertion-enabled cores. `every_pass` runs it after lowering and after each
machine pass even in release cores. The normal release default adds no graph
verification to the hot compilation path. Verification covers dominance, operand
availability, local interference, typed edge copies, recovery expressions and
low-level import/RAM certificates without retaining HIR.

`ir_dump` accepts `off` (default), `hir`, `mir`, `wasm`, or `all`. The optimized HIR,
final MIR and emitted module can be inspected using `await emulator.get_ir_dumps()`.
Records describe completed compilations, not necessarily published or executed
artifacts. The ring holds the newest 16 records, at most 64 KiB each of HIR/MIR
text and 256 KiB of Wasm per record. A record has `pc`, `tier`, `hir`, `mir`, `wasm`
(`Uint8Array`) and a `truncated` mask (HIR=1, MIR=2, Wasm=4). Truncated Wasm is not
an executable module. Text truncation preserves UTF-8 boundaries. Results are
copies; `get_ir_dumps(true)` copies and then clears the ring synchronously in the
CPU owner. Worker RPC returns the same independent data. Dumps are not included
in guest snapshots; retained records may precede a reset/restore.

`ir_stats` accepts `off` (default), `sampled` (period 128) or `debug` (period 1).
It configures the existing diagnostic session at startup. Basic scheduler/cache
counters remain available with statistics off. `configure_ir_diagnostics(period)`
can subsequently choose another sample period; `get_jit_info().ir_stats` reports
the effective off/debug/sampled category and `ir.diagnostics.sample_period` the
exact period. See [timing semantics](ir-diagnostics.md).

All three options are constructor controls for IR, pass through the Worker, are
reported by `get_jit_info()`, and are preserved by the stock page across manual
Start Emulation. For example:
`&jit_backend=ir&ir_verify=every_pass&ir_dump=all&ir_stats=sampled`.
Use default verification, dumps off and statistics off for performance acceptance;
inspection and every-pass verification intentionally add compilation overhead.

Validation entry points:

* `make ir-backend-integration-tests`: shared public API scenarios on invariant
  debug and pure experimental release cores.
* `make ir-backend-browser-tests`: the same scenarios in Chromium's main thread
  and a real dedicated CPU Worker, using an isolated browser profile.
* `make ir-backend-tests`: the pre-existing standalone Wasm emitter fixtures;
  this is distinct from public backend selection.
