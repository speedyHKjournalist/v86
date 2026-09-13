import assert from "node:assert/strict";
import fs from "node:fs";

// Independent CPU block-budget model; never compare only two compiler outputs.
function reference(count, budget, poll) {
    let left = budget - 1, completed = 0;
    for(;;) {
        if(left === 0) return {completed, pc: 0x4002};
        left--;
        if(completed === count) return {completed, pc: left === 0 ? 0x4003 : 0x5000};
        if(left === 0) return {completed, pc: 0x4001};
        left--;
        if(poll) {
            if(left === 0) return {completed, pc: 0x4001};
            left--;
        }
        completed++;
    }
}
const cases = [];
for(const a of [0, 1, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF])
for(const b of [0, 1, 0xFFFFFFFF])
for(const c of [0, 1, 3, 0x80000001]) cases.push([a, b, c]);
let seed = 0x86DEC0DE;
function random() {
    seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5;
    return seed >>> 0;
}
for(let i = 0; i < 16; i++) cases.push([random(), random(), random()]);
let executions = 0;
for(const vector of [false, true]) for(const poll of [false, true])
for(const budget of [1, 2, 3, 4, 5, 8, 17, 100]) {
    const memory = new WebAssembly.Memory({initial: 64});
    const words = new Uint32Array(memory.buffer);
    const instances = [false, true].map(opt => {
        const bytes = fs.readFileSync(`build/ir-licm-cpu/${vector}-${poll}-${budget}-${opt}.wasm`);
        assert(WebAssembly.validate(bytes));
        return new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {
            m: memory, ir_enter: () => {}, ir_tlb_base: () => 0,
            get_eflags: () => words[30],
        }});
    });
    for(const count of [0, 1, 2, 7, 0xFFFFFFFF]) {
        const expected = reference(count, budget, poll);
        for(const [a, b, c] of cases) for(const flags of [2, 0x8D7])
        for(const cs of [0, 0xFFFFF000]) {
            const input = [count, 0xDEADBEEF, a, b, c, 0x12345678, 0xABCDEF01, 0x76543210];
            const output = input.slice();
            const increment = (BigInt(a) + BigInt(b)) * (vector ? 1n : BigInt(c));
            const total = (BigInt(expected.completed) * increment) &
                (vector ? 0xFFFFFFFFn : 0xFFFFFFFFFFFFFFFFn);
            output[0] = count - expected.completed;
            output[1] = Number(total & 0xFFFFFFFFn);
            output[3] = Number(total >> 32n);
            const snapshots = instances.map(instance => {
                words.fill(0xA5A5A5A5, 0, 320);
                words.set(input, 16);
                words[25] = 0; words[26] = 0xFEDCBA98; words[30] = flags;
                words[139] = 0; words[140] = 0; words[166] = 0xFFFFFFF0; words[185] = cs;
                instance.exports.f(0);
                assert.deepEqual(Array.from(words.slice(16, 24)), output,
                    `vector=${vector}, poll=${poll}, budget=${budget}, count=${count}, operands=${a},${b},${c}`);
                assert.equal(words[30], flags, "FLAGS preservation");
                assert.equal(words[26], 0xFEDCBA98, "lazy flag operand preservation");
                assert.equal(words[139], (expected.pc + cs) >>> 0, "exact CPU recovery EIP/CS");
                assert.equal(words[166], (0xFFFFFFF0 + expected.completed) >>> 0, "completed instruction count");
                if(vector) for(let r = 0; r < 8; r++) {
                    assert.deepEqual(Array.from(words.slice(208 + r * 4, 212 + r * 4)),
                        [output[1], 0, 0, 0], "complete XMM snapshot at every exit");
                }
                executions++;
                return words.slice(0, 320);
            });
            assert.deepEqual(snapshots[1], snapshots[0], "all CPU state and guard words agree");
        }
    }
}
console.log(`PASS: ${executions} i64/V128 LICM CPU-ABI executions; independent arithmetic/budget oracle, polls, dynamic counts, CS/EIP and all XMM recovery`);
