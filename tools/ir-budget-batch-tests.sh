#!/bin/sh
# Every original budget cut remains executable; the fast body is proof-gated.
set -eu
make ir-generated-check build/v86-ir-test.wasm build/v86-ir-test-release.wasm build/libv86.mjs build/jit-capacity.bin
RUSTFLAGS="${RUSTFLAGS:-} -D warnings" cargo test ir::mir::budget
RUSTFLAGS="${RUSTFLAGS:-} -D warnings" cargo test ir::budget_batch_tests
node tests/ir/differential/budget_batch.mjs
node tests/ir/differential/budget_batch.mjs build/v86-ir-test-release.wasm

node tests/ir/differential/budget_observers.mjs
node tests/ir/differential/budget_observers.mjs build/v86-ir-test-release.wasm
