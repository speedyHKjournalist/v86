import assert from "node:assert/strict";
import fs from "node:fs";

// This model evaluates the ORIGINAL SSA expression DAG as bytes and BigInts.
// It does not import the compiler's shuffle/lane helpers or emitted MIR plans.
const cases = JSON.parse(fs.readFileSync("build/ir-simd-opt/cases.json"));
function expected(spec, input) {
    const view = new DataView(input.buffer, input.byteOffset, input.byteLength);
    const memo = new Map();
    function value(id) {
        if(memo.has(id)) return memo.get(id);
        assert(spec.nodes[id], `missing model definition ${id}`);
        const [op, ids, bits] = spec.nodes[id];
        const args = ids.map(value);
        let result;
        switch(op[0]) {
            case "xmm": result = input.slice(832 + 16 * op[1], 848 + 16 * op[1]); break;
            case "gpr": result = BigInt(view.getUint32(64 + 4 * op[1], true)); break;
            case "const": result = BigInt(op[1]); break;
            case "shuffle": {
                const both = [...args[0], ...args[1]];
                result = Uint8Array.from(op[1], index => both[index]);
                break;
            }
            case "lane_read": {
                result = 0n;
                const bytes = op[1] / 8, start = op[2] * bytes;
                for(let i = 0; i < bytes; i++) result |= BigInt(args[0][start + i]) << BigInt(i * 8);
                break;
            }
            case "lane_write": {
                result = args[0].slice();
                const bytes = op[1] / 8, start = op[2] * bytes;
                for(let i = 0; i < bytes; i++) result[start + i] = Number(args[1] >> BigInt(i * 8) & 255n);
                break;
            }
            case "packed": {
                result = Uint8Array.from(args[0], (a, i) => {
                    const b = args[1][i];
                    switch(op[1]) {
                        case "And": return a & b;
                        case "Or": return a | b;
                        case "Xor": return a ^ b;
                        case "AndNot": return ~a & b;
                        default: throw new Error(`unsupported packed model ${op[1]}`);
                    }
                });
                break;
            }
            case "extend": result = op[1] ? BigInt.asIntN(spec.nodes[ids[0]][2], args[0]) : args[0]; break;
            case "extract": result = args[0] >> BigInt(op[1]); break;
            case "insert": {
                const mask = (1n << BigInt(spec.nodes[ids[1]][2])) - 1n;
                const shift = BigInt(op[1]);
                result = args[0] & ~(mask << shift) | (args[1] & mask) << shift;
                break;
            }
            default: throw new Error(`unsupported model ${op[0]}`);
        }
        if(typeof result === "bigint") result = BigInt.asUintN(bits, result);
        memo.set(id, result);
        return result;
    }
    return {
        gpr: spec.gpr.map(id => Number(value(id))),
        xmm: Uint8Array.from(spec.xmm.flatMap(id => Array.from(value(id)))),
    };
}

const inputs = Array.from({length: 68}, (_, sample) => {
    let rng = (sample + 1) * 7919;
    const next = () => { rng ^= rng << 13; rng ^= rng >>> 17; rng ^= rng << 5; return rng >>> 0; };
    const input = Uint8Array.from({length: 4096}, () => next() & 255);
    const view = new DataView(input.buffer);
    if(sample < 4) {
        for(let i = 0; i < 128; i++) input[832 + i] = [0, 255, i, 255 - i][sample];
    }
    // Raw EFLAGS, lazy backing, CS base and count deliberately vary independently.
    view.setUint32(120, 0x897 | (sample & 1) << 6, true);
    view.setUint32(100, (sample >> 1 & 1) << 6, true);
    view.setUint32(740, [0, 0x7000, 0xFFFFF000, 0xFFFFFFFF][sample % 4], true);
    view.setUint32(664, sample & 1 ? 0xFFFFFFFD : 0, true);
    // High bits must NOT leak from PINSRW/PEXTRW into the destination GPR.
    view.setUint32(64, [0, 0xFFFF, 0xFFFF0000, 0xFFFFFFFF, 0x80008000, 0x12345678, next()][sample % 7], true);
    return input;
});

let executions = 0;
for(const [index, {name, spec}] of cases.entries()) {
    const instances = [0, 1, 2, 3].map(mode => {
        const code = fs.readFileSync(`build/ir-simd-opt/${index}-${mode}.wasm`);
        assert(WebAssembly.validate(code), `${name}/${mode} must validate`);
        const m = new WebAssembly.Memory({initial: 64});
        const data = new Uint8Array(m.buffer), words = new Uint32Array(m.buffer);
        const f = new WebAssembly.Instance(new WebAssembly.Module(code), {e: {
            m, ir_enter: () => {}, ir_tlb_base: () => 0,
            get_eflags: () => words[120 / 4],
        }}).exports.f;
        return {data, words, f};
    });
    for(const [sample, input] of inputs.entries()) {
        const model = expected(spec, input);
        let reference;
        for(const [mode, {data, words, f}] of instances.entries()) {
            data.set(input);
            f(0);
            const label = `${name}/${mode}/input-${sample}`;
            assert.deepEqual(Array.from(words.slice(16, 24)), model.gpr, `${label} GPR`);
            assert.deepEqual(data.slice(832, 960), model.xmm, `${label} XMM`);
            const before = new DataView(input.buffer);
            assert.equal(words[556 / 4], (before.getUint32(740, true) + 0x1004) >>> 0, `${label} EIP`);
            assert.equal(words[664 / 4], (before.getUint32(664, true) + 7) >>> 0, `${label} count`);
            assert.equal(words[120 / 4], before.getUint32(120, true), `${label} EFLAGS`);
            if(mode === 0) reference = data.slice(0, 4096);
            else assert.deepEqual(data.subarray(0, 4096), reference, `${label} complete CPU-state backing`);
            executions++;
        }
    }
}
console.log(`PASS: SIMD optimizer, ${cases.length} expression DAGs, ${inputs.length} input states, ${executions} Wasm executions (unoptimized / SIMD-only / full pipeline on / full pipeline off)`);

const before = cases.reduce((sum, c) => sum + c.sizes[3], 0);
const after = cases.reduce((sum, c) => sum + c.sizes[2], 0);
console.log(`SIMD corpus module bytes with the otherwise identical pipeline: ${before} -> ${after}. This is not an application runtime benchmark.`);
