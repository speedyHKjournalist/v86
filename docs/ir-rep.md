# Experimental REP HIR and terminal batch ABI

REP MOVS/CMPS/STOS/LODS/SCAS/INS/OUTS now lower to named, contracted CpuRep helper
calls. The shared [bounded semantic engine](ir-rep-engine.md) retains the audited
native bulk/element and slow memory/device behavior; this is not an InterpretOne
opcode escape. Non-REP forms keep their existing frontends. Production tiers,
IRQ scheduling and code-generation validation at reentry remain unconnected.

## Progress StateMaps and outcomes

A terminal REP call carries `ResumeKind::RepProgress` with explicit ECX/ESI/EDI
sources. The verifier requires those sources to equal the corresponding complete
GPR mappings, with no competing dynamic next EIP. The current materializer can
therefore restore every progress field using its ordinary GPR writes. Missing or
distinct hidden progress values are rejected; XMM/x87 materialization is still
pending. Generic valid aliased progress maps are no longer rejected wholesale.

The call must end its block with the same StateMap. Before calling, CPU GPRs,
FLAGS/last_op1 and completed-prefix accounting are materialized; previous_ip is
the REP instruction and instruction_pointer is decoded next IP. No segment or
operand check is moved ahead of the engine's zero-count test.

`HelperAbi::CpuRep` is a separate terminal contract:

- ControlTransferred (2): CPU-owned fault state and partial progress are
  authoritative; no final REP instruction commit occurs.
- Yield (3): remaining work resumes at the REP instruction with updated CPU
  ECX/ESI/EDI and the engine's FLAGS policy; no final instruction commit occurs.
- Invalidated (4): execution has completed, including zero-count completion;
  the adapter increments instruction_counter once and exits.

Normal and caller-delivered faults are invalid for this ABI. All three valid
outcomes exit without restoring the pre-call SSA snapshot. CpuExit retains its
stricter prohibition on Yield. Descriptor validation requires helper-owned
exceptions, no data results, transfer/invalidation effects and yielding capability.
Standalone emission rejects terminal CPU helpers.

The seven named adapters share scheduling/accounting code but keep their semantic
family identity. F2/F3 are explicit encoding-table entries; duplicate prefixes
retain the decoder's F2 precedence. REP cannot be followed by another instruction
inside the current linear artifact, and LOCK remains rejected.

## Element work versus instruction commits

`IrConfig.rep_iteration_budget` is independent of the existing block execution
budget. It accepts zero through 4,096 elements; the convenience `lift_cpu` API
uses 128, and `lift_cpu_with_rep_budget` exposes the explicit setting. The compiler
passes this limit as an ordinary typed helper argument. A zero budget can yield
without doing element work; zero count still completes first.

`ir_rep_result()` reports the last REP call's outcome in the low word and completed
elements in the high word. This runtime-only record is reset by every `ir_enter`
and by each REP adapter before work. Zero means no REP result in the current entry.
The cold caller must consume it before another entry. It is not guest snapshot
state, a global execution counter or an implemented scheduler.

A prefix instruction is charged by the caller's StateMap. Partial batches report
elements without charging a completed REP instruction. The final adapter call
charges the REP exactly once. The semantic engine itself continues to leave
instruction_counter untouched. Faults report any completed elements while
preserving the CPU-delivered exception and omitting a final REP commit.

After Yield, the caller must enter at the current REP PC, not replay an artifact's
earlier prefix. Tests use separate initial and resume artifacts. Online callers
must also revalidate code/context generations, mappings and pending interrupts;
the metadata/result API does not establish that protocol. In particular, a
self-modifying REP cannot blindly reuse an old resume artifact. General internal
CPU CFG cycles remain rejected until dynamic commit accounting is implemented.

## Evidence

`make ir-rep-tests` generates 1,176 cases, each with initial/resume artifacts,
optimized/unoptimized code and budgets zero/one/three/128. The initial artifact
contains an INC before REP. Mode/address width, byte/word/dword element size,
all seven semantic families, F2/F3 and default/all segment overrides are covered.
The CPU oracle uses the independently pinned string body described in the engine
contract. The reusable `rep_cpu.mjs` harness is shared with that engine suite.

- 14,112 CPU/observer comparisons with exact FLAGS, last_op1 and prefix state.
- 672 bounded reentry sequences accounting for 11,424 elements and exactly one
  final REP instruction commit per completed sequence.
- 672 zero-budget/count and metadata-reset cases, 336 maximal unsigned counters,
  and 336 zero-count completions before invalid operand segments/mappings.
- 336 complete memory/port device sequences and 1,176 partial-fault/page-reentry
  comparisons. Faults retain completed work and never commit the REP as complete.
- 384 REPE/REPNE terminations across budget cuts and 96 I/O permission faults
  before any element work.
- A compiled immutable CompileRequest fixture proves that its separate REP budget
  reaches executable code. Twelve injected helper outcomes reject Normal/invalid
  returns and preserve authoritative CPU registers, IP and instruction counters.

Rust checks cover the terminal ABI, progress-map aliases, missing state, invalid
budgets, standalone rejection, duplicate-prefix precedence and linear-region
boundaries. The engine suite separately checks physical aliases/LZ overlap,
maximal counters, exact fault outcomes and baseline wrapper compatibility.

The catalogue adds 84 CpuRepHelper forms from its explicit F2/F3 encoding rows.
This records experimental semantic-helper coverage; it does not claim an optimized
SSA REP loop, online scheduling, full mode/prefix acceptance or OS compatibility.
Production Pending remains 3,728. Full ISA, MIR/regions, online tiers/invalidation,
advanced optimization, OS/performance acceptance and legacy retirement remain open.
