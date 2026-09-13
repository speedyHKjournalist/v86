import assert from "node:assert/strict";
import fs from "node:fs";

// Independent block-budget/guest-state model: it does not read compiler dumps
// or use one generated module as the correctness oracle for the other modules.
function expectedState(input, budget) {
    const state = input.slice();
    const invariant = Math.imul(state[2], state[3]) >>> 0;
    let block = "entry";
    for(let work = 0; work < budget; work++) {
        switch(block) {
        case "entry": block = "header"; break;
        case "header": block = state[1] === 0 ? "exit" : "body"; break;
        case "body":
            state[0] = (state[0] + invariant) >>> 0;
            state[1] = (state[1] - 1) >>> 0;
            block = "header";
            break;
        case "exit": return {state, pc: 0x2000};
        default: throw new Error("invalid oracle block");
        }
    }
    return {state, pc: {header: 0x1003, body: 0x1002, exit: 0x1001}[block]};
}

let executions = 0;
const memory = new WebAssembly.Memory({initial: 64});
const words = new Uint32Array(memory.buffer);
for(const budget of [1, 2, 3, 4, 5, 7, 11, 32, 100]) {
    const instances = [0, 1, 2].map(mode => {
        const bytes = fs.readFileSync(`build/ir-licm/loop-${budget}-${mode}.wasm`);
        assert(WebAssembly.validate(bytes), `LICM ${budget}/${mode} validates`);
        return new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {m: memory}});
    });
    for(const initial of [0, 0xFFFFFFFF, 0x80000000])
    for(const trips of [0, 1, 2, 7, 32])
    for(const a of [0, 1, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF])
    for(const b of [1, 3, 0xFFFFFFFF])
    for(const flags of [2, 0x8D7]) {
        const input = [initial, trips, a, b, 0x90000, 0x12345678, 0xABCDEF01, 0x76543210];
        const expected = expectedState(input, budget);
        for(const [mode, instance] of instances.entries()) {
            words.set(input);
            words[8] = flags; words[9] = 0; words[10] = 99; words[11] = 0x76543210;
            instance.exports.f(0);
            const label = `budget=${budget}, mode=${mode}, trips=${trips}, a=${a}, b=${b}`;
            assert.deepEqual(Array.from(words.slice(0, 8)), expected.state, label);
            assert.equal(words[8], flags, `${label}: FLAGS`);
            assert.equal(words[9], expected.pc, `${label}: precise recovery EIP`);
            assert.equal(words[10], 0, `${label}: original instruction-count maps`);
            assert.equal(words[11], 0x76543210, `${label}: recovery-only flag operand`);
            executions++;
        }
    }
    for(const instance of instances) for(const entry of [-1, 1, 0x7FFFFFFF]) {
        words.fill(0xA5A5A5A5, 0, 12);
        instance.exports.f(entry);
        assert.deepEqual(Array.from(words.slice(0, 12)), Array(12).fill(0xA5A5A5A5));
    }
}
console.log(`PASS: ${executions} LICM Wasm executions against an independent loop oracle; zero trips, i32/i64 wrap, every budget exit, FLAGS and recovery-only values`);
