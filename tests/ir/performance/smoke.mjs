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
const median = values => {
    assert(values.length > 0, "median requires values");
    const sorted = [...values].sort((a, b) => a - b);
    const middle = sorted.length >> 1;
    return sorted.length & 1 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};
const bios = Uint8Array.from(fs.readFileSync("build/cpu-worker-test.bin")).buffer;
const loop_pc = 0x1200000;
const base_budget = {
    hot_threshold: 2,
    promotion_threshold: 4,
    max_source_bytes: 96,
    rep_iterations: 8,
};
const execution_budgets = [128, 256, 512, 1024];
const repetitions = 3;
const warm_window_ms = 250;

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

async function warm_rate(vm, duration_ms = warm_window_ms)
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

async function sample_ir(execution_budget, round)
{
    const budget = { ...base_budget, execution_budget };
    const { vm, load_ms } = await create({ jit_backend: "ir", ir_region_budget: budget });
    try
    {
        const boot_ms = await boot(vm);
        const before = await vm.get_jit_info();
        assert.equal(before.backend, "ir");
        assert.equal(before.legacy_generation_enabled, false);
        assert.equal(before.legacy_compile_requests, 0);
        assert.equal(before.ir_region_budget.execution_budget, execution_budget);

        await vm.write_memory(Uint8Array.of(0x40, 0xEB, 0xFD), loop_pc);
        const recorder = new PerformanceRecorder(vm, {
            max_ms: 30000,
            metadata: { purpose: "ir13-execution-budget-matrix", execution_budget, round },
        });
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
        const report = recorder.stop("ir13_budget_matrix");
        const after = await vm.get_jit_info();

        assert.equal(after.backend, "ir");
        assert.equal(after.legacy_generation_enabled, false);
        assert.equal(after.legacy_compile_requests, 0);
        assert(after.ir.cache_hits > before.ir.cache_hits);
        assert.equal(report.metadata.jit_backend, "ir");
        assert(report.execution_counters_available);

        const cache_hits = after.ir.cache_hits - before.ir.cache_hits;
        const cache_guest_steps = (after.ir.cache_guest_steps - before.ir.cache_guest_steps) >>> 0;
        const average_guest_steps_per_activation = cache_guest_steps / Math.max(1, cache_hits);
        return {
            round,
            execution_budget,
            load_ms,
            boot_ms,
            cold_region: { tier1_ms, tier2_ms },
            warm,
            ir: {
                tier1_attempts: after.ir.tier1_attempts - before.ir.tier1_attempts,
                tier1_published: after.ir.tier1_published - before.ir.tier1_published,
                tier2_attempts: after.ir.tier2_attempts - before.ir.tier2_attempts,
                tier2_published: after.ir.tier2_published - before.ir.tier2_published,
                cache_hits,
                cache_cached_checks: after.ir.cache_cached_checks - before.ir.cache_cached_checks,
                cache_capture_fallbacks: after.ir.cache_capture_fallbacks - before.ir.cache_capture_fallbacks,
                cache_guest_steps,
                cache_max_guest_steps: after.ir.cache_max_guest_steps,
                cache_zero_step_exits: (after.ir.cache_zero_step_exits - before.ir.cache_zero_step_exits) >>> 0,
                average_guest_steps_per_activation,
                activation_budget_utilization: average_guest_steps_per_activation / execution_budget,
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

async function sample_legacy(round)
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
            round,
            load_ms,
            boot_ms,
            cold_region: { first_compile_ms },
            warm,
            legacy_compile_requests: after.legacy_compile_requests - before.legacy_compile_requests,
        };
    }
    finally { await vm.destroy(); }
}

function summarize_ir(samples, execution_budget)
{
    assert.equal(samples.length, repetitions, "IR budget matrix requires every repetition");
    return {
        samples: samples.length,
        execution_budget,
        median_load_ms: median(samples.map(sample => sample.load_ms)),
        median_boot_ms: median(samples.map(sample => sample.boot_ms)),
        median_tier1_ms: median(samples.map(sample => sample.cold_region.tier1_ms)),
        median_tier2_ms: median(samples.map(sample => sample.cold_region.tier2_ms)),
        median_instruction_steps_per_ms: median(samples.map(sample => sample.warm.instruction_steps_per_ms)),
        median_cache_hits: median(samples.map(sample => sample.ir.cache_hits)),
        median_average_guest_steps_per_activation:
            median(samples.map(sample => sample.ir.average_guest_steps_per_activation)),
        median_activation_budget_utilization:
            median(samples.map(sample => sample.ir.activation_budget_utilization)),
        median_cache_max_guest_steps: median(samples.map(sample => sample.ir.cache_max_guest_steps)),
        median_cache_zero_step_exits: median(samples.map(sample => sample.ir.cache_zero_step_exits)),
        median_cache_capture_fallbacks: median(samples.map(sample => sample.ir.cache_capture_fallbacks)),
    };
}

function summarize_legacy(samples)
{
    assert.equal(samples.length, repetitions, "legacy matrix requires every repetition");
    return {
        samples: samples.length,
        median_load_ms: median(samples.map(sample => sample.load_ms)),
        median_boot_ms: median(samples.map(sample => sample.boot_ms)),
        median_first_compile_ms: median(samples.map(sample => sample.cold_region.first_compile_ms)),
        median_instruction_steps_per_ms: median(samples.map(sample => sample.warm.instruction_steps_per_ms)),
        median_legacy_compile_requests: median(samples.map(sample => sample.legacy_compile_requests)),
    };
}

const ir_runs = Object.fromEntries(execution_budgets.map(budget => [String(budget), []]));
const legacy_runs = [];
for(let round = 0; round < repetitions; round++)
{
    legacy_runs.push(await sample_legacy(round));
    const order = round & 1 ? [...execution_budgets].reverse() : execution_budgets;
    for(const execution_budget of order)
    {
        ir_runs[String(execution_budget)].push(await sample_ir(execution_budget, round));
    }
}

const ir_summary = {};
for(const execution_budget of execution_budgets)
{
    ir_summary[String(execution_budget)] = summarize_ir(ir_runs[String(execution_budget)], execution_budget);
}
const legacy_summary = summarize_legacy(legacy_runs);
const baseline_128 = ir_summary["128"].median_instruction_steps_per_ms;
for(const execution_budget of execution_budgets)
{
    const summary = ir_summary[String(execution_budget)];
    summary.throughput_relative_to_ir_128 = summary.median_instruction_steps_per_ms / baseline_128;
    summary.throughput_relative_to_legacy =
        summary.median_instruction_steps_per_ms / legacy_summary.median_instruction_steps_per_ms;
}

const result = {
    format: "v86-ir13-execution-budget-matrix",
    version: 2,
    policy: {
        note: "Diagnostic matrix only; CI timing has no release threshold and is not an end-to-end speed claim.",
        core: "build/v86-ir-runtime.wasm",
        base_ir_region_budget: base_budget,
        execution_budgets,
        repetitions,
        warm_window_ms,
        fresh_vm_per_sample: true,
        order: "legacy once per round; IR budgets alternate ascending/descending to reduce fixed order bias",
    },
    summary: {
        legacy: legacy_summary,
        ir: ir_summary,
    },
    raw: {
        legacy: legacy_runs,
        ir: ir_runs,
    },
};
console.log(JSON.stringify(result, null, 2));
