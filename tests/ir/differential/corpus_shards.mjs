// Keep large differential corpora below host Wasm code-space/map limits without
// sampling or dropping cases. Each child runs both debug/release and both passes.
import assert from "node:assert/strict";
import {spawnSync} from "node:child_process";
import {fileURLToPath} from "node:url";

const KEY = "V86_IR_CORPUS_SHARD";

export function partition(corpus, shard, count, required) {
    assert(Number.isSafeInteger(count) && count > 0);
    assert(Number.isSafeInteger(shard) && shard >= 0 && shard < count);
    assert(Number.isSafeInteger(required) && required >= 0 && required < corpus.length);
    const indices = corpus.map((_, i) => i).filter(i => i % count === shard || i === required);
    return {cases: indices.map(i => corpus[i]), indices};
}

export function shardCorpus(corpus, script, required, limit = 2048) {
    assert(Number.isSafeInteger(limit) && limit > 0);
    const count = Math.max(1, Math.ceil(corpus.length / limit));
    const token = process.env[KEY];
    if (token === undefined && count > 1) {
        for (let shard = 0; shard < count; shard++) {
            console.log(`IR corpus shard ${shard + 1}/${count} (${corpus.length} total cases)`);
            const result = spawnSync(process.execPath,
                [...process.execArgv, fileURLToPath(script), ...process.argv.slice(2)], {
                    stdio: "inherit",
                    env: {...process.env, [KEY]: `${shard}/${count}`},
                });
            if (result.error) throw result.error;
            if (result.status !== 0) {
                throw new Error(`IR corpus shard ${shard + 1}/${count} failed: ${result.signal || result.status}`);
            }
        }
        console.log(`PASS: all ${corpus.length} corpus cases across ${count} isolated processes`);
        process.exit(0);
    }
    let shard = 0;
    if (token !== undefined) {
        assert(/^\d+\/\d+$/.test(token), "invalid IR corpus shard token");
        const [index, expectedCount] = token.split("/").map(Number);
        assert.equal(expectedCount, count, "IR corpus shard count changed");
        shard = index;
    }
    return partition(corpus, shard, count, required);
}
