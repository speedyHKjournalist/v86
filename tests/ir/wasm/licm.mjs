import assert from "node:assert/strict";
import fs from "node:fs";

// Independently count the entry/header/body/explicit-poll/exit boundaries.
// LICM may reduce computation, not the number or placement of recovery points.
function reference(n, budget) {
    let remaining = budget - 1, completed = 0;
    for(;;) {
        if(remaining-- === 0) return {completed, pc: 0x8000};
        if(completed === n) {
            if(remaining-- === 0) return {completed, pc: 0x8002};
            return {completed, pc: 0x8003};
        }
        if(remaining-- === 0) return {completed, pc: 0x8001};
        if(remaining-- === 0) return {completed, pc: 0x8001};
        completed++;
    }
}
let seed = 0x8BADF00D;
function random() {
    seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5;
    return seed >>> 0;
}
let executions = 0;
const m = new WebAssembly.Memory({initial: 64});
const bytes = new Uint8Array(m.buffer, 0, 4096), words = new Uint32Array(m.buffer);
for(const vector of [false, true]) for(const budget of [1, 2, 3, 4, 5, 7, 16, 33, 257]) {
    const imports = {e: {m, ir_enter: () => {}, ir_tlb_base: () => 0, get_eflags: () => 0x8D7}};
    const instances = [0, 1, 2].map(opt => {
        const data = fs.readFileSync(`build/ir-licm/${vector}-${budget}-${opt}.wasm`);
        assert(WebAssembly.validate(data), `valid LICM fixture ${vector}/${budget}/${opt}`);
        return new WebAssembly.Instance(new WebAssembly.Module(data), imports);
    });
    for(let sample = 0; sample < 96; sample++) for(const n of [0, 1, 2, 7, 31, 0xFFFFFFFF]) {
        for(let i = 0; i < 1024; i++) words[i] = random();
        const gpr = vector ? 16 : 0;
        const flags = vector ? 30 : 8, pc = vector ? 139 : 9;
        const counter = vector ? 166 : 10, flagOperand = vector ? 26 : 11;
        const input = Array.from({length: 8}, random);
        input[2] = n;
        // Include adversarial overflow, all-zero, and signed-boundary patterns.
        if(sample < 4) {
            input[0] = [0, 1, 0x7FFFFFFF, 0xFFFFFFFF][sample];
            input[1] = [0, 0xFFFFFFFF, 0x80000000, 0xFFFFFFFF][sample];
        }
        words.set(input, gpr);
        words[flags] = 0x8D7;
        words[pc] = 0xDEADBEEF;
        const initialCount = sample & 1 ? 0xFFFFFFF0 : 0;
        words[counter] = initialCount;
        words[flagOperand] = 0x76543210;
        const cs = sample & 1 ? 0xFFFFF000 : 0;
        if(vector) { words[25] = 0; words[185] = cs; }
        let invariant = (input[0] + input[1]) >>> 0;
        if(vector) {
            invariant = 0;
            for(let lane = 0; lane < 16; lane++) {
                const source = 15 - lane;
                const sum = (bytes[832 + source] + bytes[848 + source]) & 255;
                invariant |= (sum >>> 7) << lane;
            }
        }
        const product = Math.imul(invariant, input[4]);
        const expected = reference(n, budget);
        const original = bytes.slice();
        let referenceState;
        for(const [opt, instance] of instances.entries()) {
            bytes.set(original);
            instance.exports.f(0);
            const label = `vector=${vector}, budget=${budget}, sample=${sample}, n=${n}, opt=${opt}`;
            assert.equal(words[gpr + 2], (n - expected.completed) >>> 0, `loop count ${label}`);
            assert.equal(words[gpr + 3], (input[3] + Math.imul(expected.completed, product)) >>> 0, `independent accumulator ${label}`);
            for(const r of [0, 1, 4, 5, 6, 7]) assert.equal(words[gpr + r], input[r], `preserved GPR ${label}`);
            assert.equal(words[flags], 0x8D7, `preserved flags ${label}`);
            assert.equal(words[pc], (expected.pc + (vector ? cs : 0)) >>> 0, `exact recovery PC ${label}`);
            assert.equal(words[counter], (expected.completed + (vector ? initialCount : 0)) >>> 0, `exact committed count ${label}`);
            assert.equal(words[flagOperand], 0x76543210, `preserved flag operand ${label}`);
            if(opt === 0) referenceState = bytes.slice();
            else assert.deepEqual(bytes, referenceState, `complete state image ${label}`);
            executions++;
        }
    }
}
console.log(`PASS: ${executions} LICM scalar/vector executions against independent loop, overflow, zero-trip and budget-exit oracles`);
