import assert from "node:assert/strict";
import fs from "node:fs";

const cases = JSON.parse(fs.readFileSync("build/ir-simd-simplify/cases.json"));
const layout = JSON.parse(fs.readFileSync("build/ir-simd-simplify/layout.json"));
const memory = new WebAssembly.Memory({initial: 64});
const bytes = new Uint8Array(memory.buffer);
const words = new Uint32Array(memory.buffer);
const shift = address => address / 4;
// No device/permission helper may be silently stubbed in a value-only test.
const env = {
    m: memory,
    ir_enter() {},
    ir_tlb_base: () => 0,
    get_eflags: () => words[shift(layout.flags)],
};
const shuffle = (a, b, lanes) => Uint8Array.from(lanes, i => i < 16 ? a[i] : b[i - 16]);
function extract(vector, bits, lane) {
    let result = 0n;
    for(let i = 0; i < bits / 8; i++) result |= BigInt(vector[lane * bits / 8 + i]) << BigInt(i * 8);
    return result;
}
function replace(vector, bits, lane, value) {
    const result = vector.slice();
    for(let i = 0; i < bits / 8; i++) result[lane * bits / 8 + i] = Number(value >> BigInt(i * 8) & 255n);
    return result;
}
function reference(description, [x, y, z], scalar) {
    const [kind, first, second, operation] = description;
    if(kind === "identity") return x;
    if(kind === "shuffle") {
        const a = shuffle(x, y, first);
        return shuffle(a, operation === 0 ? a : operation === 1 ? x : z, second);
    }
    assert.equal(kind, "lane");
    const bits = first, lane = second;
    const inserted = replace(x, bits, lane, scalar);
    switch(operation) {
        case 0: return x;
        case 1: return replace(x, Math.max(bits, 32), 0, extract(inserted, bits, lane));
        case 2: return replace(x, bits, lane, extract(x, bits, (lane + 1) % (128 / bits)));
        case 3:
        case 4: return inserted;
        default: throw new Error(`unknown lane fixture ${operation}`);
    }
}
let seed = 0x86BAD00D;
function random() {
    seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5;
    return seed >>> 0;
}
let executions = 0;
for(const [index, description] of cases.entries()) {
    const functions = [false, true].map(optimize => {
        const binary = fs.readFileSync(`build/ir-simd-simplify/${index}-${optimize}.wasm`);
        assert(WebAssembly.validate(binary));
        const module = new WebAssembly.Module(binary);
        for(const entry of WebAssembly.Module.imports(module)) {
            assert.equal(entry.module, "e");
            assert(Object.hasOwn(env, entry.name), `unexpected CPU observation: ${entry.name}`);
        }
        return new WebAssembly.Instance(module, {e: env}).exports.f;
    });
    for(let sample = 0; sample < 128; sample++) {
        const vectors = Array.from({length: 4}, () => Uint8Array.from({length: 16}, () => random() & 255));
        const registers = Array.from({length: 8}, random);
        if(sample === 0) { vectors.forEach(v => v.fill(0)); registers.fill(0); }
        if(sample === 1) { vectors.forEach(v => v.fill(255)); registers.fill(0xFFFFFFFF); }
        if(sample === 2) registers[4] = 0xFFFF1234; // Detect unmasked 16-bit extraction.
        const scalar = BigInt(registers[4]) | BigInt(registers[5]) << 32n;
        const expected = reference(description, vectors, scalar);
        const expected_words = Array.from({length: 4}, (_, i) =>
            new DataView(expected.buffer, expected.byteOffset).getUint32(i * 4, true));
        const flags = sample & 1 ? 0x8D7 : 2;
        let before;
        for(const execute of functions) {
            bytes.fill(0, 0, 2048);
            words.set(registers, shift(layout.gpr));
            vectors.forEach((v, i) => bytes.set(v, layout.xmm + i * 16));
            words[shift(layout.flags)] = flags;
            words[shift(layout.last)] = 0x89ABCDEF;
            words[shift(layout.count)] = 99;
            execute(0);
            const observed = bytes.slice(0, 2048);
            if(before) assert.deepEqual(observed, before, `SIMD ${index}/${sample}: complete CPU state`);
            else before = observed;
            assert.deepEqual(Array.from(words.slice(shift(layout.gpr), shift(layout.gpr) + 4)),
                expected_words, `SIMD ${index}/${sample}: independent byte/lane oracle`);
            assert.deepEqual(Array.from(words.slice(shift(layout.gpr) + 4, shift(layout.gpr) + 8)), registers.slice(4));
            assert.equal(words[shift(layout.flags)], flags);
            assert.equal(words[shift(layout.last)], 0x89ABCDEF);
            assert.equal(words[shift(layout.changes)], 0);
            assert.equal(words[shift(layout.count)], 100);
            assert.equal(words[shift(layout.eip)], 0x1001);
            vectors.forEach((v, i) => assert.deepEqual(bytes.slice(layout.xmm + i * 16, layout.xmm + (i + 1) * 16), v));
            executions++;
        }
    }
}
console.log(`PASS: ${executions} SIMD simplification executions; independent byte/lane oracle, shuffle compositions, multi-input rejection, narrow extraction and full-state equivalence`);
