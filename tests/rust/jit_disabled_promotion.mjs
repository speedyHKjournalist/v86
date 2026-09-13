import assert from "node:assert/strict";
import fs from "node:fs";
import { V86 } from "../../build/libv86.mjs";
const wasm = process.argv[2] || "build/v86-jit-test.wasm";
const vm = new V86({ wasm_path: wasm, memory_size: 32 << 20,
    bios: { buffer: Uint8Array.from(fs.readFileSync("build/cpu-worker-test.bin")).buffer },
    disable_keyboard: true, disable_mouse: true, disable_speaker: true, net_device: { type: "none" } });
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const until = async test => {
    const deadline = performance.now() + 15000;
    while(!test()) { assert(performance.now() < deadline, "disabled promotion test timeout"); await sleep(1); }
};
try
{
    await new Promise((resolve, reject) => { vm.add_listener("emulator-loaded", resolve); vm.add_listener("emulator-error", reject); });
    const cpu = vm.v86.cpu, e = cpu.wm.exports;
    vm.run(); await until(() => cpu.mem32s[0x500 >> 2] === 0xCAFE); await vm.stop();
    cpu.jit_clear_cache(); e.set_jit_config(7, 1);
    const pc = 0x1200000;
    vm.write_memory(Uint8Array.of(0x40, 0xEB, 0xFD), pc);
    cpu.instruction_pointer[0] = pc; cpu.in_hlt[0] = 0;
    const tier1 = e.get_jit_tier1_compiles();
    await new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error("Tier 1 publication timeout")), 15000);
        cpu.test_hook_did_finalize_wasm = () => { clearTimeout(timer); resolve(); };
        assert(e.jit_force_generate_unsafe(pc));
    });
    cpu.test_hook_did_finalize_wasm = undefined;
    assert(e.get_jit_tier1_compiles() > tier1);
    e.set_jit_config(0, 1); e.performance_recording_enable(1);
    const requests = vm.get_jit_info().legacy_compile_requests, promotions = e.get_jit_tier2_compiles();
    let frames = 0;
    const main_loop = cpu.main_loop;
    cpu.main_loop = function() { const result = main_loop.call(this); frames++; return result; };
    vm.run(); await until(() => frames >= 128); await vm.stop();
    assert(e.performance_recording_get(1) > 0, "retained Tier 1 code still executed");
    assert.equal(e.get_jit_tier2_compiles(), promotions, "disabled generation also suppresses promotion");
    assert.equal(vm.get_jit_info().legacy_compile_requests, requests);
    e.set_jit_config(0, 0); vm.run(); await until(() => e.get_jit_tier2_compiles() > promotions); await vm.stop();
    assert(vm.get_jit_info().legacy_compile_requests > requests, "same cached entry promotes after re-enabling generation");
    console.log(`PASS: ${wasm}: disabled generation prevents legacy promotion for 128 CPU frames; re-enable promotes the retained entry`);
}
finally { await vm.destroy(); }
