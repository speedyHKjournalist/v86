import assert from "node:assert/strict";
import fs from "node:fs";
import { V86 } from "../../build/libv86.mjs";
const bios = Uint8Array.from(fs.readFileSync("build/x87-fast-test.bin")).buffer;
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
async function run(wasm, interpreted) {
    const vm = new V86({ wasm_path: wasm, bios: { buffer: bios }, memory_size: 32 << 20,
        disable_jit: interpreted, disable_keyboard: true, disable_mouse: true,
        disable_speaker: true, net_device: { type: "none" }, autostart: false });
    try {
        await new Promise(resolve => vm.add_listener("emulator-loaded", resolve));
        const e = vm.v86.cpu.wm.exports;
        e.performance_recording_enable(1);
        const cycles = () => { const bytes = vm.read_memory(0x400, 2); return bytes[0] | bytes[1] << 8; };
        const until = async predicate => {
            const deadline = performance.now() + 10000;
            while(!predicate()) {
                assert(performance.now() < deadline, "x87 guest timed out");
                await sleep(10);
            }
        };
        vm.run();
        await until(() => cycles() >= 20 && (interpreted || e.performance_recording_get(1) > 0));
        await vm.stop();
        const result = Uint8Array.from(vm.read_memory(0x500, 12 * (9 * 6 * 12 + 16 * 18 + 12 * 6)));
        const state = await vm.save_state();
        const before = cycles();
        vm.run();
        await until(() => cycles() > before + 5);
        await vm.stop();
        await vm.restore_state(state);
        assert.equal(cycles(), before, "save/restore preserves the guest timeline");
        vm.run();
        await until(() => cycles() > before + 5);
        await vm.stop();
        assert.deepEqual(Uint8Array.from(vm.read_memory(0x500, result.length)), result,
            "x87 arithmetic after state restore preserves results and status words");
        return result;
    } finally { await vm.destroy(); }
}
const candidate = process.argv[3] || "build/v86.wasm";
const interpreted = await run(candidate, true);
const jit = await run(candidate, false);
assert.deepEqual(jit, interpreted, "interpreter and actual JIT must match arithmetic/comparison and binary32 conversion results/status");
if(process.argv[2]) {
    assert.deepEqual(jit, await run(process.argv[2], false), "before/after Wasm results must match");
}
console.log("PASS: actual x87 instructions, all 12 controls, interpreter/JIT, binary32 loads/stores, 80-bit stores and save/restore" +
    (process.argv[2] ? "; previous Wasm matches" : ""));
