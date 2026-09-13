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
for(const a of edge) {
    for(const b of edge) {
        for(const count of [0, 1, 2, 7, 31, 0xFFFFFFFF]) {
            cases.push([a, b, count, random(), random(), random(), random(), random()]);
        }
    }
}
for(let i = 0; i < 256; i++) {
    cases.push([random(), random(), random() & 31, random(), random(), random(), random(), random()]);
}
let executions = 0;
let completed = 0;
const recovery_sites = new Set();
for(const budget of [1, 2, 3, 4, 5, 8, 16, 64, 256]) {
    const instances = [false, true].map(optimized => {
        const bytes = fs.readFileSync(`build/ir-licm-safety/loop-${budget}-${optimized}.wasm`);
        assert(WebAssembly.validate(bytes), "LICM output validates");
        return new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {m: memory}});
    });
    for(const input of cases) {
        for(const flags of [2, 0x8D7]) {
            const outputs = instances.map(instance => {
                words.fill(0xA5A5A5A5, 0, 64);
                words.set(input);
                words[8] = flags;
                words[9] = 0xDEADBEEF;
                words[10] = 99;
                words[11] = 0x76543210;
                const result = instance.exports.f(0);
                executions++;
                return {result, state: Array.from(words.slice(0, 64))};
            });
            assert.deepEqual(outputs[1], outputs[0],
                `LICM state mismatch, budget ${budget}, input ${input}`);
            const state = outputs[1].state;
            assert.equal(state[8], flags, "FLAGS are unchanged");
            assert.equal(state[11], 0x76543210, "lazy FLAGS operand is unchanged");
            assert(state.slice(12).every(value => value === 0xA5A5A5A5),
                "CPU layout canaries are unchanged");
            recovery_sites.add(state[9]);
            if(budget === 256 && input[2] <= 31) {
                assert.equal(state[9], 0x4000, "sufficient budget completes finite loops");
            }
            if(state[9] === 0x4000) {
                completed++;
                const sum = (BigInt(input[0]) + BigInt(input[1])) & 0xFFFFFFFFn;
                const expected = Number((sum * sum * BigInt(input[2])) & 0xFFFFFFFFn);
                assert.equal(state[2], 0, "loop counter exhausted");
                assert.equal(state[3], expected, "independent modular arithmetic oracle");
                for(const reg of [0, 1, 4, 5, 6, 7]) {
                    assert.equal(state[reg], input[reg], `GPR ${reg} preserved`);
                }
            }
            if(state[9] === 0x3000) {
                assert.equal(state[7], (input[0] + input[1]) >>> 0,
                    "poll recovery materializes a value moved to the preheader");
            }
        }
    }
}
assert(completed > 0, "arithmetic oracle must be exercised");
assert(recovery_sites.has(0x3000), "post-expression poll must be exercised");
assert(recovery_sites.has(0x2002), "loop-header budget exit must be exercised");
console.log(`PASS: ${executions} LICM safety Wasm executions, ${completed} completed-loop oracles; exact poll recovery, zero-trip and overflow`);
