#!/usr/bin/env node
// Focused regressions without downloading disk images or a browser.
import {spawnSync} from "node:child_process";
import {fileURLToPath} from "node:url";
const cwd = fileURLToPath(new URL("../../", import.meta.url));
function execute(command, args, extraEnv = {}) {
    const result = spawnSync(command, args, {
        cwd, stdio: "inherit", env: {...process.env, ...extraEnv},
    });
    if(result.error) throw result.error;
    if(result.status !== 0) {
        throw new Error(`${command} failed: ${result.signal || `exit ${result.status}`}`);
    }
}
execute("make", ["ir-generated-check", ...["jit", "jit0f", "interpreter", "interpreter0f", "analyzer", "analyzer0f"]
    .map(table => `src/rust/gen/${table}.rs`)]);
execute("cargo", ["test", "ir::passes::"], {
    RUSTFLAGS: `${process.env.RUSTFLAGS || ""} -D warnings`.trim(),
});
execute(process.execPath, ["tests/ir/wasm/licm.mjs"]);
execute(process.execPath, ["tests/ir/wasm/simd_opt.mjs"]);
