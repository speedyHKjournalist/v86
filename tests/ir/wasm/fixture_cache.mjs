import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {fixtureInstances} from "../differential/fixture_cache.mjs";
const directory = fs.mkdtempSync(path.join(os.tmpdir(), "v86-ir-fixtures-"));
try {
    const emptyModule = Uint8Array.of(0, 97, 115, 109, 1, 0, 0, 0);
    for(let i = 0; i < 3; i++) for(const opt of [0, 1]) {
        fs.writeFileSync(path.join(directory, `${i}-${opt}.wasm`), emptyModule);
    }
    const get = fixtureInstances(directory, {}, 3, 2);
    const first = get(0), second = get(1);
    assert(first.every(instance => instance instanceof WebAssembly.Instance));
    assert.equal(get(0), first, "reuse refreshes the LRU order");
    get(2);
    assert.equal(get(0), first, "recent fixture survives eviction");
    assert.notEqual(get(1), second, "oldest fixture is instantiated again");
    for(const bad of [-1, 3, 0.5, NaN, Infinity]) assert.throws(() => get(bad));
    for(const bad of [0, -1, 1025, 0.5, Infinity]) {
        assert.throws(() => fixtureInstances(directory, {}, 3, bad));
    }
    assert.throws(() => fixtureInstances(directory, {}, 0));
    fs.writeFileSync(path.join(directory, "2-0.wasm"), "invalid");
    const fresh = fixtureInstances(directory, {}, 3);
    assert.throws(() => fresh(2), WebAssembly.CompileError, "invalid fixtures are not skipped");
    console.log("PASS: bounded Wasm fixture cache, LRU eviction, input validation and compile-error propagation");
} finally {
    fs.rmSync(directory, {recursive: true, force: true});
}
