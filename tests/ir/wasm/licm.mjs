import assert from "node:assert/strict";
import fs from "node:fs";

// Fixtures are emitted by ir::passes::licm::tests. Compare the entire observed
// state, not just the loop's answer, including partial exits, flags and EIP.
let seed = 0xC0FFEE;
const random = () => {
    seed ^= seed << 13;
    seed ^= seed >>> 17;
    seed ^= seed << 5;
    return seed >>> 0;
};
let executions = 0;
for(const budget of [1, 2, 3, 4, 5, 8, 17, 100]) {
    const variants = [false, true].map(opt => {
        const bytes = fs.readFileSync(`build/ir-licm/loop-${budget}-${opt}.wasm`);
        assert(WebAssembly.validate(bytes), `LICM budget=${budget} opt=${opt}: validation`);
        const memory = new WebAssembly.Memory({initial: 64});
        const module = new WebAssembly.Module(bytes);
        const instance = new WebAssembly.Instance(module, {e: {m: memory}});
        return {memory, execute: instance.exports.f};
    });
    for(const count of [0, 1, 2, 3, 7, 13, 31]) {
        for(let sample = 0; sample < 48; sample++) {
            const input = Array.from({length: 8}, random);
            input[0] = count;
            if(sample === 0) input[2] = input[3] = input[4] = 0;
            if(sample === 1) input[2] = input[3] = input[4] = 0xFFFFFFFF;
            if(sample === 2) { input[2] = 0x7FFFFFFF; input[3] = 1; input[4] = 0x80000000; }
            const flags = 2 | (random() & 0xCD5);
            const observed = variants.map(({memory, execute}) => {
                const bytes = new Uint8Array(memory.buffer, 0, 256);
                bytes.fill(0xA5);
                const words = new Uint32Array(memory.buffer, 0, 64);
                words.set(input);
                words[8] = flags;
                words[9] = 0x12345678;
                words[10] = 0;
                words[11] = input[0];
                const result = execute(0);
                executions++;
                return {result, bytes: bytes.slice(), words: words.slice()};
            });
            const message = `LICM budget=${budget} count=${count} sample=${sample}`;
            assert.equal(observed[1].result, observed[0].result, `${message}: result`);
            assert.deepEqual(observed[1].bytes, observed[0].bytes, `${message}: full state`);
            if(budget === 100) {
                const expected = Math.imul(count, Math.imul((input[2] + input[3]) | 0, input[4])) >>> 0;
                for(const state of observed) {
                    assert.equal(state.words[0], 0, `${message}: counter exhausted`);
                    assert.equal(state.words[1], expected, `${message}: independent modular arithmetic oracle`);
                    assert.equal(state.words[8], flags, `${message}: flags preserved`);
                }
            }
        }
    }
}
console.log(`PASS: ${executions} LICM Wasm executions; zero-trip, overflow, transitive invariants, loop state and budget-exit equivalence`);
