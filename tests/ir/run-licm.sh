#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

# Fresh clones do not contain the generated legacy tables used by the Rust crate.
make ir-generated-check \
    src/rust/gen/{interpreter,interpreter0f,jit,jit0f,analyzer,analyzer0f}.rs
RUSTFLAGS="${RUSTFLAGS:-} -D warnings" cargo test ir::passes::licm "$@"
node tests/ir/wasm/licm.mjs
