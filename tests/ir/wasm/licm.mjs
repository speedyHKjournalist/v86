import assert from "node:assert/strict";
import fs from "node:fs";

// Independent block-level oracle. It does not reuse the Rust optimizer or MIR.
// Values are exact modular integers; progress counts completed loop iterations.
function oracle(input, budget, vectorIncrement) {
    const expected = input.slice();
    const increment = vectorIncrement ?? Number(((BigInt(input[0]) + BigInt(input[1])) * BigInt(input[0])) & 0xFFFFFFFFn);
    let remaining = budget - 1;
    let block = "header", n = input[2], acc = input[3], done = 0;
    const recovery = {header: 0x2002, body: 0x2001, exit: 0x2003};
    for(;;) {
        if(remaining === 0) { expected[9] = recovery[block]; break; }
        remaining--;
        if(block === "header") { block = n === 0 ? "exit" : "body"; }
        else if(block === "body") {
            acc = (acc + increment) >>> 0;
            n = (n - 1) >>> 0;
            done++;
            block = "header";
        }
        else { expected[9] = 0x3000; break; }
    }
    expected[2] = n; expected[3] = acc; expected[10] = done;
    return expected;
}

const memory = new WebAssembly.Memory({initial: 64});
const words = new Uint32Array(memory.buffer);
let executions = 0;
for(const budget of [1, 2, 3, 4, 5, 6, 9, 16, 64]) {
    const instances = [false, true].map(optimized => {
        const bytes = fs.readFileSync(`build/ir-licm/loop-${budget}-${optimized}.wasm`);
        assert(WebAssembly.validate(bytes));
        return new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {m: memory}});
    });
    for(const a of [0, 1, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF])
    for(const b of [0, 1, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF])
    for(const count of [0, 1, 2, 7, 31])
    for(const accumulator of [0, 0xFFFFFFF0])
    for(const flags of [2, 0x8D7]) {
        const input = [a, b, count, accumulator, 0x90000, 0x12345678, 0xABCDEF01,
            0x76543210, flags, 0xBADF00D, 0xF00DBAD, 0x1234, 0xDEADBEEF];
        const expected = oracle(input, budget);
        for(const instance of instances) {
            words.set(input);
            instance.exports.f(0);
            assert.deepEqual(Array.from(words.slice(0, input.length)), expected,
                `budget=${budget}, a=${a}, b=${b}, n=${count}, flags=${flags}`);
            executions++;
        }
    }
    for(const instance of instances) for(const invalid of [-1, 1, 2, 0x7FFFFFFF]) {
        const sentinel = Array.from({length: 16}, (_, i) => (0xDEADBEEF ^ i) >>> 0);
        words.set(sentinel); instance.exports.f(invalid);
        assert.deepEqual(Array.from(words.slice(0, sentinel.length)), sentinel);
        executions++;
    }
}
console.log(`PASS: ${executions} LICM Wasm executions; zero-trip, overflow, loop-carried state, exact budget exits, flags, invalid entry`);

// CPU-shaped ABI test: only state capture/materialization is used here. These
// stubs are not a replacement for full CPU/MMU/system regression testing.
const cpuImports = {m: memory, ir_enter: () => {}, ir_tlb_base: () => 0,
    get_eflags: () => words[120 >> 2]};
let vectorExecutions = 0;
for(const budget of [1, 2, 3, 4, 9, 64]) {
    const instances = [false, true].map(opt => new WebAssembly.Instance(
        new WebAssembly.Module(fs.readFileSync(`build/ir-licm/vector-${budget}-${opt}.wasm`)),
        {e: cpuImports}));
    for(let seed = 0; seed < 64; seed++) for(const count of [0, 1, 2, 7]) {
        const inputVector = Uint8Array.from({length: 16}, (_, i) => (i * 37 + seed * 61) & 255);
        let increment = 0;
        for(let i = 0; i < 16; i++) {
            increment |= (((inputVector[i] + inputVector[15 - i]) & 255) >>> 7) << i;
        }
        const input = [0, 0, count, 0xFFFFFFF0, 0x90000, 0, 0, 0, 0x8D7, 0, 99, 0x1234];
        const expected = oracle(input, budget, increment);
        for(const instance of instances) {
            words.fill(0, 0, 256);
            words.set(input.slice(0, 8), 64 >> 2);
            words[120 >> 2] = input[8]; words[104 >> 2] = input[11];
            words[664 >> 2] = input[10];
            const xmm = new Uint8Array(memory.buffer, 832, 16); xmm.set(inputVector);
            instance.exports.f(0);
            assert.deepEqual(Array.from(words.slice(64 >> 2, (64 >> 2) + 8)), expected.slice(0, 8));
            assert.equal(words[120 >> 2], expected[8]);
            assert.equal(words[556 >> 2], expected[9]);
            assert.equal(words[560 >> 2], expected[9]);
            assert.equal(words[664 >> 2], (input[10] + expected[10]) >>> 0);
            assert.equal(words[104 >> 2], expected[11]);
            assert.deepEqual(xmm, inputVector, "architectural XMM source is unchanged");
            vectorExecutions++;
        }
    }
}
console.log(`PASS: ${vectorExecutions} LICM SIMD executions; CPU-shaped state ABI, v128 shuffle, packed byte wrapping, sign masks and exact loop exits`);
