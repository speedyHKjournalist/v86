// A cached absence may skip admission, never execute or hide a published owner.
import assert from "node:assert/strict";
import fs from "node:fs";
import { V86 } from "../../../build/libv86.mjs";
const wasm = process.argv[2] || "build/v86-ir-runtime.wasm";
const vm = new V86({ wasm_path: wasm, jit_backend: "ir", memory_size: 32 << 20,
    bios: { buffer: Uint8Array.from(fs.readFileSync("build/jit-capacity.bin")).buffer },
    disable_keyboard: true, disable_mouse: true, disable_speaker: true,
    net_device: { type: "none" }, autostart: false });
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
try {
    await new Promise(resolve => vm.add_listener("emulator-loaded", resolve));
    const cpu = vm.v86.cpu, e = cpu.wm.exports, PC = 0x8000, N = 8000, initial = 0xFFFFFFF0;
    const count = () => vm.get_instruction_counter() >>> 0;
    vm.run(); const deadline = performance.now() + 10000;
    while(new DataView(cpu.mem8.buffer, cpu.mem8.byteOffset).getUint16(0x500, true) !== 0xCAFE) {
        assert(performance.now() < deadline); await sleep(1);
    }
    await vm.stop();
    assert.equal(e.ir_cache_set_missing_hint(2), 0);
    const prepare = (mode = 1, cs = 0) => {
        cpu.in_hlt[0] = 0; cpu.flags[0] = 2; cpu.flags_changed[0] = 0;
        cpu.is_32[0] = mode; cpu.stack_size_32[0] = 1;
        cpu.segment_offsets.fill(0, 0, 6); cpu.segment_offsets[1] = cs;
        cpu.segment_is_null.fill(0, 0, 6);
        cpu.reg32.set([0, N, 0, 0, 0x90000, 0, 0, 0]);
        cpu.instruction_pointer[0] = PC;
        new Uint32Array(e.memory.buffer)[664 >> 2] = initial;
        e.update_state_flags();
    };
    const run = (expected = N) => {
        const end = performance.now() + 10000;
        while(!cpu.in_hlt[0]) { assert(performance.now() < end); e.main_loop(); }
        assert.equal(count(), (initial + N * 3 + 1) >>> 0);
        assert.equal(cpu.reg32[3], expected); assert.equal(cpu.reg32[1], 0);
        assert.equal(cpu.instruction_pointer[0], PC + 5);
        return { regs: Array.from(cpu.reg32), flags: e.get_eflags(), count: count(), ip: cpu.instruction_pointer[0] };
    };
    const states = [];
    for(const enabled of [0, 1]) {
        cpu.jit_clear_cache();
        assert.equal(e.ir_auto_config(1, 1000000, 1000000, 192, 256, 64), 1);
        assert.equal(e.ir_cache_set_missing_hint(enabled), 1);
        prepare(); vm.write_memory(Uint8Array.of(0x43, 0x49, 0x75, 0xFC, 0xF4), PC);
        const misses = e.ir_cache_stat(37), hits = e.ir_cache_stat(2);
        states.push(run());
        assert.equal(e.ir_cache_stat(2), hits, "no hidden legacy/IR artifact");
        if(enabled) assert(e.ir_cache_stat(37) - misses >= N - 2, "repeat misses bypass admission");
        else assert.equal(e.ir_cache_stat(37), misses);
    }
    assert.deepEqual(states[1], states[0], "absence fast path preserves exact work and state");
    // The same key used by the just-finished interpreted loop becomes callable.
    prepare(); assert(await cpu.ir_compile_cached(5, 2, 1, 1, 256, 64));
    prepare(); let hits = e.ir_cache_stat(2); run();
    assert(e.ir_cache_stat(2) > hits, "new publication clears previously absent key");
    for(const [mode, cs] of [[0, 0], [1, 0x1000]]) {
        prepare(mode, cs);
        const misses = e.ir_cache_stat(37); hits = e.ir_cache_stat(2); run();
        assert(e.ir_cache_stat(37) > misses, "alternative context is genuinely missing");
        assert.equal(e.ir_cache_stat(2), hits, "wrong mode/CS cannot reuse existing owner");
        prepare(); hits = e.ir_cache_stat(2); run();
        assert(e.ir_cache_stat(2) > hits, "missing mode/CS cannot hide another exact entry");
    }
    cpu.jit_clear_cache();
    assert.equal(e.ir_auto_config(1, 1000000, 1000000, 192, 256, 64), 1);
    prepare(); run();
    cpu.mem8[PC] = 0x4B; // No dirty notification: absence still means interpret NEW bytes.
    prepare(); const misses = e.ir_cache_stat(37); run(-N);
    assert(e.ir_cache_stat(37) > misses);
    assert.equal(e.ir_cache_stat(38), 1);
    // Cross-page edges force a real dispatch at all eight headers. Consecutive
    // pages occupy distinct hints; 64-page strides deliberately collide. Keep
    // the interpreter and compilation heat policy identical in the A/B arms.
    const iterations = 512, blocks = 8;
    const displacement = value => Array.from({length: 4}, (_, i) => value >>> (8 * i) & 255);
    let multi = 0;
    for(const kind of ["distinct", "collision", "diagnostics"]) {
        const pages = Array.from({length: blocks}, (_, i) =>
            (kind === "collision" ? 0x200000 : 0x100000) + i * (kind === "collision" ? 0x40000 : 4096));
        const chunks = pages.map((pc, i) => i + 1 === blocks
            ? [0x43, 0x49, 0x0F, 0x85, ...displacement(pages[0] - pc - 8), 0xF4]
            : [0x43, 0xE9, ...displacement(pages[i + 1] - pc - 6)]);
        const prepare_multi = () => {
            prepare(); cpu.reg32[1] = iterations; cpu.instruction_pointer[0] = pages[0];
        };
        const run_multi = (expected = blocks * iterations) => {
            const end = performance.now() + 10000;
            while(!cpu.in_hlt[0]) { assert(performance.now() < end, kind); e.main_loop(); }
            assert.equal(count(), (initial + iterations * (2 * blocks + 1) + 1) >>> 0, kind);
            assert.equal(cpu.reg32[3], expected, kind); assert.equal(cpu.reg32[1], 0, kind);
            assert.equal(cpu.instruction_pointer[0], pages.at(-1) + 9, kind);
            return {regs: Array.from(cpu.reg32), flags: e.get_eflags(), count: count(), ip: cpu.instruction_pointer[0]};
        };
        const pairs = [];
        for(const enabled of [0, 1]) {
            assert.equal(await vm.configure_ir_diagnostics(kind === "diagnostics" ? 1 : 0), true);
            cpu.jit_clear_cache(); e.ir_cache_collect();
            assert.equal(e.ir_auto_config(1, 1000000, 1000000, 192, 256, 64), 1);
            assert.equal(e.ir_cache_set_missing_hint(enabled), 1);
            for(let i = 0; i < blocks; i++) vm.write_memory(Uint8Array.from(chunks[i]), pages[i]);
            prepare_multi();
            const misses = e.ir_cache_stat(37), hits = e.ir_cache_stat(2);
            pairs.push(run_multi());
            assert.equal(e.ir_cache_stat(2), hits, `${kind}: no unpublished execution`);
            if(enabled && kind === "distinct") {
                assert(e.ir_cache_stat(37) - misses >= (iterations - 2) * blocks,
                    "all recurrent headers bypass admission, not only the last key");
            }
            else {
                assert.equal(e.ir_cache_stat(37), misses,
                    `${kind}: disabled, colliding or instrumented hints cannot report a hit`);
            }
        }
        assert.deepEqual(pairs[1], pairs[0], `${kind}: exact multi-PC A/B state`);
        multi++;
        if(kind === "diagnostics") continue;
        // Publish a formerly absent middle header, then demand a hit on every
        // visit, including its first visit after publication. A colliding hint
        // from another header must never hide the published exact key either.
        const chosen = 3, target = pages[chosen];
        prepare_multi(); cpu.instruction_pointer[0] = target;
        assert(await cpu.ir_compile_cached(chunks[chosen].length, 2, 1, 1, 256, 64));
        prepare_multi(); let hits = e.ir_cache_stat(2); run_multi();
        assert.equal(e.ir_cache_stat(2) - hits, iterations, `${kind}: immediate publication visibility`);
        vm.write_memory(Uint8Array.of(0x4B), target);
        prepare_multi(); hits = e.ir_cache_stat(2); run_multi((blocks - 2) * iterations);
        assert.equal(e.ir_cache_stat(2), hits, `${kind}: notified invalidation cannot retain an owner`);
        // Repeated absence observes raw new bytes; publishing the same key
        // again must clear every previously learned negative witness.
        cpu.mem8[target] = 0x43;
        prepare_multi(); run_multi();
        prepare_multi(); cpu.instruction_pointer[0] = target;
        assert(await cpu.ir_compile_cached(chunks[chosen].length, 2, 1, 1, 256, 64));
        prepare_multi(); hits = e.ir_cache_stat(2); run_multi();
        assert.equal(e.ir_cache_stat(2) - hits, iterations, `${kind}: replacement publication clears absence`);
        cpu.jit_clear_cache(); e.ir_cache_collect();
        prepare_multi(); hits = e.ir_cache_stat(2); run_multi();
        assert.equal(e.ir_cache_stat(2), hits, `${kind}: reset executes only current interpreter bytes`);
    }
    assert.equal(await vm.configure_ir_diagnostics(0), true);
    console.log(`PASS: ${wasm}: single-key and ${multi} multi-PC A/B cases; 64-slot collisions, diagnostics bypass, wrapped retirement, immediate publication, mode/CS, reset and raw/notified code`);
} finally { await vm.destroy(); }
