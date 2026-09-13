# IR-11 supplemental LICM recovery validation

This supplements [the LICM implementation](ir-licm.md) and does not complete
IR-00–IR-14. Existing ISA coverage, defaults, architectural ABI and retirement
gates remain unchanged. The original full roadmap status remains in
[ir-progress.md](ir-progress.md).

## Additional implementation and regression coverage

LICM now charges the instruction-removal scans and the final append work against
its existing work budget. Exhaustion at these later stages still leaves the
caller's Region unchanged. The existing atomicity test includes a limit one unit
below a completed pass, exercising late rollback.

The optimized real-CPU CFG fixtures now invoke LICM after the ordinary HIR passes.
Two new scalar/vector loops include an actual NOP preheader, so LICM has a legal
hoist destination. Each fixture asserts that instructions really moved; a pass
that silently did nothing cannot satisfy these tests. Both loops fault on their
second iteration after one data page was consumed. Comparison with interpretation
covers dirty GPR/FLAGS/XMM state, CS-relative recovery, previous EIP, CR2, saved
fault frames and wrapping retired-instruction counts. The added NOP is explicitly
included in the retirement oracle rather than copied from the old fixtures.

## Executed local checks for this supplement

Linux x86-64, Rust 1.98.1, Node.js 22.16.0. The reconciliation run used the LICM
sources/tests from `7c4e337be20acf34ee7419013a7d105db935e18d` plus the additional
budget charges and CFG cases described above. Its results were:

| Check | Result |
|---|---|
| `RUSTFLAGS="-D warnings" cargo test` | 139 passed, 0 failed; 12 focused LICM tests |
| Existing standalone Wasm runner including LICM | Passed; 12,288 LICM executions plus invalid-entry checks |
| Actual CPU LICM runner | 3,840 comparisons, 32,160 interpreter steps |
| Expanded reachable-CFG runner | 43,008 comparisons, 203,744 interpreter steps |
| Exact optimized/unoptimized budget exits | 21,504 comparisons |
| Second-iteration scalar/vector page faults | 32 comparisons |
| Constant branches avoiding absent pages | 16 comparisons |

The five experimental debug/release CPU Wasm builds, live compilation,
publication/cache/invalidation, automatic compilation/upgrade/eviction and public
backend tests also passed with this LICM compiler implementation. The shared
IR/legacy analyzer comparison completed 2,670,035 cases. These are specific
suite results, not complete ISA, OS or performance acceptance.

The branch also contains a separately added SIMD peephole increment at
`608bb8678f455fc4554fa641f4193088bc210442`. The 139-test reconciliation run above
predates that increment; it must not be represented as validation of the combined
final source tree. Subsequent integrated results belong in the PR/check logs.

## Incomplete checks and acceptance limits

The external decoder oracle was not run locally because `ndisasm` is absent.
The IR CI installs NASM and includes that check; configuration alone is not a
passing result. The large pre-existing SIMD integer and immediate differential
runners terminated with V8 Wasm code-space out-of-memory errors in this local
environment. Neither interrupted run is counted as passed. In that extended run,
the scalar shift suite completed before the SIMD integer interruption.

No Windows XP/guest-OS boot acceptance, full real-browser/Worker matrix,
application benchmark, measured performance improvement, full ISA migration or
legacy-JIT retirement is claimed by this supplement.

## Reproduce the added cases

```sh
make ir-tests
make ir-cfg-tests
tools/ir-licm-tests.sh
```

The native test phase emits the expanded CFG fixtures before their JavaScript
runner consumes them. These checks use the normal repository build prerequisites
and do not need a Windows XP image.
