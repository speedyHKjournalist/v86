"use strict";
const assert = require("node:assert/strict");
(async () => {
    global.DEBUG = false;
    const { V86GLPCI } = await import("../../src/v86gl_pci.js");
    const { PCI } = await import("../../src/pci.js");
    const { V86 } = await import("../../src/browser/starter.js");
    const checkpoint = new Uint8Array([1, 2, 3]);
    const pci = Object.create(V86GLPCI.prototype);
    pci.cpu = { memory_size: [32 * 1024 * 1024] };
    pci.virtio = { set_state() {} };
    pci.arenaGeneration = 0;
    const old = [0x56514731, [], 0, 0, 2, 16, 3, 0, undefined];
    pci.set_state(old);
    assert.deepEqual(pci.get_state().slice(2), old.slice(2), "virtio device restores transport counters");
    const calls = [];
    const graphics = {
        prepareSaveState() { calls.push("prepare"); },
        async waitForIdle() {},
        beginStateRestore() { calls.push("begin"); },
        onPCIStateRestored(value) { calls.push(value); },
        async finishStateRestore() { await Promise.resolve(); calls.push("finish"); },
        cancelStateRestore() { calls.push("cancel"); },
    };
    pci.graphics_state_handlers = {
        save() { return checkpoint; },
        restore(value) { graphics.onPCIStateRestored(value); },
    };
    const emulator = { graphics_adapter: graphics,
        with_graphics_state: V86.prototype.with_graphics_state,
        is_running() { return false; }, async stop() {},
        v86: {
        save_state() { calls.push("save"); return pci.get_state(); },
        restore_state(state) { calls.push("restore"); pci.set_state(state); },
    } };
    const saved = await V86.prototype.save_state.call(emulator);
    assert.deepEqual(calls, ["prepare", "save"]);
    assert.equal(saved[8], checkpoint);
    calls.length = 0;
    await V86.prototype.restore_state.call(emulator, saved);
    assert.deepEqual(calls, ["begin", "restore", checkpoint, "finish"]);
    calls.length = 0;
    await V86.prototype.restore_state.call(emulator, old);
    assert.deepEqual(calls, ["begin", "restore", undefined, "finish"], "snapshots without graphics data clear replay state");
    emulator.v86.restore_state = () => { throw new Error("bad state"); };
    calls.length = 0;
    await assert.rejects(V86.prototype.restore_state.call(emulator, saved), /bad state/);
    assert.deepEqual(calls, ["begin", "cancel"]);
    // The public API must stop the CPU before awaiting host work, serialize
    // concurrent requests, and resume only after the snapshot is consistent.
    const sequence = [];
    let release;
    const gate = new Promise(resolve => { release = resolve; });
    const live = {
        running: true, value: 0,
        with_graphics_state: V86.prototype.with_graphics_state,
        is_running() { return this.running; },
        async stop() { this.running = false; sequence.push("stop"); },
        run() { this.running = true; sequence.push("run"); },
        graphics_adapter: {
            async prepareSaveState() {
                sequence.push("prepare");
                assert.equal(live.running, false);
                await gate;
                live.value = 42; // an accepted GPU readback completes here
            },
            beginStateRestore() {}, async waitForIdle() {},
            async finishStateRestore() {}, cancelStateRestore() {},
        },
        v86: {
            save_state() { sequence.push("save"); return live.value; },
            restore_state() { throw new Error("invalid checkpoint"); },
        },
    };
    const first = V86.prototype.save_state.call(live);
    const second = V86.prototype.save_state.call(live);
    for (let i = 0; i < 5; ++i) await Promise.resolve();
    assert.deepEqual(sequence, ["stop", "prepare"]);
    release();
    assert.deepEqual(await Promise.all([first, second]), [42, 42]);
    assert.deepEqual(sequence, ["stop", "prepare", "save", "run", "stop", "prepare", "save", "run"]);
    await assert.rejects(V86.prototype.restore_state.call(live, new ArrayBuffer(0)), /invalid checkpoint/);
    assert.equal(live.running, false, "a failed restore cannot resume a partially restored guest");

    const bus = Object.create(PCI.prototype);
    const config = new Int32Array(64);
    config[4] = 1;
    const fixed = { fixed: true, original_bar: 0xf101 };
    bus.devices = [{ name: "sparse BARs", pci_bars: [undefined, fixed] }];
    bus.device_spaces = [new Int32Array(64)];
    const pciState = [];
    pciState[0] = config;
    for (const [i, key] of ["pci_addr", "pci_value", "pci_response", "pci_status"].entries()) {
        bus[key] = new Uint8Array(4);
        pciState[256 + i] = new Uint8Array(4);
    }
    bus.set_state(pciState);
    assert.equal(bus.device_spaces[0][5], 0xf101, "restore skips absent BARs and preserves the fixed graphics port");
    console.log("graphics_state_integration_test: ok");
})().catch(error => { console.error(error); process.exitCode = 1; });
