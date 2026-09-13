import assert from "node:assert/strict";
import {boundedInstances} from "./bounded_instances.mjs";
let collected = 0;
const cache = boundedInstances(index => ({index}), 2, () => collected++);
const first = cache(0);
cache(1);
assert.equal(cache(0), first, "cache hit preserves the instance");
cache(2); // Evicts 1, because 0 was most recently used.
assert.equal(cache(0), first);
cache(3);
assert.equal(collected, 1);
for(let index = 0; index < 1024; index++) assert.equal(cache(index).index, index);
assert.equal(cache.stats().peak, 2);
cache.clear();
assert.equal(cache.stats().entries, 0);
assert.throws(() => cache(-1));
assert.throws(() => boundedInstances(() => null, 0));
let failed = true;
const retry = boundedInstances(index => {
    if(failed) { failed = false; throw new Error("fixture failed"); }
    return index;
}, 1, () => {});
assert.throws(() => retry(0), /fixture failed/);
assert.equal(retry.stats().entries, 0);
assert.equal(retry(0), 0);
console.log("PASS: bounded fixture cache preserves indices, LRU identity, strict capacity and failure retry");
