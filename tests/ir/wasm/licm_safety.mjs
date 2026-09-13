import assert from "node:assert/strict";
import fs from "node:fs";

const memory = new WebAssembly.Memory({initial: 64});
const words = new Uint32Array(memory.buffer);
const edge = [0, 1, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF];
let seed = 0x91E10DA5;
function random() {
    seed ^= seed << 13;
    seed ^= seed >>> 17;
    seed ^= seed << 5;
    return seed >>> 0;
}
const cases = [];
for(const a of edge) for(const b of edge) {
    for(const count of [0, 1, 2, 7, 31, 0xFFFFFFFF]) {
        cases.push([a, b, count, random(), random(), random(), random(), random()]);
    }
}
for(let i = 0; i < 256; i++) {
    cases.push([random(), random(), random() & 31, random(), random(), random(), random(), random()]);
}
let executions = 0;
for(const budget of [1, 2, 3, 4, 5, 8, 16, 64, 256]) {
    const instances = [false, true].map(optimized => {
        const bytes = fs.readFileSync(`build/ir-licm-safety/loop-${budget}-${optimized}.wasm`);
        assert(WebAssembly.validate(bytes), "LICM output validates");
        return new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {m: memory}});
    });
    for(const input of cases) for(const flags of [2, 0x8D7]) {
        const outputs = instances.map(instance => {
            words.fill(0, 0, 64);
            words.set(input);
            words[8] = flags;
            words[9] = 0xDEADBEEF;
            words[10] = 99;
            words[11] = 0x76543210;
            instance.exports.f(0);
            executions++;
            return Array.from(words.slice(0, 12));
        });
        assert.deepEqual(outputs[1], outputs[0], `LICM state mismatch, budget ${budget}, input ${input}`);
        assert.equal(outputs[1][8], flags, "FLAGS are unchanged");
        assert.equal(outputs[1][11], 0x76543210, "lazy FLAGS operand is unchanged");
        if(outputs[1][9] === 0x4000) {
            const sum = (BigInt(input[0]) + BigInt(input[1])) & 0xFFFFFFFFn;
            const expected = Number((sum * sum * BigInt(input[2])) & 0xFFFFFFFFn);
            assert.equal(outputs[1][2], 0, "loop counter exhausted");
            assert.equal(outputs[1][3], expected, "modular arithmetic oracle");
            for(const reg of [0, 1, 4, 5, 6, 7]) {
                assert.equal(outputs[1][reg], input[reg], `GPR ${reg} preserved`);
            }
        }
    }
}
console.log(`PASS: ${executions} LICM safety Wasm executions; overflow, zero-trip loops, dependent invariants, exact poll/block recovery and independent arithmetic oracle`);
