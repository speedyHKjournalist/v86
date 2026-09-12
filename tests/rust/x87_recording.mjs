import assert from "node:assert/strict";
import fs from "node:fs";
const { instance } = await WebAssembly.instantiate(fs.readFileSync("build/softfloat-fast-test.wasm"));
const e = instance.exports;
const read = () => Array.from({ length: 72 }, (_, i) =>
    e.performance_recording_x87_get(i / 24 | 0, (i / 12 | 0) % 2, (i / 4 | 0) % 3, i % 4));
const exercise = () => {
    for(const precision of [32, 64, 80]) for(let rounding = 0; rounding < 4; rounding++) {
        for(let op = 0; op < 3; op++) for(const fallback of [false, true]) {
            assert.equal(e.recording_arithmetic(op, fallback, precision, rounding), 1,
                "recording preserves arithmetic bits and exception flags");
        }
        assert.equal(e.performance_recording_x87_state(0), precision);
        assert.equal(e.performance_recording_x87_state(1), rounding);
    }
};
exercise();
assert(read().every(n => n === 0), "disabled recording must not count");
e.performance_recording_x87_enable(1);
exercise();
assert(read().every(n => n === 1), "each operation/path/mode counted exactly once");
e.performance_recording_x87_enable(0);
exercise();
assert(read().every(n => n === 1), "stopped counters remain frozen");
e.performance_recording_x87_enable(1);
assert(read().every(n => n === 0), "a new recording resets every mode");
assert.equal(e.performance_recording_x87_get(3, 0, 0, 0), 0);
assert.equal(e.performance_recording_x87_get(0, 2, 0, 0), 0);
console.log("PASS: x87 recording lifecycle, exact/fallback counts, all 12 modes and unchanged results/flags");
