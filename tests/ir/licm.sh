#!/usr/bin/env bash
# Focused native/CPU-ABI checks without a guest OS image or host assembler.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
mkdir -p build
make src/rust/gen/interpreter.rs src/rust/gen/interpreter0f.rs \
    src/rust/gen/jit.rs src/rust/gen/jit0f.rs \
    src/rust/gen/analyzer.rs src/rust/gen/analyzer0f.rs ir-generated-check
RUSTFLAGS="${RUSTFLAGS:+${RUSTFLAGS} }-D warnings" cargo test ir::passes::licm
node tests/ir/wasm/licm.mjs
node tests/ir/wasm/licm_typed.mjs
