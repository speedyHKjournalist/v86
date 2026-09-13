#!/usr/bin/env python3
"""Run reproducible IR checks, not ISA completion or Windows XP acceptance."""
import argparse
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
TABLES = [f"src/rust/gen/{name}.rs" for name in
          ("interpreter", "interpreter0f", "jit", "jit0f", "analyzer", "analyzer0f")]
# Keep CPU builds serial: feature variants share Cargo's output paths before
# Make copies them to isolated artifacts. The reference tests need full history.
TARGETS = """
ir-tests ir-analyzer-tests ir-memory-tests ir-stack-tests ir-control-tests
ir-shift-tests ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests
ir-misc-tests ir-loop-tests ir-cfg-tests ir-system-stack-tests ir-segment-tests
ir-string-tests ir-io-tests ir-rep-engine-tests ir-rep-tests ir-cpu-info-tests
ir-cpu-system-tests ir-control-regs-tests ir-descriptor-tests ir-task-regs-tests
ir-selector-query-tests ir-flags-observer-tests ir-verr-tests ir-cmpxchg8b-tests
ir-simd-move-tests ir-simd-integer-tests ir-simd-immediate-tests
ir-simd-shuffle-tests ir-simd-transfer-tests ir-simd-lane-tests ir-simd-masked-tests
ir-entry-tests ir-live-tests ir-cache-tests ir-auto-tests ir-backend-integration-tests
jit-publication-tests jit-disabled-tests
""".split()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--optimizations-only", action="store_true",
                        help="Run the focused LICM/SIMD Rust and emitted-Wasm checks")
    args = parser.parse_args()
    if args.optimizations_only:
        subprocess.run(["make", "-j1", *TABLES, "ir-generated-check"], cwd=ROOT, check=True)
        env = os.environ.copy()
        env["RUSTFLAGS"] = (env.get("RUSTFLAGS", "") + " -D warnings").strip()
        subprocess.run(["cargo", "test", "ir::passes"], cwd=ROOT, env=env, check=True)
        for script in ("licm", "licm_extended", "simd_opt", "corpus_shards"):
            subprocess.run(["node", f"tests/ir/wasm/{script}.mjs"], cwd=ROOT, check=True)
    else:
        # Fail before building if an archive/shallow checkout cannot provide the
        # independently pinned interpreter bodies. Never silently skip an oracle.
        subprocess.run(["git", "cat-file", "-e",
                        "8ee73e538daaab15411344d39a1f271e778ac7f3^{commit}"],
                       cwd=ROOT, check=True)
        subprocess.run(["make", "-j1", *TARGETS], cwd=ROOT, check=True)
    print("PASS: requested IR checks completed; full ISA, browser and XP acceptance are separate.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as error:
        raise SystemExit(error.returncode)
