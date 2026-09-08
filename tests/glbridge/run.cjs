#!/usr/bin/env node
"use strict";
const { readdirSync } = require("node:fs");
const { join } = require("node:path");
const { spawnSync } = require("node:child_process");
let failures = 0;
for (const file of readdirSync(__dirname).sort()) {
    if (!file.endsWith("_test.js") || file.includes("_perf_") || file.includes("_corpus_")) continue;
    const result = spawnSync(process.execPath, [join(__dirname, file)], { encoding: "utf8", timeout: 120000 });
    const ok = result.status === 0;
    const skipped = ok && /^SKIP:/m.test(result.stdout || "");
    console.log(`${skipped ? "SKIP" : ok ? "PASS" : "FAIL"} ${file}`);
    if (ok && /naga absent/.test(result.stdout || "")) console.log("  WGSL compiler unavailable; structural assertions passed");
    if (!ok) { failures++; console.error(result.stdout, result.stderr, result.error || ""); }
}
process.exitCode = failures ? 1 : 0;
