#!/bin/sh
# Guarded scalar-store continuation: native RAM can continue, observing/fault/code paths exit.
set -eu
make ir-generated-check build/v86-ir-test.wasm build/libv86.mjs build/jit-capacity.bin
RUSTFLAGS="${RUSTFLAGS:-} -D warnings" cargo test ir::store_continuation_tests
node tests/ir/differential/store_continuation.mjs
