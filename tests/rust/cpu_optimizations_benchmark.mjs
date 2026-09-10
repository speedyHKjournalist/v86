import assert from "node:assert/strict";
import fs from "node:fs";
import { V86 } from "../../build/libv86.mjs";
const baseline = process.argv[2] || "build/cpu-opt-baseline.wasm";
const candidate = process.argv[3] || "build/v86.wasm";
const u32 = n => [n & 255, n >>> 8 & 255, n >>> 16 & 255, n >>> 24 & 255];
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const bios = Uint8Array.from(fs.readFileSync("build/jit-capacity.bin")).buffer;
const median = a => a.toSorted((x, y) => x - y)[a.length >> 1];
const machines = [];
const word = (vm, a) => new DataView(Uint8Array.from(vm.read_memory(a, 4)).buffer).getUint32(0, true);
try {
    for(const wasm_path of [baseline, candidate]) {
        const vm = new V86({ wasm_path, bios: { buffer: bios.slice(0) }, memory_size: 32 << 20,
            disable_keyboard: true, disable_mouse: true, disable_speaker: true,
            net_device: { type: "none" }, autostart: false });
        machines.push(vm);
        await new Promise(resolve => vm.add_listener("emulator-loaded", resolve));
        vm.run();
        const end = performance.now() + 10000;
        while(word(vm, 0x500) !== 0xCAFE) { assert(performance.now() < end); await sleep(1); }
        await vm.stop();
        if(machines.length === 1 && process.env.BASELINE_SIMD_CACHE !== undefined)
            vm.v86.cpu.wm.exports.set_jit_config(6,Number(process.env.BASELINE_SIMD_CACHE));
        if(machines.length === 2) for(const [index, name] of [[1, "JIT_PAGES"], [3, "JIT_BLOCKS"], [4, "JIT_THRESHOLD"], [5, "JIT_LINKS"], [6, "JIT_SIMD_CACHE"]]) {
            if(process.env[name] !== undefined) vm.v86.cpu.wm.exports.set_jit_config(index, Number(process.env[name]));
        }
    }
    const address = 0x100000;
    const kinds = ["sse3-duplicate-chain", "sse3-addsub", "sse-register-logic", "integer-flags", "indirect-call-ret", "short-cross-page",
        "x87-add-hit", "x87-add-fallback", "x87-f32-load-store", "x87-f32-rounded-store", "x87-f32-subnormal-store",
        "mmx-movq", "mmx-paddd", "mmx-pmaddwd", "mmx-packed", "sse-packed", "sse-float", "sse-scalar-float", "x87-f64-load-store", "x87-f64-rounded-store", "x87-f64-subnormal-store", "integer-flags-separated", "ram-reads", "rep-overlap", "x87-round", "x87-scale"];
    for(const kind of kinds.filter(kind => !process.env.BENCH_KINDS || process.env.BENCH_KINDS.split(",").includes(kind))) {
        const program = [0xB8, ...u32(1), 0xBB, ...u32(kind === "indirect-call-ret" ? address + 512 : 17)];
        if(kind === "sse-register-logic") {
            for(let r = 0; r < 8; r++) program.push(0x66, 0x0F, 0x6E, 0xC0 | r << 3);
        }
        if(kind === "sse-float" || kind === "sse-scalar-float") {
            for(let r = 0; r < 8; r++) program.push(0x0F,0x10,0x05 | r << 3,...u32(0x220000));
        }
        if(kind.startsWith("x87")) program.push(0xDB, 0xE3);
        if(kind.startsWith("sse3")) {
            for(let r = 0; r < 8; r++) program.push(0xF3,0x0F,0x6F,5 | r << 3,...u32(0x220000));
        }
        const loop = program.length;
        if(kind.startsWith("sse3")) {
            for(let i = 0; i < 32; i++) {
                const [prefix,op] = kind === "sse3-addsub" ? [i&1 ? 0xF2 : 0x66,0xD0] :
                    [[0xF2,0x12],[0xF3,0x12],[0xF3,0x16],[0xF2,0x7D]][i&3];
                program.push(prefix,0x0F,op,0xC0 | (i&7)<<3 | ((i+1)&7));
            }
        } else if(kind === "sse-register-logic") {
            for(let i = 0; i < 32; i++) program.push(0x0F, [0x54, 0x55, 0x56, 0x57][i & 3], 0xC0 | (i & 7) << 3 | ((i + 1) & 7));
        } else if(kind === "sse-packed" || kind === "mmx-packed") {
            for(let i = 0; i < 32; i++) program.push(...(kind === "sse-packed" ? [0x66] : []),
                0x0F, [0xEC,0xD5,0xE4,0xF6][i & 3], 0xC0 | (i & 7) << 3 | ((i + 1) & 7));
        } else if(kind === "sse-float" || kind === "sse-scalar-float") {
            for(let i = 0; i < 32; i++) program.push(...(kind === "sse-scalar-float" ? [0xF3] : []),
                0x0F, [0x5D,0x5F,0x59,0x51][i & 3], 0xC0 | (i & 7) << 3 | ((i + 1) & 7));
        } else if(kind === "x87-f64-load-store") {
            for(let i = 0; i < 16; i++) program.push(0xDD,0x05,...u32(0x220030),0xDD,0x1D,...u32(0x220020));
        } else if(kind === "x87-f64-rounded-store" || kind === "x87-f64-subnormal-store") {
            for(let i = 0; i < 16; i++) program.push(0xDB,0x2D,...u32(0x220000),0xDD,0x1D,...u32(0x220020));
        } else if(kind === "integer-flags-separated") {
            for(let i = 0; i < 16; i++) program.push(0x01, 0xD8, 0x89, 0xDA, 0x90, 0x29, 0xD8);
        } else if(kind === "ram-reads") {
            for(let i = 0; i < 32; i++) program.push(0x8B, 5 | (i & 3) << 3, ...u32(0x220000 + i * 4));
        } else if(kind === "rep-overlap") {
            program.push(0xFC, 0xBE, ...u32(0x220000), 0xBF, ...u32(0x220001), 0xB9, ...u32(1024), 0xF3, 0xA4);
        } else if(kind === "x87-scale") {
            for(let i = 0; i < 16; i++) program.push(0xD9, 0x05, ...u32(0x220030),
                0xD9, 0x05, ...u32(0x220030), 0xD9, 0xFD, 0xD9, 0x1D, ...u32(0x220020), 0xDD, 0xD8);
        } else if(kind === "x87-round") {
            for(let i = 0; i < 16; i++) program.push(0xD9, 0x05, ...u32(0x220030), 0xD9, 0xFC, 0xD9, 0x1D, ...u32(0x220020));
        } else if(kind === "integer-flags") {
            for(let i = 0; i < 16; i++) program.push(0x01, 0xD8, 0x29, 0xD8);
        } else if(kind === "x87-f32-load-store") {
            for(let i = 0; i < 16; i++) program.push(0xD9, 0x05, ...u32(0x220030), 0xD9, 0x1D, ...u32(0x220020));
        } else if(kind === "x87-f32-rounded-store" || kind === "x87-f32-subnormal-store") {
            for(let i = 0; i < 16; i++) program.push(0xDB, 0x2D, ...u32(0x220000), 0xD9, 0x1D, ...u32(0x220020));
        } else if(kind.startsWith("mmx")) {
            for(let i = 0; i < 32; i++) program.push(0x0F,
                kind === "mmx-movq" ? 0x6F : kind === "mmx-paddd" ? 0xFE : 0xF5,
                0xC0 | (i & 7) << 3 | ((i + 1) & 7));
        } else if(kind === "short-cross-page") {
            // Indirect exits force independently compiled pages through the
            // core dispatcher, unlike a long loop inside one generated module.
        } else if(kind.startsWith("x87")) {
            for(let i = 0; i < 16; i++) program.push(0xDB, 0x2D, ...u32(0x220000),
                0xDB, 0x2D, ...u32(0x220010), 0xDE, 0xC1, 0xDB, 0x3D, ...u32(0x220020));
        } else {
            for(let i = 0; i < 8; i++) program.push(0xFF, 0xD3);
        }
        program.push(0xFF, 0x05, ...u32(0x600));
        if(kind === "short-cross-page") {
            program.push(0xB8, ...u32(address + 4096), 0xFF, 0xE0);
            while(program.length < 4096) program.push(0x90);
            program.push(0xB8, ...u32(address), 0xFF, 0xE0);
        } else program.push(0xE9, ...u32(loop - program.length - 5));
        if(kind === "indirect-call-ret") {
            while(program.length < 512) program.push(0x90);
            program.push(0x83, 0xC0, 1, 0xC3);
        }
        for(const vm of machines) {
            const operands = new Uint8Array(64), view = new DataView(operands.buffer);
            const low = kind === "x87-add-fallback" || kind === "x87-f32-rounded-store" || kind === "x87-f64-rounded-store" ? 0x8000000001n : 0n;
            view.setBigUint64(0, 0x9000000000000000n | low, true); view.setUint16(8, kind === "x87-f32-subnormal-store" ? 0x3F70 : kind === "x87-f64-subnormal-store" ? 0x3BFF : 0x4000, true);
            view.setBigUint64(16, 0x8800000000000000n | low, true); view.setUint16(24, 0x3FFF, true);
            view.setUint32(48, 0x3FC12345, true);
            if(kind === "x87-f64-load-store") view.setFloat64(48, 1.23456789, true);
            if(kind === "sse-float" || kind === "sse-scalar-float" || kind.startsWith("sse3")) {
                for(let j = 0; j < 4; j++) view.setFloat32(j * 4, 1, true);
            }
            vm.write_memory(operands, 0x220000);
            vm.write_memory(Uint8Array.from(program), address);
            vm.v86.cpu.instruction_pointer[0] = address;
            vm.v86.cpu.in_hlt[0] = 0;
            vm.run(); await sleep(500); await vm.stop();
        }
        if(kind.startsWith("x87-f64")) {
            assert.deepEqual(Uint8Array.from(machines[1].read_memory(0x220020, 8)),
                Uint8Array.from(machines[0].read_memory(0x220020, 8)), "binary64 store bits must match");
        }
        const times = [[], []];
        for(let round = 0; round < 9; round++) for(const index of round & 1 ? [1, 0] : [0, 1]) {
            const vm = machines[index], before = word(vm, 0x600), start = performance.now();
            while(performance.now() - start < 120) vm.v86.cpu.main_loop();
            const elapsed = performance.now() - start;
            const iterations = (word(vm, 0x600) - before) >>> 0;
            assert(iterations > 0);
            if(round >= 2) times[index].push(iterations / elapsed);
        }
        const old_rate = median(times[0]), new_rate = median(times[1]);
        console.log(JSON.stringify({ kind, baseline, candidate, baseline_iterations_per_ms: old_rate,
            candidate_iterations_per_ms: new_rate, speedup: new_rate / old_rate, rounds: 7,
            baseline_samples: times[0], candidate_samples: times[1],
            baseline_simd_cache: process.env.BASELINE_SIMD_CACHE,
            candidate_simd_cache: process.env.JIT_SIMD_CACHE }));
    }
} finally { for(const vm of machines) await vm.destroy(); }
