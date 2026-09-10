import assert from "node:assert/strict";
import fs from "node:fs";
import { V86 } from "../../build/libv86.mjs";

const candidate = process.argv[3] || "build/v86.wasm";
const baseline = process.argv[2] || candidate;
const bios = Uint8Array.from(fs.readFileSync("build/jit-capacity.bin")).buffer;
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const u32 = n => [n & 255, n >>> 8 & 255, n >>> 16 & 255, n >>> 24 & 255];
const CODE = 0x100000, DATA = 0x200000, OUT = 0x210000;
const machines = [];
const word = (vm, a) => new DataView(Uint8Array.from(vm.read_memory(a, 4)).buffer).getUint32(0, true);
const data = Uint8Array.from({ length: 8192 }, (_, i) => (i * 37 ^ i >> 3) & 255);
const programs = [];
for(const mmx of [false, true]) {
    const prefix = mmx ? [] : [0x66];
    for(let seed = 0; seed < 16; seed++) {
        const p = [0xDB, 0xE3];
        for(let r = 0; r < 8; r++) p.push(...(mmx ? [] : [0xF3]), 0x0F, 0x6F, 5 | r << 3, ...u32(DATA + r * 16));
        // Consecutive cached destinations, source aliases, shifts reading a
        // cached count, old custom logic emitters, and MMX custom emitters.
        const ops = [0xFC,0xD5,0xE4,0xF6,0xDB,0xDF,0xEB,0xEF,0xF5,0xFE,0xD1,0xF3];
        for(let i = 0; i < 96; i++) {
            p.push(...prefix, 0x0F, ops[(i + seed) % ops.length], 0xC0 | (i & 7) << 3 | ((i + seed) & 7));
            // Memory, float/helper and group boundaries must flush all values.
            if(i % 19 === 18) p.push(...(mmx ? [] : [0xF3]), 0x0F, 0x6F, 0x05, ...u32(DATA + i));
            if(i % 23 === 22) p.push(...prefix, 0x0F, 0x71, 0xD0, seed);
        }
        for(let r = 0; r < 8; r++) p.push(...(mmx ? [] : [0xF3]), 0x0F, 0x7F, 5 | r << 3, ...u32(OUT + r * 16));
        if(mmx) p.push(0xD9, 0x05, ...u32(DATA), 0xDD, 0x35, ...u32(OUT + 128)); // x87 observer/FNSAVE
        programs.push([`cached ${mmx ? "MMX" : "XMM"} sequence ${seed}`, p]);
    }
}
for(const prefix of [[], [0x66], [0xF2], [0xF3]]) for(let seed = 0; seed < 8; seed++) {
    const p = [];
    for(let r = 0; r < 8; r++) p.push(0xF3, 0x0F, 0x6F, 5 | r << 3, ...u32(DATA + r * 16));
    for(let i = 0; i < 64; i++) {
        p.push(...prefix, 0x0F, [0x58,0x59,0x5C,0x5D,0x5E,0x5F,0x51][i % 7], 0xC0 | (i & 7) << 3 | ((i + seed) & 7));
        if(i % 11 === 10) p.push(0x66, 0x0F, 0xEF, 0xC1);
    }
    for(let r = 0; r < 8; r++) p.push(0xF3, 0x0F, 0x7F, 5 | r << 3, ...u32(OUT + r * 16));
    programs.push([`floating cache prefix=${prefix} aliases=${seed}`, p]);
}
const bits = new DataView(data.buffer);
for(const [i, v] of [0n, 0x8000000000000000n, 0x7FF0000000000000n, 0x7FF123456789ABCDn,
    0x3FF0000000000000n, 0x7FC123457F812345n, 0x3F800000BF800000n, 1n].entries()) bits.setBigUint64(i * 16, v, true);
for(let pc = 0; pc < 3; pc++) for(let rc = 0; rc < 4; rc++) {
    const control = [0,2,3][pc] << 8 | rc << 10 | 0x3F;
    bits.setUint16(7800 + (pc * 4 + rc) * 2,control,true);
    const p = [0xDB,0xE3,0xD9,0x2D,...u32(DATA+7800+(pc*4+rc)*2)];
    for(const [i, exponent] of [-1023,-1022,-1.75,0,1.75,1023,1024].entries()) {
        bits.setFloat64(2000+i*16,1.23456789,true);
        bits.setFloat64(2008+i*16,exponent,true);
        p.push(0xDD,0x05,...u32(DATA+2008+i*16),0xDD,0x05,...u32(DATA+2000+i*16),
            0xD9,0xFD,0xDB,0x3D,...u32(OUT+i*32),0xDB,0x3D,...u32(OUT+i*32+16));
    }
    p.push(0xDF,0xE0,0xA3,...u32(OUT+256));
    programs.push([`FSCALE pc=${pc} rc=${rc}`,p]);
}
// Exercise both FST and FSTP through the JIT, with exact values, rounding,
// signed zero, subnormal, overflow and NaN cases under every x87 control mode.
const doubles = [0n, 0x8000000000000000n, 1n, 0xFFFFFFFFFFFFFn, 0x10000000000000n,
    0x3FF3C0CA4283DE1Bn, 0xBFF3C0CA4283DE1Bn, 0x7FEFFFFFFFFFFFFFn,
    0xFFEFFFFFFFFFFFFFn, 0x7FF0000000000000n, 0x7FF0000000000001n, 0x7FF8123456789ABCn];
const wide_stores = [[0x8000000000000000n,0x3C01], [0xFFFFFFFFFFFFF800n,0x43FE],
    [0x8000000000000400n,0x3FFF], [0x8000000000000C00n,0xBFFF],
    [0xFFFFFFFFFFFFFFFFn,0x43FE], [0x8000000000000001n,0x3C00],
    [0x8000000000000000n,0x7FFF], [0x8000000000000001n,0x7FFF]];
for(const [i,value] of doubles.entries()) bits.setBigUint64(3000+i*16,value,true);
for(const [i,[mantissa,exponent]] of wide_stores.entries()) {
    bits.setBigUint64(3400+i*16,mantissa,true); bits.setUint16(3408+i*16,exponent,true);
}
for(let pc = 0; pc < 3; pc++) for(let rc = 0; rc < 4; rc++) {
    const p = [0xDB,0xE3,0xD9,0x2D,...u32(DATA+7800+(pc*4+rc)*2)];
    // FNINIT marks registers empty without erasing their old payloads. Seed
    // all eight slots so FNSAVE is independent of earlier benchmark programs.
    p.unshift(0xDB,0xE3,...Array.from({ length: 8 }, () => [0xD9,0xEE]).flat());
    for(let i = 0; i < doubles.length + wide_stores.length; i++) {
        p.push(...(i < doubles.length ? [0xDD,0x05,...u32(DATA+3000+i*16)] :
            [0xDB,0x2D,...u32(DATA+3400+(i-doubles.length)*16)]),
        0xDD,0x15,...u32(OUT+i*32), 0xDD,0x3D,...u32(OUT+i*32+8),
        0xDD,0x1D,...u32(OUT+i*32+16), 0xDD,0x3D,...u32(OUT+i*32+24));
    }
    p.push(0xDD,0x35,...u32(OUT+4096));
    programs.push([`binary64 stores pc=${pc} rc=${rc}`,p]);
}
for(const start of [0, 1, 4080, 4093, 4095]) {
    const p = [];
    for(let r = 0; r < 4; r++) p.push(0x8B, 5 | r << 3, ...u32(DATA + start + r * 4));
    for(let r = 0; r < 4; r++) p.push(0x89, 5 | r << 3, ...u32(OUT + r * 4));
    // A write ends the translation region and must be visible on the next read.
    p.push(0xA1, ...u32(DATA), 0xC7, 0x05, ...u32(DATA + 4), ...u32(0xABCD1234), 0xA1, ...u32(DATA + 4), 0xA3, ...u32(OUT + 16));
    programs.push([`RAM translation, offset ${start}`, p]);
}
for(const first of [[0x05, ...u32(1)], [0x2D, ...u32(1)], [0x3D, ...u32(1)],
    [0x83, 0xC0, 1], [0x83, 0xE8, 1], [0x83, 0xF8, 1], [0x21, 0xD8], [0x09, 0xD8], [0x31, 0xD8], [0x85, 0xD8]]) {
    for(const barrier of [[], [0xB9,...u32(7),0x89,0xDA,0x90], [0x9C,0x59], [0x83,0xD0,0], [0x40], [0xD3,0xE0]]) {
        const p = [0xB8,...u32(0xFFFFFFFF),0xBB,...u32(1),0xB9,...u32(0),0xBA,...u32(0),
            ...first,...barrier,0x29,0xD8,0xA3,...u32(OUT),0x89,0x0D,...u32(OUT+4),0x9C,0x58,0xA3,...u32(OUT+8)];
        programs.push([`flags opcode=${first} barrier=${barrier}`,p]);
    }
}
for(const width of [1, 2, 4]) for(const backward of [false, true]) for(const distance of [1, 2, 3, 16, 127]) {
    const source = 0x220000 + (backward ? 4200 : 4000);
    const dest = source + (backward ? -distance : distance);
    const p = [0xFC, 0xBE, ...u32(DATA), 0xBF, ...u32(0x220000), 0xB9, ...u32(8192), 0xF3, 0xA4,
        backward ? 0xFD : 0xFC, 0xBE, ...u32(source), 0xBF, ...u32(dest), 0xB9, ...u32(64),
        ...(width === 2 ? [0x66] : []), 0xF3, width === 1 ? 0xA4 : 0xA5,
        0x89, 0x0D, ...u32(OUT + 8000), 0x89, 0x35, ...u32(OUT + 8004), 0x89, 0x3D, ...u32(OUT + 8008),
        0xFC, 0xBE, ...u32(0x220000 + 3600), 0xBF, ...u32(OUT), 0xB9, ...u32(1024), 0xF3, 0xA4];
    programs.push([`ordered REP width=${width} backward=${backward} distance=${distance}`, p]);
}
try {
    for(const wasm_path of [baseline, candidate]) {
        const vm = new V86({ wasm_path, bios: { buffer: bios.slice(0) }, memory_size: 32 << 20,
            disable_jit: machines.length === 0 && !process.argv[2] && !process.env.CACHE_CONTROL,
            disable_keyboard: true, disable_mouse: true, disable_speaker: true,
            net_device: { type: "none" }, autostart: false });
        machines.push(vm);
        await new Promise(resolve => vm.add_listener("emulator-loaded", resolve));
        vm.run();
        const deadline = performance.now() + 10000;
        while(word(vm, 0x500) !== 0xCAFE) { assert(performance.now() < deadline); await sleep(1); }
        await vm.stop();
    }
    const selected = programs.filter(([name]) => !process.env.SEQUENCE_FILTER || (process.env.SEQUENCE_FILTER === "nonfloating" ? !name.startsWith("floating") : name.includes(process.env.SEQUENCE_FILTER)));
    for(const [name, body] of selected) {
        const p = [...body, 0xC7, 0x05, ...u32(0x600), ...u32(0xCAFE)];
        p.push(0xE9, ...u32(-p.length - 5));
        const results = [];
        for(const vm of machines) {
            vm.write_memory(data, DATA);
            vm.write_memory(new Uint8Array(8192), OUT);
            vm.write_memory(new Uint8Array(4), 0x600);
            vm.write_memory(Uint8Array.from(p), CODE);
            const cpu = vm.v86.cpu, e = cpu.wm.exports;
            if(process.env.CACHE_CONTROL) e.set_jit_config(6, Number(vm !== machines[0]));
            cpu.instruction_pointer[0] = CODE; cpu.in_hlt[0] = 0;
            e.performance_recording_enable(1);
            vm.run();
            const deadline = performance.now() + 10000;
            while(word(vm, 0x600) !== 0xCAFE || (vm !== machines[0] || process.argv[2] || process.env.CACHE_CONTROL) && e.performance_recording_get(1) === 0) {
                assert(performance.now() < deadline, name); await sleep(1);
            }
            await sleep(20); await vm.stop(); e.performance_recording_enable(0);
            results.push(Uint8Array.from(vm.read_memory(OUT, 8192)));
        }
        assert.deepEqual(results[1], results[0], name);
    }
    console.log(`PASS: ${selected.length} actual JIT sequences, aliases, MMX/x87 transitions and RAM boundaries`);
} finally { for(const vm of machines) await vm.destroy(); }
