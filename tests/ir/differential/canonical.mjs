import assert from "node:assert/strict";
import fs from "node:fs";
import {V86} from "../../../build/libv86.mjs";

const cases = JSON.parse(fs.readFileSync("build/ir-canonical/vector.json"));
const vm = new V86({wasm_path: process.argv[2] || "build/v86-ir-test.wasm", memory_size: 32 << 20,
    bios: {buffer: Uint8Array.from(fs.readFileSync("build/jit-capacity.bin")).buffer},
    disable_keyboard: true, disable_mouse: true, disable_speaker: true,
    net_device: {type: "none"}, autostart: false});
function extract(bytes, bits, lane) {
    let n = 0n;
    for(let i = 0; i < bits / 8; i++) n |= BigInt(bytes[lane * bits / 8 + i]) << BigInt(i * 8);
    return n;
}
function replace(bytes, bits, lane, value) {
    const out = bytes.slice();
    for(let i = 0; i < bits / 8; i++) out[lane * bits / 8 + i] = Number(value >> BigInt(i * 8) & 255n);
    return out;
}
function shuffle(a, b, select) {
    return Uint8Array.from(select, i => i < 16 ? a[i] : b[i - 16]);
}
function expected(kind, bits, lane, a, b, c, scalar) {
    let bytes;
    switch(kind) {
    case "identity_left": case "identity_same": case "and_same": case "or_same": case "restore": bytes = a; break;
    case "identity_right": bytes = b; break;
    case "nested": case "three_sources": {
        const inner = shuffle(a, b, Array.from({length: 16}, (_, i) => i * 3 % 32));
        bytes = shuffle(inner, kind === "nested" ? inner : c, Array.from({length: 16}, (_, i) => (i * 7 + 3) % 32));
        break;
    }
    case "overwrite": bytes = replace(a, bits, lane, extract(b, bits, lane)); break;
    case "read_same": case "read_other": {
        const inserted = replace(a, bits, lane, scalar);
        const n = extract(inserted, bits, kind === "read_same" ? lane : (lane + 1) % (128 / bits));
        return [Number(n & 0xFFFFFFFFn), Number(n >> 32n)];
    }
    case "extract_shuffle": case "extract_unaligned": {
        const lanes = Array.from({length: 16}, (_, i) => kind === "extract_shuffle" ? (i + 16) % 32 : (i * 3 + 1) % 32);
        const n = extract(shuffle(a, b, lanes), bits, lane);
        return [Number(n & 0xFFFFFFFFn), Number(n >> 32n)];
    }
    default: throw new Error(`unknown SIMD oracle ${kind}`);
    }
    return Array.from({length: 4}, (_, lane) => Number(extract(bytes, 32, lane)));
}
try {
    await new Promise(resolve => vm.add_listener("emulator-loaded", resolve));
    const cpu = vm.v86.cpu, e = cpu.wm.exports;
    const words = new Uint32Array(e.memory.buffer);
    const xmm = new Uint8Array(e.memory.buffer, 832, 128);
    // This corpus consists of pure HIR value operations. It uses the real CPU
    // entry, FLAGS and state ABI, but needs no guest instruction fetch or MMIO.
    cpu.segment_offsets.fill(0, 0, 6);
    let executions = 0;
    for(const [index, [kind, bits, lane]] of cases.entries()) {
        const instances = [0, 1, 2].map(mode => {
            const bytes = fs.readFileSync(`build/ir-canonical/vector-${index}-${mode}.wasm`);
            assert(WebAssembly.validate(bytes));
            return new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {...e, m: e.memory}});
        });
        for(const seed of [0, 1, 127, 128, 255])
        for(const scalar of [0n, 1n, 0x80000000n, 0xFFFFFFFFn, 0x89ABCDEF76543210n, 0xFFFFFFFFFFFFFFFFn]) {
            const vectors = Uint8Array.from({length: 128}, (_, i) => (seed + i * 37 + (i >> 4) * 73) & 255);
            const input = [0x87654321, 0x89ABCDEF, 0x55555555, 0xAAAAAAAA,
                Number(scalar & 0xFFFFFFFFn), Number(scalar >> 32n), 0xFEDCBA98, 0x12345678];
            const output = expected(kind, bits, lane, vectors.slice(0, 16), vectors.slice(16, 32), vectors.slice(32, 48), scalar);
            const gpr = input.slice(); gpr.splice(0, output.length, ...output);
            for(const [mode, instance] of instances.entries()) {
                cpu.reg32.set(input); xmm.set(vectors);
                cpu.flags[0] = 0x8D7; cpu.flags_changed[0] = 0;
                cpu.instruction_pointer[0] = 0x1000; words[664 >> 2] = 100; words[104 >> 2] = 0;
                instance.exports.f(0);
                const label = `${kind}/${bits}/${lane}, mode=${mode}, seed=${seed}, scalar=${scalar}`;
                assert.deepEqual(Array.from(cpu.reg32, n => n >>> 0), gpr, label);
                assert.deepEqual(xmm, vectors, `${label}: read-only XMM backing state`);
                assert.equal(e.get_eflags() >>> 0, 0x8D7, `${label}: FLAGS`);
                assert.equal(cpu.instruction_pointer[0] >>> 0, 0x1002);
                assert.equal(words[664 >> 2], 101);
                assert.equal(words[104 >> 2], input[7]);
                executions++;
            }
        }
    }
    console.log(`PASS: ${executions} SIMD canonicalization executions against independent byte-array semantics; every supported lane, narrow high-bit masking, two/three-source shuffle composition, real CPU state ABI`);
} finally {
    await vm.destroy();
}
