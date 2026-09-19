# Five-part IR coverage and backend increment

The five requested implementation areas now have executable code and focused
regressions. Experimental coarse Pending falls from 950 to zero. This does not
complete IR-00 through IR-14: production Pending remains 3,728, default execution
remains legacy, and full prefix/mode/OS/application/performance acceptance is open.

## 1. Control transfers and STI

Immediate/memory far CALL/JMP, RETF, INT3/INT/INTO and IRET use CPU-owned terminal
adapters. Checked completion APIs preserve the existing CPU entry points and
only retire successful instructions. Faults and transfers never restore a stale
pre-call StateMap. Invalid far-pointer register encodings explicitly deliver #UD.

STI uses a privilege-check helper and HIR IF update. A bounded fragment contains
consecutive STIs and the first following instruction; incomplete fragments cannot
publish. A Wasm local tracks successful STI depth. Budget exits are suppressed
inside the shadow; every final exit performs the baseline IRQ checks, including
fault/control/helper exits and nested STI unwind. The fragment never invokes
`run_instruction`. Unsupported or unreadable shadow instructions reject compilation
and retain the existing runtime fallback policy.

## 2. x87 memory and environment

D8–DF memory forms reuse canonical F80 CPU operations, with CR0.EM/TS checks before
segment resolution. Ordered loads, stores, conversions, stack updates and MMIO
behavior follow the baseline; a failed store cannot pop the stack. FLDENV/FSTENV
keep their 14/28-byte layouts, and 32-bit FSAVE/FRSTOR their 108-byte layout.

## 3. MMX, SSE floating point and state

MMX adapters cover arithmetic, packing, shifts, logic, moves, lane operations,
EMMS, MOVNTQ, XMM/MMX transfers and MASKMOVQ. Task guards, MMX tag transitions,
F80 register aliasing, full-range masked-store preflight and observer ordering
remain explicit.

SSE FP adapters cover all 68 variants present in the baseline semantic table:
arithmetic, comparisons, horizontal operations, reciprocal/square-root operations
and integer/FP/packed/scalar conversions. They call value-level CPU semantics,
with explicit source loads and fault guards. Normal returns reload CPU state and
continue executing IR; faults exit without restoring old values.

FXSAVE/FXRSTOR and LDMXCSR/STMXCSR use terminal state adapters. The first pair
retains the baseline x87 guard and 288-byte accessed extent; the second uses the
SSE/MMX guard. Invalid MXCSR is checked before restore.

## 4. Remaining coarse encoding forms

F6/F7 /1 uses the native TEST alias. ARPL, FWAIT, RDRAND and MOVNTI have explicit
adapters; prefetch/fence/no-op and reserved forms preserve their baseline behavior.
The catalogue adds 950 forms across far-control (42), x87 memory (160), FP-state
(8), TEST aliases (12), illegal far-pointer registers (20), SSE FP (272), MMX
(274), remaining baseline forms (160) and STI (2).

Zero experimental Pending is a catalogue classification, not proof that every
prefix, mode, nested subopcode, privilege or fault combination compiles. The
102 existing BaselineUD metadata forms also retain their decoder classification.

## 5. Helper contracts and machine passes

`helper/cpu_registry.rs` centralizes byte-frontend `CallHelper` signatures, effects,
outcomes and exception ownership. Verification rejects unknown frontend names,
forged arity/types, pure effects, exception ownership and incompatible ABIs.
Low-level memory imports still have separate typed contracts.

`HelperAbi::CpuReload` reloads eight GPRs, concrete and raw/lazy FLAGS state, and
eight XMM values only after Normal. These 22 SSA results are loaded from CPU
memory in Wasm, not passed as JS V128 results. Normal must preserve execution
context and retirement count; canonical x87 state stays CPU-owned. Dynamically
valid lazy FLAGS backing survives CFG joins. See [helper contracts](ir-helper-contracts.md).

Memory FP calls also compare their execution context and a non-wrapping dirty/reset
epoch before continuing. Segment/mode/paging/PC changes, cache reset and code writes
force a successful terminal commit. The epoch is deliberately conservative: even
a page-walk A-bit write can end the region. Saturation permanently disables this
continuation fast path. Register-only semantic calls cannot invoke those observers.

Owned MIR now supports bounded operand-stack scheduling and post-rewrite typed
local allocation after the source HIR has been discarded. Scheduling fuses only
adjacent, single-use pure I32/I64 value programs; it never moves reads, helpers,
state observations, faults or CFG edges. Allocation retains recovery uses,
recomputes interference and rebuilds typed parallel edge copies. Budget failure
publishes no partial rewrite. Both passes run in optimized runtime compilation.
This is a conservative first scheduling policy, not unrestricted MIR reordering.

## Baseline restrictions and remaining acceptance

- FBLD and 16-bit FSAVE/FRSTOR remain unsupported: debug asserts, release #UD,
  with #NM taking priority. BOUND/ICEBP and reserved instruction differences
  retain the baseline's debug/release policy.
- Existing task/gate/selector assertions and partial-side-effect limitations
  remain. Checked completion does not implement missing CPU semantics.
- Existing CR4, alignment, rounding, DAZ/FZ and FP exception policies remain.
  There is no new hardware-conformance claim or no-SIMD backend in this increment.
- Full VM86/task-gate/OS workload acceptance, pure-IR XP boot, stable performance
  thresholds, remaining shared-decoder contracts and IR-14 retirement are open.

No instruction adapter fetches/dispatches opcodes, and the IR backend does not
call the legacy JIT emitter. The runtime's established fallback remains available.

## Validation

All new instruction suites compare actual CPU interpretation with IR and check
architectural state, memory/MMIO observations, fault ordering and exact retirement.
Instruction-family variants include optimized/unoptimized machine scheduling and
allocation. Per-build scenario counts (debug / release) are:

| Suite | Scenarios | Make target |
|---|---:|---|
| Far control | 2,016 / 2,016 | `ir-far-control-tests` |
| x87 memory/environment | 3,240 / 3,304 | `ir-x87-memory-tests` |
| FP state | 256 / 256 | `ir-fp-state-tests` |
| SSE FP/conversions | 38,640 / 38,640 | `ir-sse-fp-tests` |
| MMX | 21,196 / 21,196 | `ir-mmx-tests` |
| Remaining forms | 1,160 / 1,608 | `ir-coverage-tests` |
| STI shadow | 1,026 / 1,026 | `ir-sti-tests` |
| Helper continuation | 176 / 176 | `ir-helper-reload-tests` |

STI covers raw/optimized/budget/CFG variants, pending PIC delivery, privilege
faults, nested STI and 13 shadow instruction families. Reload covers straight-line
and CFG variants, loops, dirty GPR/FLAGS/XMM, MMIO mutations and pre/post-helper
faults. MIR-owned tests additionally execute 768 scheduling/allocation comparisons,
including cyclic phi copies, idempotence and transactional budget failure.
Reload also checks segment/paging/mode changes, PC redirection, code mutation,
cache clear and page-walk writes: the helper commits once and the suffix does not run.

`make ir-control-reference-tests` repeats far-control, x87 memory, FP-state and
remaining-form suites with original CPU semantic bodies from
`90f90481d438317a14f8f68c2a81e3d28ae0a11c`. Original control transfers,
FSAVE/FRSTOR, FXSAVE/FXRSTOR and word RMW remain in the reference interpreter;
IR uses checked adapters. `build/control-reference.json` records source/Wasm
hashes. Other components are shared, so this is not an external hardware oracle.

The auto-tier failure-suppression fixture now uses LOCK NOP: the baseline tolerates
the prefix, while IR deliberately rejects it. Its previous FLD/ADDSS inputs now
compile and no longer exercise the intended negative path. New gates are included
in IR-core CI; CI fetches full history for the pinned reference build.

The native suite passes all 213 tests with warnings denied. Production and
experimental Wasm checks also pass with warnings denied. Existing CFG, integer
memory/control, segment/system-stack, x87, FLAGS-observer, IR-10 and standalone
Wasm regressions pass. Live compilation, cache/publication and automatic Tier
lifecycle suites pass in debug, release and production-shaped experimental builds.
The production-default gate is separately checked to
continue rejecting its 3,728 Pending forms.
