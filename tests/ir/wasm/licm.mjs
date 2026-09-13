import assert from "node:assert/strict";
import fs from "node:fs";

// A separate arithmetic oracle prevents identical optimized/unoptimized bugs
// from passing the full-budget cases. Compare the complete CPU snapshot at
// each budget, including zero iterations and invalid external entry indices.
const budgets = [1, 2, 3, 4, 5, 8, 17, 100];
const counts = [0, 1, 2, 3, 7, 15, 31];
const values = [0, 1, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF, 0x12345678];
let executions = 0;
for(const budget of budgets) {
    const modules = [false, true].map(opt => {
        const bytes = fs.readFileSync(`build/ir-licm/loop-${budget}-${opt}.wasm`);
        assert(WebAssembly.validate(bytes), `LICM module budget=${budget} opt=${opt}`);
        return new WebAssembly.Module(bytes);
    });
    for(const count of counts) {
        for(let sample = 0; sample < values.length; sample++) {
            const x = values[sample];
            const y = values[(sample + 1) % values.length];
            const z = values[(sample + 3) % values.length];
            for(const entry of [0, -1, 1]) {
                const outputs = modules.map(module => {
                    const m = new WebAssembly.Memory({initial: 64});
                    const state = new Uint32Array(m.buffer);
                    state.set([count, 0x76543210, x, y, z, 5, 6, 7, 0xAD7, 0x1000, 0, 0xDEADBEEF]);
                    new WebAssembly.Instance(module, {e: {m}}).exports.f(entry);
                    executions++;
                    return Array.from(state.slice(0, 16));
                });
                const label = `budget=${budget} count=${count} sample=${sample} entry=${entry}`;
                assert.deepEqual(outputs[1], outputs[0], label);
                if(entry !== 0) {
                    assert.deepEqual(outputs[0].slice(0, 12),
                        [count, 0x76543210, x, y, z, 5, 6, 7, 0xAD7, 0x1000, 0, 0xDEADBEEF], label);
                } else if(budget === 100) {
                    const increment = Math.imul((x + y) >>> 0, z);
                    assert.equal(outputs[0][0], 0, `${label}: loop completed`);
                    assert.equal(outputs[0][1], Math.imul(count, increment) >>> 0, `${label}: arithmetic oracle`);
                    assert.equal(outputs[0][9], 0x3000, `${label}: exit EIP`);
                }
            }
        }
    }
}
console.log(`PASS: ${executions} LICM Wasm executions; budget snapshots, zero iterations, overflow and invalid entries`);
