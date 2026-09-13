import assert from "node:assert/strict";
import {partition} from "../differential/corpus_shards.mjs";
for (const length of [1, 17, 2048, 2049, 33120]) {
    const corpus = Array.from({length}, (_, i) => ({id: i}));
    const count = Math.max(1, Math.ceil(length / 2048));
    const required = Math.floor(length / 2);
    const seen = new Uint32Array(length);
    for (let shard = 0; shard < count; shard++) {
        const result = partition(corpus, shard, count, required);
        assert(result.indices.includes(required));
        assert(result.cases.length <= 2049);
        result.indices.forEach((index, offset) => {
            assert.equal(result.cases[offset], corpus[index]);
            seen[index]++;
        });
    }
    for (let i = 0; i < length; i++) assert.equal(seen[i], i === required ? count : 1);
    for (const [shard, badCount, badRequired] of [[-1,count,required],[count,count,required],
        [0,0,required],[0,count,-1],[0,count,length]]) {
        assert.throws(() => partition(corpus, shard, badCount, badRequired));
    }
}
console.log("PASS: corpus partitioning preserves every case and the shared chain fixture");
