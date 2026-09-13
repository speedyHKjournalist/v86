import assert from "node:assert/strict";
import fs from "node:fs";

const memory = new WebAssembly.Memory({initial: 64});
const words = new Uint32Array(memory.buffer);
let executions = 0;
for(const poll of [false, true]) {
    for(const budget of [1, 2, 3, 4, 5, 8, 16, 32, 100]) {
        const instances = [false, true].map(optimized => {
            const bytes = fs.readFileSync(`build/ir-licm/loop-${poll}-${budget}-${optimized}.wasm`);
            assert(WebAssembly.validate(bytes));
            return new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {m: memory}});
        });
        for(const a of [0, 123, 0xFFFFFFFF]) {
            for(const n of [0, 1, 2, 7, 12]) {
                for(const c of [0, 1, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF]) {
                    for(const d of [0, 1, 3, 0xFFFFFFFF]) {
                        for(const flags of [2, 0x8D7]) {
                            const input = [a, n, c, d, 0x90000, 0x12345678, 0xABCDEF01, 0x2468ACE0];
                            const results = instances.map(instance => {
                                words.fill(0, 0, 64);
                                words.set(input);
                                words[8] = flags;
                                words[9] = 0xDEADBEEF;
                                words[10] = 17;
                                words[11] = 0x76543210;
                                instance.exports.f(0);
                                executions++;
                                return Array.from(words.slice(0, 12));
                            });
                            assert.deepEqual(results[1], results[0], `LICM recovery: poll=${poll}, budget=${budget}, input=${input}`);
                            if(budget === 100) {
                                const expected = input.slice();
                                expected[0] = Number((BigInt(a) + BigInt(n) * (BigInt(c) * BigInt(d) + 7n)) & 0xFFFFFFFFn);
                                expected[1] = 0;
                                assert.deepEqual(results[0], [...expected, flags, 0x4000, 0, 0x76543210], "independent arithmetic oracle");
                            }
                        }
                    }
                }
            }
        }
        for(const instance of instances) {
            for(const entry of [-1, 1, 0x7FFFFFFF]) {
                words.fill(0x12345678, 0, 64);
                instance.exports.f(entry);
                assert(words.slice(0, 64).every(value => value === 0x12345678), "invalid entry must not execute hoists");
            }
        }
    }
}
console.log(`PASS: ${executions} LICM Wasm executions; independent wrapping arithmetic, zero trips, exact budget/poll recovery and invalid entries`);
