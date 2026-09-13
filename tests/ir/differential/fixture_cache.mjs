import fs from "node:fs";

// Large ISA corpora must not retain every native Wasm code mapping at once.
// This changes fixture lifetime only: every requested optimized/unoptimized
// pair is still compiled, instantiated with the caller's imports and executed.
export function fixturePairs(directory, imports, capacity = 32) {
    if(!Number.isSafeInteger(capacity) || capacity < 1) throw new RangeError("invalid fixture capacity");
    const cache = new Map();
    return index => {
        if(!Number.isSafeInteger(index) || index < 0) throw new RangeError("invalid fixture index");
        let pair = cache.get(index);
        if(pair) cache.delete(index);
        else {
            if(cache.size >= capacity) cache.delete(cache.keys().next().value);
            pair = [0, 1].map(opt => new WebAssembly.Instance(
                new WebAssembly.Module(fs.readFileSync(`${directory}/${index}-${opt}.wasm`)), imports));
        }
        cache.set(index, pair);
        return pair;
    };
}
