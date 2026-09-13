import assert from "node:assert/strict";
import fs from "node:fs";

let executions = 0;
let seed = 0x19C0FFEE;
function randomWord() {
    seed ^= seed << 13;
    seed ^= seed >>> 17;
    seed ^= seed << 5;
    return seed >>> 0;
}
const boundary = [0, 1, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF];
const mask = 0xFFFFFFFFn;
const cases = [];
for(const count of [0, 1, 2, 3, 7, 17, 0xFFFFFFFF]) {
    for(const a of boundary) for(const b of boundary) for(const flags of [2, 0x8D7]) {
        cases.push({input: [0xFFFFFFF0, count, a, b, 0x12345678, 0x90ABCDEF, 0x76543210, 19], flags});
    }
}
for(let i = 0; i < 128; i++) {
    const input = Array.from({length: 8}, randomWord);
    input[1] = i % 18;
    cases.push({input, flags: i & 1 ? 2 : 0x8D7});
}
for(const budget of [1, 2, 3, 4, 5, 8, 9, 16, 64, 100]) {
    const memory = new WebAssembly.Memory({initial: 64});
    const words = new Uint32Array(memory.buffer);
    const bytes = new Uint8Array(memory.buffer);
    const instances = [false, true].map(opt => {
        const binary = fs.readFileSync(`build/ir-licm/loop-${budget}-${opt}.wasm`);
        assert(WebAssembly.validate(binary));
        return new WebAssembly.Instance(new WebAssembly.Module(binary), {e: {m: memory}});
    });
    for(const {input, flags} of cases) {
        const outputs = instances.map(instance => {
            bytes.fill(0, 0, 4096);
            words.set(input);
            words[8] = flags; words[9] = 0; words[10] = 99; words[11] = 0x76543210;
            instance.exports.f(0);
            executions++;
            // Independent arithmetic oracle, including every early budget exit.
            const iterations = BigInt(input[1]) - BigInt(words[1]);
            assert(iterations >= 0n && iterations <= BigInt(input[1]));
            const mixed = ((BigInt(input[2]) + BigInt(input[3])) & mask) ^ 0x55AA55AAn;
            assert.equal(words[0], Number((BigInt(input[0]) + mixed * 3n * iterations) & mask));
            assert.deepEqual(Array.from(words.slice(2, 8)), input.slice(2));
            assert.equal(words[8], flags);
            assert.equal(words[10], 0, "same instruction accounting at every recovery point");
            assert.equal(words[11], 0x76543210);
            if(budget >= 64 && input[1] < 18) {
                assert.equal(words[9], 0x1400);
                assert.equal(words[1], 0);
            }
            return bytes.slice(0, 4096);
        });
        assert.deepEqual(outputs[1], outputs[0], `LICM budget recovery: ${budget}/${input}`);
    }
}
console.log(`PASS: ${executions} LICM loop executions; independent integer oracle, zero-trip, overflow and exact budget recovery`);

const observationStart = executions;
for(const budget of [1, 2, 3, 5, 16, 128]) {
    for(const mode of ["normal", "fault", "yield", "invalidated", "transferred"]) {
        const memory = new WebAssembly.Memory({initial: 64});
        const words = new Uint32Array(memory.buffer);
        const bytes = new Uint8Array(memory.buffer);
        let observations;
        let calls;
        let transferred;
        let deliveries;
        const environment = {
            m: memory,
            observe_iteration: (count, chain) => {
                count >>>= 0; chain >>>= 0;
                observations.push({count, chain, state: Array.from(words.slice(0, 12))});
                assert.equal(words[0], count, "helper observes the current loop state");
                assert.equal(words[4], chain, "hoisted SSA value materializes before the helper");
                calls++;
                if(mode !== "normal" && calls === 2) {
                    transferred = true;
                    words.fill(0xC0FFEE, 0, 12);
                    return [{fault: 1, transferred: 2, yield: 3, invalidated: 4}[mode], 0xBAD];
                }
                return [0, (count ^ chain ^ calls) >>> 0];
            },
            deliver_fault: () => {
                deliveries++;
                const restored = Array.from(words.slice(0, 12));
                assert.deepEqual(restored, observations.at(-1).state, "caller-owned fault restores its exact snapshot");
                observations.push({delivered: restored});
                words.fill(0xBADF00D, 0, 12);
            },
        };
        const instances = [false, true].map(opt => {
            const binary = fs.readFileSync(`build/ir-licm/observed-${budget}-${opt}.wasm`);
            assert(WebAssembly.validate(binary));
            return new WebAssembly.Instance(new WebAssembly.Module(binary), {e: environment});
        });
        for(const {input: original, flags} of cases) {
            const input = original.slice();
            // The observed fixture uses EAX for its counter and EBX for the sum.
            [input[0], input[1]] = [input[1], input[0]];
            const outputs = instances.map(instance => {
                bytes.fill(0, 0, 4096); words.set(input);
                words[8] = flags; words[9] = 0; words[10] = 99; words[11] = 0x76543210;
                observations = []; calls = 0; deliveries = 0; transferred = false;
                instance.exports.f(0);
                executions++;
                if(transferred) {
                    assert.equal(deliveries, mode === "fault" ? 1 : 0);
                    const authoritative = mode === "fault" ? 0xBADF00D : 0xC0FFEE;
                    assert(words.slice(0, 12).every(word => word === authoritative), "no stale restoration after transfer/yield/invalidation");
                } else {
                    assert.equal(words[8], flags);
                    assert.equal(words[11], 0x76543210);
                    if(words[9] === 0xA000) {
                        const expected = input.slice();
                        expected[0] = 0;
                        const increment = (BigInt(input[1]) + BigInt(input[2])) * 3n;
                        expected[3] = Number((BigInt(input[3]) + increment * BigInt(input[0])) & mask);
                        assert.deepEqual(Array.from(words.slice(0, 8)), expected);
                        assert.equal(calls, input[0]);
                    }
                }
                if(mode === "normal" && budget === 128 && input[0] < 18) {
                    assert.equal(words[9], 0xA000);
                }
                return {memory: bytes.slice(0, 4096), observations, calls, deliveries};
            });
            assert.deepEqual(outputs[1], outputs[0], `LICM observations: ${mode}/${budget}/${input}`);
        }
    }
}
console.log(`PASS: ${executions - observationStart} LICM helper executions; exact call ordering, fault ownership, yield/transfer/invalidation and StateMap observations`);
