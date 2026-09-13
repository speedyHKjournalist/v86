import fs from "node:fs";
import path from "node:path";

// Generated fixtures have no instantiation effects. Reject sections that could
// write imported memory/tables or execute a start function before using an LRU.
function inertModule(bytes) {
    const module = new WebAssembly.Module(bytes);
    for(let at = 8; at < bytes.length;) {
        const section = bytes[at++];
        let size = 0, shift = 0, part;
        do {
            part = bytes[at++];
            size += (part & 127) * 2 ** shift;
            shift += 7;
        } while(part & 128);
        if(section === 8 || section === 9 || section === 11) {
            throw new Error("Fixture cache requires modules without start, element or data sections");
        }
        at += size;
    }
    return module;
}

// Bound live compiled artifacts, not case coverage. Large SIMD matrices exceed
// Linux/V8 mapping limits when every optimized/unoptimized module is kept alive.
// This is only a test-fixture cache; it does not replace the guest JIT cache.
export function fixtureInstances(directory, imports, count, capacity = 32) {
    if(!Number.isSafeInteger(count) || count < 0 ||
       !Number.isSafeInteger(capacity) || capacity < 1) {
        throw new RangeError("Invalid fixture count or cache capacity");
    }
    const cache = new Map();
    let hits = 0, misses = 0;
    function get(index) {
        if(!Number.isSafeInteger(index) || index < 0 || index >= count) {
            throw new RangeError(`Invalid fixture index: ${index}`);
        }
        if(cache.has(index)) {
            const pair = cache.get(index);
            cache.delete(index);
            cache.set(index, pair);
            hits++;
            return pair;
        }
        if(cache.size === capacity) cache.delete(cache.keys().next().value);
        const pair = [0, 1].map(opt => {
            const bytes = fs.readFileSync(path.join(directory, `${index}-${opt}.wasm`));
            return new WebAssembly.Instance(inertModule(bytes), imports);
        });
        cache.set(index, pair);
        misses++;
        // Optional deterministic collection for constrained CI. Unreachable
        // artifacts can also be reclaimed by the engine's ordinary code GC.
        if(misses % capacity === 0) globalThis.gc?.();
        return pair;
    }
    get.stats = () => ({entries: cache.size, capacity, hits, misses});
    get.clear = () => { cache.clear(); };
    return get;
}
