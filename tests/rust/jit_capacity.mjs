import assert from "node:assert/strict";
import fs from "node:fs";
import { V86 } from "../../build/libv86.mjs";
const offset = 1024, base = 0x100000;
const vm = new V86({ wasm_path: "build/v86-jit-test.wasm", memory_size: 32 << 20,
    bios: { buffer: Uint8Array.from(fs.readFileSync("build/jit-capacity.bin")).buffer },
    disable_keyboard: true, disable_mouse: true, disable_speaker: true,
    net_device: { type: "none" }, autostart: false });
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const word = address => { const bytes = vm.read_memory(address, 4); return new DataView(bytes.buffer, bytes.byteOffset, 4).getUint32(0, true); };
try {
    await new Promise(resolve => vm.add_listener("emulator-loaded", resolve));
    const original_loop = vm.v86.cpu.main_loop;
    vm.v86.cpu.main_loop = () => {
        try { return original_loop(); }
        catch(error) {
            console.error("JIT test CPU failure", { eip: (vm.v86.cpu.instruction_pointer[0] >>> 0).toString(16),
                cr0: vm.v86.cpu.cr[0], cs: vm.v86.cpu.get_seg_cs() });
            throw error;
        }
    };
    vm.run();
    const deadline = performance.now() + 5000;
    while(word(0x500) !== 0xCAFE) { assert(performance.now() < deadline); await sleep(1); }
    await vm.stop();
    const cpu = vm.v86.cpu, e = cpu.wm.exports, table = cpu.wm.wasm_table;
    assert.equal(e.performance_recording_version(), 4);
    e.performance_recording_enable(1);
    const occupied = () => Array.from({ length: 899 }, (_, i) => table.get(offset + 1 + i)).filter(Boolean).length;
    const program = value => {
        const bytes = new Uint8Array([0xB8, 0, 0, 0, 0, 0xE8, 1, 0, 0, 0, 0xF4, 0xA3, 0x00, 0x06, 0, 0, 0xC3]);
        new DataView(bytes.buffer).setUint32(1, value, true);
        return bytes;
    };
    for(let i = 0; i < 1100; i++) vm.write_memory(program(i + 1), base + i * 4096);
    // A spans two pages. B later replaces the second page's primary module,
    // leaving a hidden reference to A until A is evicted.
    vm.write_memory(new Uint8Array([0xE9, 0xFB, 0x0F, 0, 0]), base);
    vm.write_memory(program(0xBEEF), base + 4096 + 32);
    let pending = null;
    const finish = cpu.codegen_finalize_finished;
    cpu.codegen_finalize_finished = (index, address, flags) => {
        finish(index, address, flags);
        const done = pending;
        pending = null;
        done({ index, address, fn: table.get(offset + index) });
    };
    const compile = address => new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error("JIT publication timed out at " + address.toString(16))), 5000);
        pending = entry => { clearTimeout(timer); resolve(entry); };
        try { e.jit_force_generate_unsafe(address); } catch(error) { clearTimeout(timer); reject(error); }
    });
    const execute = async (address, expected, compiled) => {
        vm.write_memory(new Uint8Array(4), 0x600);
        const jit_before = e.performance_recording_get(1);
        cpu.instruction_pointer[0] = address;
        cpu.in_hlt[0] = 0;
        vm.run();
        await sleep(5);
        await vm.stop();
        assert.equal(word(0x600), expected, "execution must never call a reused stale slot");
        if(compiled) assert(e.performance_recording_get(1) > jit_before, "retained module still executes through JIT");
    };
    const entries = [await compile(base)];
    await execute(0x800000, 2, true);
    for(let i = 2; i < 102; i++) entries.push(await compile(base + i * 4096));
    const b = await compile(base + 4096 + 32);
    entries.push(b);
    let next_page = 102;
    while(entries.length < 899) entries.push(await compile(base + next_page++ * 4096));
    assert.equal(occupied(), 899, "fill the actual fixed-size JIT table");
    const pressure = await compile(base + 1000 * 4096);
    assert.equal(occupied(), 884, "capacity reclaims 16 modules and publishes one, retaining 883");
    assert.equal(e.performance_recording_get(2), 0, "no full capacity flush");
    assert.equal(e.performance_recording_get(3), 1);
    assert.equal(e.performance_recording_get(4), 16);
    assert.equal(entries.filter(entry => table.get(offset + entry.index) === entry.fn).length, 883);
    assert.equal(table.get(offset + b.index), b.fn, "eviction removes hidden A without deleting newer B");
    assert(e.performance_recording_get(5) > 0 && e.performance_recording_get(7) === 900,
        "synchronous generation time and calls cover actual compilations");
    await execute(base, 2, false);
    await execute(0x800000, 2, false);
    await execute(0x801020, 0xBEEF, true);
    await execute(base + 4096 + 32, 0xBEEF, true);
    await execute(pressure.address, 1001, true);
    vm.write_memory(program(0x123456), pressure.address);
    await execute(pressure.address, 0x123456, false);
    await compile(pressure.address);
    await execute(pressure.address, 0x123456, true);
    // Force another eviction after slot reuse and SMC have changed queue order.
    while(occupied() < 899) await compile(base + next_page++ * 4096);
    await compile(base + next_page++ * 4096);
    assert.equal(occupied(), 884);
    assert.equal(e.performance_recording_get(3), 2);
    assert.equal(e.performance_recording_get(4), 32);
    const pending_address = base + 1098 * 4096;
    const written = compile(pending_address);
    vm.write_memory(program(0xAA55), pending_address);
    assert.equal((await written).fn, null, "code written during compilation must not be published");
    await execute(pending_address, 0xAA55, false);
    const saved = await vm.save_state();
    await vm.restore_state(saved);
    assert.equal(occupied(), 0, "state restore still invalidates all compiled code");
    const cancelled = compile(base + 1099 * 4096);
    cpu.jit_clear_cache();
    assert.equal((await cancelled).fn, null, "late completion after explicit clear cannot republish stale code");
    assert.equal(occupied(), 0);
    await compile(pending_address);
    await execute(pending_address, 0xAA55, true);
    console.log("PASS: 899-slot pressure, repeated bounded eviction, cross-page hidden modules, paging aliases, stale-slot execution, SMC, pending writes, restore and late completion (JIT invariants enabled)");
} finally { await vm.destroy(); }
