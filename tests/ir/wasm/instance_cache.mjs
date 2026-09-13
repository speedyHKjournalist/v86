import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {bounded_instances} from "../differential/wasm_instances.mjs";
const root = fs.mkdtempSync(path.join(os.tmpdir(), "v86-ir-fixtures-"));
try {
    for(let i = 0; i < 3; i++) for(let opt = 0; opt < 2; opt++) {
        fs.writeFileSync(path.join(root, `${i}-${opt}.wasm`), Uint8Array.from([
            0,97,115,109,1,0,0,0, 1,5,1,96,0,1,127, 3,2,1,0,
            7,5,1,1,102,0,0, 10,6,1,4,0,65,i * 2 + opt,11,
        ]));
    }
    for(const capacity of [0, -1, 0.5, NaN, Infinity]) assert.throws(() => bounded_instances(root, 3, {}, capacity));
    for(const count of [-1, 0.5, Infinity]) assert.throws(() => bounded_instances(root, count, {}));
    const cache = bounded_instances(root, 3, {}, 2);
    assert.equal(cache.length, 3);
    for(const key of [-1, 3, "01", "1.0", Symbol.iterator]) assert.equal(cache[key], undefined);
    const a = cache[0], b = cache[1];
    assert.equal(cache[0], a, "a hit refreshes recency");
    const c = cache[2];
    assert.equal(cache[0], a, "the recently used pair survives");
    assert.notEqual(cache[1], b, "the least recently used pair is re-instantiated");
    for(const [i, pair] of [a, b, c].entries()) for(const [opt, instance] of pair.entries()) {
        assert.equal(instance.exports.f(), i * 2 + opt, "eviction never changes caller-owned instances");
    }
    for(let i = 0; i < 3; i++) for(let opt = 0; opt < 2; opt++) assert.equal(cache[i][opt].exports.f(), i * 2 + opt);
    console.log("PASS: bounded Wasm fixture cache capacity, LRU eviction, both optimization modes, stable caller ownership and input validation");
} finally {
    fs.rmSync(root, {recursive: true, force: true});
}
