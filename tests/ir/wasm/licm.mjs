import assert from "node:assert/strict";
import fs from "node:fs";

function reference(n, budget, poll) {
    let left = budget - 1, block = "header", completed = 0;
    for(;;) {
        if(left-- === 0) return {completed, normal: false};
        if(block === "header") block = completed === n ? "exit" : "body";
        else if(block === "exit") return {completed, normal: true};
        else {
            if(poll && left-- === 0) return {completed, normal: false};
            completed++;
            block = "header";
        }
    }
}

let executions = 0;
for(const poll of [false, true]) for(const budget of [1, 2, 3, 4, 5, 9, 16, 100]) {
    const memory = new WebAssembly.Memory({initial: 64});
    const words = new Uint32Array(memory.buffer);
    let flags = 2;
    const instances = [0, 1, 2].map(opt => {
        const bytes = fs.readFileSync(`build/ir-licm/${poll}-${opt}-${budget}.wasm`);
        assert(WebAssembly.validate(bytes));
        return new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {
            m: memory, ir_enter: () => {}, ir_tlb_base: () => 0,
            get_eflags: () => flags,
        }});
    });
    for(const n of [0, 1, 2, 7, 0xFFFFFFFF]) for(const x of [0, 0xFFFFFFF0]) {
        for(const a of [0, 1, 0x7F00, 0x8000, 0xFFFFFF00, 0xFFFFFFFF]) {
            for(const b of [0, 7, 0xFFFFFFFF]) for(const initialCount of [0, 0xFFFFFFFC]) {
                for(const cs of [0, 0xFFFFFF00]) for(flags of [2, 0x8D7]) {
                    const expected = reference(n, budget, poll);
                    const signedByte = (a << 16) >> 24;
                    const increment = ((Math.imul(a, b) ^ 1) + signedByte) >>> 0;
                    const result = (x + Math.imul(increment, expected.completed)) >>> 0;
                    const input = [x, n, a, b, 0x90000, 0x12345678, 0xABCDEF01, 0x76543210];
                    for(const instance of instances) {
                        words.fill(0);
                        words.set(input, 16); words[166] = initialCount; words[185] = cs;
                        words[30] = flags; words[26] = 0x76543210;
                        instance.exports.f(0);
                        assert.equal(words[16], result, "loop accumulator incl. narrow sign extension");
                        assert.equal(words[17], (n - expected.completed) >>> 0, "loop carried countdown");
                        assert.deepEqual(Array.from(words.slice(18, 24)), input.slice(2));
                        assert.equal(words[166], (initialCount + expected.completed) >>> 0, "exact count on every exit");
                        assert.equal(words[139], (cs + (expected.normal ? 0x8001 : 0x8000)) >>> 0, "exact recovery EIP");
                        assert.equal(words[30], flags);
                        assert.equal(words[26], 0x76543210);
                        executions++;
                    }
                }
            }
        }
    }
}
console.log(`PASS: ${executions} independent CPU LICM executions: zero-trip, overflow, sign extension, polls and exact budget recovery`);
