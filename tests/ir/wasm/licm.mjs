import assert from "node:assert/strict";
import fs from "node:fs";

// Independent arithmetic/control-flow oracle. It does not execute or inspect HIR,
// MIR or the unoptimized Wasm variant to produce the expected architectural state.
function expected(input, budget) {
    const registers = input.slice();
    const invariant = Math.imul(((input[2] + input[3]) >>> 0) ^ 0x55AA55AA, 3) >>> 0;
    let block = "entry";
    for(let remaining = budget; remaining > 0; remaining--) {
        switch(block) {
            case "entry":
                block = "header";
                break;
            case "header":
                block = registers[1] === 0 ? "exit" : "body";
                break;
            case "body":
                registers[0] = (registers[0] + invariant) >>> 0;
                registers[1] = (registers[1] - 1) >>> 0;
                block = "header";
                break;
            case "exit":
                return {registers, pc: 0x1400};
            default:
                throw new Error("invalid oracle state");
        }
    }
    return {registers, pc: {header: 0x1100, body: 0x1200, exit: 0x1300}[block]};
}

let executions = 0;
const memory = new WebAssembly.Memory({initial: 64});
const words = new Uint32Array(memory.buffer);
for(const budget of [1, 2, 3, 4, 5, 8, 9, 16, 64, 100]) {
    const variants = [false, true].map(optimize => {
        const bytes = fs.readFileSync(`build/ir-licm/loop-${budget}-${optimize}.wasm`);
        assert(WebAssembly.validate(bytes), `LICM module budget=${budget} optimized=${optimize}`);
        return new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {m: memory}});
    });
    for(const count of [0, 1, 2, 7])
    for(const a of [0, 1, 0x7FFFFFFF, 0xFFFFFFFF])
    for(const c of [0, 0x80000000, 0xFFFFFFFF])
    for(const d of [3, 0xFFFFFFFE])
    for(const flags of [2, 0x8D7]) {
        const input = [a, count, c, d, 0x90000, 0x12345678, 0xABCDEF01, 7];
        const result = expected(input, budget);
        assert.notEqual(result.pc, undefined);
        for(const instance of variants) {
            words.fill(0xDEADBEEF, 0, 32);
            words.set(input);
            words[8] = flags;
            words[9] = 0;
            words[10] = 99;
            words[11] = 0x76543210;
            instance.exports.f(0);
            assert.deepEqual(Array.from(words.slice(0, 8)), result.registers);
            assert.equal(words[8], flags, "unchanged architectural FLAGS");
            assert.equal(words[9], result.pc, "exact budget/terminal resume PC");
            assert.equal(words[10], 0, "unchanged StateMap instruction accounting");
            assert.equal(words[11], 0x76543210, "lazy flag operand is not materialized");
            assert(words.slice(12, 32).every(value => value === 0xDEADBEEF));
            executions++;
        }
    }
}
console.log(`PASS: ${executions} independent LICM Wasm executions: zero-trip loops, i32 overflow, loop-carried state and exact budget recovery`);
