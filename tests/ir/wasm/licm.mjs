import assert from "node:assert/strict";
import fs from "node:fs";

// Independent guest-progress model: entry, header, body, header, ..., exit.
// Hoisting must not change this work accounting or any helper observation.
function reference(n, budget, outcome, at) {
    let remaining = budget - 1, completed = 0, block = "header";
    for(;;) {
        if(remaining-- === 0) return {kind: "budget", completed};
        if(block === "header") block = completed === n ? "exit" : "body";
        else if(block === "exit") return {kind: "normal", completed};
        else {
            if(outcome && completed === at) return {kind: outcome === 1 ? "fault" : "owned", completed};
            completed++;
            block = "header";
        }
    }
}
let executions = 0, observations = 0;
for(const audit of [false, true]) for(const budget of [1, 2, 3, 4, 5, 9, 16, 100]) {
    const outcomes = audit ? [[0, -1], [1, 0], [1, 2], [2, 2], [3, 2], [4, 2]] : [[0, -1]];
    for(const opt of [false, true]) {
        const bytes = fs.readFileSync(`build/ir-licm/${audit}-${opt}-${budget}.wasm`);
        assert(WebAssembly.validate(bytes));
        const m = new WebAssembly.Memory({initial: 64}), w = new Uint32Array(m.buffer);
        let input, initialCount, base, delta, outcome, at, completed, calls, deliveries;
        const increment = () => ((input[2] + input[3]) ^ input[6]) >>> 0;
        const accumulator = k => (input[0] + Math.imul(k, increment())) >>> 0;
        const count = () => (initialCount + completed * (1 + delta)) >>> 0;
        function before(decoded) {
            assert.equal(w[16], accumulator(completed), "precise accumulator snapshot");
            assert.equal(w[17], (input[1] - completed) >>> 0, "remaining iterations");
            assert.equal(w[166], count(), "SSA count plus preserved callback increments");
            assert.equal(w[140], (base + 0x8000) >>> 0, "fault PC");
            assert.equal(w[139], (base + (decoded ? 0x8001 : 0x8000)) >>> 0, "resumption PC");
            assert.equal(w[30], 0x8D7, "arithmetic and system flags");
        }
        const e = {m, ir_enter: () => {}, ir_tlb_base: () => 0, get_eflags: () => 0x8D7};
        if(audit) {
            e.licm_audit = next => {
                before(true);
                assert.equal(next >>> 0, accumulator(completed + 1), "speculated result not yet committed");
                observations++; calls++;
                if(outcome && completed === at) {
                    w[16] = 0xABCDEF01; w[139] = 0xDEADBEEF; w[30] = 0x202;
                    if(outcome !== 1) w[166] = (count() + 777) >>> 0;
                    return outcome;
                }
                w[166] = (w[166] + delta) >>> 0;
                completed++;
                return 0;
            };
            e.licm_fault = () => {
                before(false); deliveries++;
                w[139] = 0xFA170001; w[30] = 0x202;
            };
        }
        const f = new WebAssembly.Instance(new WebAssembly.Module(bytes), {e}).exports.f;
        for(const n of [0, 1, 2, 7, 0xFFFFFFFF]) for(const x of [0, 0xFFFFFFF0]) {
            for(const values of [[0, 0, 0], [1, 3, 7], [0xFFFFFFFF, 0x80000000, 0x12345678]]) {
                for(const startCount of [0, 0xFFFFFFFC]) for(const cs of [0, 0xFFFFFF00]) {
                    for(const adjustment of (audit ? [0, 11] : [0])) for(const [result, stop] of outcomes) {
                        input = [x, n, values[0], values[1], 0x90000, 0x13572468, values[2], 0x76543210];
                        initialCount = startCount; base = cs; delta = adjustment;
                        outcome = result; at = stop; completed = calls = deliveries = 0;
                        w.fill(0); w.set(input, 16); w[166] = initialCount; w[185] = base;
                        w[30] = 0x8D7; w[25] = 0; w[26] = 0x43218765;
                        f(0);
                        const expected = reference(n, budget, outcome, at);
                        if(audit) {
                            assert.equal(completed, expected.completed);
                            assert.equal(calls, completed + Number(expected.kind === "fault" || expected.kind === "owned"));
                        } else completed = expected.completed;
                        assert.equal(deliveries, Number(expected.kind === "fault"));
                        assert.equal(w[166], (count() + (expected.kind === "owned" ? 777 : 0)) >>> 0);
                        assert.equal(w[16], expected.kind === "owned" ? 0xABCDEF01 : accumulator(completed));
                        assert.equal(w[17], (n - completed) >>> 0);
                        assert.deepEqual(Array.from(w.slice(18, 24)), input.slice(2));
                        assert.equal(w[139], expected.kind === "owned" ? 0xDEADBEEF : expected.kind === "fault" ? 0xFA170001 :
                            (base + (expected.kind === "normal" ? 0x8001 : 0x8000)) >>> 0);
                        assert.equal(w[30], ["fault", "owned"].includes(expected.kind) ? 0x202 : 0x8D7);
                        assert.equal(w[26], 0x43218765, "lazy flag operand preserved");
                        executions++;
                    }
                }
            }
        }
    }
}
console.log(`PASS: ${executions} independently checked LICM CPU executions and ${observations} helper observations`);
