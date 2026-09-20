# IR-09 backend completion

IR-09 owns the complete IR-to-Wasm backend and the production Tier-1 compiler
infrastructure. Its upstream dependencies (IR-05 through IR-08) still own the
remaining x86 integer/control/system/SIMD/x87 lowering catalogue. Completing
IR-09 therefore means that every HIR/MIR program those packages can legally
produce has one backend/Tier-1 contract; it does not turn their still-Pending
opcode forms into implemented instructions.

## Completed backend pipeline

The backend path is:

```text
shared decoded bytes
  -> HIR SSA + block parameters + effects + StateMaps
  -> Tier-1 canonicalization or Tier-2 optimization
  -> owned MIR + typed local allocation + parallel-copy schedules
  -> reducible MIR structurer
  -> direct Wasm block/loop control
  -> generic MIR pc dispatcher only for irreducible/multi-entry fallback
  -> WasmBuilder encoding
```

No node in `src/rust/ir` calls the legacy `jit_instruction` emitter,
`jit_instructions`, `codegen`, or the legacy `control_flow` structurer.
The IR-core completion gate checks this source dependency permanently.

## General reducible structuring

`backend/structure.rs` operates only on lowered MIR `ControlFlow`.

It:

- builds a deterministic BlockId graph;
- rejects unreachable/multi-entry regions;
- decomposes the graph into strongly connected components;
- accepts a cyclic SCC only when it has one externally reachable header;
- recursively structures nested reducible loops;
- represents forward joins with nested Wasm `Block` structure;
- emits every MIR block exactly once;
- rejects multi-entry SCCs as irreducible instead of duplicating blocks or
  StateMaps.

The resulting tree uses:

```text
BasicBlock(BlockId)
Block(children)
Loop(children)
```

The Wasm emitter consumes that tree with a label map. Each MIR edge still executes
its already-lowered typed parallel-copy schedule before fallthrough or `br`.
StateMap materialization, dynamic retirement accounting, RAM-loop-cache resets,
helper/fault exits and execution-budget polls remain attached to the original MIR
blocks.

The generic pc-local dispatcher is retained only as a correctness fallback for a
graph the structurer cannot prove. It is not a second instruction emitter.

## Locals, phi and exits

HIR block parameters are eliminated by the existing typed local allocation and
MIR edge-copy scheduling:

- simultaneous copies are scheduled before emission;
- cycles use typed scratch locals;
- Effect values are not materialized as Wasm locals;
- StateMap references participate in liveness;
- i32/i64/v128 locals retain type-safe reuse;
- every Exit materializes the exact StateMap/count plan selected during lowering.

The structure tree does not invent another phi representation and does not change
the recovery contract.

## Tier-1 region formation

Automatic IR compilation no longer chooses a region by scanning sequentially
until the first backward/unconditional branch.

`runtime/region.rs` uses the shared decoder to walk reachable direct control
flow inside an immutable bounded snapshot. It follows:

- ordinary fallthrough;
- forward and backward direct jumps;
- both in-window sides of conditional branches.

Calls, indirect/boundary/system stops and direct targets outside the selected
window remain explicit region exits. If a cross-page candidate cannot be captured
without side effects, the selector falls back to a current-page snapshot rather
than performing a guest access.

Tier policy is intentionally bounded:

| Policy | Tier 1 | Tier 2 |
|---|---:|---:|
| configured byte window | 1x, max 960 | 2x, max 1920 |
| reachable decoded instructions | 32 | 96 |
| code dependency pages accepted by compiler | 2 | 8 |

These are compilation budgets, not guest execution limits.

## Tier-1 optimization boundary

Tier 1 and Tier 2 use the same HIR, MIR, local allocator, structurer and Wasm
backend.

Tier 1 performs one low-cost canonicalization round:

- prune;
- merge;
- trivial phi elimination;
- copy propagation.

It deliberately does not run the Tier-2-only families:

- GVN/DCE;
- FLAGS/value state liveness;
- helper-state observation trimming;
- LICM;
- RAM forwarding/loop caches.

Tier 1 uses the verified local allocation from lowering directly. MIR constant
folding, operand-stack fusion and the subsequent local reallocation run only for
optimized Tier 2 compilation, which retains the full configured pass pipeline.

## Fallback boundary

A generic MIR dispatcher is valid for:

- multiple external entries;
- irreducible SCCs with multiple loop headers;
- any graph rejected by the bounded structurer verifier.

Fallback does not invoke the legacy JIT emitter. The same MIR instructions,
parallel copies, StateMaps, memory plans and helper ABI are emitted by the IR Wasm
backend.

## Permanent completion gate

`make ir09-completion-tests` requires:

1. the general structurer unit tests, including nested reducible loops and
   irreducible rejection;
2. Tier-aware region-formation unit tests;
3. the full reachable bytecode CFG Rust corpus;
4. executable CPU CFG differential comparison;
5. a source-independence scan rejecting legacy emitter/control-flow references
   from `src/rust/ir`.

The normal IR-core workflow then continues with the complete native suite,
Wasm/decoder oracles, FLAGS, Tier-2 passes, memory/SIMD/store recovery, IR-12
lifecycle and IR-13 host/browser/Worker/device acceptance.

## Completion boundary

IR-09 completion establishes a complete backend/Tier-1 infrastructure for all
currently lowered IR semantics. It does **not** claim:

- IR-05/06/07 integer/memory/system coverage is already exhaustive;
- IR-08 MMX/SSE/x87 coverage is already exhaustive;
- production Pending encoding forms are zero;
- Windows XP/application acceptance is complete;
- IR is ready to become the default backend.

Those remain explicit gates for the later work packages and IR-13/IR-14.
