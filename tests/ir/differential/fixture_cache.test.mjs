import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {fixturePairs} from "./fixture_cache.mjs";

const directory = fs.mkdtempSync(path.join(os.tmpdir(), "v86-ir-fixtures-"));
// Import e.m and re-export it; no guest code or host callbacks are necessary.
const bytes = Uint8Array.from([
    0,97,115,109,1,0,0,0,
    2,8,1,1,101,1,109,2,0,1,
    7,10,1,6,109,101,109,111,114,121,2,0,
]);
try {
    assert(WebAssembly.validate(bytes));
    for(let i = 0; i < 4; i++) {
        for(const opt of [0, 1]) fs.writeFileSync(path.join(directory, `${i}-${opt}.wasm`), bytes);
    }
    const memory = new WebAssembly.Memory({initial: 1});
    const imports = {e: {m: memory}};
    const get = fixturePairs(directory, imports, 2);
    const first = get(0), second = get(1);
    assert.strictEqual(get(0), first, "cache hit preserves pair identity and refreshes LRU");
    get(2);
    assert.strictEqual(get(0), first, "most recent pair survives eviction");
    assert.notStrictEqual(get(1), second, "least recent pair is re-instantiated");
    assert.notStrictEqual(first[0], first[1], "both optimization modes have distinct instances");
    for(const instance of first) assert.strictEqual(instance.exports.memory, memory);
    const otherMemory = new WebAssembly.Memory({initial: 1});
    const other = fixturePairs(directory, {e: {m: otherMemory}}, 1)(0);
    assert.strictEqual(other[0].exports.memory, otherMemory, "independent CPUs never share imports");
    for(const invalid of [-1, 0.5, NaN, Infinity, "0"]) {
        assert.throws(() => get(invalid), RangeError);
    }
    for(const invalid of [0, -1, 0.5, NaN, Infinity, "2"]) {
        assert.throws(() => fixturePairs(directory, imports, invalid), RangeError);
    }
    assert.throws(() => get(99), /ENOENT/, "missing fixtures are failures, not skipped cases");
    assert.strictEqual(get(0)[1].exports.memory, memory, "cache remains usable after a failed load");
    console.log("PASS: bounded differential fixture reuse, eviction, imports and error handling");
} finally {
    fs.rmSync(directory, {recursive: true, force: true});
}
