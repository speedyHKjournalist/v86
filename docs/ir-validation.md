# IR foundation validation — 2026-09-12

Baseline: `8ee73e538daaab15411344d39a1f271e778ac7f3`, initially clean working tree.
Host: Darwin arm64; rustc 1.93.1; Node v25.6.0; NASM/NDISASM 3.01;
`wasm32-unknown-unknown` installed. No new third-party compiler dependency.
The baseline Wasm digest and raw initial A/A samples are in [ir-baseline.json](ir-baseline.json).

## Commands and results

| Command / check | Result |
|---|---|
| Baseline `make build/v86.wasm`, `cargo test`, CPU regression | Built successfully; original 2 Rust tests passed; initial CPU regression passed |
| `make ir-tests` | Passed: 47 warnings-as-errors Rust tests, generated-file check, independent decoder oracle, executable backend tests and register differential |
| `cargo test --release wasmgen::wasm_builder` | 7 passed, including release double-free/signature/unfreed-local rejection and module generation |
| `cargo check --target wasm32-unknown-unknown --features ir-experimental` | Passed |
| `make build/v86-debug.wasm` | Passed |
| `node tests/ir/differential/registers.mjs build/v86-debug.wasm` | Passed on debug CPU artifact |
| `node tests/rust/cpu_optimizations.mjs build/v86-debug.wasm` | Passed after test-harness fix |
| `JIT_LINKS=1 JIT_RMW_CACHE=1 node tests/rust/cpu_optimizations.mjs` | Passed after test-harness fix |
| `CACHE_CONTROL=1 node tests/rust/cpu_plan_sequences.mjs` | 1,653 sequences passed |
| `SEQUENCE_FILTER=nonfloating node tests/rust/cpu_plan_sequences.mjs` | 1,621 sequences passed |
| `JIT_RMW_CACHE=1 SEQUENCE_FILTER="rmw cache" node tests/rust/cpu_plan_sequences.mjs` | 40 sequences passed |
| `make flags-provenance-tests jit-tiers-tests mmx-fast-tests x87-fast-tests` | Passed: 152,064 FLAGS/condition cases in 2,376 programs; tier promotion/SMC/restore; 512 x87 boundary states, 63 MMX exception cases, 576 MMX cases; all 12 x87 controls |
| `node tests/ir/coverage.mjs --require-complete` | Expected rejection: 3,728 production forms remain Pending |
| `git diff --check` | Passed |

`make ir-tests` regenerates executable fixtures through Rust tests. Individual
Node fixture consumers require that generation first. `ir-generated-check` detects
stale generated source/coverage; use `make ir-generated` to update it.

## What the tests establish

- The generated decoder checks more than one million encoding/ModRM/SIB samples
  and all their truncated prefixes. An independent NDISASM oracle checks 16,384
  instruction boundaries with operand/address/segment prefixes in 16/32-bit code.
  Catalogue-length checks are not complete semantic-invalid-encoding verification.
- Wasm tests execute 24 capacity modules at 127/128, 255/256 and 1023/1024 boundaries:
  locals, alternating type groups, long import names/type indices and branch depths.
  They also execute a Wasm-to-Wasm v128 helper and a typed multivalue return.
- Synthetic SSA loops test multiple entry prologues, cyclic parallel copies,
  typed local reuse and cold budget recovery. Verifier negative tests cover
  invalid edge arity/types, non-dominating values, use before definition, broken
  effects, helper outcome ownership and uninitialized SSA.
- 296 x86 register programs × 6 inputs = 1,776 cases compare unoptimized IR,
  optimized IR, the interpreter and a warmed legacy JIT workload. The latter
  retains legitimate interpreter exits; the test proves compiled execution
  occurred, not that every guest instruction executed in the legacy JIT.
  A separate BigInt oracle verifies the single-instruction arithmetic subset.
- 4,096 terminal Jcc executions check all conditions before/after optimization,
  FLAGS preservation, committed count and 16-bit target wrapping.
- DCE retains recovery-only values and ordered helpers. Allocation budget excess
  returns a compile error. Artifact checks reject changed job/reset/slot/page versions.
  These are compiler-side checks, not live asynchronous publication stress tests.

## Baseline failure diagnosed and fixed in the test harness

The original CPU regression intermittently failed `byte-loop partial writes` on
both the pinned baseline and the changed builder. The first 16 output bytes were
a guest #PF frame: `[2, 0x387018, 8, 3]` as little-endian words.

The preceding cached-POP test could stop asynchronously after entering its
borrowed stack page. On replay, `mov ebp,esp` then captured that temporary stack
as the original, and the next test's exception frames overwrote the copy buffer.
The test now loads a fixed captured original stack into EBP and restores host-visible
ESP after stopping. Fault-EIP and partial-write assertions remain intact.
No CPU exception/copy semantics changed. The fixed harness passed 10 baseline
and 10 builder runs; linked/RMW and debug runs also passed.

The first combined `make cpu-plan-tests ...` invocation failed before this fix.
Its three sequence commands had passed; the linked CPU command was subsequently
rerun successfully with the fix. The other regression targets were run separately.

## Logs, performance and unexecuted gates

Detailed local logs/artifacts are under ignored `build/ir-*`, including
`ir-validation.log`, `ir-release-builder-tests.log`, `ir-integer-differential.log`,
`ir-debug-differential.log`, `ir-other-regressions.log`, `ir-builder-repeat.json`
and `ir-harness-repeat.json`. Fixtures are reproducible; build outputs are not
part of the committed source set.

The initial performance capture ran the same baseline artifact on both sides
while other work was active. Its variability is recorded, and it cannot support
an IR speedup claim. Controlled compile-cost, memory, cold/hot browser and
end-to-end workload measurements have not been completed.

Not run as a full matrix: NASM/QEMU/KVM/expect, JIT paging, full OS boots, API/device
and browser/worker suites, XP/Everest, WZ/RHO loading, game maps, graphics/input/audio.
The workspace contains XP images/snapshots; no compatibility or performance result
for them is asserted. The first real CPU MOV memory/#PF/#GP/MMIO cases now pass (see the follow-up
below); REP, other system/FP and the full fault matrix remain pending. See [ir-progress.md](ir-progress.md).

## Shared analyzer integration follow-up

`make ir-tests ir-analyzer-tests` passed after adding the production adapter.
The dedicated `build/v86-ir-test.wasm` includes `ir-test-hooks`; normal artifacts
have no test exports. A 2,670,035-record corpus compares the original generated
analyzer with shared decoding and the actual production adapter. It checks
consumed length, packed prefixes, no-next/absolute/conditional flow, helper block
boundaries and ModRM base/index/scale/displacement/segment/address width. It also
covers conflicting/repeated prefixes on every catalogue opcode in both modes.
Tests check snapshot refusal for MMIO/out-of-RAM addresses, a missing next page,
and preservation of real CPU EIP.

The corpus exposed and fixed two earlier decoder omissions: non-custom ModRM
helper boundaries and the family's unconditional ModRM fetch before selecting
invalid unprefixed 0F 7C/7D. Focused regression cases accompany the corpus.

Logs: `build/ir-shared-decoder-tests.log`, `build/ir-analyzer-differential.log` and
`build/ir-shared-decoder-regressions.log`. This validates compiler analysis,
not complete IR guest memory, system, FP or exception execution.

The shared-analyzer production change also passed `make cpu-optimization-tests
flags-provenance-tests jit-tiers-tests`, including 152,064 FLAGS conditions,
precise-fault/partial-write checks and tier promotion/SMC/save-restore. An export
inspection confirmed no `ir_test_` functions in the normal release artifact.

## Executable helper ABI follow-up

`cargo test ir::helper_tests` and `node tests/ir/wasm/helpers.mjs` passed: two
Rust fixture/rejection tests and 42 Wasm executions, optimized and unoptimized.
They check state observation, narrow return normalization, caller-owned restore
and single delivery, helper-owned transfer, yield, invalidation, invalid-outcome
traps, and fault snapshot/result local reuse. Instrumented callees model control
state changes; this does not establish guest exception delivery through real CPU
adapters. `make ir-tests` includes these fixtures and executions.

The complete `make ir-tests` run passed with 23 Rust tests, 16,384 independent
NDISASM boundaries, all Wasm backend fixtures, 4,096 terminal Jcc executions and
1,776 register/FLAGS cases against both interpreter and warmed legacy execution.
`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed.
Logs: `build/ir-helper-suite.log` and `build/ir-helper-final.log`.

## Real CPU memory follow-up

`make ir-tests ir-memory-tests` passed with 25 Rust tests and the existing
standalone Wasm/NDISASM/register differential suite. The new real CPU run compares
192 MOV memory cases with exact interpreter steps, confirms 32 warm native RAM
paths, delivers and compares 20 real #PF/#GP cases, and compares 12 MMIO callback
sequences including FLAGS/current EIP observation. It checks one exception frame,
fault EIP after a preceding INC, cross-page write atomicity, non-zero CS exits,
lazy FLAGS, materialization accounting, and exit after self-modifying alias
stores. A CPL3 negative guard test intercepts the slow path; a full ring3 fault
matrix has not been run. Both optimization settings are covered.

The tests use the dedicated `ir-test-hooks` CPU artifact. Production Tier 1/Tier 2
remain legacy. Details and limits: [ir-memory.md](ir-memory.md). Logs:
`build/ir-memory-suite.log`, `build/ir-memory-differential.log`.

The final fault run additionally compares both zero and non-zero CS bases (20
real fault cases total). Normal release exports were inspected: no experimental
CPU entry/memory adapters or test hooks are present.

## Integer memory and RMW follow-up

`make ir-tests ir-memory-tests` passed with 26 Rust tests, including rejection of
uncommitted/mismatched/effect-invalidated RMW tickets. The expanded CPU run passes
2,040 integer memory cases (170 instruction forms, optimized/unoptimized), 340
instrumented native warm RAM paths, 36 real fault cases and 340 MMIO callback/state
comparisons. Additional comparisons cover remapping during a device read and
noncontiguous physical pages. Permission failure must prevent all MMIO reads;
false CMOV still faults on inaccessible memory. Fault CR2, frame, GPRs, FLAGS,
EIP and memory are compared with exact interpreter steps. The initial CR2 mismatch
on cross-page dword RMW was fixed to match the baseline's aligned second lookup.

Logs: `build/ir-rmw-suite.log`, `build/ir-rmw-differential.log`. The other full-plan
limits above remain; this is not full instruction coverage or online IR Tier 1.

## PUSH/POP follow-up

`make ir-tests ir-memory-tests ir-stack-tests` passed with 27 Rust tests and both
real CPU differential suites. New stack coverage: 864 normal comparisons, 432
instrumented native warm paths, 128 SP-wrap/high-ESP cases, 100 real faults and
80 MMIO callback/state comparisons. Modes cover 16/32-bit decoding, operands and
stack widths. Ring3 PUSH faults use a real TSS and compare the saved user ESP and
single kernel exception frame. POP memory segment faults compare the baseline's
temporary-ESP and post-delivery adjustment; 5C/8F POP SP high-half behavior is
separately preserved. See [ir-stack.md](ir-stack.md) for limits.

Logs: `build/ir-stack-suite.log`, `build/ir-stack-differential.log`.

## Near control follow-up

The new `ir-control-tests` suite passes 1,632 near-control comparisons, 816 warmed
runs without slow memory calls, 48 loaded-target edge cases, 200 real instruction
faults, six faults on the subsequent target fetch and 192 MMIO comparisons.
CALL target/return-slot overlap, CS-relative return addresses, RET imm16 unsigned
adjustment and ring3 CALL stack-write faults are included. A standalone Wasm test
also executes a computed EIP used only by StateMap after DCE; verifier negatives
reject wrong-type or pre-instruction dynamic EIP maps. See [ir-control.md](ir-control.md).

Logs: `build/ir-control-differential.log`, `build/ir-control-suite.log`.


## Multiple-stack instruction extension

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests` passed after adding
PUSHA/POPA, LEAVE, permission preflight and partial-store verification. Full output
is in the local `build/ir-multi-stack-suite.log`; all 30 Rust tests and the existing
backend/register/memory/control regressions passed.

The stack suite now executes 120 generated fixtures: 960 ordinary comparisons,
480 warm data-memory paths, 128 existing wrap cases, 100 existing fault cases,
and 128 MMIO comparisons. New focused checks add 72 preflight/LEAVE faults,
16 skipped-SP device checks, 144 multiple-stack wrap cases, 8 self-alias PUSHA
exits, and 32 callback-driven remaps of later stack pages. These compare all GPRs,
FLAGS, EIP, fault frames and memory against actual interpreter instruction steps.
The permission preflight itself still calls the audited CPU MMU adapter on warm
runs; the no-slow-call assertion applies to data-memory accesses.

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-multi-stack-check.log`), generated catalogue checks and `git diff --check`
passed, and the rebuilt normal `build/v86.wasm` exports no experimental `ir_` names.
This does not prove production IR publication, ENTER or the full OS/performance
acceptance matrix. Production Pending remains 3,728.


## Integer shifts, rotates and retained FLAGS operand

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests`
passed (`build/ir-shifts-suite.log`). All 32 Rust tests ran with warnings as errors;
existing native backend, register, memory, stack and control differentials passed.
The new 1,632 shift fixtures ran 746,688 ordinary CPU comparisons, also checked
against a BigInt bit-serial oracle. Coverage includes every CL byte value for
register forms, with 50,208 confirmed warm native RMW executions.

The focused matrix passed 2,976 real faults, 672 MMIO comparisons, 672 lazy-FLAGS
slow-memory cases, and 4,032 in-region / IR-to-interpreter arithmetic-to-shift
sequences. The latter validate `last_op1` state across 8/16/32-bit arithmetic and
high-byte aliases. Standalone provenance and invalid type/layout cases passed.
Precise count and undefined-flag policies are in [ir-shifts.md](ir-shifts.md).

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-shifts-check.log`); normal Wasm exports no `ir_` symbols, generated files
are current, and `git diff --check` passes. Production Pending remains 3,728;
full guest ISA, online IR tiers, OS and performance acceptance remain incomplete.


## Native wide multiply/divide and i64 backend

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests` passed (`build/ir-multiply-suite.log`): all 35 Rust tests with
warnings as errors, generated decoder checks, native backend execution and all
existing register/memory/stack/control/shift differentials passed.

New multiply/divide fixtures passed 129,500 successes and 16,900 exactly-once #DE
cases against BigInt and real CPU interpreter steps, with 29,280 confirmed warm
source reads. Additional checks passed 688 source-fault precedence cases, 240
MMIO reads, 240 lazy-FLAGS paths, 24 ring3/TSS #DE frames, 244 completed-prefix
counts and 12 standalone i64 phi-copy / mixed-width helper ABI executions.
See [ir-multiply.md](ir-multiply.md) for exception and ABI contracts.

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-multiply-check.log`). The rebuilt normal Wasm exports no experimental
`ir_` symbols. Generated files and `git diff --check` pass. Production coverage
still has 3,728 Pending forms; this does not establish full IR tiers, ISA coverage,
OS compatibility or performance acceptance.


## Bit operations and scalar bit counts

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests` passed (`build/ir-bits-suite.log`). All 37 Rust tests
ran with warnings as errors; every existing native backend and CPU differential
continued to pass after the bit-operation extension.

New results: 54,720 ordinary comparisons, 26,720 warm native memory paths,
480 real faults, 168 MMIO comparisons, 168 lazy-FLAGS cases and 384 signed-index /
page / address16 boundaries. BSF/BSR/POPCNT additionally passed 393,216 exhaustive
16-bit cases and 768 dword bit-basis/complement cases. The scalar count backend
passed 28 I32/I64 zero/full-width and folded/unfolded executions. The suite checks
one-byte bit-string accesses and mandatory POPCNT decoding explicitly.

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-bits-check.log`). Normal Wasm exports no experimental `ir_` adapters,
generated files are current and `git diff --check` passes. Production Pending is
still 3,728; full ISA, online IR tiers and OS/performance acceptance remain open.
See [ir-bits.md](ir-bits.md) for the precise compatibility policies.


## Exchange and audited LOCK pairs

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests` passed
(`build/ir-exchange-suite.log`): all 39 warnings-as-errors Rust tests and the
complete existing executable backend / CPU differential suites passed.

New results: 48,384 exchange/alias/FLAGS comparisons, 20,736 warm native paths,
5,616 real faults, 1,296 MMIO observation comparisons, 864 cross-page successes,
and 576 LOCK arithmetic/unary/bit-family comparisons. Shared-memory import
rejection and synchronous read/write scheduling checks also passed. Scope and
assumptions are explicit in [ir-exchange.md](ir-exchange.md).

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-exchange-check.log`), normal CPU Wasm exports contain no IR adapters,
generated files are current, and `git diff --check` passes. Production Pending
remains 3,728; the full IR-00–IR-14 goal remains incomplete.


## ENTER, interleaved partial accesses and pinned fault behavior

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests` passed
(`build/ir-enter-suite.log`): all 41 warnings-as-errors Rust tests and every
existing executable backend / CPU differential continued to pass.

ENTER added 1,600 CPU/pointer-model comparisons, 800 warm native memory paths,
384 wrap/frame-alias cases, 192 MMIO comparisons, 32 callback remaps and 16
self-alias exits. Forty-eight real ring3 faults compare exact progress and TSS
frames; 32 additionally compare the pinned host abort after fault delivery.
Expected caught Rust panic diagnostics appear in the baseline fault log.
The complete ENTER oracle uses a release CPU because the pinned debug CPU has a
separate raw-word assertion. The scope and both explicit compatibility policies
are documented in [ir-enter.md](ir-enter.md); debug assertion equivalence is not
claimed for high-ESP native ENTER16.

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-enter-check.log`). Normal CPU exports have no IR adapters; generated
files and `git diff --check` pass. Experimental CpuStackHIR coverage is 104 forms;
production Pending remains 3,728. The full implementation goal is still active:
remaining ISA, full MIR/tiers, online publication/invalidation, OS/performance
acceptance and legacy emitter retirement are not established by these tests.


## Scalar conversions, FLAGS/BCD and implicit addresses

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests ir-misc-tests`
passed (`build/ir-misc-suite.log`): all 43 warnings-as-errors Rust tests and all
existing executable backend / CPU differentials passed.

New scalar results: 88,032 ordinary cases with independent and CPU oracles,
3,292,160 exhaustive inputs and 378 composed regions. The DAA/DAS undefined OF
policy is independently checked; only that bit is masked in the corresponding
CPU comparisons, as documented in [ir-misc.md](ir-misc.md). Other scalar flags,
registers, EIP and retained operand values are compared without that mask.

Implicit-address memory added 3,136 cases, 1,184 instrumented native paths,
1,048 real #PF/#GP faults and 392 MMIO observation comparisons. AAM zero-base
checks include 84 ordinary #DE cases and 16 ring3/TSS fault frames. AAM faults
and memory faults compare complete FLAGS with no undefined-bit masking.

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-misc-check.log`). Normal CPU Wasm has no experimental IR exports;
generated files and `git diff --check` pass. Experimental coverage is now
838 NativeHIR, 614 CpuMemoryHIR, 104 CpuStackHIR, 28 CpuControlHIR,
14 CpuArithmeticHIR and 136 TerminalBranchHIR forms. Production Pending remains
3,728. Full ISA, online tiers/publication/invalidation, OS/performance acceptance
and old-emitter retirement remain incomplete.


## Counter branches and coverage evidence paths

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests ir-misc-tests
ir-loop-tests` passed (`build/ir-loop-suite.log`): all 45 warnings-as-errors Rust
tests and every existing executable backend / CPU differential passed after
extracting the shared Jcc/JMP/counter-branch frontend.

New results: 138,240 CPU comparisons, 69,120 standalone reference executions,
1,048,576 exhaustive CX cases, 432 composed counter/FLAGS regions and 64 real
following-target #PF comparisons after branch commit. Full semantics and the
separate remaining internal-cycle scope are in [ir-loops.md](ir-loops.md).

Coverage suite paths now identify the relevant semantic fixture and executable
consumer for each instruction family. After regenerating that metadata,
`make ir-coverage` passed, including attributed-path existence checks. This is
navigation to evidence, not an assertion of exhaustive per-form acceptance.
The refreshed [coverage report](ir-coverage.md) records all current categories.

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-loop-check.log`); normal CPU exports contain no IR adapters, generated
files are current and `git diff --check` passes. TerminalBranchHIR has 152 forms;
production Pending remains 3,728. Full ISA, online regions/tiers/invalidation,
internal CPU loops and scheduling, OS/performance acceptance and legacy emitter
retirement remain incomplete.


## FLAGS/segment stacks and terminal CPU helper state

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests ir-misc-tests
ir-loop-tests ir-system-stack-tests` passed (`build/ir-system-stack-suite.log`):
all 47 warnings-as-errors Rust tests and all prior executable/CPU differential
suites passed with the extended helper ABI and typed selector reads.

New results include 864 ordinary comparisons, 432 native operand paths, 416
wrap/stack-width cases, 208 stack/descriptor MMIO comparisons, 480 selector cases,
104 ring3 stack #PF cases and 64 CPL/IOPL mask cases. Forty-four page boundaries
verify the asymmetric segment PUSH/POP widths. Eighty descriptor #PF cases
include the pinned post-delivery accessed-bit write abort; expected caught panic
diagnostics appear in the log.

Mode/control evidence adds 156 real-mode cases, 32 VM86 permission/mask cases,
16 immediate IF-enable IRQ deliveries (including real TSS transitions), and
24 injected terminal-helper outcomes. The caller preserves all authoritative
post-call state and rejects Normal/invalid terminal returns. Exact ownership and
remaining scheduling scope are in [ir-system-stack.md](ir-system-stack.md).

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-system-stack-check.log`); normal CPU Wasm exports no experimental IR
adapters. `make ir-coverage`, generated-file checks and `git diff --check` pass.
CpuStackHIR now has 156 forms; production Pending remains 3,728. Full ISA, normal
state-reloading helpers, online regions/tiers/invalidation, OS/performance
acceptance and old-emitter retirement remain incomplete.


## Segment transfers, far pointer tails and descriptor commit

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests ir-misc-tests
ir-loop-tests ir-system-stack-tests ir-segment-tests` passed
(`build/ir-segment-suite.log`): all 49 warnings-as-errors Rust tests and all
existing executable/CPU differential suites passed with LinearOffset and the
shared frontend helper constructor.

The new suite passed 9,344 ordinary comparisons, 3,264 native operand paths,
816 MMIO observations, 1,800 selector cases, 3,104 real ring3 operand faults,
1,952 page/address16 boundaries, 640 device-driven selector-page remaps, and
624 descriptor read/accessed-bit write faults. It independently checks CR2
addresses, starts each execution with a CR2 sentinel, and compares complete
architectural/fault state. Expected caught post-delivery baseline unwrap panics
are retained in descriptor-write fault cases.

Mode evidence adds 612 real-mode and 1,224 VM86 cases with IOPL zero/three.
The second far-pointer read uses its own translation after a linear offset;
GPR commit follows successful descriptor validation. Details and the remaining
online scheduling scope are in [ir-segments.md](ir-segments.md).

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-segment-check.log`). Normal CPU Wasm exports no experimental IR
adapters; generated coverage and `git diff --check` pass. The catalogue now has
618 CpuMemoryHIR forms and 28 CpuStateHIR forms; production Pending remains
3,728. Full ISA, online tiers/regions/invalidation, OS/performance acceptance and
legacy emitter retirement remain incomplete.


## Single-iteration strings and operand ordering

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests ir-misc-tests
ir-loop-tests ir-system-stack-tests ir-segment-tests ir-string-tests` passed
(`build/ir-string-suite.log`): all 51 warnings-as-errors Rust tests and every
existing executable/CPU differential suite passed with the new string frontend.

New evidence includes 3,360 ordinary CPU comparisons, 1,680 confirmed native data
paths, 1,200 complete-FLAGS operand patterns, 240 MMIO observations, 264 data #PF
cases and 2,040 null-segment cases. The segment tests corrected the SCAS lowering
to use its wrapper's fixed ES argument rather than an ordinary source override.

Further results: 720 page/address16 boundaries, 96 MMIO-driven destination remaps
or delayed faults, 112 ring3 second-page faults through a kernel TSS stack,
72 physical-alias/overlap copies and 48 stores aliasing following instruction
bytes. Real mode and VM86 each add 120 comparisons. Exact single-iteration
commit order and the distinct remaining REP scope are in
[ir-strings.md](ir-strings.md).

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-string-check.log`). Normal CPU Wasm has no experimental IR exports;
generated coverage and `git diff --check` pass. CpuStringHIR records 30 non-REP
coarse forms; repeat-prefix variants remain unsupported. Production Pending
remains 3,728. REP/IO, remaining ISA, full MIR/regions, online tiers/invalidation,
OS/performance acceptance and old-emitter retirement remain incomplete.


## Port I/O, permissions and normal CPU helper observations

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests ir-misc-tests
ir-loop-tests ir-system-stack-tests ir-segment-tests ir-string-tests ir-io-tests`
passed (`build/ir-io-suite.log`): all 53 warnings-as-errors Rust tests and all
existing executable/CPU differential suites passed after changing BeforeInstruction
CPU helper preparation to expose decoded next IP for normal returning calls too.
Standalone helper behavior and caller-owned fault restoration remain unchanged.

New I/O evidence includes 6,336 CPU/device comparisons, 1,008 native OUTS source
paths, 2,688 CPL/IOPL/VM86 cases, 480 per-byte bitmap checks, 272 string fault-order
cases and 720 TSS form/limit/header/bitmap faults. The device suite found and
verified the normal CPU helper IP preparation correction through real TSS MMIO.

Additional results: 144 combined TSS/bitmap/data MMIO cases, 48 INS remaps or
write faults after the port read, 288 DF/page/address16 boundaries and 144 real-mode
comparisons. INS destination writes use the audited safe-write adapter, not a
claimed native fast path. See [ir-io.md](ir-io.md) for exact ownership, observations
and the remaining repeat/scheduling scope.

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-io-check.log`). Normal CPU Wasm exports no experimental IR adapters;
generated coverage and `git diff --check` pass. CpuIoHIR adds 36 coarse forms;
production Pending remains 3,728. REP, remaining ISA, full MIR/regions, online
tiers/invalidation, OS/performance acceptance and emitter retirement remain
incomplete.


## Bounded REP semantic engine and independent legacy reference

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests ir-misc-tests
ir-loop-tests ir-system-stack-tests ir-segment-tests ir-string-tests ir-io-tests
ir-rep-engine-tests` passed (`build/ir-rep-engine-suite.log`): all 53 warnings-as-errors
Rust tests, all prior executable/CPU suites, the isolated reference build and new
REP engine differential passed after refactoring the shared CPU string executor.

The independent reference uses the pinned baseline string function body while
sharing current surrounding CPU code. It is rebuilt from local Git history in an
isolated directory; `build/rep-reference.json` records provenance and hashes.
The new suite passed 1,344 old/new legacy and explicit-engine comparisons, 84
complete device sequences, 252 partial/fault states, 588 budget cases and 84
bounded continuation sequences. Additional cases cover 288 early terminations,
84 maximal unsigned counts, 84 zero-count precedence cases, 42 page reentry faults,
84 ambiguous-EIP faults, 144 LZ/physical aliases, 24 I/O permission faults and
504 source overrides/fixed ES forms. See [ir-rep-engine.md](ir-rep-engine.md).

Because this change touches the shared production executor,
`node tests/rust/cpu_optimizations.mjs` also passed on the rebuilt normal CPU
(`build/ir-rep-engine-legacy.log`), including real-JIT code invalidation, cached
MMIO/batched partial write faults and overlapping REP partial progress at #PF.

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-rep-engine-check.log`); normal CPU exports contain no experimental IR
hooks, generated coverage is current and `git diff --check` passes. The semantic
engine reports element work without changing instruction_counter. REP HIR,
StateMap/scheduling accounting and online integration remain pending; no REP
coverage count was promoted. Production Pending remains 3,728, and the full
IR-00–IR-14 objective is incomplete.


## REP HIR progress, bounded entries and final instruction commit

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests ir-misc-tests
ir-loop-tests ir-system-stack-tests ir-segment-tests ir-string-tests ir-io-tests
ir-rep-engine-tests ir-rep-tests` passed (`build/ir-rep-suite.log`): all 56
warnings-as-errors Rust tests, all existing executable/CPU suites, the independent
engine reference and the new REP HIR suite passed with CpuRep and validated
aliased progress StateMaps.

New evidence includes 14,112 CPU/observer comparisons, 672 bounded reentry
sequences (11,424 elements, one final instruction commit per sequence), 672
zero-budget/count and result-reset cases, 336 full device sequences, 1,176
fault/page-reentry states and 384 comparison terminations across budget cuts.
Further checks cover 336 maximal counters, 336 zero-count completions before
invalid operands, 96 I/O permission faults, a CompileRequest budget fixture and
12 injected terminal-helper outcomes. Exact accounting and online reentry limits
are documented in [ir-rep.md](ir-rep.md).

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-rep-check.log`). Normal CPU Wasm exports no experimental IR hooks;
generated coverage and `git diff --check` pass. The catalogue adds 84 CpuRepHelper
forms from explicit F2/F3 encoding rows. The older string report's statement that
repeat prefixes had no separate coarse rows was corrected after checking the
actual opcode catalogue. Production Pending remains 3,728. Full ISA, general
CPU regions/loops, online scheduling/tiers/invalidation, OS/performance acceptance
and legacy-emitter retirement remain incomplete.

## CPU identification, deterministic timestamps and MSR state

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests ir-misc-tests
ir-loop-tests ir-system-stack-tests ir-segment-tests ir-string-tests ir-io-tests
ir-rep-engine-tests ir-rep-tests ir-cpu-info-tests` passed
(`build/ir-cpu-info-suite.log`): all 58 warnings-as-errors Rust tests and every
listed executable/CPU differential suite passed. The new suite runs independently
in debug and release CPUs, with optimized and unoptimized artifacts.

Per build, new evidence includes 3,920 CPUID leaf/subleaf/ACPI comparisons, 2,870
recognized MSR cases, 224 timestamp/interpolation cases, 280 privilege/VM86 checks,
28 real-mode cases, 42 configured CPUID levels and four persistent six-instruction
TSC sequences. Sixteen unknown-MSR and sixteen restricted-APIC tests preserve
build-specific abort/release behavior. Expected debug assertion messages are
caught and checked; they are not test failures. The clock is deterministic only
in the test instance, with exact callback observations and internal TSC state.
Shared-body integration evidence and independent assertions are distinguished in
[ir-cpu-info.md](ir-cpu-info.md).

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-cpu-info-check.log`). Normal CPU exports contain no experimental IR
hooks, coverage generation/checks pass and `git diff --check` passes. Coverage
adds eight CpuInfoHelper forms; production Pending remains 3,728. Online CPU
tiers/invalidation, full ISA/MIR/regions, advanced passes, XP/application/performance
acceptance and legacy-emitter retirement remain incomplete.

## System mode transfers, privilege state and HLT observations

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests ir-misc-tests
ir-loop-tests ir-system-stack-tests ir-segment-tests ir-string-tests ir-io-tests
ir-rep-engine-tests ir-rep-tests ir-cpu-info-tests ir-cpu-system-tests` passed
(`build/ir-cpu-system-suite.log`): all 60 warnings-as-errors Rust tests and every
listed executable/CPU differential suite passed.

The new SYSENTER/SYSEXIT/HLT/CLI/CLTS/WBINVD suite passes 672 ordinary context
comparisons, 3,024 permission/VME cases, 504 selector/FLAGS cases, 168 real-mode
cases, 252 target EIP/ESP boundaries, 56 post-transfer target-fetch faults, 112
HLT halt/timer/PIC observer sequences, 56 successful transfers without descriptor
access and 84 exception-delivery IDT/GDT/TSS MMIO sequences. Both IR variants
are checked. Expected VM86/VME dispatcher panics are caught and compared with the
baseline; the log messages are not test failures or new VME support. HLT's time
and device stimuli are controlled by test-instance imports; production imports
and clock/device policy are unchanged. See [ir-cpu-system.md](ir-cpu-system.md).

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-cpu-system-check.log`). Normal CPU exports contain no experimental IR
hooks; generated coverage, coverage attribution and `git diff --check` pass. The
catalogue adds twelve CpuSystemHelper forms. Production Pending remains 3,728.
STI shadow, remaining system/ISA coverage, full MIR/regions, online tiers and
scheduling/publication/invalidation, advanced optimization, XP/performance
acceptance and legacy-emitter retirement remain incomplete.

## Control/debug register state and mapping transitions

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests ir-misc-tests
ir-loop-tests ir-system-stack-tests ir-segment-tests ir-string-tests ir-io-tests
ir-rep-engine-tests ir-rep-tests ir-cpu-info-tests ir-cpu-system-tests
ir-control-regs-tests` passed (`build/ir-control-regs-suite.log`): all 62
warnings-as-errors Rust tests and all listed executable/CPU suites passed.

The CR/DR suite generates 5,120 fixtures. Per debug/release build, evidence includes
7,680 ordinary transfers, 1,792 permission/VM86 cases (including CPL priority over
DR alias faults), 64 DR4/5 DE faults, 128 invalid-CR cases, 64 CR4 bit cases, 168
real-mode cases, 14 warmed TLB mapping checks, 60 RAM/MMIO PDPTE loads/partial
aborts, 96 PDPTE bit cases, 18 pinned CR0/CR3 boundary cases and four successful
CR3 commits followed by next-fetch #PF. Both optimized and unoptimized artifacts
are compared. Expected debug assertions and the existing PG-without-PE panic are
caught and checked, not counted as suite failures. Shared-body integration evidence
and independent mapping/state assertions are distinguished in
[ir-control-regs.md](ir-control-regs.md).

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-control-regs-check.log`). The normal CPU exports no experimental IR
hooks; generated coverage/attribution and `git diff --check` pass. The catalogue
adds eight CpuControlRegHelper forms, with ignore_mod remaining register-only.
Production Pending remains 3,728. Full remaining ISA/system semantics, STI shadow,
MIR/regions, online tier/scheduling/publication/invalidation, advanced passes,
XP/performance acceptance and legacy-emitter retirement remain incomplete.

## Descriptor tables, machine-status word and INVLPG

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests ir-misc-tests
ir-loop-tests ir-system-stack-tests ir-segment-tests ir-string-tests ir-io-tests
ir-rep-engine-tests ir-rep-tests ir-cpu-info-tests ir-cpu-system-tests
ir-control-regs-tests ir-descriptor-tests` passed (`build/ir-descriptor-suite.log`):
all 64 warnings-as-errors Rust tests and all listed executable/CPU suites passed.

The new suite generates 2,520 fixtures. Each debug/release build passes 5,040
ordinary comparisons, 280 permission/VM86/illegal-register cases, 448 memory
boundary/fault cases, 560 segment-before-helper checks, 168 linear address tails,
48 base masks, 56 MMIO sequences, 32 remaps, 32 late faults/aborts, 16 delayed
table-field observations, 224 real-mode cases and 16 warmed/global INVLPG checks
with neighboring TLB retention. Both optimized and unoptimized artifacts execute.
Expected unwrap panics after late store faults preserve the pinned CPU behavior
and are caught and checked. Details and the distinction between CPU semantic
helpers and native memory lowering are in [ir-descriptor.md](ir-descriptor.md).

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-descriptor-check.log`). Normal CPU exports contain no experimental IR
hooks; generated coverage/attribution and `git diff --check` pass. Coverage adds
56 CpuDescriptorHelper forms, including 20 explicit invalid-register #UD forms
not tagged reg_ud by the source opcode table. Production Pending remains 3,728
and BaselineUD remains 102. Remaining ISA/system semantics, full MIR/regions,
online tiers/scheduling/publication/invalidation, advanced passes, XP/performance
acceptance and legacy-emitter retirement remain incomplete.

## Task/LDTR registers and checked LTR completion

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests ir-misc-tests
ir-loop-tests ir-system-stack-tests ir-segment-tests ir-string-tests ir-io-tests
ir-rep-engine-tests ir-rep-tests ir-cpu-info-tests ir-cpu-system-tests
ir-control-regs-tests ir-descriptor-tests ir-task-regs-tests` passed
(`build/ir-task-regs-suite.log`): all 66 warnings-as-errors Rust tests and every
listed executable/CPU suite passed. The finalized task-register target passed
again (`build/ir-task-regs-final.log`) after splitting its full descriptor-attribute
abort matrix into fresh CPU instances. An isolated replay of the previously
observed long-lived-instance mismatch also passed (`build/ir-task-regs-isolated.log`).

Per debug/release build, the independent pinned LTR/LLDT reference checks 1,440
ordinary cases, 128 mode/privilege cases, 128 operand faults, 160 segment checks,
160 null/outside-table cases, 640 attribute cases across width slices, eight TI
cases, 32 MMIO sequences, 48 descriptor read/readonly cases, 16 busy-write
fault/aborts, 32 remaps and 32 physical-tail/busy-write translations. The main
suite additionally repeats the canonical 80-case attribute subset. Expected host
panics are checked with precise partial state, not converted into guest faults.
Reference provenance and scope are recorded in `build/task-reference.json` and
[ir-task-regs.md](ir-task-regs.md).

Because load_tr now delegates to a checked core while retaining its original
void ABI, `node tests/rust/cpu_optimizations.mjs` also passed on the rebuilt normal
CPU (`build/ir-task-regs-legacy.log`), including real-JIT invalidation, MMIO/REP
partial faults, FLAGS/IRET and SSE checks.

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-task-regs-check.log`). Normal CPU exports contain no experimental IR
hooks; generated coverage/attribution and `git diff --check` pass. Coverage adds
32 CpuTaskRegHelper forms. Production Pending remains 3,728. Remaining ISA/system
semantics, full task switching and MIR/regions, online tiers/scheduling/lifecycle,
advanced optimization, XP/performance acceptance and legacy retirement remain open.

## LAR/LSL queries and raw-ZF observer evidence

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests ir-misc-tests
ir-loop-tests ir-system-stack-tests ir-segment-tests ir-string-tests ir-io-tests
ir-rep-engine-tests ir-rep-tests ir-cpu-info-tests ir-cpu-system-tests
ir-control-regs-tests ir-descriptor-tests ir-task-regs-tests ir-selector-query-tests`
passed (`build/ir-selector-query-suite.log`): all 68 warnings-as-errors Rust tests
and all listed executable/CPU suites passed, including the independently pinned
LTR/LLDT matrix in fresh instances.

Per debug/release build, LAR/LSL evidence includes 1,920 ordinary cases, 64 mode
checks, 320 invalid selectors, 8,192 independent descriptor/privilege result cases,
32 source faults, 64 descriptor faults with post-fault destination/ESP writes, 80
segment checks, 64 MMIO sequences, 32 post-source old-value captures and 64 LDT
lookups. Both optimized/unoptimized artifacts are checked against the original
CPU query bodies; [ir-selector-query.md](ir-selector-query.md) records exact scope.

`make ir-flags-observer-tests` separately passed eight VERR/VERW baseline
diagnostics (`build/ir-flags-observer-tests.log`). They establish raw-versus-computed
ZF in descriptor-fault frames after INC. At this stage VERR/VERW remained unsupported
until the backing-state work recorded below; these differences are not masked
or treated as undefined outputs.

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-selector-query-check.log`). Normal CPU exports contain no experimental
IR hooks; generated coverage/attribution and `git diff --check` pass. The catalogue
adds 16 CpuSelectorQueryHelper forms. Production Pending remains 3,728. Remaining
FLAGS/ISA/system contracts, full MIR/regions, online tiers/lifecycle, advanced
passes, XP/performance acceptance and legacy retirement remain incomplete.

## VERR/VERW and raw ZF across CPU boundaries

`make ir-tests ir-memory-tests ir-stack-tests ir-control-tests ir-shift-tests
ir-multiply-tests ir-bit-tests ir-exchange-tests ir-enter-tests ir-misc-tests
ir-loop-tests ir-system-stack-tests ir-segment-tests ir-string-tests ir-io-tests
ir-rep-engine-tests ir-rep-tests ir-cpu-info-tests ir-cpu-system-tests
ir-control-regs-tests ir-descriptor-tests ir-task-regs-tests ir-selector-query-tests
ir-verr-tests ir-flags-observer-tests` passed (`build/ir-verr-suite.log`): all
70 warnings-as-errors Rust tests and every listed executable suite passed,
including independently pinned REP and task-register references. This broad run
covers the changed materializer used by all current CPU IR families.

Per debug/release build, VERR/VERW checks 720 ordinary cases, 32 mode guards,
160 invalid selectors, 8,192 independently modeled descriptor permissions,
16 source faults, 64 descriptor faults/raw-ZF frames, 80 segment-priority cases,
64 MMIO sequences and 32 LDT queries. An additional 8,640 transition checks
compare 36 flag-producing/preserving sequences with both initial raw-ZF values,
lazy/materialized entries and five operand seeds. Both optimized and unoptimized
artifacts run as fused IR, IR followed by interpreter, and two IR entries followed
by interpreter. State and exception frames retain both architectural ZF and the
independently observable raw backing bit without masking the difference.
See [ir-selector-query.md](ir-selector-query.md) and the focused logs
`build/ir-verr-diff.log` / `build/ir-raw-zero-diff.log`.

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-verr-check.log`). Normal CPU exports contain no ir_ hooks, generated
coverage/attribution and `git diff --check` pass. An initial attempt to apply
-D warnings globally to the normal CPU build stopped on the already unused
experimental WasmBuilder methods (`build/ir-verr-global-warnings-check.log`);
the normal project build and its explicitly scoped warnings-as-errors test target
then passed as recorded above. No warning suppression was added.

VERR/VERW add 16 experimental forms, bringing CpuSelectorQueryHelper to 32.
Production Pending stays at 3,728. This completes the previously documented
raw-ZF dependency for these queries, not the entire FLAGS/ISA/MIR audit or the
online tier, lifecycle, optimization, XP/performance and legacy-retirement work.

## CMPXCHG8B and ZF lazy-marker preservation

The complete previously listed suite, extended with `ir-cmpxchg8b-tests`, passed
(`build/ir-cmpxchg8b-suite.log`): 72 warnings-as-errors Rust tests, every existing
IR executable/CPU suite, independent REP/task-register references and the original
FLAGS observer diagnostics. This run covers the shared RAM-guard extraction and
changed FLAGS state/materialization across existing instruction families.

Per debug/release CPU, optimized and unoptimized CMPXCHG8B artifacts passed:

- 21,504 independent-result/CPU scenarios using 3,584 fixtures; 21,504 warmed
  native RAM entries asserted zero slow-adapter calls.
- 60 boundary cases, 24 preflight faults, 10 segment faults, four real-mode cases,
  eight injected code-bit guard checks, 20 CPL/WP cases and 48 address16 tails.
- 48 MMIO/lazy-ZF sequences, 16 callback register-change sequences, four remaps,
  24 post-preflight read/write faults/aborts including partial writes, and 20
  pinned same-page signed-low versus cross-page zero-extended MMIO reads.

The existing raw_zero suite now also compares flags_changed.ZF and checks eight
IR-exit-to-interpreter CMPXCHG8B write-callback observations per build, alongside
its 8,640 existing raw/computed ZF transitions. The prior failing diagnostic is
retained as evidence in `build/ir-cmpxchg8b-flags-probe.log`; the regression checks
are permanent in `tests/ir/differential/raw_zero.mjs`.

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-cmpxchg8b-check.log`). Generated catalogue/attribution, normal CPU export
isolation and `git diff --check` passed. CMPXCHG8B adds four CpuMemoryHIR forms,
now 622 total; production Pending remains 3,728. [The contract](ir-cmpxchg8b.md)
distinguishes native execution, CPU slow-path semantics, guard tests and the
remaining full MIR/ISA/online tier/XP/performance/retirement work.

## XMM V128 state and packed/scalar SIMD transfers

The complete previously listed suite, extended with `ir-simd-move-tests`, passed
(`build/ir-simd-moves-suite.log`): 74 warnings-as-errors Rust tests, all existing
IR differential suites and independently pinned REP/task-register references.
The normal CPU remains isolated from experimental IR exports.

Per debug/release build, 7,680 fixtures passed 11,264 independent/CPU move
scenarios, including 7,168 native RAM entries with no slow transfer call. Further
checks passed 192 EM/TS priority cases, 160 unaligned/boundary/fault cases, 80
segment faults, 96 OSFXSR/real/VM86 cases, 64 MMIO/pinned wide reads, 16 callback
state changes, 14 partial faults/aborts and 16 pre-EA guard exception observations.
Register and memory cases verify exact payload bits, scalar high-lane semantics,
full XMM/GPR/FLAGS state, PCs, frames and completed instruction counts.

Eight vector-chain executions per build check native V128 SSA continuation,
CPU-owned cold completion exits and separately compiled reentry. Two edge-copy
cases check coalesced V128 slot cycles before/after optimization, and two source
fault cases verify materialization of vector values changed by preceding register
transfers. Invalid lane/state types and generic V128 helper ABIs are rejected.
The ABI never passes a v128 through a JavaScript import.

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
(`build/ir-simd-moves-check.log`); generated coverage/attribution, normal export
isolation and `git diff --check` passed. The catalogue adds 64 CpuSimdHIR forms;
production Pending stays at 3,728. [The transfer contract](ir-simd-moves.md)
records the baseline alignment/task checks and distinguishes this first IR-08
family from remaining vector arithmetic, MMX, FP controls, x87, feature fallback,
full MIR/regions, online runtime and XP/performance acceptance.

## Packed XMM integer validation

The full current IR suite passed after adding typed packed operations, CPU slow
read completion and native SIMD selection (`build/ir-packed-full-suite.log`).
It includes 77 warnings-as-errors Rust tests, all previously connected IR and
independent baseline suites, the XMM move suite and the new packed suite.

`make ir-simd-integer-tests` generated 22,080 fixture pairs. Per debug/release
CPU, 32,384 ordinary comparisons passed, with no slow adapter call in 20,608
warmed native RAM entries. An independent BigInt model checked 5,888 additional
boundary/random/alias cases. Further tests passed 552 EM/TS priority cases,
460 alignment/boundary/fault cases, 230 segment faults, 276 OSFXSR/real/VM86 cases,
184 warmed supervisor/read-only page checks, 184 MMIO/wide reads, 46 callback
state changes, 46 late source faults, 46 early exception observations, 92 callback
remaps, eight SSA/cold-completion/reentry chains and two dirty-vector fault exits.
The detailed standalone run is `build/ir-packed-diff.log`.

Final feature compilation and the warnings-denied packed contract test passed
(`build/ir-packed-check.log`, `build/ir-packed-contract-check.log`). Generated
catalogue and suite attribution checks, normal Wasm export isolation and
whitespace checks passed.
The experimental XMM category now has 248 forms; production Pending remains
3,728. See [the packed integer contract](ir-simd-integer.md) for semantic scope
and the remaining IR-08 and production integration work.

## Packed XMM packing/unpacking and shift validation

The full current suite passed (`build/ir-packed-permute-full-suite.log`): 79
warnings-as-errors Rust tests and all connected IR, CPU and independent baseline
suites. The expanded packed suite passes 48,576 ordinary scenarios per CPU build,
8,832 boundary/random/alias cases, 1,344 full-u64 shift-count cases and 276 signed
narrowing boundary cases. It requires zero slow calls in 30,912 warmed RAM entries.

The initial differential failure exposed UNPCKLPS/UNPCKLPD's eight-byte baseline
read. After carrying read width explicitly through HIR, native guard and adapter,
138 absent-upper-page cases per build prove both exact eight-byte completion and
mandatory sixteen-byte faults (including zero-count variable shifts). MMIO,
remapping, callback destination mutations and exception observations pass without
masking CPU wide-read behavior. The diagnostic run remains in
`build/ir-packed-permute-suite.log`; corrected evidence is in
`build/ir-packed-permute-fixed.log` and `build/ir-packed-permute-diff.log`.

The new immediate suite passes 14,720 fixture pairs per build, all imm8 values for
XMM0 across both execution defaults/address sizes and boundary counts for every
other XMM destination. It also passes 1,440 randomized vectors, 540 EM/TS guards,
270 OSFXSR/real/VM86 cases and 90 post-ModRM/pre-immediate exception observations.
Eight mixed byte-shuffle/unpack/shift/pack/load/store executions per build check
SSA continuation, CPU cold-completion exits and reentry; two source faults check
materialization of changed vector values before exception delivery. The final
mixed-chain evidence is in the full suite log.

Generated catalogue and attribution, normal Wasm export isolation and whitespace
checks passed. Experimental CpuSimdHIR now has 360 forms, while production Pending
remains 3,728. [The packing and shift contract](ir-simd-permute.md) records the
coverage and remaining requirements; no full ISA, production Tier, XP or
performance acceptance follows from these tests.

`cargo check --target wasm32-unknown-unknown --features ir-experimental` passed
on the final source (`build/ir-packed-permute-check.log`).

## Immediate XMM shuffle validation and host-trap test isolation

The connected regression matrix passed in two segments. The initial run
`build/ir-simd-shuffle-full-suite.log` passed 81 warnings-as-errors Rust tests and
all suites through the descriptor family, then stalled in the release task test's
repeated expected-host-panic matrix. A probe of the test reference stack showed
that caught panics retained their linear-memory frames (including a 12,544-byte
loss in one call). The still-running test was stopped after this diagnosis.

The reference test artifact now exports `__stack_pointer`; task comparisons
restore it after a caught RuntimeError and require normal calls to restore it
without intervention. Guest-state, fault, callback and partial-completion checks
remain unchanged. The complete task suite passed in `build/ir-task-stack-fixed.log`.
The subsequent make run `build/ir-simd-shuffle-resume-suite.log` passed the task
suite again and every remaining target: selector queries, VERR/VERW and raw ZF,
CMPXCHG8B, all SIMD families and the flags-observer diagnostic. Together these
runs cover the complete current target list. The original interrupted run is
retained rather than represented as a successful terminal make invocation.

The shuffle suite generates 29,400 fixture pairs. Each debug/release CPU passes
41,120 ordinary scenarios and 3,840 boundary/random/alias scenarios against an
independent word/dword/qword model. There are 23,440 warmed native RAM entries
with zero slow adapter calls. The final run uses distinguishable word patterns
for the exhaustive controls, including SHUFPD's ignored high immediate bits.

Additional checks pass 360 EM/TS guards, 300 alignment/boundary/fault cases, 150
segment faults, 180 OSFXSR/real/VM86 cases, 120 permission checks, 60 full-source
reads with an absent upper page, 120 MMIO/wide reads, 30 callback mutations, 30
late source faults, 30 pre-EA observations and 60 remaps per build. Eight mixed
shuffle chains check native SSA, cold completion and separately compiled reentry;
two source faults check dirty-XMM state before CPU exception delivery.

Generated catalogue and attribution, normal export isolation (including absence
of the test-only host-stack export) and whitespace checks passed. CpuSimdHIR
now has 380 experimental forms; production Pending remains 3,728. See the
[shuffle contract](ir-simd-shuffle.md) and [task test contract](ir-task-regs.md).
Full ISA/MIR, online Tier integration, host feature fallback, XP/performance
acceptance and legacy retirement remain incomplete.

Final feature compilation passed: `cargo check --target wasm32-unknown-unknown --features ir-experimental` (`build/ir-simd-shuffle-check.log`).

## Half/scalar/duplicate XMM transfer validation

The full connected regression matrix passed in one make invocation after adding
half transfers, MOVD/MOVQ, duplication, XmmTransferLoad and explicit XmmStore lanes
(`build/ir-simd-transfer-full-suite.log`). It includes 83 warnings-as-errors Rust
tests, all prior IR/independent reference suites, the repaired task host-stack
isolation, all SIMD families and the flags-observer diagnostic.

The new suite generates 5,664 fixture pairs and passes 9,024 independent/CPU
scenarios plus 1,536 random transfer/GPR scenarios per debug/release CPU. It
requires zero slow calls in 6,720 native RAM entries. Further checks pass 144
EM/TS guards, 150 alignment/boundary/fault cases, 75 segment faults, 72
OSFXSR/real/VM86 cases and 30 exact 4/8/16-byte extents with an absent following
page. The MMIO matrix passes 60 wide accesses, 15 source/target/GPR mutation
cases, 13 partial faults/aborts and 15 pre-EA exception observations per build.

Eight mixed-transfer chain executions per build verify native V128 continuation,
CPU-owned cold completion and reentry. Two faults verify changed vectors before
exception delivery; two XMM/GPR/XMM chains verify GPR SSA propagation and zeroed
high bits. The independent model checks low/high source selection, retained
halves, MOVQ/MOVD zero-fill, duplicated lanes and high-qword stores. Comparisons
retain full CPU state, frames, memory, callback events and instruction counts.

Generated catalogue and suite attribution, normal Wasm export isolation and
whitespace checks passed. Experimental CpuSimdHIR totals 428 forms, while
production Pending remains 3,728. The [transfer contract](ir-simd-transfer.md)
records exact sampling, read-width and store-lane behavior and remaining scope.

Final feature compilation passed: `cargo check --target wasm32-unknown-unknown --features ir-experimental` (`build/ir-simd-transfer-check.log`).


## XMM sign masks, word lanes and memory-only transfers

The focused suite passed in `build/ir-simd-lane-suite.log`: 13,944 fixture pairs,
18,064 independent/CPU scenarios, 832 random cases and 8,240 native RAM entries
per debug/release build. It exhausts 65,556 byte/dword/qword sign masks and all
word immediates in representative aliases. Ten exact two-/sixteen-byte extents
end at an absent following page; guard, segment, MMIO, callback, partial-fault
and native/cold reentry comparisons retain complete CPU state and frames.

Eight mixed insert/extract/mask/store chains and two dirty-XMM fault exits pass
per build. Verifier tests reject invalid lanes/widths, destination/state
mismatches, forbidden forms and unsupported MMX equivalents. See the
[lane contract](ir-simd-lane.md) for sampling and access-width requirements.
Experimental CpuSimdHIR totals 448; production Pending remains 3,728.

Feature compilation passed in `build/ir-simd-lane-check.log`. Generated catalogue,
suite attribution, production Wasm export isolation and whitespace checks pass.
Full IR-00–IR-14 acceptance remains open, including full ISA/MIR, online runtime,
XP, performance and legacy retirement.

The full connected regression matrix passed in one make invocation
(`build/ir-simd-lane-full-suite.log`), including 85 warnings-as-errors Rust tests,
all connected IR/independent-reference targets, every SIMD family and the final
FLAGS-observer diagnostic. No production source changed after this verification.


## MASKMOVDQU validation

The full connected regression matrix passed in one make invocation
(`build/ir-simd-masked-full-suite.log`), with 87 warnings-as-errors Rust tests and
all existing IR/independent-reference suites. The added target generates 1,792
fixture pairs and runs 21,504 independent/CPU scenarios, 128 random cases and all
65,536 byte masks per debug/release CPU. Native warm paths require zero helpers.

Twenty-four MMIO cases require exactly the selected byte writes in address order,
including no writes for a zero mask. Callback mutations and late faults preserve
captured source/mask/address and CPU-owned state. Six page-table callback cases
verify sampling after the complete write preflight, including mutations followed
by an upper-page fault. Twenty-seven zero/sparse-mask permission and DI-boundary
checks, eight dirty-vector completion/reentry chains and two dirty-XMM faults
pass per build, alongside SSE guards, segments and exact access extents.

The [masked-store contract](ir-simd-masked.md) records the full sixteen-byte
preflight even for zero masks and the pinned partial-write/abort behavior.
Feature compilation (`build/ir-simd-masked-check.log`), generated catalogue,
suite attribution, production export isolation and whitespace checks pass.
Experimental CpuSimdHIR totals 450; production Pending remains 3,728. Full
ISA/MIR, online Tier/runtime integration, host fallback, XP/performance acceptance
and legacy retirement remain incomplete.


## Explicit MIR memory selection

Lowering now records the RAM guard, physical action, CPU call signature/arguments
and result/exit policy for scalar accesses, partial stores, RMW reads and all
implemented XMM memory operations. The emitter consumes those plans directly.
Focused tests (`build/ir-mir-memory-contracts.log`) pass seventeen access forms
with and without optimization, ENTER partial completion, unsafe/stale plans and
memory/helper import conflicts. See the [MIR memory contract](ir-mir-memory.md).

The new plan consistency check compares against canonical lowering; it does not
replace independent execution evidence. Values, states and CFG remain in shared
HIR arenas, and remaining memory/control operations still require MIR migration.
Feature compilation passes in `build/ir-mir-memory-check.log`; this increment does
not change ISA coverage or production Pending.

The complete connected regression matrix passed in one make invocation
(`build/ir-mir-memory-full-suite.log`), including 90 warnings-as-errors Rust tests,
all scalar/stack/RMW/partial-fault suites, every SIMD family, independent CPU
references and the FLAGS-observer diagnostic. Thus the new planned paths have
been exercised with native RAM, MMIO, callbacks, faults and reentry, not only
inspected as metadata. Generated catalogue, normal Wasm export isolation and
whitespace checks pass. No production source changed after these checks.


## MIR observation, address and check plans

Lowering now selects RMW observation/count phases, segment/POP address globals
and adapters, whole-range checks and SSE guard calls. Three focused
warnings-as-errors tests (`build/ir-mir-effect-contracts.log`) cover all RMW
scalar widths, optimized/unoptimized address and check forms, eleven unsafe plan
mutations, missing plans, all twenty-two reserved CPU imports and an effect/helper
signature conflict. MIR dump includes HIR definitions/recovery maps followed by
memory and effect plans. See [the effect contract](ir-mir-effects.md).

Feature compilation passes in `build/ir-mir-effect-check.log`. Catalogue,
production Wasm export isolation and whitespace checks pass. The change adds no
ISA forms and does not establish an independent MIR CFG or production Tier
integration; those completion conditions remain open.

The complete connected regression matrix passed in one make invocation
(`build/ir-mir-effect-full-suite.log`), with 93 warnings-as-errors Rust tests and
all existing IR/independent-reference targets. Real CPU tests exercise the new
address/check/RMW plans with native RAM, MMIO, callback observations, partial
completion, faults and SSE EM/TS priority. All SIMD families and the final
FLAGS-observer diagnostic pass. No production source changed after verification.


## MIR division and compare/exchange plans

Checked division and CMPXCHG8B now consume explicit MIR arithmetic plans.
Focused warnings-as-errors tests (`build/ir-mir-arithmetic-contracts.log`) pass
24 division and 16 compare/exchange combinations, including optimization, signs,
widths, operand forms and prefixes. Sixteen unsafe mutations are rejected before
emission. See [the arithmetic-plan contract](ir-mir-arithmetic.md).

The plan fixes quotient bounds and host-trap protection, or the full writable
qword guard, implicit CPU slots, raw/lazy ZF update, count and CPU-exit outcomes.
It participates in existing effect-plan consistency and import checks. Feature
compilation passes in `build/ir-mir-arithmetic-check.log`; catalogue, normal Wasm
export isolation and whitespace checks pass. Independent MIR CFG, generic helper
observation lowering, dynamic accounting and production Tier integration remain
unfinished; ISA coverage is unchanged.

The full connected regression matrix passed in one make invocation
(`build/ir-mir-arithmetic-full-suite.log`), with 96 warnings-as-errors Rust tests
and every existing IR/independent-reference target. It executes planned division
under guest faults and boundary values, and planned CMPXCHG8B under RAM/MMIO,
callback mutations, partial faults and raw/lazy ZF observations. All other memory,
SIMD and final FLAGS-observer suites pass. No production source changed after
verification.


## MIR generic helper call-site plans

Generic calls now consume explicit observation, argument, typed staging,
fault-delivery and outcome plans. Three focused warnings-as-errors tests pass
caller/helper/no-fault contracts, optimized/unoptimized sites, CPU/standalone
observations, CPUID/REP terminal outcomes, thirteen unsafe table/site mutations,
missing vectors and unused arena records. See [the call-site contract](ir-mir-calls.md)
and `build/ir-mir-call-contracts.log`.

The full connected regression matrix passed in one make invocation
(`build/ir-mir-call-full-suite.log`), including 99 warnings-as-errors Rust tests
and all existing IR/independent-reference targets. Actual Wasm checks include
42 generic helper ABI executions, mixed-width results, snapshot-slot reuse,
single fault delivery and preservation of helper-owned CPU state. CPU/I/O/REP
and system suites verify observed PCs and terminal outcomes. All SIMD targets
and the final FLAGS-observer diagnostic pass.

Feature compilation passed in `build/ir-mir-call-check.log`; catalogue, production
export isolation and whitespace checks pass. No production source changed after
verification. Independent MIR CFG, explicit materialization, dynamic accounting,
full ISA and online Tier integration remain unfinished; production Pending is
unchanged at 3,728.


## Lowered MIR dispatcher CFG and typed parallel copies

The dispatcher now consumes MIR entries, blocks, branch conditions, edge-copy
schedules and recovery exits. Lowering eliminates identity copies, directly
orders acyclic assignments and saves one source per copy cycle. HIR topology
is currently preserved; instruction/state arenas and liveness remain shared.
See [the control-flow contract](ir-mir-control.md).

Three new warnings-as-errors Rust tests cover all 150,207 source mappings for
up to six I32/I64/V128 destinations against independent simultaneous assignment,
mixed-type cycles, invalid slots/types, duplicate destinations, multi-entry loops,
CPU cycle rejection and twelve unsafe graph/schedule mutations plus stale HIR.
The full Rust unit run passes 102 tests. The new JS oracle passes 648 actual Wasm
executions from 108 modules, testing both arms of critical edges, repeated source
values, optimization, budget exits, all GPRs, FLAGS and recovery-only state.

Feature compilation passes in `build/ir-mir-control-check.log`; catalogue,
normal production export isolation and whitespace checks pass. This work does
not establish independent MIR instruction/graph optimization, dynamic CPU
accounting or online Tier integration. ISA and production coverage are unchanged.

The full connected regression matrix passed in one make invocation in
`build/ir-mir-control-full-suite.log`, with all 102 Rust tests and every existing
IR/independent-reference target. Actual i32/i64/v128 edge-copy fixtures, memory
and system fault paths, CPU-owned helper outcomes, all SIMD families and final
FLAGS observations pass. No production source changed after this verification.


## MIR value programs and selected packed kernels

Value instructions now lower to explicit machine-width scalar programs, state
read bindings and SIMD lane/shuffle operations. Packed kernel selection is shared
by pure operations and native vector-memory combines. Wasm emission no longer
matches HIR value opcodes or PackedOp. See [the value-program contract](ir-mir-values.md).

Focused warnings-as-errors checks pass in `build/ir-mir-value-contracts.log`:
60 scalar width/operator combinations, 228 packed register/memory/optimization
combinations, nine invalid machine stacks, five unsafe scalar contracts and six
corrupted shift kernels. Machine-stack verification reads selected operations
and value types; canonical consistency additionally protects semantic choices.
The independent BigInt oracle passes 17,280 actual Wasm executions from 120
modules, including signs, width boundaries, shifts, comparisons, normalization,
preserved GPRs/FLAGS and recovery-only state.

Independent MIR lifecycle/graph transformations, explicit materialization,
dynamic CPU accounting, remaining ISA/host fallback and online Tier/runtime
integration remain incomplete. Production Pending remains 3,728.

The full connected regression matrix passed in one make invocation in
`build/ir-mir-value-full-suite.log`, including 105 warnings-as-errors Rust tests,
17,280 new scalar Wasm executions and every existing IR/independent-reference
target. All packed integer, immediate-shift, shuffle, transfer, lane and masked
store suites execute through selected value/kernel plans, with native RAM,
MMIO, callbacks, fault and continuation checks. Final FLAGS observers pass.
Feature compilation passed in `build/ir-mir-value-check.log`; catalogue, normal
production export isolation and whitespace checks pass. No production source
changed after verification.


## MIR ordered state materialization

StatePlans now encode ordered target-specific writes, typed expressions, decoded
observer PC writes and separate absolute/delta count phases. The emitter uses
these plans without interpreting HIR StateMap fields or cloning a commit map to
create an RMW observation. See [the state-plan contract](ir-mir-state.md).

Focused warnings-as-errors tests pass in `build/ir-mir-state-contracts.log`:
four PC modes, XMM/backing presence, both optimization modes, CPU/standalone
requirements, twelve corrupted plans plus stale HIR, and RMW count/value phase
selection across all three scalar widths. Forty-eight modules pass 3,456
independent memory-observation executions, including all GPRs/XMMs, raw/lazy ZF,
optional last_op1, CS/count wrapping and repeated entry.

Dynamic CPU accounting, independent MIR lifecycle/graph transforms, remaining
ISA and online Tier integration remain incomplete. Production Pending is 3,728.

The full connected regression matrix passed in one make invocation in
`build/ir-mir-state-full-suite.log`, with 108 warnings-as-errors Rust tests,
3,456 new state observation executions and all existing IR/independent-reference
targets. Actual CPU paths verify observer PCs, raw/computed FLAGS, RMW phases,
helper-owned mutations, partial faults and vector completion/reentry through
the new plans. All SIMD families and final FLAGS observers pass. Feature
compilation passed in `build/ir-mir-state-check.log`; catalogue, normal production
export isolation and whitespace checks pass. No production source changed after
verification.


## SSA dynamic counts and CPU loops

StateMap count bases now participate in availability/liveness and optimization
rewriting. MIR materialization uses base plus static offset; CPU cycle admission
requires all live observation/recovery states to carry dynamic bases. Existing
static frontend behavior remains unchanged. See [the count contract](ir-dynamic-count.md).

Three focused warnings-as-errors tests pass in `build/ir-dynamic-count-contracts.log`:
CPU loop emission at eight budgets, type/dominance/commit-base rejection,
static/mixed-count cycle rejection, stale MIR checks, orphan state records and
count-only GVN/DCE. The independent execution model passes 7,786 cases with
17,604 helper observations and 106 bounded reentries. It checks before/after
representations of the same completed-work count, wrapping, noncached observer
adjustments, caller fault restoration/single delivery and authoritative
helper-owned control/yield/invalidation exits.

This proves the controlled CPU-global ABI loop fixtures, not automatic guest CFG
lifting or online scheduler/interrupt integration. Complete ISA/MIR/runtime,
XP/performance acceptance and legacy retirement remain unfinished.

The complete connected regression matrix passed in one make invocation in
`build/ir-dynamic-count-full-suite.log`, with 111 warnings-as-errors Rust tests,
all dynamic loop/counter executions and every existing IR/independent-reference
target. Static frontend memory/helper/REP/system behavior, all SIMD families and
final FLAGS observers pass. Feature compilation passed in
`build/ir-dynamic-count-check.log`; catalogue, normal production export isolation
and whitespace checks pass. No production source changed after verification.

## Reachable bytecode CFGs

Two focused Rust tests pass in `build/ir-cfg-contracts.log`, covering reachable
CFG construction, overlap/truncation/budget rejection and immutable publication
identity. `build/ir-cfg-differential.log` records 35,328 optimized/unoptimized
executions compared with 173,056 actual interpreter steps, plus sixteen scalar/
vector faults in the second loop iteration. Self-loop retirement counts have an
independent oracle; fault tests explicitly require five completed instructions
before the sixth faults. See [the frontend contract](ir-cfg-frontend.md).

This is automatic direct CFG compilation through a cold-entry API. Online Tier
integration, complete ISA/MIR/runtime, XP/performance and legacy retirement remain
unfinished; production Pending is unchanged at 3,728.

The full connected matrix passed in one make invocation in
`build/ir-cfg-full-suite.log`: 113 warnings-as-errors Rust tests, the new CFG
comparisons and every existing IR/independent-reference target, all SIMD
families and final FLAGS observers. Feature compilation passed in
`build/ir-cfg-check.log`; catalogue checks, normal production export isolation
and whitespace checks pass. Compiler source remained unchanged during this
verification. No XP or online IR workload was run.

## Straight-block merging and explicit MIR polls

`build/ir-merge-focused.log` records two passing structural/negative tests:
preserved recovery boundaries, loops/diamonds/backward block IDs, terminal stores,
fixed points, disabled pass, stale MIR cost/state and unavailable HIR snapshots.
`build/ir-merge-differential.log` records 35,328 CPU comparisons and 17,664 exact
optimized/unoptimized budget exits, including previous IP and count. Sixteen
second-iteration scalar/vector faults also pass. The first structural run rejected
a mistyped test jump into an instruction; its displacement was corrected before
the passing run. See [the merge contract](ir-cfg-merge.md).

The full connected regression matrix passed in one make invocation in
`build/ir-merge-full-suite.log`: 115 warnings-as-errors Rust tests, exact CFG
budget comparisons, all existing IR/independent-reference targets, all SIMD
families and final FLAGS observers. `build/ir-merge-check.log` records successful
experimental Wasm compilation. Catalogue, production export isolation and
whitespace checks pass. Compiler source was unchanged throughout that full run.
XP, online scheduling and workload performance were not tested.

## Dominator GVN and constant CFG cleanup

`build/ir-dataflow-focused.log` records three passing structural/negative tests:
non-topological block order, i32/i64 keys, sibling/independent-entry rejection,
CPU-read exclusions, true/false/identical branches, retained external roots,
arena compaction and removal of unreachable unadapted calls. An initial
identical-edge fixture incorrectly left a pre-existing unreachable block; the
verifier rejected it and the fixture was replaced with a valid two-block graph.

The independent diamond oracle executes 3,072 Wasm cases. The expanded bytecode
suite in `build/ir-dataflow-differential.log` passes 39,936 CPU comparisons,
19,968 exact budget exits, sixteen second-loop faults and sixteen constant-branch
absent-page cases. See [the dataflow contract](ir-dataflow.md). Online/XP/workload
performance acceptance is not established by these tests.

The full connected matrix passed in one make invocation in
`build/ir-dataflow-full-suite.log`: 118 warnings-as-errors Rust tests, all new
independent/CPU/budget cases, every existing IR/independent-reference target,
all SIMD families and final FLAGS observers. Experimental Wasm compilation passed
in `build/ir-dataflow-check.log`; catalogue, production export isolation and
whitespace checks pass. Compiler source was unchanged during that full run.
No online IR/XP/workload performance claim follows from this validation.

## Online publication bridge foundation

`build/ir-publication-final-focused.log` records 159 controlled real Wasm
instantiations in each debug/release build. Cases cover
pre-install rejection, immediate cancellation/slot ABA, duplicate/full-width wrong
keys, actual execution, browser/table/export errors, dependent-page retry,
restore/instance replacement, immutable snapshots, bounded failure metadata and
u64 ticket boundaries. An initial cross-page fixture only built a one-page region;
it was corrected to disable one-page tiering and register the secondary cold entry.
The expanded checks also cover synchronous browser errors without reentering the
locked generator, and a failed replacement retaining actual execution through the
previously published JIT module. `build/ir-publication-capacity.log` and
`build/ir-publication-production-final.log` record the strengthened 899-slot test:
execution through the first module's secondary page proves the fixture really
contains a cross-page dependency before creating a hidden reference.
`tests/glbridge/performance_recorder_test.js` verifies ticket forwarding and stale
callback accounting. See [the publication contract](ir-publication.md).

These are actual online legacy-bridge tests, not online IR execution or XP
acceptance. The compiler's IR artifact/publication-key integration remains pending.

`build/ir-publication-production-final.log` records a successful `make all`,
899-slot capacity/hidden-page pressure, tier promotion, SMC/restore, CPU optimization
regressions, 768 real-JIT SSE cases, 152,064 FLAGS cases across 2,376 programs and
the native performance recording test. The recorder's JS unit test also passes.
Normal debug/release Wasm exports contain the matching new publication callbacks
and exclude IR/test/stack-pointer exports; release force-compilation remains absent.

Worker CPU, snapshot interchange and real performance recording pass in
`build/ir-publication-worker-repeat.log`; separate audio and compiled-UI browser
tests pass in `build/ir-publication-worker-audio.log` and
`build/ir-publication-worker-ui.log`. The complete Worker target is **not green**:
the GPU test twice read transparent black instead of red. An isolated candidate
run passed, and a matched baseline JS/worker build using the pinned legacy Wasm
also produced one pass and the same failure. See `build/ir-publication-gpu-repeat.log`,
`build/ir-publication-gpu-baseline.log` and `build/ir-publication-gpu-baseline-2.log`.
This reproduces the symptom on the baseline; its underlying graphics/timing cause
is unresolved. No pixel expectation or graphics implementation was weakened.

The API run first lacked OS images; the fixtures were obtained using the repository
README's download source. Clean shutdown passed, but the first state test then
stalled at synchronous CD-ROM restore and was stopped after over five minutes.
All four state configurations subsequently passed both an instrumented 90-second
watchdog run and the unchanged test with a 120-second outer timeout, including
2 GiB memory (`build/ir-publication-api-state-diagnostic.log` and
`build/ir-publication-api-state-repeat.log`). Reset, floppy, parallel, CD-ROM
insert/eject, ISO9660, serial and reboot passed in
`build/ir-publication-api-remaining.log`. PIC explicitly skipped because
`images/fs.json` is absent. These results do not constitute a fully passing API
matrix, and the first restore stall has not been causally attributed.

The complete connected IR matrix passed in one make invocation in
`build/ir-publication-full-suite.log`: 118 warnings-as-errors Rust tests, 3,072
independent diamond executions, 39,936 CPU CFG comparisons and 19,968 exact
budget exits, all existing IR/independent-reference targets, every SIMD family and
the final FLAGS observers. Experimental Wasm compilation passed in
`build/ir-publication-check.log`. Catalogue and whitespace checks passed;
production Pending remains 3,728. These results validate the existing experimental
compiler and shared bridge changes, not online IR, XP or workload performance.

## CPU entry specialization and admission

`build/ir-entry-focused.log` records two passing native contract/fixture tests and,
in each debug/release build, 1,760 no-effect rejection cases, 176 actual interpreter
comparisons and 32 precise page-fault comparisons. Cases include linear aliases of
one physical code page, CS wrapping, default widths, prefixes, halted/legacy frames,
entry-index boundaries and unchanged pre-existing REP result metadata. The native
contract test rejects standalone/CPU confusion and changes to context, dependency
versions or slot generation. See [the entry contract](ir-entry.md).

The first fixture run rejected a supposedly matching entry because its REP metadata
seed helper had rewound IP to previous_ip. The fixture now reestablishes the intended
CPU entry context after seeding that metadata and checks that it is admissible before
each test. Guest expectations were not relaxed. These are cold compiled CPU entry
tests; online cache admission and instruction scheduling are still pending.

Review found that the legacy frame marker was maintained only in debug builds.
It is now maintained in experimental IR release builds as well. The final focused
run (`build/ir-entry-final-focused.log`) repeats every admission check and also
executes a real guest OUT/LOOP workload in each build, with performance recording
off and on. Compiled I/O callbacks reject otherwise matching entry keys; cold
interpreter callbacks admit them, and the marker is cleared after the JIT returns.
This validates actual CPU dispatch in addition to the explicit test-hook cases.

The complete connected matrix passed in `build/ir-entry-full-suite.log`: 120
warnings-as-errors Rust tests, all existing IR/independent-reference targets,
39,936 CPU CFG comparisons, 19,968 exact budget exits, every SIMD family and the
final FLAGS observers. The release frame-marker change was made after that run
began; the final focused run above and `build/ir-entry-cpu-regression.log` separately
validate it in both final debug/release IR builds. The latter includes actual
CALL/RET, tier-cache SMC/restore, 2,304 arithmetic FLAGS cases, faults/IRET, MMIO
partial writes, REP progress and 768 real-JIT SSE cases per build.

Experimental feature compilation passed in `build/ir-entry-check.log`; ordinary
production debug/release builds passed in `build/ir-entry-production-build.log`
and exclude IR/test/stack-pointer exports. A separate release artifact built with
only `ir-experimental` (`build/ir-entry-feature-artifact.log`) exports the entry
guard and every import required by the 176 generated modules, while excluding
test and stack-pointer exports. Catalogue and whitespace checks pass. Production
Pending remains 3,728; online IR, XP and workload performance were not validated.

## Live in-Wasm compilation and read-only code capture

`build/ir-live-final-focused.log` records three passing native entry/mapping tests,
and in each Node-hosted debug/release CPU 44 actual live compilations compared with the
interpreter, eight precise data page faults, 19 paging/capture cases and lifecycle
checks. Capture/revalidation preserve CPU fields, RAM page tables, the complete
TLB and MMIO observations. Tests distinguish stale visible TLB translations from
post-invalidation mappings, including identical code bytes on a different physical
page. They include CR3 low bits, PSE/PAE, cached PDPTEs, physical aliases, high-address
wrap, missing pages and MMIO table/code addresses. Async instantiation, writes,
obsolete handles, mode changes, restore/reset and compilation failures also pass.
A real legacy JIT I/O callback confirms that the live compiler refuses an active
legacy frame without mutating guest state.

The experimental-only release target also compiles and executes CFG/store artifacts
for both tier requests and optimization settings, with independently expected
register/memory/IP/wrapped-count results and no exported test hooks. The initial
live metadata assertion treated Wasm's signed i32 EIP result as an unsigned Number;
the fixture now normalizes that documented JS ABI field with `>>> 0`.
See [the live compiler contract](ir-live-compile.md) for task-local dependency
versions, exact-byte revalidation and the remaining online cache limitations.

The complete connected IR matrix passed after the final compiler changes in
`build/ir-live-full-suite.log`: 121 warnings-as-errors Rust tests, every existing
IR/independent-reference target, entry/live compilation suites, 39,936 CPU CFG
comparisons, 19,968 exact budget exits, all SIMD families and final FLAGS observers.
`build/ir-live-check.log` records successful experimental feature compilation.
The experimental-only release CPU also passes the CPU optimization regression in
`build/ir-live-cpu-regression.log`, exercising real CALL/RET, cache SMC/restore,
arithmetic FLAGS, faults/IRET, MMIO writes, REP progress and real-JIT SSE with the
new live invalidation hooks enabled.

Ordinary debug/release build checks passed in `build/ir-live-production-build.log`.
`build/ir-live-exports.log` confirms that both omit IR/test/stack-pointer exports,
while the experimental-only release exports the live compiler API and entry guard
without test hooks. Catalogue (`build/ir-live-coverage.log`) and whitespace checks
pass; production Pending remains 3,728. The Makefile's optional disabled wasm-opt
step emits its existing ignored `false` exit; Rust compilation and the matrix
complete successfully. No browser-hosted, OS-boot or performance acceptance was
added by this live-compilation test run.

## Explicit IR publication and normal CPU dispatch

`build/ir-cache-final-focused.log` records the actual JS bridge and CPU dispatcher
passing in debug/release with JIT pool invariants and in an experimental-only
release. Each runs eight independently checked CFG/store combinations across tier
requests, optimization and recording, including wrapped instruction counts. Tests
also cover initial/cross-page/unreachable fetch A bits, source/PTE aliases altered
by fetch, precise data #PF, pending/forged/duplicate/ABA publication, raw/notified
and secondary-page writes, changed mappings, running SMC, restore, active I/O
write/reset with deferred reclamation, nested compiler refusal, zero-retirement
REP recovery, capacity rejection and browser/export/table failure and late completion.
The bridge also rejects changes to its captured VM/exports/table identities.
Each invariants build publishes 900 actual legacy modules beside 32 IR records,
evicts legacy capacity, executes a surviving IR entry and reclaims all 899 slots.

The first capacity assertion used the existing free-slot statistic, which returned
zero without the profiler feature. Experimental IR builds now expose the actual
count through that export; ordinary production behavior is unchanged. The table
error fixture rejects function installation while permitting null removal, matching
the existing cleanup contract. No guest result expectation was weakened.

Review identified a missing initial instruction-fetch side effect in the first
cache prototype. Admission now performs the CPU's initial fetch translation and
rechecks code afterward. A code/PTE-alias test proves that changing opcode 03 to 23
through a page-table A-bit update rejects the pre-fetch artifact and executes the
new AND instruction. Secondary source pages require already-visible translations;
an unreachable cold page retains a clear A bit, while warm cross-page code can hit.

The experimental-only release passes the CPU optimization regression in
`build/ir-cache-cpu-regression.log`, including CALL/RET, cache SMC/restore, 2,304
arithmetic FLAGS cases, precise faults, MMIO partial writes, REP and 768 real-JIT
SSE cases. See [the cache contract](ir-cache.md). These are Node-hosted tests of
explicit publication; automatic IR compilation/promotion, Worker/browser hosting,
the full shared dependency/link graph, OS boot and performance acceptance remain
outside the evidence established here.

The connected IR matrix passed in `build/ir-cache-full-suite.log`: 121
warnings-as-errors Rust tests, all existing independent/CPU targets, entry/live
compilation tests, 39,936 CFG comparisons, 19,968 exact budget exits, all SIMD
families and final FLAGS observers. The final JIT-lock-availability guard was added
after that run began. `build/ir-cache-final-focused.log` separately validates the
final cache builds, including refusal to publish/collect inside a synchronous
legacy-generator host callback while its JIT mutex is held.

`build/ir-cache-final-regression.log` repeats entry/live tests on the final builds,
then passes `make all`, the ordinary 899-slot pressure suite, and 159 transactional
legacy publication cases in each debug/release build. The final experimental-only
CPU regression also passes (`build/ir-cache-cpu-regression.log`). Catalogue checks
pass with 3,728 production forms still Pending (`build/ir-cache-coverage.log`).
Experimental feature checking (`build/ir-cache-check.log`), ordinary debug build
(`build/ir-cache-production-debug.log`) and final export isolation
(`build/ir-cache-exports.log`) also pass. Ordinary debug/release omit IR/test/stack
pointer exports; the experimental-only release exposes the cache API without test
hooks. Whitespace checks pass. No browser-hosted or OS/performance acceptance was
added in this cache integration run.

## Opt-in automatic IR compilation and promotion

`build/ir-auto-final-focused.log` records the automatic policy and the entire
explicit-cache matrix passing on debug/release builds with JIT pool invariants and
an experimental-only release. Automatic tests independently check 32-bit loop
counts with recording off/on, 16-bit CS/AX behavior and a 42-instruction loop that
spans lightweight regions before larger optimized compilation. They cover failed
input suppression and changed-code retry, a rejected upgrade retaining executable
Tier 1 code, one current pending job, reset/out-of-order completion, snapshot
restore and recompilation, premature completion rejection and 40-entry capacity
eviction while retaining an explicit entry. Both invariants builds additionally
show actual recording-off legacy linked entries contributing heat, and check that
publication only starts after the legacy guest frame has returned.

The first capacity run observed 31 published entries at cancellation instead of
32: a global publication counter alone did not prove that the current entry had
executed and all current pending work had settled. The fixture now waits for an
actual cache hit and settled publication before checking the unchanged 32-record
bound. Review also found that evicted entries retained old heat. Eviction now clears
that heat; a stationary workload verifies no repeated recompilation of inactive
historical entries. Exact guest-state expectations and the capacity limit were
not weakened.

The policy's final candidate-credit change bounds scanning to once per main loop
even when nothing is ready. That change was made after the connected matrix had
begun; the final focused run above rebuilds and validates the automatic/cache paths.
`build/ir-auto-cpu-regression.log` records the experimental-only release passing
the existing CPU optimization regression with the policy at its default disabled
setting. See [automatic IR scheduling](ir-auto.md) for the new import/API, bounded
selection, failure lifetime and remaining production/Worker/OS/performance scope.

The connected matrix completed successfully in `build/ir-auto-full-suite.log`:
121 warnings-as-errors Rust tests, entry/live compilation, all existing independent
and CPU differential targets, 39,936 CFG comparisons, 19,968 exact budget exits,
all SIMD families and final FLAGS observers. The final policy-credit change is
covered separately as described above. Catalogue checks pass with 3,728 production
forms still Pending (`build/ir-auto-coverage.log`).

`build/ir-auto-policy-final.log` adds a final run with legacy generation disabled
in all three experimental builds. Automatic IR reaches optimized publication and
actual cache execution with exact loop counts, while an instrumented legacy
publisher records zero calls. The existing ordinary/interpreter fallback still
exists; this is not full-ISA or default-backend retirement evidence.

The final entry/live repeats, `make all` and ordinary 899-slot pressure passed in
`build/ir-auto-final-regression.log`. That run then failed a legacy publication
fixture with 0 instead of 12 after its fixed five-millisecond execution wait.
An unchanged repeat and twelve isolated diagnostic runs passed. Concurrent
diagnostic runs (`build/ir-auto-publication-stress.log`) reproduced the same class
of zero-result failure twice and observed **zero CPU main-loop calls**, with the
CPU still unhalted at the requested entry. The fixture now waits for actual guest
HLT completion with a ten-second watchdog, then performs the original exact-value
and actual-JIT-step assertions. Both debug/release publication suites pass in
`build/ir-auto-publication-fixed.log`, and all eight concurrent mixed debug/release
repeats pass in `build/ir-auto-publication-fixed-stress.log`. Guest expectations
and publication invariants were not changed.

Final feature checking (`build/ir-auto-check.log`), ordinary debug build
(`build/ir-auto-production-debug.log`) and import/export isolation
(`build/ir-auto-exports.log`) pass. Ordinary debug/release have neither IR exports
nor the automatic publication import. The experimental-only release has the new
API/import and no test hooks or exported stack pointer. Whitespace checks pass.
No browser-hosted policy, OS-boot or performance acceptance was added in this run.

## Public backend selection and CPU Worker integration

Added constructor `jit_backend: "legacy" | "ir"`, validated bounded
`ir_region_budget`, and copied `get_jit_info()` snapshots, with real CPU Worker
option transport and RPC. IR selection enables automatic scheduling and disables
legacy generation. `disable_jit` overrides both generators. Unsupported core and
configuration errors are reported before guest autostart. The starter's deferred
initialization completion now participates in its Promise error chain. Performance
recordings include the selected backend alongside the existing core hash metadata.

The shared public-API scenarios pass on invariant debug and pure experimental
release cores (`build/ir-backend-final-build.log`). They cover automatic Tier 1/2
publication and execution, copied effective limits/statistics, host SMC, unsupported
x87 interpreter fallback, save/restore/restart and initial snapshots crossing both
backend directions. Destination policy survives restore; generated caches do not.
Every IR-selected running scenario retains **zero legacy publication requests**.
Disabled/default modes execute the guest with their expected compiler settings.
Unknown/null backends, unknown budget keys, zero/out-of-range/fractional/NaN/null
budget values and a core without IR fail explicitly. Synchronous and asynchronous
custom Wasm loader failures also emit `emulator-error`.

The same scenarios pass in real Chromium main-thread and dedicated CPU Worker
modes (`build/ir-backend-browser.log`), using the repository's localhost server
and an isolated browser profile. The Worker scenario asserts there is no
main-thread CPU and uses only public APIs for guest memory and runtime statistics.
Custom Wasm callbacks are tested only on the main thread, since the Worker API
intentionally does not transfer callbacks. The first SMC exit fixture overwrote
an instruction boundary at which the guest could be stopped; the final patch has
valid exits at both original loop boundaries and retains the guest arithmetic,
mailbox return, cache execution and zero-legacy assertions.

An additional regression fixes legacy Tier 1 promotion ignoring `JIT_DISABLED`.
Debug/release tests retain a published Tier 1 module, disable generation for 128
actual CPU frames, assert no promotion or publication request, then re-enable it
and observe the same entry promote. These pass alongside all three auto/cache
variants and both 159-instantiation publication suites in
`build/ir-backend-regression.log`.

`build/ir-backend-build-suite.log` records successful `make all`, `make ir-tests`
(121 warnings-as-errors Rust tests, the independent decoder/Wasm/MIR oracles and
the 1,776-case interpreter/legacy/IR register comparisons), plus ordinary CPU
optimization regression. The pure experimental release passes that CPU regression
too (`build/ir-backend-experimental-cpu.log`). The final null-backend rejection and
expanded initialization-error scenarios are covered by the final production build
and focused Node/browser runs above. The existing real Worker CPU/display/memory,
cross-mode snapshot, I/O and performance-recorder regression passes in
`build/ir-backend-worker-regression.log`. Recorder metadata/lifecycle and the
300,000-command Worker screen test also pass (`build/ir-backend-recorder.log` and
`build/ir-backend-screen.log`). Whitespace checks pass. Standalone ESLint and
TypeScript executables are unavailable in this environment; Closure's configured
checks pass for production bundles.

No full ISA, Windows XP/application, GPU performance or IR-default retirement
claim is made. Previous unrelated GPU/PIC limitations remain as recorded above.
See [the public backend contract](ir-backend.md) for implemented settings and the
remaining optimization/verifier/dump/stats controls.

## Owned MIR lifetime and machine literal rewriting

`MirRegion` no longer retains HIR. A transient lowering transaction borrows HIR,
checks canonical memory/effect/call/control/value/materialization contracts and
machine types/local allocation, then constructs an owned artifact with shared-only
plan access. Runtime compile requests explicitly destroy HIR before optimization
and Wasm emission. The emitter consumes only the owned type and plan arenas.
Malformed-plan tests retain their original mutations and expected rejection, now
at transaction sealing rather than at emission. Target-specific emitter checks
remain in place. This is a restricted construction/ownership contract; it is not
an independent verifier for arbitrary mutable MIR graphs.

The new machine pass folds literal i32/i64 arithmetic, bit operations, masked
shifts, comparisons, bit counts, conversions and selects. It preserves value
definitions, local maps, all observations, helper/effect/state/control plans and
budget polls. Replacement expressions are checked against the owned type arena
and committed together after the bounded scan. Compile artifacts record
`mir_folds`; optimized standalone, CPU and CPU CFG requests exercise the pass.

Focused checks cover HIR destruction and repeated emission, AL/AH/AX rewriting,
idempotence, invalid types/local ownership, retained reads, nested literal folds,
all three compiler entry points and the actual one-million-step work-limit
failure leaving an earlier queued rewrite unchanged. The independent BigInt
oracle checks 6,384 literal programs unoptimized and optimized (12,768 actual Wasm
results) and 36 HIR-free narrowed-register MIR executions. Aggregate literal
fixtures shrink from 94,451 to 64,986 bytes; this is not a workload speed claim.

The first connected run (`build/ir-mir-owned-suite.log`) passes 126
warnings-as-errors Rust tests, independent decoder/Wasm/MIR/register oracles and
all three automatic compilation/cache variants. The subsequent budget-failure
fixture passes with the five other focused tests in
`build/ir-mir-owned-full-matrix.log`. No production source changed for that final
test-only addition. The real Chromium backend scenarios pass on the final
experimental core in `build/ir-mir-owned-browser.log`, in both main-thread and
dedicated Worker modes, including promotion, SMC, x87 interpreter fallback,
snapshot/reset, cross-backend restore and zero legacy publication requests.

Sealing currently recomputes allocation for validation and therefore adds bounded
lowering work. No compilation-time, OS/application or production-default
acceptance is claimed. See [owned MIR](ir-mir-owned.md) for the remaining graph,
SSA allocation and stack-scheduling work.

The complete connected differential matrix finished successfully in
`build/ir-mir-owned-full-matrix.log`: shared production analysis, integer/memory/
stack/control/string/REP families, all implemented system and SIMD families,
final FLAGS observers, entry/live compilation and public backend integration.
It includes 39,936 reachable-CFG comparisons and 19,968 exact optimized/unoptimized
budget exits. Final focused warnings-as-errors checks pass in
`build/ir-mir-owned-final-native.log`; the six owned-MIR tests include the late
test-only budget-failure addition. The first full 126-test run plus that additional
test account for 127 distinct passing native tests, not a single 127-test invocation.

Production `make all` and the ordinary CPU optimization suite pass in
`build/ir-mir-owned-production.log`; the pure experimental core passes the same
CPU regression in `build/ir-mir-owned-experimental-cpu.log`. Direct import/export
inspection confirms the normal core has no automatic IR publication import or
IR runtime export, while the experimental core has them without differential
test hooks. Catalogue/coverage checks pass in `build/ir-mir-owned-coverage.log`
with 3,728 production forms still Pending. Whitespace checks pass.
The separate bounded REP-engine/reference matrix also passes in
`build/ir-mir-owned-rep-engine.log`, including early termination, maximal/zero
counts, fault reentry, overlap/alias copies, I/O permissions and segment overrides.
