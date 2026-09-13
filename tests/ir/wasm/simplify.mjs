import assert from "node:assert/strict";
import fs from "node:fs";
let seed = 0x51A51A51;
function random() { seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; return seed >>> 0; }
const m = new WebAssembly.Memory({initial: 64}), words = new Uint32Array(m.buffer), data = new Uint8Array(m.buffer);
let scalarRuns = 0, vectorRuns = 0, guardRuns = 0;
const inputs = [...Array(256).keys(), 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF, ...Array.from({length: 256}, random)];
for(const bits of [1, 8, 16, 32, 64]) for(let kind = 0; kind < 10; kind++) {
    for(const optimized of [false, true]) {
        const bytes = fs.readFileSync(`build/ir-simplify/scalar-${bits}-${kind}-${optimized}.wasm`);
        assert(WebAssembly.validate(bytes));
        const f = new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {m}}).exports.f;
        for(const input of inputs) for(const flags of [2, 0x8D7]) {
            const regs = [input, random(), random(), random(), 0x90000, 0x13572468, 0x24681357, 0x87654321];
            const mask = bits >= 32 ? 0xFFFFFFFF : 2 ** bits - 1;
            let expected = (input & mask) >>> 0;
            if([1, 3, 7].includes(kind)) expected = 0;
            if(kind === 2) expected = 1;
            if(kind === 8) expected = mask;
            words.fill(0); words.set(regs); words[8] = flags; words[10] = 99; words[11] = 0x12345678;
            f(0);
            assert.equal(words[0], expected, `scalar ${bits}/${kind}/${optimized}/${input}`);
            assert.deepEqual(Array.from(words.slice(1, 8)), regs.slice(1));
            assert.equal(words[8], flags); assert.equal(words[9], 0x9001);
            assert.equal(words[10], 1); assert.equal(words[11], 0x12345678);
            scalarRuns++;
        }
    }
}
function u32(a, offset) { return new DataView(Uint8Array.from(a).buffer).getUint32(offset, true); }
function store(a, offset, value, bytes) {
    let n = BigInt.asUintN(bytes * 8, BigInt(value));
    for(let i = 0; i < bytes; i++) { a[offset+i] = Number(n & 255n); n >>= 8n; }
}
function reference(kind, regs, vectors) {
    const a = vectors[1].slice(), c = vectors[2], d = vectors[3];
    let eax = regs[0], result;
    switch(kind) {
        case 0: result = a; break;
        case 1: result = [...a.slice(0, 8), ...c.slice(0, 8)].reverse(); break;
        case 2: result = [...a.slice(0, 8), ...c.slice(0, 4), ...d.slice(0, 4)]; break;
        case 3: store(a, 6, eax, 2); eax &= 65535; result = a; break;
        case 4: store(a, 4, regs[2], 4); eax = regs[2]; result = a; break;
        case 5: store(a, 0, BigInt.asIntN(32, BigInt(eax)), 8); eax = u32(a, 12); result = a; break;
        case 6: store(a, 4, eax, 4); result = a; break;
        case 7: result = Array(16).fill(0); break;
        case 8: result = Array(16).fill(255); break;
        case 9: result = a; break;
        case 10: result = Array.from({length: 16}, (_, i) => (~(i*17) & (255-i*7)) & 255); break;
        case 11: result = [...a.slice(8), ...a.slice(0, 8)]; eax = u32(a, 0); break;
        case 12: result = Array.from({length: 16}, (_, i) => i*17); store(result, 14, 0x1234, 2); eax = 0x1234; break;
        default: throw new Error("unknown case");
    }
    return {eax: eax >>> 0, result};
}
for(let kind = 0; kind < 13; kind++) for(const optimized of [false, true]) {
    let guards = 0, input, vectors;
    const bytes = fs.readFileSync(`build/ir-simplify/vector-${kind}-${optimized}.wasm`);
    assert(WebAssembly.validate(bytes));
    const f = new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {
        m, ir_enter: () => {}, ir_tlb_base: () => 0, get_eflags: () => 0x8D7,
        ir_sse_guard: () => {
            guards++;
            assert.deepEqual(Array.from(words.slice(16, 24)), input, "guard observes original GPRs");
            assert.deepEqual(Array.from(data.slice(832, 960)), vectors.flat(), "zero idiom does not corrupt fault recovery");
            assert.equal(words[166], 100);
            words[16] = 0xABCDEF01; words[139] = 0xFA170006; words[30] = 0x202;
            data[832] = 0xAA;
            return 2;
        },
    }}).exports.f;
    for(let sample = 0; sample < 128; sample++) for(const task of [0, 4]) {
        input = Array.from({length: 8}, random);
        // Boundary payloads exercise zero-extension, sign extension and lost bits.
        input[0] = [0, 65535, 65536, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF, random()][sample % 7];
        vectors = Array.from({length: 8}, () => Array.from({length: 16}, () => random() & 255));
        const expected = reference(kind, input, vectors);
        words.fill(0); words.set(input, 16); data.set(vectors.flat(), 832);
        words[145] = task; words[166] = 100; words[30] = 0x8D7; words[26] = 0x13572468;
        guards = 0;
        f(0);
        if(task) {
            assert.equal(guards, 1);
            assert.equal(words[16], 0xABCDEF01); assert.equal(words[139], 0xFA170006);
            assert.equal(words[30], 0x202); assert.equal(words[166], 100); assert.equal(data[832], 0xAA);
            assert.deepEqual(Array.from(data.slice(833, 960)), vectors.flat().slice(1));
            guardRuns++;
        } else {
            assert.equal(guards, 0);
            assert.equal(words[16], expected.eax, `SIMD extract ${kind}/${optimized}/${sample}`);
            assert.deepEqual(Array.from(words.slice(17, 24)), input.slice(1));
            assert.deepEqual(Array.from(data.slice(832, 848)), expected.result, `SIMD result ${kind}/${optimized}/${sample}`);
            assert.deepEqual(Array.from(data.slice(848, 960)), vectors.slice(1).flat());
            assert.equal(words[139], 0x9001); assert.equal(words[166], 101); assert.equal(words[30], 0x8D7);
            vectorRuns++;
        }
        assert.equal(words[26], 0x13572468);
    }
}
console.log(`PASS: ${scalarRuns} independent scalar identities, ${vectorRuns} SIMD byte/lane cases and ${guardRuns} SSE fault-observer exits`);
