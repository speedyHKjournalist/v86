import assert from "node:assert/strict";
import fs from "node:fs";

const cases = JSON.parse(fs.readFileSync("build/ir-simd-simplify/cases.json"));
const samples = [0, 1, 0xFFFF0001, 0x80000000, 0xFFFFFFFF];
const seeds = [0, 1, 3, 17, 63, 127, 128, 255];
const memory = new WebAssembly.Memory({initial: 64});
const bytes = new Uint8Array(memory.buffer), words = new Uint32Array(memory.buffer);
let executions = 0;

function model(test, gpr, xmm) {
    const output = xmm.map(v => v.slice()), registers = gpr.slice();
    const [kind, argument, lane] = test;
    if(kind === "shuffle") {
        const inner = argument.map(i => xmm[0].concat(xmm[1])[i]);
        output[0] = lane.map(i => inner[i & 15]);
    } else if(kind === "overlap") {
        for(let i = 0; i < 4; i++) {
            output[0][i] = (gpr[0] >>> (8 * i)) & 255;
            output[0][i + 4] = (gpr[2] >>> (8 * i)) & 255;
        }
    } else if(kind !== "roundtrip") {
        const bits = argument;
        const scalar = kind === "overwrite" ? BigInt(gpr[2])
            : bits === 64 ? BigInt(gpr[0]) | BigInt(gpr[1]) << 32n : BigInt(gpr[0]);
        for(let i = 0; i < bits / 8; i++) output[0][lane * bits / 8 + i] = Number(scalar >> BigInt(8 * i) & 255n);
        if(kind === "forward") registers[0] = Number(BigInt.asUintN(Math.min(bits, 32), scalar));
    }
    return {output, registers};
}

for(const [index, test] of cases.entries()) {
    const instances = [false, true].map(opt => {
        const binary = fs.readFileSync(`build/ir-simd-simplify/${index}-${opt}.wasm`);
        assert(WebAssembly.validate(binary));
        const module = new WebAssembly.Module(binary);
        for(const entry of WebAssembly.Module.imports(module)) {
            assert.equal(entry.module, "e");
            assert(["m", "get_eflags", "ir_enter", "ir_tlb_base"].includes(entry.name), `unexpected CPU import ${entry.name}`);
        }
        // This corpus has no MMU, device or helper operations. With lazy flags
        // disabled, get_eflags is exactly the architectural FLAGS memory value.
        return new WebAssembly.Instance(module, {e: {m: memory,
            // No guest memory or REP state is consumed by these pure fixtures.
            // Full cold-entry adapter semantics are covered by the CPU suites.
            ir_enter: () => { words[560 / 4] = words[556 / 4]; },
            ir_tlb_base: () => 0,
            get_eflags: () => {
            assert.equal(words[100 / 4], 0);
            return words[120 / 4];
        }}});
    });
    for(const sample of samples) for(const seed of seeds) {
        const input = [sample, (sample ^ 0x81FFFF42) >>> 0, (sample + 0x89ABCDEF) >>> 0,
            0xABCDEF, 0x90000, 0x12345678, 0x87654321, 0xFFFFFFFF];
        const xmm = Array.from({length: 8}, (_, r) => Array.from({length: 16}, (_, i) => (seed + r * 37 + i * 13) & 255));
        const expected = model(test, input, xmm);
        let baseline;
        for(const instance of instances) {
            bytes.fill(0xA5, 0, 4096);
            words.set(input, 64 / 4);
            words[100 / 4] = 0; words[104 / 4] = 0x76543210; words[120 / 4] = 0x8D7;
            words[740 / 4] = 0; // CS base
            words[664 / 4] = 99;
            for(let r = 0; r < 8; r++) bytes.set(xmm[r], 832 + r * 16);
            instance.exports.f(0);
            assert.deepEqual(Array.from(words.slice(64 / 4, 96 / 4)), expected.registers, `${index} scalar result`);
            for(let r = 0; r < 8; r++) assert.deepEqual(Array.from(bytes.slice(832 + r * 16, 848 + r * 16)), expected.output[r], `${index} xmm${r}`);
            assert.equal(words[556 / 4], 0x1001);
            assert.equal(words[664 / 4], 100);
            assert.equal(words[120 / 4], 0x8D7);
            const observed = bytes.slice(0, 4096);
            if(baseline) assert.deepEqual(observed, baseline, "complete CPU observation preserved");
            else baseline = observed;
            executions++;
        }
    }
}
console.log(`PASS: ${executions} SIMD simplification Wasm executions; byte-array oracle, 16-bit truncation and complete CPU state equality`);
