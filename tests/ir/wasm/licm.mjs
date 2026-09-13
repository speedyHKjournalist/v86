import assert from "node:assert/strict";
import fs from "node:fs";

// Independent interpreter of the fixture CFG, not an HIR evaluator. Check each
// budget boundary, including a loop skipped before its first body execution.
function model(input, budget) {
    const result = input.slice();
    const step = Math.imul((input[2] + input[3]) >>> 0, 3) >>> 0;
    let block = "entry";
    for(;;) {
        if(budget-- === 0 || block === "exit") {
            const pc = block === "header" ? 0x2200 : block === "body" ? 0x2300 : 0x9000;
            return {result, pc};
        }
        if(block === "entry") block = "header";
        else if(block === "header") block = result[0] === 0 ? "exit" : "body";
        else {
            result[0] = (result[0] - 1) >>> 0;
            result[1] = (result[1] + step) >>> 0;
            block = "header";
        }
    }
}

let executions = 0;
const memory = new WebAssembly.Memory({initial: 64});
const words = new Uint32Array(memory.buffer);
const bytes = new Uint8Array(memory.buffer);
for(const budget of [1, 2, 3, 4, 5, 6, 9, 16, 100]) {
    const instances = [false, true].map(opt => {
        const binary = fs.readFileSync(`build/ir-licm/loop-${budget}-${opt}.wasm`);
        assert(WebAssembly.validate(binary));
        return new WebAssembly.Instance(new WebAssembly.Module(binary), {e: {m: memory}});
    });
    for(const count of [0, 1, 2, 7, 25]) for(const initial of [0, 1, 0xFFFFFFFE])
    for(const a of [0, 1, 0x7FFFFFFF, 0xFFFFFFFF]) for(const b of [0, 3, 0x80000000, 0xFFFFFFFF])
    for(const flags of [2, 0x202, 0x8D7]) {
        const input = [count, initial, a, b, 0x90000, 0x12345678, 0xABCDEF01, 42];
        const expected = model(input, budget);
        let baseline;
        for(const instance of instances) {
            bytes.fill(0xA5, 0, 4096);
            words.set(input);
            words[8] = flags; words[9] = 0; words[10] = 99; words[11] = 0x76543210;
            instance.exports.f(0);
            assert.deepEqual(Array.from(words.slice(0, 8)), expected.result);
            assert.equal(words[8], flags, "FLAGS preserved");
            assert.equal(words[9], expected.pc, "exact budget recovery PC");
            assert.equal(words[10], 0, "instruction-count StateMap preserved");
            assert.equal(words[11], 0x76543210, "lazy-flags operand preserved");
            const observation = bytes.slice(0, 4096);
            if(baseline) assert.deepEqual(observation, baseline, "all state and memory equal");
            else baseline = observation;
            executions++;
        }
    }
    for(const entry of [-1, 1, 0x7FFFFFFF]) for(const instance of instances) {
        bytes.fill(0xA5, 0, 4096);
        const before = bytes.slice(0, 4096);
        instance.exports.f(entry);
        assert.deepEqual(bytes.slice(0, 4096), before, "invalid entry does not execute moved code");
        executions++;
    }
}
console.log(`PASS: ${executions} LICM Wasm executions; independent CFG oracle, wraparound, zero-trip loops and precise budget exits`);
