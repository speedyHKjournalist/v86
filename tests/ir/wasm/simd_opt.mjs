import assert from "node:assert/strict";
import fs from "node:fs";
const memory = new WebAssembly.Memory({initial: 64});
const w = new Uint32Array(memory.buffer), bytes = new Uint8Array(memory.buffer);
const functions = [false, true].map(optimized => {
    const code = fs.readFileSync(`build/ir-simd-opt/${optimized}.wasm`);
    assert(WebAssembly.validate(code));
    return new WebAssembly.Instance(new WebAssembly.Module(code), {e: {
        m: memory, ir_enter: () => {}, ir_tlb_base: () => 0, get_eflags: () => w[30],
    }}).exports.f;
});
let seed = 0x13579BDF;
function random() { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed; }
const read32 = (v, at) => new DataView(v.buffer, v.byteOffset, v.byteLength).getUint32(at, true);
function insert(v, bits, lane, value) {
    const view = new DataView(v.buffer, v.byteOffset, v.byteLength);
    if(bits === 32) view.setUint32(lane * 4, value, true);
    else if(bits === 16) view.setUint16(lane * 2, value, true);
    else view.setUint8(lane, value);
}
let executions = 0;
for(let trial = 0; trial < 2048; trial++) {
    const input = Array.from({length: 8}, random);
    const vectors = Array.from({length: 8}, () => {
        const v = new Uint8Array(16), view = new DataView(v.buffer);
        for(let i = 0; i < 4; i++) view.setUint32(i * 4, random(), true);
        return v;
    });
    if(trial < 8) vectors[0].fill([0, 0xFF, 0x80, 0x7F, 1, 0x55, 0xAA, 0xFE][trial]);
    const expectedVectors = vectors.map(v => v.slice());
    const combined = Uint8Array.from([...vectors[1], ...vectors[2]]);
    expectedVectors[1] = Uint8Array.from({length: 16}, (_, i) => combined[((15 - i) * 7) % 32]);
    insert(expectedVectors[3], 32, 2, input[5]);
    insert(expectedVectors[4], 32, 1, input[6]);
    insert(expectedVectors[5], 16, 3, input[7]);
    insert(expectedVectors[6], 16, 2, input[0]);
    insert(expectedVectors[7], 32, 0, input[7]);
    insert(expectedVectors[7], 32, 1, input[6]);
    const expectedGpr = [input[6], input[7] & 65535, input[0] & 65535, read32(vectors[5], 0),
        read32(expectedVectors[5], 4), input[7], input[6], input[7]];
    let baseline;
    for(const execute of functions) {
        bytes.fill(0, 0, 2048);
        w.set(input, 16);
        for(let i = 0; i < 8; i++) bytes.set(vectors[i], 832 + i * 16);
        w[30] = 0x8D7; w[26] = 0x12345678; w[166] = 0xFFFFFFFF;
        execute(0);
        assert.deepEqual(Array.from(w.slice(16, 24)), expectedGpr);
        for(let i = 0; i < 8; i++) assert.deepEqual(bytes.slice(832 + i * 16, 848 + i * 16), expectedVectors[i]);
        assert.equal(w[30], 0x8D7); assert.equal(w[26], 0x12345678); assert.equal(w[166], 0);
        const state = Buffer.from(bytes.slice(0, 2048));
        if(baseline) assert.deepEqual(state, baseline);
        else baseline = state;
        executions++;
    }
}
console.log(`PASS: ${executions} SIMD simplification Wasm executions; 16/32/64-bit insertion, overlap, two-source shuffle composition and full recovery state`);
