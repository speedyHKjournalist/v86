import assert from "node:assert/strict";
import {spawnSync} from "node:child_process";

// Very large suites must not retain tens of thousands of native Wasm modules.
// Keep every case and oracle; bound only how many compiled pairs stay live.
export function reexecWithGc() {
    if(typeof globalThis.gc === "function") return;
    const child = spawnSync(process.execPath,
        [...process.execArgv, "--expose-gc", ...process.argv.slice(1)], {stdio: "inherit"});
    if(child.error) throw child.error;
    process.exit(child.status ?? 1);
}

export function boundedInstances(factory, capacity = 64, collect = globalThis.gc) {
    assert(Number.isSafeInteger(capacity) && capacity > 0, "positive cache capacity required");
    assert.equal(typeof collect, "function", "call reexecWithGc before creating the cache");
    const cache = new Map();
    let evicted = 0, created = 0, peak = 0;
    const get = index => {
        assert(Number.isSafeInteger(index) && index >= 0, "invalid fixture index");
        if(cache.has(index)) {
            const value = cache.get(index);
            cache.delete(index);
            cache.set(index, value);
            return value;
        }
        if(cache.size === capacity) {
            cache.delete(cache.keys().next().value);
            // Code-space/mmap limits can be reached long before the JS heap
            // would otherwise force collection. Retained entries remain roots.
            if(++evicted % capacity === 0) collect();
        }
        const value = factory(index); // Do not cache a failed construction.
        cache.set(index, value);
        created++;
        peak = Math.max(peak, cache.size);
        return value;
    };
    get.stats = () => ({entries: cache.size, created, peak});
    get.clear = () => { cache.clear(); collect(); };
    return get;
}
