import assert from "node:assert/strict";
import fs from "node:fs";

const samples = [0n, 1n, 2n, 0x7Fn, 0x80n, 0xFFn, 0x7FFFn, 0x8000n,
    0xFFFFn, 0x7FFFFFFFn, 0x80000000n, 0xFFFFFFFFn,
    0x7FFFFFFFFFFFFFFFn, 0x8000000000000000n, 0xFFFFFFFFFFFFFFFFn];
let scalarExecutions = 0;
for(const bits of [1, 8, 16, 32, 64]) for(let kind = 0; kind < 18; kind++) {
    for(const opt of [0, 1]) {
        const memory = new WebAssembly.Memory({initial: 64});
        const words = new Uint32Array(memory.buffer);
        const bytes = fs.readFileSync(`build/ir-canonicalize/scalar-${bits}-${kind}-${opt}.wasm`);
        assert(WebAssembly.validate(bytes));
        const f = new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {m: memory}}).exports.f;
        for(const input of samples) for(const flags of [2, 0x8D7]) {
            const x = BigInt.asUintN(bits, input);
            let expected = x;
            if([9, 10, 12, 15].includes(kind)) expected = 0n;
            if(kind === 11) expected = 1n;
            if(kind === 14) expected = (1n << BigInt(bits)) - 1n;
            const gpr = [Number(input & 0xFFFFFFFFn), Number(input >> 32n), 2, 3, 4, 5, 6, 7];
            words.fill(0); words.set(gpr); words[8] = flags; words[11] = 0x12345678;
            f(0);
            assert.equal(BigInt(words[0]) | BigInt(words[1]) << 32n, expected, `scalar ${bits}/${kind}/${opt}`);
            assert.deepEqual(Array.from(words.slice(2, 8)), gpr.slice(2));
            assert.equal(words[8], flags); assert.equal(words[9], 0x1002);
            assert.equal(words[10], 1); assert.equal(words[11], 0x12345678);
            scalarExecutions++;
        }
    }
}

function vectorReference(kind, vectors, gpr) {
    const result = vectors.map(v => v.slice());
    const regs = gpr.slice();
    const a = result[0], b = result[1], c = result[2];
    const v = new DataView(a.buffer, a.byteOffset, 16);
    switch(kind) {
        case 0: case 2: case 5: case 6: case 7: break;
        case 1: a.set(b); break;
        case 3: a.set(Uint8Array.from([...a.slice(0, 8), ...b.slice(8)]).reverse()); break;
        case 4: a.set(b.slice(8, 12), 8); a.set(c.slice(12), 12); break;
        case 8: v.setUint32(4, gpr[0], true); regs[1] = gpr[0]; break;
        case 9:
            v.setUint32(8, gpr[0], true); v.setUint32(12, gpr[1], true);
            regs[2] = gpr[0]; regs[3] = gpr[1]; break;
        case 10: v.setUint16(2, gpr[0], true); regs[1] = gpr[0] & 65535; break;
        case 11: v.setUint32(4, gpr[0], true); regs[1] = v.getUint32(0, true); break;
        case 12: v.setUint32(8, gpr[1], true); break;
        case 13: v.setUint16(2, gpr[0], true); regs[1] = v.getUint32(0, true); break;
        default: throw new Error(`unknown vector case ${kind}`);
    }
    return {vectors: result, regs};
}
let vectorExecutions = 0;
for(let kind = 0; kind < 14; kind++) for(const opt of [0, 1]) {
    const memory = new WebAssembly.Memory({initial: 64});
    const words = new Uint32Array(memory.buffer), bytes = new Uint8Array(memory.buffer);
    const moduleBytes = fs.readFileSync(`build/ir-canonicalize/vector-${kind}-${opt}.wasm`);
    assert(WebAssembly.validate(moduleBytes));
    const f = new WebAssembly.Instance(new WebAssembly.Module(moduleBytes), {e: {
        m: memory, ir_enter: () => {}, ir_tlb_base: () => 0, get_eflags: () => 0x8D7,
    }}).exports.f;
    for(let seed = 0; seed < 260; seed++) {
        let rng = seed + 1;
        const next = () => { rng ^= rng << 13; rng ^= rng >>> 17; rng ^= rng << 5; return rng >>> 0; };
        const vectors = Array.from({length: 8}, () => Uint8Array.from({length: 16}, next));
        if(seed < 4) vectors.forEach((v, i) => v.fill([0, 255, 128, 127][(seed + i) % 4]));
        const gpr = Array.from({length: 8}, next);
        if(seed < 4) gpr[0] = [0, 0xFFFFFFFF, 0xFFFF8000, 0xABCD1234][seed];
        const expected = vectorReference(kind, vectors, gpr);
        words.fill(0); words.set(gpr, 16); words[166] = 0xFFFFFFFF; words[30] = 0x8D7;
        vectors.forEach((v, i) => bytes.set(v, 832 + 16 * i));
        f(0);
        assert.deepEqual(Array.from(words.slice(16, 24)), expected.regs, `vector regs ${kind}/${opt}/${seed}`);
        expected.vectors.forEach((v, i) => assert.deepEqual(bytes.slice(832 + 16 * i, 848 + 16 * i), v, `vector ${kind}/${opt}/${seed}/${i}`));
        assert.equal(words[166], 0); assert.equal(words[139], 0x1002); assert.equal(words[30], 0x8D7);
        vectorExecutions++;
    }
}
console.log(`PASS: ${scalarExecutions} integer identity executions and ${vectorExecutions} exact SIMD-byte/lane executions against independent models`);
