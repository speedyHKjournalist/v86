import assert from "node:assert/strict";
import fs from "node:fs";

// Independent oracle for the counted-loop fixture, including every budget exit.
// Both emitted variants must match this oracle, not just match each other.
const memory = new WebAssembly.Memory({initial: 64});
const words = new Uint32Array(memory.buffer);
const budgets = [1, 2, 3, 4, 5, 8, 17, 100];
const counts = [0, 1, 2, 3, 7, 32, 0xFFFFFFFF];
const values = [0, 1, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF];
let executions = 0;
for(const budget of budgets) {
    const instances = [false, true].map(opt => {
        const bytes = fs.readFileSync(`build/ir-licm/loop-${budget}-${opt}.wasm`);
        assert(WebAssembly.validate(bytes), `budget=${budget}, opt=${opt}`);
        const module = new WebAssembly.Module(bytes);
        assert.deepEqual(WebAssembly.Module.imports(module), [
            {module: "e", name: "m", kind: "memory"},
        ], "pure loop must not acquire runtime helpers");
        return new WebAssembly.Instance(module, {e: {m: memory}});
    });
    for(const count of counts) for(const a of values) for(const b of values)
    for(const multiplier of [0, 3, 0xFFFFFFFF]) for(const flags of [2, 0x8D7, 0x202]) {
        const completed = Math.min(count, Math.floor((budget - 1) / 2));
        const sum = Number(((BigInt(a) + BigInt(b)) * BigInt(multiplier) * BigInt(completed)) & 0xFFFFFFFFn);
        const ip = budget >= 2 * count + 3 ? 0x3000 : budget === 2 * count + 2 ? 0x2002 : budget % 2 ? 0x2001 : 0x2003;
        const input = [count, 0xA5A5A5A5, a, b, multiplier, 0x12345678, 0x9ABCDEF0, 0x87654321];
        const expected = [count - completed, sum, ...input.slice(2), flags, ip, 0, 0x76543210];
        for(const instance of instances) {
            words.fill(0xDEADBEEF, 0, 32);
            words.set([...input, flags, 0x11223344, 99, 0x76543210]);
            instance.exports.f(0);
            assert.deepEqual(Array.from(words.slice(0, 12)), expected,
                `budget=${budget}, count=${count}, a=${a}, b=${b}, multiplier=${multiplier}`);
            for(let i = 12; i < 32; i++) assert.equal(words[i], 0xDEADBEEF, "no extra state writes");
            executions++;
        }
    }
    for(const entry of [-1, 1, 0x7FFFFFFF]) for(const instance of instances) {
        words.fill(0x12345678, 0, 32);
        instance.exports.f(entry);
        assert(words.slice(0, 32).every(x => x === 0x12345678), "invalid entry must not execute");
        executions++;
    }
}
console.log(`PASS: ${executions} LICM Wasm executions: independent arithmetic oracle, zero iterations, overflow, exact budget/FLAGS recovery, invalid entry rejection`);
