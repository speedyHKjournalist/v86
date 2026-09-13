// Large corpora must not pin tens of thousands of separate executable mappings.
// CPU state lives in the imported memory, not in these generated code instances.
import assert from "node:assert/strict";
import fs from "node:fs";

export function fixtureInstances(directory, imports, count, capacity = 128) {
    assert(Number.isSafeInteger(count) && count > 0, "invalid fixture count");
    assert(Number.isSafeInteger(capacity) && capacity > 0 && capacity <= 1024, "invalid cache capacity");
    const cache = new Map();
    return index => {
        assert(Number.isSafeInteger(index) && index >= 0 && index < count, "invalid fixture index");
        let pair = cache.get(index);
        if(pair) cache.delete(index);
        else {
            pair = [0, 1].map(opt => {
                const bytes = fs.readFileSync(`${directory}/${index}-${opt}.wasm`);
                return new WebAssembly.Instance(new WebAssembly.Module(bytes), imports);
            });
            if(cache.size === capacity) cache.delete(cache.keys().next().value);
        }
        cache.set(index, pair);
        return pair;
    };
}
