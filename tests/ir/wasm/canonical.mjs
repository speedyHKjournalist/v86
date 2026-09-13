import assert from "node:assert/strict";
import fs from "node:fs";

const samples = [0n, 1n, 2n, 0x7Fn, 0x80n, 0x8000n, 0x7FFFFFFFn, 0x80000000n,
    0xFFFFFFFFn, 0x7FFFFFFFFFFFFFFFn, 0x8000000000000000n, 0xFFFFFFFFFFFFFFFFn];
const zeroKinds = new Set(["self_sub", "self_xor", "self_ult", "self_slt", "mul_zero", "and_zero", "zero_shl"]);
const cases = JSON.parse(fs.readFileSync("build/ir-canonical/scalar.json"));
let executions = 0;
const m = new WebAssembly.Memory({initial: 64});
const words = new Uint32Array(m.buffer);
for(const [index, [bits, kind]] of cases.entries()) for(const mode of [0, 1, 2]) {
    const bytes = fs.readFileSync(`build/ir-canonical/scalar-${index}-${mode}.wasm`);
    assert(WebAssembly.validate(bytes));
    const f = new WebAssembly.Instance(new WebAssembly.Module(bytes), {e: {m}}).exports.f;
    for(const a of samples) for(const b of samples) {
        const input = [Number(a & 0xFFFFFFFFn), Number(a >> 32n), Number(b & 0xFFFFFFFFn), Number(b >> 32n),
            0x12345678, 0x87654321, 0xFEDCBA98, 0x89ABCDEF];
        words.set(input); words[8] = 0x8D7; words[9] = 0; words[10] = 99; words[11] = 0xAABBCCDD;
        f(0);
        const actual = BigInt(words[0]) | BigInt(words[1]) << 32n;
        const expected = kind === "self_eq" ? 1n : zeroKinds.has(kind) ? 0n : BigInt.asUintN(bits, a);
        assert.equal(actual, expected, `${bits}/${kind}, mode ${mode}, ${a}/${b}`);
        assert.deepEqual(Array.from(words.slice(2, 8)), input.slice(2));
        assert.equal(words[8], 0x8D7);
        assert.equal(words[9], 0x1002);
        assert.equal(words[10], 1);
        assert.equal(words[11], input[7], "recovery-only alias was rewritten");
        executions++;
    }
}
console.log(`PASS: ${executions} scalar canonicalization executions; independent BigInt width, shift, comparison and subregister identities plus recovery-only aliases`);
