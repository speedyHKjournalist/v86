import assert from "node:assert/strict";
import fs from "node:fs";
import { performance } from "node:perf_hooks";
import { V86 } from "../../../build/libv86.mjs";
import { PerformanceRecorder } from "../../../src/browser/performance_recorder.js";

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const until = async (test, message, timeout = 15000) => {
    const deadline = performance.now() + timeout;
    while(!await test())
    {
        assert(performance.now() < deadline, message);
        await sleep(5);
    }
};
const bytes = value => Uint8Array.of(value & 255, value >>> 8 & 255, value >>> 16 & 255, value >>> 24);
const word = async (vm, address) => {
    const data = await vm.read_memory(address, 4);
    return new DataView(data.buffer, data.byteOffset, data.byteLength).getUint32(0, true);
};
const counter = async vm => Number(await vm.get_instruction_counter()) >>> 0;
const delta32 = (after, before) => (after - before) >>> 0;
const bios = Uint8Array.from(fs.readFileSync("build/cpu-worker-test.bin")).buffer;
const loop_pc = 0x1200000;
const smoke_budget = { hot_threshold: 2, promotion_threshold: 4, max_source_bytes: 96,
    execution_budget: 128, rep_iterations: 8 };

async function create(extra)
{
    const started = performance.now();
    const vm = new V86({
        wasm_path: "build/v86-ir-runtime.wasm",
        bios: { buffer: bios.slice(0) },
        memory_size: 32 << 20,
        disable_keyboard: true,
        disable_mouse: true,
        disable_speaker: true,
        net_device: { type: "none" },
        autostart: false,
        ...extra,
    });
    let loaded = false, failure;
    vm.add_listener("emulator-loaded", () => { loaded = true; });
    vm.add_listener("emulator-error", error => { failure = String(error?.message || error); });
    await until(() => loaded || failure, "performance smoke core initialization");
    assert(!failure, failure);
    return { vm, load_ms: performance.now() - started };
}

async function boot(vm)
{
    const started = performance.now();
    await vm.run();
    await until(async () => await word(vm, 0x500) === 0xCAFE, "performance smoke protected-mode boot");
    return performance.now() - started;
}

async function warm_rate(vm, duration_ms = 250)
{
    await sleep(25);
    const before = await counter(vm);
    const started = performance.now();
    await sleep(duration_ms);
    const elapsed_ms = performance.now() - started;
    const after = await counter(vm);
    const steps = delta32(after, before);
    assert(steps > 0, "warm window must execute guest instructions");
    return { elapsed_ms, instruction_steps: steps, instruction_steps_per_ms: steps / elapsed_ms };
}

async function sample_ir()
{
    const { vm, load_ms } = await create({ jit_backend: "ir", ir_region_budget: smoke_budget });
    try
    {
        const boot_ms = await boot(vm);
        const before = await vm.get_jit_info();
        assert.equal(before.backend, "ir");
        assert.equal(before.legacy_generation_enabled, false);
        assert.equal(before.legacy_compile_requests, 0);

        await vm.write_memory(Uint8Array.of(0x40, 0xEB, 0xFD), loop_pc);
        const recorder = new PerformanceRecorder(vm, { max_ms: 30000, metadata: { purpose: "ir13-cold-warm-smoke" } });
        recorder.start();
        const cold_started = performance.now();
        await vm.write_memory(bytes(loop_pc), 0x600);

        await until(async () => (await vm.get_jit_info()).ir.tier1_published > before.ir.tier1_published,
            "IR Tier 1 publication in performance smoke");
        const tier1_ms = performance.now() - cold_started;
        await until(async () => {
            const info = await vm.get_jit_info();
            return info.ir.tier2_published > before.ir.tier2_published && info.ir.cache_hits > before.ir.cache_hits;
        }, "IR Tier 2 publication/cache execution in performance smoke");
        const tier2_ms = performance.now() - cold_started;
        const warm = await warm_rate(vm);
        await vm.stop();
        const report = recorder.stop("ir13_smoke");
        const after = await vm.get_jit_info();

        assert.equal(after.backend, "ir");
        assert.equal(after.legacy_generation_enabled, false);
        assert.equal(after.legacy_compile_requests, 0);
        assert(after.ir.cache_hits > before.ir.cache_hits);
        assert.equal(report.metadata.jit_backend, "ir");
        assert(report.execution_counters_available);

        return {
            load_ms,
            boot_ms,
            cold_region: { tier1_ms, tier2_ms },
            warm,
            ir: {
                tier1_attempts: after.ir.tier1_attempts - before.ir.tier1_attempts,
                tier1_published: after.ir.tier1_published - before.ir.tier1_published,
                tier2_attempts: after.ir.tier2_attempts - before.ir.tier2_attempts,
                tier2_published: after.ir.tier2_published - before.ir.tier2_published,
                cache_hits: after.ir.cache_hits - before.ir.cache_hits,
                cache_cached_checks: after.ir.cache_cached_checks - before.ir.cache_cached_checks,
                cache_capture_fallbacks: after.ir.cache_capture_fallbacks - before.ir.cache_capture_fallbacks,
            },
            recorder: {
                duration_ms: report.duration_ms,
                sync_codegen_ms: report.execution?.sync_codegen_ms ?? null,
                sync_codegen_calls: report.execution?.sync_codegen_calls ?? null,
                jit_compile_publish_latency_ms: report.summary.jit_compile_publish_latency_ms,
                jit_compile_publish_max_ms: report.summary.jit_compile_publish_max_ms,
                jit_wasm_bytes: report.summary.jit_wasm_bytes,
            },
        };
    }
    finally { await vm.destroy(); }
}

async function sample_legacy()
{
    // Use the same experimental release core so host/core build differences do not
    // contaminate this smoke. Only the selected compiler policy differs.
    const { vm, load_ms } = await create({});
    try
    {
        const boot_ms = await boot(vm);
        const before = await vm.get_jit_info();
        assert.equal(before.backend, "legacy");
        assert.equal(before.legacy_generation_enabled, true);

        await vm.write_memory(Uint8Array.of(0x40, 0xEB, 0xFD), loop_pc);
        const cold_started = performance.now();
        await vm.write_memory(bytes(loop_pc), 0x600);
        await until(async () => (await vm.get_jit_info()).legacy_compile_requests > before.legacy_compile_requests,
            "legacy compilation in paired performance smoke");
        const first_compile_ms = performance.now() - cold_started;
        const warm = await warm_rate(vm);
        await vm.stop();
        const after = await vm.get_jit_info();
        return {
            load_ms,
            boot_ms,
            cold_region: { first_compile_ms },
            warm,
            legacy_compile_requests: after.legacy_compile_requests - before.legacy_compile_requests,
        };
    }
    finally { await vm.destroy(); }
}

const result = {
    format: "v86-ir13-performance-smoke",
    version: 1,
    policy: {
        note: "Diagnostic smoke only; CI timing has no release threshold and is not an end-to-end speed claim.",
        core: "build/v86-ir-runtime.wasm",
        ir_region_budget: smoke_budget,
        warm_window_ms: 250,
    },
    ir: await sample_ir(),
    legacy: await sample_legacy(),
};
console.log(JSON.stringify(result, null, 2));
