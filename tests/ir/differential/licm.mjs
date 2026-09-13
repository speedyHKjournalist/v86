import assert from "node:assert/strict";
import fs from "node:fs";
import {V86} from "../../../build/libv86.mjs";

// Compile inside the CPU Wasm, then compare the emitted Tier 2 entry with the
// baseline interpreter. The pure pass unit tests additionally assert nonzero
// loop_hoisted; equality alone would not prove that an optimization ran.
const wasm = process.argv[2] || "build/v86-ir-test.wasm";
const PC = 0x100000, DATA = 0x110000, STACK = 0x90000, HANDLER = 0x180000;
const vm = new V86({
    wasm_path: wasm, memory_size: 32 << 20,
    bios: {buffer: Uint8Array.from(fs.readFileSync("build/jit-capacity.bin")).buffer},
    disable_keyboard: true, disable_mouse: true, disable_speaker: true,
    net_device: {type: "none"}, autostart: false,
});
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const programs = [
    // NOP; loop: IMUL EBX,ESI,7; ADD EAX,EBX; DEC ECX; JNZ loop; NOP
    [0x90, 0x6B, 0xDE, 7, 0x01, 0xD8, 0x49, 0x75, 0xF8, 0x90],
    // Same arithmetic with a zero-trip JECXZ guard.
    [0x90, 0xE3, 8, 0x6B, 0xDE, 7, 0x01, 0xD8, 0x49, 0x75, 0xF8, 0x90],
    // Invariant register arithmetic followed by a faultable load each iteration.
    // NOP; loop: MOV EAX,EBX; ADD EAX,EDX; ADD EAX,[ESI];
    // ADD ESI,4096; DEC ECX; JNZ loop; NOP
    [0x90, 0x89, 0xD8, 0x01, 0xD0, 0x03, 0x06,
        0x81, 0xC6, 0, 0x10, 0, 0, 0x49, 0x75, 0xF1, 0x90],
];
try {
    await new Promise(resolve => vm.add_listener("emulator-loaded", resolve));
    const cpu = vm.v86.cpu, e = cpu.wm.exports;
    let mem, words, view;
    const refresh = () => {
        mem = cpu.mem8;
        words = new Uint32Array(e.memory.buffer);
        view = new DataView(mem.buffer, mem.byteOffset);
    };
    refresh();
    vm.run();
    const deadline = performance.now() + 10000;
    while(view.getUint16(0x500, true) !== 0xCAFE) {
        assert(performance.now() < deadline, "test BIOS did not finish");
        await sleep(1);
    }
    await vm.stop();
    await sleep(20);
    cpu.jit_clear_cache();
    const set32 = (a, value) => view.setUint32(a, value, true);
    function reset(code, count, operand, flags, initialCount, fault = false) {
        refresh();
        cpu.in_hlt[0] = 0;
        cpu.is_32[0] = 1;
        cpu.stack_size_32[0] = 1;
        words[612 >> 2] = 0;
        cpu.segment_offsets.fill(0, 0, 6);
        cpu.segment_offsets[1] = PC - 0x1000;
        cpu.segment_is_null.fill(0, 0, 6);
        cpu.segment_limits.fill(0xFFFFFFFF, 0, 6);
        cpu.sreg.set([16, 8, 16, 16, 16, 16]);
        cpu.segment_access_bytes.set([0x93, 0x9B, 0x93, 0x93, 0x93, 0x93]);
        cpu.reg32.set([0x7FFFFFFF, count, operand, 0x80000000, STACK,
            0xFEDCBA98, DATA, 0x76543210]);
        cpu.flags[0] = flags;
        cpu.flags_changed[0] = 0x8D5;
        words[96 >> 2] = 31;
        words[104 >> 2] = 0x7FFFFFFF;
        words[112 >> 2] = flags & 64 ? 0 : 0x80000000;
        words[664 >> 2] = initialCount;
        words[560 >> 2] = 0x11223344;
        cpu.cr[0] = 0x80010011 | 0;
        cpu.cr[2] = 0xBADF000;
        cpu.cr[3] = 0x12000;
        cpu.cr[4] = 512;
        set32(0x12000, 0x13003);
        set32(0x13000 + (PC >>> 12) * 4, PC | 3);
        for(let n = 0; n < 16; n++) {
            const page = DATA + n * 4096;
            set32(0x13000 + (page >>> 12) * 4, page | 3);
            set32(page, (0x12345678 + n) >>> 0);
        }
        if(fault) set32(0x13000 + ((DATA + 4096) >>> 12) * 4, 0);
        cpu.idtr_offset[0] = 0x2000;
        cpu.idtr_size[0] = 0x7FF;
        set32(0x2000 + 14 * 8, 8 << 16 | HANDLER & 65535);
        set32(0x2004 + 14 * 8, HANDLER & 0xFFFF0000 | 0x8E00);
        mem.fill(0xCC, STACK - 32, STACK);
        mem.set(code, PC);
        cpu.instruction_pointer[0] = PC;
        e.full_clear_tlb();
        e.update_state_flags();
    }
    const state = () => ({
        regs: Array.from(cpu.reg32, value => value >>> 0),
        flags: e.get_eflags() >>> 0, last: words[104 >> 2],
        ip: cpu.instruction_pointer[0] >>> 0, cr2: cpu.cr[2] >>> 0,
        xmm: Array.from(words.slice(832 >> 2, 960 >> 2)),
        data: Array.from(mem.slice(DATA, DATA + 16)),
        frame: Array.from(mem.slice(STACK - 32, STACK)),
    });
    function compile(code, budget, optimize) {
        const before = state();
        const id = e.ir_compile_live(code.length, 2, optimize, 1, budget, 8);
        refresh();
        assert(id > 0n, `live compiler error ${e.ir_live_error()}`);
        assert.deepEqual(state(), before, "compilation changes architectural state");
        assert.equal(e.ir_live_validate(id), 1);
        const bytes = new Uint8Array(e.memory.buffer,
            e.ir_live_info(id, 0), e.ir_live_info(id, 1)).slice();
        assert(WebAssembly.validate(bytes));
        const instance = new WebAssembly.Instance(new WebAssembly.Module(bytes),
            {e: {...e, m: e.memory}});
        assert.equal(e.ir_live_release(id), 1);
        return instance;
    }
    let comparisons = 0, budgetComparisons = 0, faults = 0;
    for(const [program, code] of programs.entries()) {
        for(const budget of [1, 2, 3, 8, 17, 64]) {
            const baselines = [];
            for(const optimize of [0, 1]) {
                reset(code, 3, 0xFFFFFFFF, 0x8D7, 0xFFFFFFFC);
                const instance = compile(code, budget, optimize);
                let trial = 0;
                for(const count of [0, 1, 2, 7])
                for(const operand of [0, 0x7FFFFFFF, 0xFFFFFFFF])
                for(const flags of [2, 0x8D7])
                for(const initialCount of [100, 0xFFFFFFFC]) {
                    reset(code, count, operand, flags, initialCount);
                    instance.exports.f(0);
                    refresh();
                    const actual = state();
                    const retired = (words[664 >> 2] - initialCount) >>> 0;
                    assert(retired <= budget, "loop exceeded its guest instruction budget");
                    const observed = {...actual, retired, previous: words[560 >> 2]};
                    const label = `program=${program}, budget=${budget}, opt=${optimize}, count=${count}`;
                    if(optimize) {
                        assert.deepEqual(observed, baselines[trial], label);
                        budgetComparisons++;
                    } else baselines.push(observed);
                    trial++;
                    reset(code, count, operand, flags, initialCount);
                    for(let step = 0; step < retired; step++) e.ir_test_step();
                    refresh();
                    assert.deepEqual(actual, state(), label);
                    comparisons++;
                }
            }
        }
    }
    for(const optimize of [0, 1]) {
        const code = programs[2];
        reset(code, 3, 0xFFFFFFFF, 0x8D7, 0xFFFFFFFC, true);
        const instance = compile(code, 64, optimize);
        for(const initialCount of [100, 0xFFFFFFFC]) {
            reset(code, 3, 0xFFFFFFFF, 0x8D7, initialCount, true);
            instance.exports.f(0);
            refresh();
            const actual = state();
            const retired = (words[664 >> 2] - initialCount) >>> 0;
            assert.equal(actual.ip, HANDLER, "expected second-iteration page fault");
            assert.equal(actual.cr2, DATA + 4096);
            assert.equal(retired, 9, "faulting instruction must not retire");
            reset(code, 3, 0xFFFFFFFF, 0x8D7, initialCount, true);
            for(let step = 0; step <= retired; step++) e.ir_test_step();
            refresh();
            assert.deepEqual(actual, state(), "LICM must preserve the full #PF frame");
            faults++;
        }
    }
    console.log(`PASS: ${comparisons} live-compiled LICM CPU/interpreter comparisons; ${budgetComparisons} exact optimized/unoptimized budget exits; ${faults} second-iteration page faults`);
} finally {
    await vm.destroy();
}
