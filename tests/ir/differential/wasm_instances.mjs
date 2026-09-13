import fs from "node:fs";

// A large corpus must not keep tens of thousands of Wasm code spaces alive.
// Cache only a bounded number of case pairs; do not skip or change any cases.
export function bounded_instances(prefix, count, imports, capacity = 32) {
    if(!Number.isSafeInteger(count) || count < 0) throw new Error("invalid Wasm fixture count");
    if(!Number.isSafeInteger(capacity) || capacity < 1) throw new Error("invalid Wasm fixture cache capacity");
    const cache = new Map();
    return new Proxy(Object.create(null), {
        get(_target, key) {
            if(key === "length") return count;
            if(typeof key !== "string" || !/^(0|[1-9][0-9]*)$/.test(key)) return undefined;
            const index = Number(key);
            if(!Number.isSafeInteger(index) || index >= count) return undefined;
            let pair = cache.get(index);
            if(pair) {
                cache.delete(index);
            } else {
                while(cache.size >= capacity) cache.delete(cache.keys().next().value);
                pair = [0, 1].map(opt => new WebAssembly.Instance(
                    new WebAssembly.Module(fs.readFileSync(`${prefix}/${index}-${opt}.wasm`)),
                    {e: imports}));
            }
            cache.set(index, pair);
            return pair;
        },
    });
}
