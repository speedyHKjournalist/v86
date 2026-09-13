import assert from "node:assert/strict";
import fs from "node:fs";

function model(kind, regs, vectors) {
    const gpr = regs.slice(), xmm = vectors.map(v => Uint8Array.from(v));
    const mixed = (a, b) => Uint8Array.from({length: 16}, (_, i) => (i % 2 ? b : a)[i]);
    if(kind === 1) xmm[0] = Uint8Array.from(vectors[1]);
    if(kind === 3) xmm[0] = mixed(vectors[0], vectors[1]).reverse();
    if(kind === 4) {
        const a = mixed(vectors[0], vectors[1]), b = mixed(vectors[2], vectors[3]);
        xmm[0] = Uint8Array.from({length: 16}, (_, i) => (i % 4 < 2 ? a : b)[i]);
    }
    const data = () => new DataView(xmm[0].buffer);
    if(kind === 5 || kind === 6 || kind === 10) {
        data().setUint32(8, regs[2], true);
        gpr[0] = kind === 10 ? data().getUint32(0, true) : regs[2];
        if(kind === 6) { data().setUint32(12, regs[3], true); gpr[1] = regs[3]; }
    }
    if(kind === 7) { data().setUint16(4, regs[2], true); gpr[0] = regs[2] & 65535; }
    if(kind === 9) {
        xmm[1] = Uint8Array.from(vectors[0]);
        new DataView(xmm[1].buffer).setUint32(12, regs[2], true);
        data().setUint32(12, regs[3], true);
    }
    return {gpr, xmm};
}
const memory = new WebAssembly.Memory({initial: 64});
const bytes = new Uint8Array(memory.buffer), words = new Uint32Array(memory.buffer);
const next = state => { let x = state.value; x ^= x << 13; x ^= x >>> 17; x ^= x << 5; return state.value = x >>> 0; };
const boundaries = [0, 1, 65535, 65536, 0xDEADABCD, 0x80000000, 0x7FFFFFFF, 0xFFFFFFFF,
    0x7FC12345, 0x7F812345, 0x80000001, 0x5555AAAA];
let executions = 0;
for(let kind = 0; kind < 19; kind++) {
    let enters = 0;
    const instances = [false, true].map(opt => {
        const module = new WebAssembly.Module(fs.readFileSync(`build/ir-simd-rewrite/${kind}-${opt}.wasm`));
        const allowed = new Set(["ir_enter", "ir_tlb_base", "get_eflags", "m"]);
        assert(WebAssembly.Module.imports(module).every(i => i.module === "e" && allowed.has(i.name)));
        return new WebAssembly.Instance(module, {e: {
            m: memory,
            ir_enter: () => { enters++; },
            ir_tlb_base: () => 0,
            get_eflags: () => words[120 >> 2] | 0,
        }});
    });
    for(let seed = 0; seed < 1024; seed++) {
        const random = {value: seed + 1};
        const regs = Array.from({length: 8}, (_, i) => seed < boundaries.length ? boundaries[(seed + i) % boundaries.length] : next(random));
        const vectors = Array.from({length: 8}, () => Uint8Array.from({length: 16}, () => next(random) & 255));
        const flags = seed & 1 ? 0x8D7 : 2, cs = seed & 2 ? 0x20000 : 0;
        const result = model(kind, regs, vectors);
        for(const instance of instances) {
            bytes.fill(0xA5, 0, 1024);
            words.set(regs, 64 >> 2);
            vectors.forEach((v, i) => bytes.set(v, 832 + 16 * i));
            words[100 >> 2] = 0; words[104 >> 2] = 0xABCDEF01;
            words[120 >> 2] = flags; words[664 >> 2] = 99;
            words[(736 + 4) >> 2] = cs;
            const expected = bytes.slice(0, 1024), expectedWords = new Uint32Array(expected.buffer);
            expectedWords.set(result.gpr, 64 >> 2);
            result.xmm.forEach((v, i) => expected.set(v, 832 + i * 16));
            expectedWords[96 >> 2] = 31;
            expectedWords[112 >> 2] = flags & 64 ? 0 : 1;
            expectedWords[556 >> 2] = expectedWords[560 >> 2] = 0x1000 + cs;
            instance.exports.f(0);
            assert.deepEqual(bytes.slice(0, 1024), expected, `vector rewrite ${kind}, seed=${seed}`);
            executions++;
        }
    }
    assert.equal(enters, 2048, "one CPU entry prologue per execution");
}
console.log(`PASS: ${executions} SIMD rewrite Wasm executions against independent byte/lane models and exact CPU state bytes`);
