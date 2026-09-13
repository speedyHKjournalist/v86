import assert from "node:assert/strict";
import fs from "node:fs";

// Independent model of the synthetic CFG, including every budget boundary.
function reference(input, budget) {
    const state = input.slice();
    const invariant = Math.imul((state[2] + state[3]) >>> 0, 7) >>> 0;
    let block = "entry";
    while(budget-- > 0) {
        switch(block) {
            case "entry": block = "header"; break;
            case "header": block = state[0] === 0 ? "exit" : "body"; break;
            case "body":
                state[0] = state[0] - 1 >>> 0;
                state[1] = state[1] + invariant >>> 0;
                block = "header";
                break;
            case "exit": return {state, pc: 0x2000};
            default: throw new Error("invalid reference block");
        }
    }
    return {state, pc: {header: 0x1002, body: 0x1001, exit: 0x1003}[block]};
}
let executions = 0;
const memory = new WebAssembly.Memory({initial: 64});
const words = new Uint32Array(memory.buffer);
for(const budget of [1, 2, 3, 4, 5, 9, 16, 100]) {
    const instances = [false, true].map(opt => {
        const bytes = fs.readFileSync(`build/ir-licm/loop-${budget}-${opt}.wasm`);
        assert(WebAssembly.validate(bytes));
        return new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {m: memory}});
    });
    for(const count of [0, 1, 2, 5, 17])
    for(const acc of [0, 0x7FFFFFFF, 0xFFFFFFFF])
    for(const c of [0, 1, 0x80000000, 0xFFFFFFFF])
    for(const d of [0, 3, 0x7FFFFFFF, 0xFFFFFFFF])
    for(const flags of [2, 0x8D7]) {
        const input = [count, acc, c, d, 0x90000, 0x12345678, 0xABCDEF01, 0];
        const expected = reference(input, budget);
        for(const instance of instances) {
            words.fill(0, 0, 64);
            words.set(input);
            words[8] = flags;
            words[10] = 99;
            words[11] = 0x76543210;
            instance.exports.f(0);
            assert.deepEqual(Array.from(words.slice(0, 8)), expected.state);
            assert.equal(words[8], flags, "FLAGS preserved");
            assert.equal(words[9], expected.pc, "exact recovery PC");
            assert.equal(words[10], 0, "synthetic loop adds no guest instructions");
            assert.equal(words[11], 0x76543210, "raw lazy-flags operand preserved");
            executions++;
        }
    }
}
console.log(`PASS: ${executions} LICM reference/optimized Wasm executions: zero-trip, loop-carried state, overflow and exact budget exits`);
