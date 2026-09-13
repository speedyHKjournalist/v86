import assert from "node:assert/strict";
import fs from "node:fs";

// These use the actual CPU ABI, including dynamic instruction counts, the
// logical-EIP/CS conversion, lazy FLAGS and complete eight-register XMM maps.
const operands = [];
for(const a of [0, 1, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF])
for(const b of [0, 1, 0xFFFFFFFF])
for(const c of [0, 1, 3, 0x80000001]) operands.push([a, b, c]);
let seed = 0x86DEC0DE;
function random() {
    seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5;
    return seed >>> 0;
}
for(let i = 0; i < 64; i++) operands.push([random(), random(), random()]);
let executions = 0;
for(const vector of [false, true]) for(const budget of [1, 2, 3, 4, 5, 8, 17, 100]) {
    const memory = new WebAssembly.Memory({initial: 64});
    const words = new Uint32Array(memory.buffer);
    const instances = [false, true].map(opt => {
        const bytes = fs.readFileSync(`build/ir-licm/typed-${vector}-${budget}-${opt}.wasm`);
        assert(WebAssembly.validate(bytes));
        return new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {
            m: memory, ir_enter: () => {}, ir_tlb_base: () => 0,
            get_eflags: () => words[30],
        }});
    });
    for(const count of [0, 1, 2, 3, 7, 19, 0xFFFFFFFF]) {
        const iterations = Math.min(count, Math.max(0, Math.floor((budget - 1) / 2)));
        const pc = budget >= 2 * count + 3 ? 0x5000 :
            budget === 2 * count + 2 ? 0x4003 : budget % 2 ? 0x4002 : 0x4001;
        for(const [a, b, c] of operands) for(const flags of [2, 0x8D7, 0x202])
        for(const cs of [0, 0xFFFFF000]) {
            const input = [count, 0xDEADBEEF, a, b, c, 0x12345678, 0xABCDEF01, 0x76543210];
            const expected = input.slice();
            const product = (BigInt(a) + BigInt(b)) * (vector ? 1n : BigInt(c));
            const total = (BigInt(iterations) * product) & (vector ? 0xFFFFFFFFn : 0xFFFFFFFFFFFFFFFFn);
            expected[0] = count - iterations;
            expected[1] = Number(total & 0xFFFFFFFFn);
            expected[3] = Number(total >> 32n);
            const snapshots = instances.map(instance => {
                words.fill(0xA5A5A5A5, 0, 320);
                words.set(input, 16);
                words[25] = 0; words[26] = 0xFEDCBA98; words[30] = flags;
                words[139] = 0; words[140] = 0; words[166] = 0xFFFFFFF0; words[185] = cs;
                instance.exports.f(0);
                assert.deepEqual(Array.from(words.slice(16, 24)), expected,
                    `vector=${vector}, budget=${budget}, count=${count}, operands=${a},${b},${c}`);
                assert.equal(words[30], flags, "FLAGS preservation");
                assert.equal(words[26], 0xFEDCBA98, "lazy flag operand preservation");
                assert.equal(words[139], (pc + cs) >>> 0, "exact CPU recovery EIP/CS");
                assert.equal(words[166], (0xFFFFFFF0 + iterations) >>> 0, "completed instruction count");
                if(vector) {
                    for(let r = 0; r < 8; r++) {
                        assert.deepEqual(Array.from(words.slice(208 + r * 4, 212 + r * 4)),
                            [expected[1], 0, 0, 0], "complete XMM snapshot at every exit");
                    }
                }
                executions++;
                return words.slice(0, 320);
            });
            assert.deepEqual(snapshots[1], snapshots[0], "all CPU state and guard words agree");
        }
    }
}
console.log(`PASS: ${executions} i64/V128 LICM CPU-ABI executions with independent arithmetic, dynamic counts, CS/EIP and complete XMM recovery`);
