import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {fixtureInstances} from "./fixture_cache.mjs";

const directory = fs.mkdtempSync(path.join(os.tmpdir(), "v86-ir-fixtures-"));
const code = value => Uint8Array.from([
    0,97,115,109,1,0,0,0,
    1,5,1,0x60,0,1,0x7F,
    3,2,1,0,
    7,5,1,1,0x66,0,0,
    10,6,1,4,0,0x41,value,0x0B,
]);
try {
    for(let i = 0; i < 3; i++) for(const opt of [0, 1]) {
        fs.writeFileSync(path.join(directory, `${i}-${opt}.wasm`), code(32 + i + opt));
    }
    const get = fixtureInstances(directory, {}, 3, 2);
    const first = get(0), second = get(1);
    assert.equal(first[0].exports.f(), 32);
    assert.equal(first[1].exports.f(), 33);
    assert.strictEqual(get(0), first, "cache hit retains the exact instance pair");
    assert.equal(get(2)[1].exports.f(), 35);
    assert.deepEqual(get.stats(), {entries: 2, capacity: 2, hits: 1, misses: 3});
    assert.notStrictEqual(get(1), second, "LRU evicts the least recently used case");
    assert.equal(second[0].exports.f(), 33, "eviction does not invalidate caller-held instances");
    const snapshot = get.stats(); snapshot.entries = 999;
    assert.equal(get.stats().entries, 2);
    for(const index of [-1, 3, NaN, Infinity, 0.5, "0"]) assert.throws(() => get(index), RangeError);
    for(const capacity of [0, -1, Infinity, 0.5]) {
        assert.throws(() => fixtureInstances(directory, {}, 3, capacity), RangeError);
    }
    assert.throws(() => fixtureInstances(directory, {}, -1), RangeError);
    get.clear(); assert.equal(get.stats().entries, 0);
    assert.equal(get(0)[0].exports.f(), 32);
    fs.unlinkSync(path.join(directory, "0-0.wasm"));
    get.clear(); assert.throws(() => get(0), /ENOENT/);
    fs.writeFileSync(path.join(directory, "0-0.wasm"), Uint8Array.of(0));
    assert.throws(() => get(0), WebAssembly.CompileError);
    const initializers = [
        // Valid void start function, deliberately not permitted in cached fixtures.
        [0,97,115,109,1,0,0,0,1,4,1,0x60,0,0,3,2,1,0,8,1,0,10,4,1,2,0,0x0B],
        // Valid memory and active data segment.
        [0,97,115,109,1,0,0,0,5,3,1,0,1,11,7,1,0,0x41,0,0x0B,1,42],
    ];
    for(const bytes of initializers) {
        assert(WebAssembly.validate(Uint8Array.from(bytes)));
        fs.writeFileSync(path.join(directory, "0-0.wasm"), Uint8Array.from(bytes));
        assert.throws(() => get(0), /without start, element or data/);
    }
    console.log("PASS: bounded fixture LRU, copied statistics, retained live instances, validation and instantiation-effect rejection");
} finally {
    fs.rmSync(directory, {recursive: true, force: true});
}
