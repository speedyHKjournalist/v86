// Reproducible focused entry point; never accepts pre-existing Wasm as evidence.
import {spawnSync} from "node:child_process";
import {fileURLToPath} from "node:url";
import path from "node:path";
import fs from "node:fs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
function run(command, args) {
    const result = spawnSync(command, args, {cwd: root, stdio: "inherit"});
    if(result.error) throw result.error;
    if(result.status !== 0) throw new Error(`${command} failed: ${result.signal || result.status}`);
}
fs.mkdirSync(path.join(root, "build"), {recursive: true});
run(process.execPath, ["tests/ir/differential/fixture_cache.test.mjs"]);
run("make", ["src/rust/gen/interpreter.rs", "src/rust/gen/interpreter0f.rs",
    "src/rust/gen/jit.rs", "src/rust/gen/jit0f.rs", "src/rust/gen/analyzer.rs",
    "src/rust/gen/analyzer0f.rs", "ir-generated-check"]);
run("cargo", ["test", "ir::passes"]);
run(process.execPath, ["tests/ir/wasm/licm.mjs"]);
run(process.execPath, ["tests/ir/wasm/simd_simplify.mjs"]);
