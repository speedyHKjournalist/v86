import assert from "node:assert/strict";
import fs from "node:fs";

// Model the original CFG independently: entry -> header -> body/header or exit.
// Budget recovery is checked against this model, not just opt vs. no-opt.
function expected(input, flags, operand, budget) {
    const result = [...input, flags, 0, 0, operand];
    const step = Number(((BigInt(input[2]) + BigInt(input[3])) * BigInt(input[4])) & 0xFFFFFFFFn);
    let block = 0, count = input[0], sum = 0;
    for(let remaining = budget; remaining > 0; remaining--) {
        if(block === 0) block = 1;
        else if(block === 1) block = count === 0 ? 2 : 3;
        else if(block === 3) {
            count = count - 1 >>> 0;
            sum = sum + step >>> 0;
            block = 1;
        }
        else {
            result[0] = count;
            result[1] = sum;
            result[9] = 0x3000;
            return result;
        }
    }
    result[0] = count;
    result[1] = sum;
    result[9] = 0x2000 + block;
    return result;
}
let executions = 0;
const memory = new WebAssembly.Memory({initial: 64});
const words = new Uint32Array(memory.buffer);
for(const budget of [1, 2, 3, 4, 5, 8, 17, 100]) {
    const instances = [false, true].map(opt => {
        const bytes = fs.readFileSync(`build/ir-licm/loop-${budget}-${opt}.wasm`);
        assert(WebAssembly.validate(bytes));
        return new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {m: memory}});
    });
    for(const count of [0, 1, 2, 7, 100, 0xFFFFFFFF])
    for(const a of [0, 1, 0x7FFFFFFF, 0xFFFFFFFF])
    for(const b of [0, 1, 0x80000000, 0xFFFFFFFF])
    for(const c of [0, 1, 0x12345678, 0xFFFFFFFF])
    for(const flags of [2, 0x8D7]) {
        const input = [count, 0xDEADBEEF, a, b, c, 0x87654321, 0xABCDEF01, 0x12345678];
        const operand = 0xCAFEBABE;
        const reference = expected(input, flags, operand, budget);
        for(const instance of instances) {
            words.fill(0xA5A5A5A5, 0, 32);
            words.set([...input, flags, 0, 99, operand]);
            instance.exports.f(0);
            assert.deepEqual(Array.from(words.slice(0, 12)), reference,
                `budget=${budget} count=${count} inputs=${a},${b},${c}`);
            assert(words.slice(12, 32).every(value => value === 0xA5A5A5A5), "no extra architectural stores");
            executions++;
        }
    }
    words.fill(0xA5A5A5A5, 0, 32);
    for(const entry of [-1, 1, 0x7FFFFFFF]) {
        for(const instance of instances) {
            instance.exports.f(entry);
            assert(words.slice(0, 32).every(value => value === 0xA5A5A5A5), "invalid entry has no effects");
            executions++;
        }
    }
}
console.log(`PASS: ${executions} LICM Wasm executions with an independent loop/budget model, overflow, zero-trip loops, flags and recovery state`);
