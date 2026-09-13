# IR-11: pure loop and packed-value optimizations

This is a **partial implementation of IR-11**, not completion of IR-00–IR-14.
It builds on `ir` at `372ccdc42cc8cb66c61c283295ab7ab673b73f2f` and does not
change the production default, retire the legacy emitter, establish Windows XP
compatibility, or claim an application-level speedup.

## Pipeline and controls

`runtime::compile::compile_inner` runs the following only when
`IrConfig::optimize` is true, the request is `Tier::Two`, and
`PassConfig::rounds != 0`:

1. Simplify pure SIMD expressions (`passes::simd`).
2. Remove bypassed dead expressions, only when the caller enabled DCE.
3. Move pure loop-invariant expressions (`passes::licm`).

This shared path covers standalone, CPU, and CPU-CFG compilation. Tier 1 and
disabled/zero-round configurations skip both new passes. Existing lightweight
passes and MIR folding retain their controls. The existing `PassConfig` fields
are unchanged; `PassStats` adds `loop_hoisted`, `simd_rewritten` and
`simd_eliminated`. These are compiler counters, not JavaScript API statistics
or measured runtime speedups. Nested loops can move one instruction twice.

## Conservative LICM

Natural loops are discovered from dominance-qualified backedges. Latches with
a common header are combined; nested loops are processed inner-first. An
existing unique, unconditional preheader is mandatory. Externally callable
headers, ambiguous preheaders and side-entry/irreducible shapes are skipped.
The CFG and loop-carried parameters are not rewritten.

Only single-result, total expressions on an explicit allowlist may move:
integer arithmetic/comparisons/bit operations, selection/conversions, and
existing pure vector operations. All operands must be external definitions
dominating the insertion point or previously hoisted definitions. Dominance
ordering plus instruction order preserves dependency order even when block
arena numbering is not execution order.

CPU GPR/FLAGS/XMM/segment reads are not treated as pure. Memory, helpers,
division, guards, polls, fault/commit metadata and address operations requiring
separate proofs do not move. StateMaps and guest accounting are not rewritten.
There is **no** inferred RAM, alias, mapping, permission or non-faulting proof:
this does not implement load reuse, store forwarding or memory LICM.

## Pure SIMD simplification

The pass implements two-input byte-shuffle composition and identity removal,
selected idempotent packed integer operations, extract/replace cancellation,
reading a disjoint lane through a replacement, and bypassing an overwritten
lane replacement. A composition requiring three or four independent vectors
is rejected rather than inventing a two-input representation.

Lane widths are part of every rule. In particular, extracting a just-replaced
16-bit lane cannot become its unmasked i32 source: insertion truncated that
source. The executable tests widen the extracted result to expose this error.
Only pure SSA expressions are eligible; memory-form XMM operations, SSE
checks, exception owners and instruction boundaries remain observable.

Canonical value substitution updates instruction operands, edge arguments and
all StateMaps, including recovery-only XMM values. It does not combine guest
instructions or change their architectural retirement count. SIMD arithmetic
on floating-point values, MMX/x87 state and MXCSR are outside this pass.

## Budgets and atomicity

Both passes validate input, plan on a private clone, validate the staged graph
and publish only on success. Any error, including late work-budget exhaustion,
leaves the caller's region unchanged. Fixed arena caps bound verifier/CFG and
copy costs: 64 blocks, 8192 instructions, 16384 values, 8192 states, 1024 helpers.
The default visit budgets are 1,000,000 for LICM and 262,144 for SIMD. These are
bounded search/operand counters, not wall-clock limits or complete accounting
of every existing verifier operation.

## Validation

After the normal generated dispatch tables are available:

```sh
cargo test ir::passes::licm
cargo test ir::passes::simd
node tests/ir/wasm/licm.mjs
node tests/ir/wasm/simd_simplify.mjs
make ir-cfg-tests
```

The LICM corpus performs 41,664 Wasm executions against an independent BigInt
and block-budget oracle, including zero-trip and bounded unfinished loops,
wrapping arithmetic, unchanged FLAGS and exact recovery EIP. Native tests cover
nested/multi-latch/self loops, multi-entry rejection, preheader constraints,
CPU observations and error atomicity.

The SIMD corpus performs 44,544 executions of original and optimized modules,
checking an independent byte/lane oracle and the complete observed CPU-state
buffer. It covers composition, input-count limits, narrow lanes, packed
identities and unchanged input vectors. Native tests also verify recovery-only
value substitution, observation retention and real CPU compiler Tier controls.

The actual CPU CFG differential generator invokes both new passes on optimized
variants. It includes an invariant-register loop with a real preheader and two
packed permutations whose composition is identity. Its interpreter oracle,
precise budget comparisons and second-iteration page-fault cases remain in
place. Existing memory, cache, automatic compilation and public backend suites
are separate regressions; a passing subset is not full system acceptance.

The `IR core` workflow runs native, emitted-Wasm and decoder checks, then a
selected CPU/runtime matrix. Review the run for the exact commit: suite design
and command listings do not by themselves assert a successful execution.

## Remaining work

IR-11 still needs proof-based memory reuse/forwarding, broader loop transforms,
additional SIMD optimization and profitability measurement. The full project
still requires remaining ISA/helper coverage, MIR graph/scheduling work,
complete runtime linking/version management, system and application testing,
Windows XP and performance acceptance, and only then default-backend switching
and legacy retirement. Production Pending coverage labels remain unchanged.
