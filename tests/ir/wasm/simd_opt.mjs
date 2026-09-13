import assert from "node:assert/strict";
import fs from "node:fs";
const cases = JSON.parse(fs.readFileSync("build/ir-simd-opt/cases.json"));
const memory = new WebAssembly.Memory({initial: 64});
const words = new Uint32Array(memory.buffer);
const bytes = new Uint8Array(memory.buffer);
function expected(test, input) {
    const values = input.map(v => v.slice());
    for(const [op, a, b, lanes] of test.nodes) {
        const x = values[a], y = values[b];
        switch(op) {
            case "shuffle": values.push(lanes.map(i => i < 16 ? x[i] : y[i - 16])); break;
            case "and": values.push(x.map((v, i) => v & y[i])); break;
            case "or": values.push(x.map((v, i) => v | y[i])); break;
            default: throw new Error(`unknown reference operation ${op}`);
        }
    }
    return values[test.output];
}
let executions = 0;
for(const [index, test] of cases.entries()) {
    const instances = [false, true].map(opt => {
        const code = fs.readFileSync(`build/ir-simd-opt/${index}-${opt}.wasm`);
        assert(WebAssembly.validate(code));
        return new WebAssembly.Instance(new WebAssembly.Module(code), {e: {
            m: memory, get_eflags: () => words[30], ir_enter: () => {}, ir_tlb_base: () => 0,
        }});
    });
    for(let seed = 0; seed < 64; seed++) {
        let random = seed + 12345;
        const input = Array.from({length: 8}, () => Array.from({length: 16}, () => {
            random ^= random << 13; random ^= random >>> 17; random ^= random << 5;
            return random & 255;
        }));
        const vector = expected(test, input);
        for(const instance of instances) {
            bytes.fill(0, 0, 2048);
            input.forEach((v, r) => bytes.set(v, 832 + r * 16));
            words.set([0, 1, 2, 3, 0x12345678, 0xFFFFFFFF, 0xABCDEF01, 0x76543210], 16);
            words[30] = 0x8D7; words[166] = 0xFFFFFFFF; words[26] = 0x87654321;
            instance.exports.f(0);
            for(let r = 0; r < 8; r++) {
                assert.deepEqual(Array.from(bytes.slice(832 + r * 16, 848 + r * 16)), r === 4 ? vector : input[r]);
            }
            assert.deepEqual(Array.from(bytes.slice(64, 80)), vector);
            assert.deepEqual(Array.from(words.slice(20, 24)), [0x12345678, 0xFFFFFFFF, 0xABCDEF01, 0x76543210]);
            assert.equal(words[30], 0x8D7); assert.equal(words[139], 0x1002);
            assert.equal(words[166], 0); assert.equal(words[26], 0x87654321);
            executions++;
        }
    }
}
console.log(`PASS: ${executions} SIMD identity/shuffle executions against independent byte semantics, including four-source rejection and GPR/XMM recovery`);
