// Shared Node/main-thread/real Worker checks, exclusively through public V86 APIs.
export async function backend_scenarios(V86, options, log = console.log)
{
    const check = (value, message) => { if(!value) throw new Error(message); };
    const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
    const until = async (test, message) => {
        const deadline = performance.now() + 15000;
        while(!await test()) { check(performance.now() < deadline, message); await sleep(5); }
    };
    const bytes = value => Uint8Array.of(value & 255, value >>> 8 & 255, value >>> 16 & 255, value >>> 24);
    const word = async (vm, address) => {
        const data = await vm.read_memory(address, 4);
        return new DataView(data.buffer, data.byteOffset, data.byteLength).getUint32(0, true);
    };
    const budget = { hot_threshold: 2, promotion_threshold: 4, max_source_bytes: 96, execution_budget: 128, rep_iterations: 8 };
    let vm, ir_snapshot;
    const create = async (extra = {}, expected_error) => {
        vm = new V86({ memory_size: 32 << 20, disable_keyboard: true, disable_mouse: true,
            disable_speaker: true, net_device: { type: "none" }, ...options, autostart: false, ...extra });
        let loaded = false, failure;
        vm.add_listener("emulator-loaded", () => { loaded = true; });
        vm.add_listener("emulator-error", error => { failure = String(error?.message || error); });
        await until(() => loaded || failure, "backend initialization settled");
        if(expected_error)
        {
            check(!loaded && failure.includes(expected_error), "initialization must reject: " + expected_error + "; actual " + failure);
            check(!vm.is_running(), "rejected backend did not start guest");
        }
        else check(!failure, failure);
        return vm;
    };
    const destroy = async () => { if(vm) { await vm.destroy(); vm = null; } };
    const boot = async () => { await vm.run(); await until(async () => await word(vm, 0x500) === 0xCAFE, "protected-mode BIOS boot"); };
    const info = () => vm.get_jit_info();
    const assert_ir = async () => {
        const i = await info();
        check(i.backend === "ir" && i.ir_available && i.ir.enabled === 1, "IR backend is active");
        check(!i.legacy_generation_enabled && i.legacy_compile_requests === 0, "IR mode generated no legacy code");
        check(JSON.stringify(i.ir_region_budget) === JSON.stringify(budget), "all region limits reached CPU");
        return i;
    };
    try
    {
        await create({ jit_backend: "ir", ir_region_budget: budget });
        check(!options.cpu_worker || !vm.v86, "Worker owns the CPU");
        const copied = await info(); copied.ir_region_budget.hot_threshold = 999;
        await assert_ir();
        await boot();
        const pc = 0x1200000;
        await vm.write_memory(Uint8Array.of(0x40, 0xEB, 0xFD), pc);
        const before = await info();
        await vm.write_memory(bytes(pc), 0x600);
        await until(async () => {
            const i = await info(); return i.ir.tier2_published > before.ir.tier2_published && i.ir.cache_hits > before.ir.cache_hits;
        }, "IR Tier 1/2 publish and execute through normal CPU scheduling");
        await vm.stop(); await assert_ir();
        const snapshot = await vm.save_state();
        ir_snapshot = snapshot.slice(0);
        const saved = await info();
        await vm.restore_state(snapshot);
        check((await info()).ir.cache_entries === 0, "snapshot restore invalidated generated code");
        await assert_ir(); await vm.run();
        await until(async () => (await info()).ir.tier2_published > saved.ir.tier2_published, "restore recompiles and promotes IR");
        // End the hot loop with a memory write and RET. The host write must
        // invalidate both tiers before the mailbox function returns.
        await vm.write_memory(Uint8Array.of(0x90, 0xE9, 10, 0, 0, 0, ...Array(10).fill(0x90), 0xA3, 0, 7, 0, 0, 0xC3), pc);
        await until(async () => await word(vm, 0x600) === 0, "self-modified IR loop returned");
        const arithmetic = await word(vm, 0x700);
        check(arithmetic > pc, "IR loop performed guest arithmetic: " + arithmetic);
        // x87 is intentionally not lowered yet: the IR-selected VM must execute
        // it through the interpreter and continue to the independently checked store.
        await vm.write_memory(Uint8Array.of(0xD9, 0xEE, 0xDD, 0xD8, 0xC7, 5, 4, 7, 0, 0, 0x78, 0x56, 0x34, 0x12, 0xC3), pc);
        await vm.write_memory(bytes(pc), 0x600);
        await until(async () => await word(vm, 0x704) === 0x12345678 && await word(vm, 0x600) === 0, "unsupported IR instruction executes through interpreter");
        await vm.stop(); await assert_ir();
        await vm.write_memory(bytes(0), 0x500);
        await vm.restart(); await boot(); await vm.stop(); await assert_ir();
        log("PASS: public IR backend, bounded policy, copied stats, Tier 1/2, SMC, x87 fallback, save/restore/restart, zero legacy requests");
        await destroy();

        await create({ jit_backend: "ir", ir_region_budget: budget, disable_jit: true });
        await boot(); await vm.stop();
        const disabled = await info();
        check(!disabled.legacy_generation_enabled && !disabled.ir.enabled && !disabled.ir.tier1_attempts && !disabled.legacy_compile_requests,
            "disable_jit disables both generators");
        await destroy();
        await create(); await boot();
        await until(async () => (await info()).legacy_compile_requests > 0, "default legacy compiler runs");
        await vm.stop();
        const legacy = await info();
        check(legacy.backend === "legacy" && legacy.legacy_generation_enabled && !legacy.ir.enabled, "default remains legacy");
        await destroy();
        log("PASS: explicit disable_jit and unchanged default legacy backend");

        await create({ initial_state: { buffer: ir_snapshot } });
        const destination = await info();
        check(destination.backend === "legacy" && destination.legacy_generation_enabled && !destination.ir.enabled && !destination.ir.cache_entries,
            "IR snapshot keeps the destination legacy policy and contains no compiled cache");
        await vm.run();
        await until(async () => (await info()).legacy_compile_requests > 0, "legacy recompiles the restored IR guest");
        await vm.stop();
        const legacy_snapshot = await vm.save_state();
        await destroy();
        await create({ jit_backend: "ir", ir_region_budget: budget, initial_state: { buffer: legacy_snapshot } });
        await assert_ir();
        await vm.run();
        await until(async () => (await info()).ir.tier2_published > 0, "IR recompiles the restored legacy guest");
        await vm.stop(); await assert_ir(); await destroy();
        log("PASS: snapshots cross both backend directions and preserve the destination compiler policy");

        for(const [extra, error] of [
            [{ jit_backend: "unknown" }, "jit_backend must"],
            [{ jit_backend: null }, "jit_backend must"],
            [{ jit_backend: "ir", ir_region_budget: { execution_budget: 0 } }, "execution_budget"],
            [{ jit_backend: "ir", ir_region_budget: { hot_threshold: 1.5 } }, "hot_threshold"],
            [{ jit_backend: "ir", ir_region_budget: { promotion_threshold: NaN } }, "promotion_threshold"],
            [{ jit_backend: "ir", ir_region_budget: { max_source_bytes: 4294967296 } }, "max_source_bytes"],
            [{ jit_backend: "ir", ir_region_budget: { rep_iterations: null } }, "rep_iterations"],
            [{ jit_backend: "ir", ir_region_budget: { typo: 1 } }, "Unknown ir_region_budget"],
            [{ ir_region_budget: {} }, "requires jit_backend ir"],
        ]) { await create({ ...extra, autostart: true }, error); await destroy(); }
        log("PASS: invalid backend and budgets report emulator-error before autostart");
        await create({ wasm_path: options.wasm_path.replace(/v86-ir-[^/]+\.wasm$/, "v86.wasm"), jit_backend: "ir", autostart: true }, "requires a core built with ir-experimental");
        await destroy();
        log("PASS: a core without IR rejects the IR backend explicitly");
        if(!options.cpu_worker)
        {
            for(const wasm_fn of [
                () => { throw new Error("synchronous core load failure"); },
                () => Promise.reject(new Error("asynchronous core load failure")),
            ]) { await create({ wasm_fn, autostart: true }, "core load failure"); await destroy(); }
            log("PASS: synchronous and asynchronous custom core failures report emulator-error");
        }
    }
    finally { await destroy(); }
}
