import assert from "node:assert/strict";
import fs from "node:fs";

const memory = new WebAssembly.Memory({ initial: 64 });
const words = new Uint32Array(memory.buffer);
const mask = 0xFFFFFFFFn;
let seed = 0xC001D00D;
function random() {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed;
}
const vectors = [];
for(const a of [0, 1, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF]) {
    for(const b of [0, 1, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF]) {
        vectors.push([a, b, b, a, b ^ 0xA5A5A5A5, 0x12345678, 0xDEADBEEF, 0xABCDEF01]);
    }
}
for(let i = 0; i < 128; i++) vectors.push(Array.from({ length: 8 }, random));
let executions = 0, complete = 0;
for(const budget of [1, 2, 3, 4, 5, 7, 8, 12, 31, 256]) {
    const instances = ["plain", "licm", "full"].map(name => {
        const bytes = fs.readFileSync(`build/ir-licm/loop-${budget}-${name}.wasm`);
        assert(WebAssembly.validate(bytes));
        return new WebAssembly.Instance(new WebAssembly.Module(bytes), { e: { m: memory } });
    });
    for(const vector of vectors) for(const count of [0, 1, 2, 9, 17]) for(const flags of [2, 0x8D7]) {
        const input = vector.map(v => v >>> 0);
        input[1] = count;
        const wide = BigInt(input[2]) + BigInt(input[3]);
        const step = (((wide & mask) ^ BigInt(input[4])) + (wide >> 32n)) & mask;
        const expected = input.slice();
        expected[0] = Number((BigInt(input[0]) + BigInt(count) * step) & mask);
        expected[1] = 0;
        let reference;
        for(const instance of instances) {
            words.fill(0x5A5A5A5A, 0, 128);
            words.set(input);
            words[8] = flags;
            words[9] = 0x77777777;
            words[10] = 99;
            words[11] = 0x76543210;
            instance.exports.f(0);
            const result = Array.from(words.slice(0, 128));
            if(reference) assert.deepEqual(result, reference, `budget ${budget}, count ${count}: identical recovery`);
            else reference = result;
            assert.equal(words[8], flags, "unchanged FLAGS");
            assert.equal(words[11], 0x76543210, "unchanged lazy flag operand");
            assert(result.slice(12).every(v => v === 0x5A5A5A5A), "no out-of-state writes");
            if(budget === 256) {
                assert.deepEqual(result.slice(0, 8), expected, "independent BigInt loop oracle");
                assert.equal(words[9], 0x9000, "completed at the exit");
                assert.equal(words[10], count, "exact dynamic retirement count");
                complete++;
            }
            executions++;
        }
    }
}
console.log(`PASS: ${executions} LICM Wasm executions, ${complete} independent completed-loop results; zero-trip, overflow, FLAGS, polling and precise budget recovery`);
