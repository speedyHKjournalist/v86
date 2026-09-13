import assert from "node:assert/strict";
import fs from "node:fs";

// Independent arithmetic/CFG oracle, not just opt-vs-unopt equality.
// Trace: entry, (header, body)*, header, exit. A body accumulates
// (input[2] + input[3]) * input[4], wrapping at the i32 machine width.
const memory = new WebAssembly.Memory({initial: 64});
const words = new Uint32Array(memory.buffer);
const cases = [];
for(const a of [0, 1, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF]) {
    for(const b of [0, 1, 0xFFFFFFFF]) {
        for(const c of [0, 1, 3, 0x80000001]) cases.push([a, b, c]);
    }
}
let seed = 0x86C0FFEE;
function random() {
    seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5;
    return seed >>> 0;
}
for(let i = 0; i < 64; i++) cases.push([random(), random(), random()]);
let executions = 0;
for(const budget of [1, 2, 3, 4, 5, 8, 17, 100]) {
    const instances = [false, true].map(opt => {
        const bytes = fs.readFileSync(`build/ir-licm/loop-${budget}-${opt}.wasm`);
        assert(WebAssembly.validate(bytes), `LICM budget ${budget}, optimized=${opt}`);
        return new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {m: memory}});
    });
    for(const count of [0, 1, 2, 3, 7, 19, 0xFFFFFFFF]) {
        const iterations = Math.min(count, Math.max(0, Math.floor((budget - 1) / 2)));
        const pc = budget >= 2 * count + 3 ? 0x3000 :
            budget === 2 * count + 2 ? 0x2002 : budget % 2 ? 0x2001 : 0x2003;
        for(const [a, b, c] of cases) for(const flags of [2, 0x8D7, 0x202]) {
            const input = [count, 0xDEADBEEF, a, b, c, 0x12345678, 0xABCDEF01, 0x76543210];
            const expected = input.slice();
            const product = ((BigInt(a) + BigInt(b)) & 0xFFFFFFFFn) * BigInt(c);
            expected[0] = count - iterations;
            expected[1] = Number((BigInt(iterations) * product) & 0xFFFFFFFFn);
            for(const instance of instances) {
                words.fill(0xA5A5A5A5, 0, 32);
                words.set(input);
                words[8] = flags;
                words[9] = 0;
                words[10] = 99;
                words[11] = 0xFEDCBA98;
                instance.exports.f(0);
                assert.deepEqual(Array.from(words.slice(0, 8)), expected,
                    `budget=${budget}, count=${count}, operands=${a},${b},${c}`);
                assert.equal(words[8], flags, "FLAGS must survive motion");
                assert.equal(words[9], pc, "exact block-budget recovery EIP");
                assert.equal(words[10], 0, "preserve fixture StateMap instruction count");
                assert.equal(words[11], 0xFEDCBA98, "preserve lazy flag operand");
                assert(words.slice(12, 32).every(value => value === 0xA5A5A5A5),
                    "do not write beyond the declared CPU state layout");
                executions++;
            }
        }
    }
}
console.log(`PASS: ${executions} independent LICM Wasm executions; zero-trip, overflow, transitive invariants, unchanged FLAGS and exact budget exits`);
