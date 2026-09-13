import assert from "node:assert/strict";
import fs from "node:fs";

// Both paths use real emitted Wasm. A separate BigInt model checks completion;
// every exit budget also compares all observable state, not only the result GPR.
let executions = 0;
const memory = new WebAssembly.Memory({initial: 64});
const words = new Uint32Array(memory.buffer);
for(const budget of [1, 2, 3, 4, 5, 8, 16, 31, 100]) {
    const instances = [false, true].map(opt => {
        const bytes = fs.readFileSync(`build/ir-licm/loop-${budget}-${opt}.wasm`);
        assert(WebAssembly.validate(bytes));
        return new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {m: memory}});
    });
    for(const a of [0, 1, 0x7FFFFFFF, 0xFFFFFFFF])
    for(const b of [0, 3, 0x80000000, 0xFFFFFFFF])
    for(const iterations of [0, 1, 2, 7, 16])
    for(const factor of [0, 1, 7, 0xFFFFFFFF])
    for(const flags of [2, 0x8D7]) {
        const input = [0xFFFFFFFD, a, iterations, b, 0x90000, 0x12345678, factor, 0xABCDEF01];
        const outcomes = instances.map(instance => {
            words.fill(0, 0, 64);
            words.set(input);
            words[8] = flags; words[9] = 0; words[10] = 99; words[11] = 0x76543210;
            instance.exports.f(0);
            executions++;
            return Array.from(words.slice(0, 12));
        });
        assert.deepEqual(outcomes[1], outcomes[0], `budget=${budget}, input=${input}`);
        if(budget === 100) {
            const expected = input.slice();
            const sum = BigInt(a) + BigInt(b);
            expected[0] = Number((BigInt(input[0]) + BigInt(iterations) * (sum & 0xFFFFFFFFn) * BigInt(factor)) & 0xFFFFFFFFn);
            expected[2] = 0; expected[3] = Number(sum >> 32n); expected[7] = Number(sum & 0xFFFFFFFFn);
            assert.deepEqual(outcomes[1], [...expected, flags, 0x3000, 0, 0x76543210]);
        }
    }
}
console.log(`PASS: ${executions} LICM Wasm executions; zero-trip, dependency order, i32/i64 overflow and exact budget recovery`);
