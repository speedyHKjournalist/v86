import assert from "node:assert/strict";
import fs from "node:fs";

// Independent dispatcher-and-poll model: this fixture has one entry charge,
// one charge per block, and a PollBudget in the body before guest work commits.
function reference(n, budget) {
    let left = budget - 1, completed = 0, block = "header";
    for(;;) {
        if(left-- === 0) return completed;
        if(block === "header") block = completed === n ? "exit" : "body";
        else if(block === "exit") return completed;
        else {
            if(left-- === 0) return completed;
            completed++;
            block = "header";
        }
    }
}
const budgets = [1, 2, 3, 4, 5, 9, 16, 1000];
const memory = new WebAssembly.Memory({initial: 64});
const w = new Uint32Array(memory.buffer), bytes = new Uint8Array(memory.buffer);
let executions = 0;
for(const cpu of [false, true]) for(const budget of budgets) {
    const modules = [false, true].map(opt => {
        const code = fs.readFileSync(`build/ir-licm/${cpu}-${opt}-${budget}.wasm`);
        assert(WebAssembly.validate(code));
        const imports = {m: memory, ir_enter: () => {}, ir_tlb_base: () => 0,
            get_eflags: () => w[30]};
        return new WebAssembly.Instance(new WebAssembly.Module(code), {e: imports}).exports.f;
    });
    for(const n of [0, 1, 2, 7, 0xFFFFFFFF])
    for(const start of [0, 0x7FFFFFFF, 0xFFFFFFFF])
    for(const a of [0, 1, 0xFFFFFFFF])
    for(const shift of [0, 1, 31, 32, 63])
    for(const flags of [2, 0x8D7]) {
        const input = [start, n, a, 0x80000003, 0xFFFFFFFD, shift, 0x12345678, 0xABCDEF01];
        const completed = reference(n, budget);
        // CPU variant: Add32 then byte-reverse, extracting reversed high lane.
        const v = (a + 0x12345678) >>> 0;
        const doubled = (v * 2) >>> 0;
        const reverse = ((doubled >>> 24) | ((doubled >>> 8) & 0xFF00)
            | ((doubled << 8) & 0xFF0000) | (doubled << 24)) >>> 0;
        const invariant = (cpu ? reverse : Math.imul((a + input[3]) >>> 0, input[4])) << (shift & 31);
        const expected = input.slice();
        expected[0] = (start + Math.imul(completed, invariant)) >>> 0;
        expected[1] = (n - completed) >>> 0;
        let unoptimized;
        for(const execute of modules) {
            bytes.fill(0, 0, 2048);
            const base = cpu ? 16 : 0;
            w.set(input, base);
            w[cpu ? 30 : 8] = flags;
            w[cpu ? 26 : 11] = 0x87654321;
            if(cpu) {
                w[166] = 0xFFFFFFF0;
                w[185] = 0xFFFFF000;
                w.set([0xDEADBEEF, 0xF0F0F0F0, 0x0F0F0F0F, v], 208);
            }
            execute(0);
            assert.deepEqual(Array.from(w.slice(base, base + 8)), expected);
            assert.equal(w[cpu ? 30 : 8], flags, "arithmetic and system FLAGS preserved");
            assert.equal(w[cpu ? 26 : 11], 0x87654321, "lazy flag operand preserved");
            assert.equal(w[cpu ? 166 : 10], ((cpu ? 0xFFFFFFF0 : 0) + completed) >>> 0,
                "exact dynamic committed count, including overflow");
            const actual = Buffer.from(bytes.slice(0, 2048));
            if(unoptimized) assert.deepEqual(actual, unoptimized, "whole backing state matches at every exit");
            else unoptimized = actual;
            executions++;
        }
    }
}
console.log(`PASS: ${executions} LICM scalar/SIMD Wasm executions; independent result/count oracle and byte-identical recovery state`);
