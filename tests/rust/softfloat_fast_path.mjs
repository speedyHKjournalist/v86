import assert from "node:assert/strict";
import fs from "node:fs";
const { instance } = await WebAssembly.instantiate(fs.readFileSync("build/softfloat-fast-test.wasm"));
const e = instance.exports;
const hits = [0, 0, 0, 0];
for(const precision of [32, 64, 80]) for(const rounding of [0, 1, 2, 3]) {
    assert.equal(e.verify_scale(precision, rounding, 50000), 0, JSON.stringify({ precision, rounding,
        words: Array.from({ length: 10 }, (_, i) => e.failure_word(i).toString(16)) }));
    assert.equal(e.verify_integral(precision, rounding, 100000), 0, JSON.stringify({ precision, rounding,
        words: Array.from({ length: 10 }, (_, i) => e.failure_word(i).toString(16)) }));
}
console.log("PASS: scale and round/trunc F80 bit patterns and sticky flags, all 12 control modes");
for(const precision of [32, 64, 80]) {
    for(const rounding of [0, 1, 2, 3]) {
        const failure = e.verify(precision, rounding, 100000);
        assert.equal(failure, 0, JSON.stringify({ precision, rounding, failure,
            words: Array.from({ length: 10 }, (_, i) => e.failure_word(i).toString(16)) }));
        for(let op = 0; op < 4; op++) hits[op] += e.hits(op);
    }
}
for(const precision of [32, 64, 80]) for(const rounding of [0, 1, 2, 3]) {
    const failure = e.verify_conversions(precision, rounding, 100000);
    assert.equal(failure, 0, JSON.stringify({ precision, rounding, failure,
        words: Array.from({ length: 10 }, (_, i) => e.failure_word(i).toString(16)) }));
}
for(const precision of [32, 64, 80]) for(const rounding of [0, 1, 2, 3]) {
    const failure = e.verify_wide_conversions(precision, rounding, 100000);
    assert.equal(failure, 0, JSON.stringify({ precision, rounding, failure,
        words: Array.from({ length: 10 }, (_, i) => e.failure_word(i).toString(16)) }));
}
console.log("PASS: binary64/int32/int64 to F80 and binary64 stores, all 12 modes, raw bits and sticky flags");
console.log("PASS: binary32/F80 load and store conversions, all 12 modes, raw bits and sticky flags");
assert(hits.every(n => n > 1000), "all four exact arithmetic paths must be exercised");
console.log("PASS: all 12 precision/rounding combinations; F80 bits and raw sticky exception flags match SoftFloat", { hits });
const median = a => a.sort((x, y) => x - y)[a.length >> 1];
for(let op = 0; op < 5; op++) {
    for(const fallback of [false, true]) {
        const times = [[], []];
        const count = 500000;
        for(let round = 0; round < 9; round++) {
            for(const fast of round & 1 ? [1, 0] : [0, 1]) {
                const start = performance.now();
                const checksum = op === 4 ? e.benchmark_compare(fast, fallback, count) : e.benchmark(op, fast, fallback, count);
                const elapsed = performance.now() - start;
                assert.equal(checksum, op === 4 ? e.benchmark_compare(!fast, fallback, count) : e.benchmark(op, !fast, fallback, count));
                if(round >= 2) times[fast].push(elapsed);
            }
        }
        const reference = median(times[0]), fast = median(times[1]);
        console.log(JSON.stringify({ op: ["add", "sub", "mul", "div", "compare"][op], fallback,
            reference_ms: reference, fast_ms: fast, speedup: reference / fast }));
    }
}

for(let op = 0; op < 4; op++) {
    const times = [[], []];
    for(let round = 0; round < 9; round++) for(const fast of round & 1 ? [1, 0] : [0, 1]) {
        const start = performance.now();
        const checksum = e.benchmark_conversion(op, fast, 1000000);
        const elapsed = performance.now() - start;
        assert.equal(checksum, e.benchmark_conversion(op, !fast, 1000000));
        if(round >= 2) times[fast].push(elapsed);
    }
    console.log(JSON.stringify({ conversion: ["load", "exact-store", "rounded-store", "subnormal-fallback"][op],
        reference_ms: median(times[0]), fast_ms: median(times[1]), speedup: median(times[0]) / median(times[1]) }));
}

for(const kind of ["add", "sub", "mul", "div", "sqrt"]) for(const precision of [32,64,80]) {
    const benchmark = kind === "sqrt" ? e.benchmark_native_sqrt : e.benchmark_native_binary;
    const op = ["add", "sub", "mul", "div"].indexOf(kind);
    const times = [[], []];
    for(let round = 0; round < 9; round++) for(const fast of round & 1 ? [1, 0] : [0, 1]) {
        const start = performance.now();
        const checksum = benchmark(fast, precision, 500000, op);
        const elapsed = performance.now() - start;
        assert.equal(checksum, benchmark(!fast, precision, 500000, op));
        if(round >= 2) times[fast].push(elapsed);
    }
    console.log(JSON.stringify({ kind, native_precision: precision, reference_ms: median(times[0]),
        fast_ms: median(times[1]), speedup: median(times[0]) / median(times[1]) }));
}

{
    const times = [[], []];
    for(let round = 0; round < 9; round++) for(const fast of round & 1 ? [1, 0] : [0, 1]) {
        const start = performance.now(), checksum = e.benchmark_scale(fast, 1000000);
        const elapsed = performance.now() - start;
        assert.equal(checksum, e.benchmark_scale(!fast, 1000000));
        if(round >= 2) times[fast].push(elapsed);
    }
    console.log(JSON.stringify({ kind: "scale", reference_ms: median(times[0]), fast_ms: median(times[1]),
        speedup: median(times[0]) / median(times[1]) }));
}
