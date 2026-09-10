import assert from "node:assert/strict";
import fs from "node:fs";
import { V86 } from "../../build/libv86.mjs";

const wasm = process.argv[2] || "build/v86.wasm";
const bios = Uint8Array.from(fs.readFileSync("build/jit-capacity.bin")).buffer;
const vm = new V86({ wasm_path: wasm, bios: { buffer: bios }, memory_size: 32 << 20,
    disable_keyboard: true, disable_mouse: true, disable_speaker: true,
    net_device: { type: "none" }, autostart: false });
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const word = address => new DataView(Uint8Array.from(vm.read_memory(address, 4)).buffer).getUint32(0, true);
const u32 = n => [n & 255, n >>> 8 & 255, n >>> 16 & 255, n >>> 24 & 255];
const DATA = 0x200000, RESULT = 0x210000;
let cpu, e;
async function run(address) {
    vm.write_memory(new Uint8Array(4), 0x600);
    cpu.instruction_pointer[0] = address;
    cpu.in_hlt[0] = 0;
    vm.run();
    const end = performance.now() + 10000;
    while(word(0x600) !== 0xCAFE) { assert(performance.now() < end, "guest timeout"); await sleep(1); }
    await vm.stop();
}
async function compile(address) {
    e.performance_recording_enable(1);
    cpu.instruction_pointer[0] = address;
    cpu.in_hlt[0] = 0;
    vm.run();
    const deadline = performance.now() + 10000;
    while(e.performance_recording_get(1) === 0) {
        assert(performance.now() < deadline, "natural JIT warmup timeout");
        await sleep(1);
    }
    await vm.stop();
    e.performance_recording_enable(0);
}
const done = [0xC7, 0x05, ...u32(0x600), ...u32(0xCAFE)];
try {
    await new Promise(resolve => vm.add_listener("emulator-loaded", resolve));
    vm.run();
    const deadline = performance.now() + 10000;
    while(word(0x500) !== 0xCAFE) { assert(performance.now() < deadline); await sleep(1); }
    await vm.stop();
    cpu = vm.v86.cpu; e = cpu.wm.exports;
    for(const [index, input, expected] of [[1,0,1],[1,100,16],[3,5000,1024],[4,0,1000],[4,3000000,2000000]]) {
        const previous = e.get_jit_config(index);
        e.set_jit_config(index,input);
        assert.equal(e.get_jit_config(index),expected,"bounded JIT experiment setting");
        e.set_jit_config(index,previous);
    }
    assert.equal(e.get_jit_config(2),1,"experiments keep loop safety enabled");
    if(process.env.JIT_LINKS) e.set_jit_config(5, Number(process.env.JIT_LINKS));
    const patterns = new Uint8Array(512);
    let seed = 0x9132913;
    for(let i = 0; i < patterns.length; i++) {
        seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5;
        patterns[i] = seed;
    }
    const pattern_view = new DataView(patterns.buffer);
    for(const [i, bits] of [0, 0x80000000, 0x7F800000, 0xFF800000,
        0x7FC12345, 0x7F812345, 1, 0xFFFFFFFF].entries()) {
        pattern_view.setUint32(i * 4, bits, true);
        pattern_view.setUint32(256 + i * 4, ~bits, true);
    }
    vm.write_memory(patterns, DATA);
    const operations = [
        [[], 0x54, "and"], [[0x66], 0x54, "and"], [[0x66], 0xDB, "and"],
        [[], 0x55, "andnot"], [[0x66], 0x55, "andnot"], [[0x66], 0xDF, "andnot"],
        [[], 0x56, "or"], [[0x66], 0x56, "or"], [[0x66], 0xEB, "or"],
        [[], 0x57, "xor"], [[0x66], 0x57, "xor"], [[0x66], 0xEF, "xor"],
    ];
    let cases = 0;
    for(const [index, [prefix, opcode, op]] of operations.entries()) {
        const address = 0x100000 + index * 4096;
        const program = [], expected = new Uint8Array(64 * 20);
        for(let src = 0; src < 8; src++) for(let dst = 0; dst < 8; dst++) {
            const n = src * 8 + dst;
            program.push(0xF3, 0x0F, 0x6F, 0x05 | src << 3, ...u32(DATA + src * 16));
            program.push(0xF3, 0x0F, 0x6F, 0x05 | dst << 3, ...u32(DATA + 256 + dst * 16));
            program.push(0xF9, ...prefix, 0x0F, opcode, 0xC0 | dst << 3 | src);
            program.push(0xF3, 0x0F, 0x7F, 0x05 | dst << 3, ...u32(RESULT + n * 20));
            program.push(0x9C, 0x58, 0xA3, ...u32(RESULT + n * 20 + 16));
            for(let k = 0; k < 16; k++) {
                const b = patterns[256 + dst * 16 + k];
                const a = src === dst ? b : patterns[src * 16 + k];
                expected[n * 20 + k] = op === "and" ? a & b : op === "andnot" ? a & ~b : op === "or" ? a | b : a ^ b;
            }
        }
        program.push(...done);
        program.push(0xE9, ...u32(-program.length - 5));
        assert(program.length < 4096);
        vm.write_memory(Uint8Array.from(program), address);
        await compile(address);
        e.performance_recording_enable(1);
        await run(address);
        assert(e.performance_recording_get(1) > 0, "execute real JIT");
        e.performance_recording_enable(0);
        const result = Uint8Array.from(vm.read_memory(RESULT, expected.length));
        for(let n = 0; n < 64; n++) {
            assert.deepEqual(result.slice(n * 20, n * 20 + 16), expected.slice(n * 20, n * 20 + 16), `${op} ${index} register pair ${n}`);
            assert.equal(result[n * 20 + 16] & 1, 1, "SSE logic preserves carry");
        }
        cases += 64;
    }
    // Same-module and cross-module indirect CALL/RET, followed by live code
    // invalidation. Results must change after replacing the callee.
    for(const target of [0x180100, 0x190100]) {
        const address = 0x180000;
        const program = [0x31, 0xC0, 0xBB, ...u32(target), 0xB9, ...u32(100),
            0xFF, 0xD3, 0x49, 0x75, 0xFB, 0xA3, ...u32(RESULT), ...done];
        program.push(0xE9, ...u32(-program.length - 5));
        vm.write_memory(Uint8Array.from(program), address);
        vm.write_memory(Uint8Array.from([0x83, 0xC0, 7, 0xC3]), target);
        await compile(address);
        await run(address);
        assert.equal(word(RESULT), 700, "indirect call/ret result");
        vm.write_memory(Uint8Array.from([0x83, 0xC0, 11, 0xC3]), target);
        await compile(address);
        await run(address);
        assert.equal(word(RESULT), 1100, "invalidated callee is never reused");
        const state = await vm.save_state();
        await vm.restore_state(state);
        await compile(address);
        await run(address);
        assert.equal(word(RESULT), 1100, "indirect targets after save/restore");
    }
    if(process.env.JIT_LINKS === "1") assert(e.get_jit_link_count() > 0, "actual cross-module links executed");
    console.log("PASS: same/cross-module indirect CALL/RET, SMC and save/restore");
    const arith = [[0x01, 0xD8, false, true], [0x03, 0xC3, false, true],
        [0x29, 0xD8, true, true], [0x2B, 0xC3, true, true],
        [0x39, 0xD8, true, false], [0x3B, 0xC3, true, false]];
    const arithmetic = (a, b, sub) => {
        const value = (sub ? a - b : a + b) >>> 0;
        let parity = value & 255;
        parity ^= parity >>> 4; parity ^= parity >>> 2; parity ^= parity >>> 1;
        const carry = sub ? a < b : a + b > 0xFFFFFFFF;
        const overflow = sub ? ((a ^ b) & (a ^ value)) >>> 31 : (~(a ^ b) & (a ^ value)) >>> 31;
        const flags = Number(carry) | ((~parity & 1) << 2) | ((a ^ b ^ value) & 16) |
            (value === 0 ? 64 : 0) | (value >>> 24 & 128) | (overflow << 11);
        return { value, flags };
    };
    let flag_cases = 0;
    for(let first = 0; first < 6; first++) for(let second = 0; second < 6; second++) {
        const program = [], expected = [];
        for(let i = 0; i < 64; i++) {
            const edge = [0, 1, 0xFFFFFFFF, 0x7FFFFFFF, 0x80000000, 0x80000001, 0xFFFF, 0x10000];
            const a = edge[i & 7], b = edge[i >>> 3];
            const f = arith[first], g = arith[second];
            const intermediate = arithmetic(a, b, f[2]);
            const result = arithmetic(f[3] ? intermediate.value : a, b, g[2]);
            expected.push([g[3] ? result.value : f[3] ? intermediate.value : a, result.flags]);
            program.push(0xB8, ...u32(a), 0xBB, ...u32(b), f[0], f[1], 0xB9, ...u32(0x12345678), 0x89, 0xDA, 0x90, g[0], g[1],
                0xA3, ...u32(RESULT + i * 8), 0x9C, 0x58, 0xA3, ...u32(RESULT + i * 8 + 4));
        }
        program.push(...done);
        program.push(0xE9, ...u32(-program.length - 5));
        assert(program.length < 4096);
        const address = 0x300000 + (first * 6 + second) * 4096;
        vm.write_memory(Uint8Array.from(program), address);
        await compile(address); await run(address);
        for(let i = 0; i < expected.length; i++) {
            assert.equal(word(RESULT + i * 8), expected[i][0], "arithmetic result");
            assert.equal(word(RESULT + i * 8 + 4) & 0x8D5, expected[i][1], `flags ${first}/${second}/${i}`);
        }
        flag_cases += expected.length;
    }
    // A flags observer/consumer between arithmetic operations must keep the
    // first operation's carry/overflow, rather than exposing stale metadata.
    for(const [index, barrier] of [[0, [0x9C, 0x5A]], [1, [0x83, 0xD0, 0]],
        [2, [0x40]], [3, [0xD3, 0xE0]]]) {
        const address = 0x340000 + index * 4096;
        const program = [0xB8, ...u32(0xFFFFFFFF), 0xBB, ...u32(1), 0xB9, ...u32(0),
            0x01, 0xD8, ...barrier, 0x89, 0x15, ...u32(RESULT + 8),
            0x9C, 0x5A, 0x89, 0x15, ...u32(RESULT + 4), 0x03, 0xC3, 0xA3, ...u32(RESULT), ...done];
        program.push(0xE9, ...u32(-program.length - 5));
        vm.write_memory(Uint8Array.from(program), address);
        await compile(address); await run(address);
        assert.equal(word(RESULT), index === 1 || index === 2 ? 2 : 1, "flags consumer result");
        if(index !== 1) assert.equal(word(RESULT + 4) & 1, 1, "preserved carry at observer");
        if(index === 0) assert.equal(word(RESULT + 8) & 0x8D5, arithmetic(0xFFFFFFFF, 1, false).flags);
    }
    console.log(`PASS: ${flag_cases} arithmetic flag cases across MOV/NOP and PUSHFD/ADC/INC/zero-shift barriers`);
    // A real #NM must still be raised by the optimized register instruction.
    // Install a guest handler that skips the faulting instruction and clears TS.
    const handler = 0x381000, idt = 0x250000, descriptor = 0x251000;
    const gate = [handler & 255, handler >>> 8 & 255, 8, 0, 0, 0x8E, handler >>> 16 & 255, handler >>> 24];
    vm.write_memory(Uint8Array.from(gate), idt + 7 * 8);
    vm.write_memory(Uint8Array.from([255, 7, ...u32(idt)]), descriptor);
    vm.write_memory(Uint8Array.from([
        0x8B, 0x04, 0x24, 0xA3, ...u32(RESULT),
        0xFF, 0x05, ...u32(RESULT + 4),
        0x83, 0x04, 0x24, 3,
        0x0F, 0x20, 0xC0, 0x83, 0xE0, 0xF7, 0x0F, 0x22, 0xC0, 0xCF,
    ]), handler);
    const fault_program = [0x0F, 0x01, 0x1D, ...u32(descriptor),
        0xF3, 0x0F, 0x6F, 0x05, ...u32(DATA),
        0xF3, 0x0F, 0x6F, 0x0D, ...u32(DATA + 16),
        0x0F, 0x20, 0xC0, 0x83, 0xC8, 8, 0x0F, 0x22, 0xC0];
    const fault_eip = 0x380000 + fault_program.length;
    fault_program.push(0x0F, 0x57, 0xC1, 0xF3, 0x0F, 0x7F, 0x05, ...u32(RESULT + 16), ...done);
    fault_program.push(0xE9, ...u32(-fault_program.length - 5));
    vm.write_memory(Uint8Array.from(fault_program), 0x380000);
    vm.write_memory(new Uint8Array(8), RESULT);
    await compile(0x380000); await run(0x380000);
    assert.equal(word(RESULT), fault_eip, "#NM points to the SSE instruction");
    assert(word(RESULT + 4) > 0, "SSE still raises #NM with TS set");
    assert.deepEqual(Uint8Array.from(vm.read_memory(RESULT + 16, 16)), patterns.slice(0, 16), "faulting XORPS did not change XMM");
    console.log("PASS: optimized SSE retains guest #NM and precise fault EIP");
    // Flags before a faulting memory access must remain observable in the
    // hardware exception frame, even when later arithmetic overwrites them.
    const pf_handler = 0x383000, pf_address = 0x382000;
    vm.write_memory(Uint8Array.from([pf_handler & 255, pf_handler >>> 8 & 255,
        8, 0, 0, 0x8E, pf_handler >>> 16 & 255, pf_handler >>> 24]), idt + 14 * 8);
    vm.write_memory(Uint8Array.from([
        0x8B, 0x44, 0x24, 12, 0xA3, ...u32(RESULT),
        0x8B, 0x44, 0x24, 4, 0xA3, ...u32(RESULT + 4),
        0x83, 0x44, 0x24, 4, 6, 0x83, 0xC4, 4, 0xCF,
    ]), pf_handler);
    const pf_program = [0xB8, ...u32(0xFFFFFFFF), 0xBB, ...u32(1), 0x01, 0xD8];
    const pf_eip = pf_address + pf_program.length;
    pf_program.push(0x8B, 0x15, ...u32(0x900000), 0x9C, 0x59, 0x89, 0x0D, ...u32(RESULT + 8),
        0x03, 0xC3, ...done);
    pf_program.push(0xE9, ...u32(-pf_program.length - 5));
    vm.write_memory(Uint8Array.from(pf_program), pf_address);
    await compile(pf_address); await run(pf_address);
    const pf_flags = arithmetic(0xFFFFFFFF, 1, false).flags;
    assert.equal(word(RESULT) & 0x8D5, pf_flags, "flags in page fault frame");
    assert.equal(word(RESULT + 4), pf_eip, "precise memory fault EIP");
    assert.equal(word(RESULT + 8) & 0x8D5, pf_flags, "flags survive fault/IRET");
    console.log("PASS: flags remain precise across guest page fault and IRET");
    // A mapped device read must occur for each access, never reuse paging scratch.
    const old_mmio = cpu.memory_map_read32[0xA0000 >>> 17];
    let device_reads = 0;
    cpu.memory_map_read32[0xA0000 >>> 17] = () => ++device_reads;
    try {
        const p = [0xA1,...u32(0xA0000),0x8B,0x1D,...u32(0xA0004),0x29,0xC3,
            0x89,0x1D,...u32(RESULT),...done];
        p.push(0xE9,...u32(-p.length-5));
        vm.write_memory(Uint8Array.from(p),0x384000);
        await compile(0x384000); await run(0x384000);
        assert(device_reads > 2);
        assert.equal(word(RESULT),1,"each MMIO read observes a fresh device value");
    } finally { cpu.memory_map_read32[0xA0000 >>> 17] = old_mmio; }
    // Overlap ends at an unmapped virtual page. The first 47 byte stores must
    // complete, and the fault frame/registers must describe the remaining 17.
    vm.write_memory(Uint8Array.from([
        0x89,0x0D,...u32(RESULT),0x89,0x35,...u32(RESULT+4),0x89,0x3D,...u32(RESULT+8),
        0x8B,0x44,0x24,4,0xA3,...u32(RESULT+12),
        0x83,0x44,0x24,4,2,0x83,0xC4,4,0xCF,
    ]),pf_handler);
    const overlap = [0xFC,0xBE,...u32(0x801FD0),0xBF,...u32(0x801FD1),0xB9,...u32(64)];
    const overlap_eip = 0x385000 + overlap.length;
    overlap.push(0xF3,0xA4,...done);
    overlap.push(0xE9,...u32(-overlap.length-5));
    vm.write_memory(Uint8Array.from({length:48},(_,i)=>i+17),0x101FD0);
    vm.write_memory(Uint8Array.from(overlap),0x385000);
    await compile(0x385000); await run(0x385000);
    assert.equal(word(RESULT),17,"REP remaining ECX after partial page fault");
    assert.equal(word(RESULT+4),0x801FFF,"REP partial ESI");
    assert.equal(word(RESULT+8),0x802000,"REP partial EDI");
    assert.equal(word(RESULT+12),overlap_eip,"fault restarts the REP instruction");
    assert.deepEqual(Uint8Array.from(vm.read_memory(0x101FD0,48)),new Uint8Array(48).fill(17));
    console.log("PASS: MMIO reads are not cached; overlapping REP preserves partial progress at #PF");
    console.log(`PASS: ${cases} real-JIT SSE logical register cases, aliases and carry; ${wasm}`);
} finally { await vm.destroy(); }
