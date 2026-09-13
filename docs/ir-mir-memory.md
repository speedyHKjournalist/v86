# Explicit memory paths in transitional MIR

Ownership update: [owned MIR](ir-mir-owned.md) supersedes the retained-HIR lifecycle described in this stage report. Canonical checks now run at transaction sealing; the resulting read-only machine artifact owns its types/plans and is emitted after HIR is released. Historical test results below remain stage-specific.

Lowering now produces a MemoryPlan for every GuestLoad, GuestStore, PartialStore,
RmwLoad and implemented XMM memory operation. Wasm emission dispatches by this
plan before its remaining shared-HIR operations. The former scalar/vector memory
branches no longer select guest memory widths, CPU adapter names or shuffle maps
from HIR opcodes during emission.

## Representation and execution

A plan records:

- the linear-address SSA value and BeforeInstruction StateMap;
- RAM guard width, TLB flag mask/required bits, CPL3 restriction and the exact
  page-offset limit;
- a native physical action: scalar load/store, load with an RMW ticket, vector
  load with an optional combine, or vector store with an optional byte mask;
- the CPU call's complete signature and ordered SSA/immediate arguments;
- slow result handling: packed value/fault, RMW ticket and value, committing or
  partial store, or terminal CPU-owned success/fault.

The guard remains adjacent to its native action. Writes and RMW reads exclude
read-only and code pages, all accesses exclude MMIO and invalid TLB entries,
and CPL3 includes the user permission restriction. The page-offset limit is
`4097 - bytes`; MASKMOVDQU retains a sixteen-byte guard even for zero masks.
An RMW fast read produces its physical ticket before loading the value.

Lowering fixes vector transfer widths, shuffle byte maps, register/lane adapter
arguments and CPU-exit outcomes. PINSRW uses a two-byte load plus word replacement;
UNPCKLPS/LPD retains its pinned eight-byte read; full packed operations use sixteen
bytes. Pure vector combines remain explicit vector operations. The emitter only
encodes the selected physical operation and its structured fast/slow branches.

Slow edges materialize the specified pre-instruction state and decoded next PC,
then issue the planned call. Packed reads and RMW tickets retain their respective
fault protocols. Partial stores rejoin only on the existing success outcome;
committing stores restore the specified commit map and return. XMM CPU paths
accept only outcomes 2 and 4 and return without stale SSA restoration. Existing
late-fault host-abort policies remain explicit in the plan.

## Validation and diagnostics

MIR plans remain associated with their HIR instruction positions. Emission first
verifies HIR and checks the plans against canonical lowering of that HIR. This
rejects missing, stale or altered plans before byte emission, including weakened
permission/range checks, changed calls or arguments and changed exit policies.
The association check is a consistency check, not independent proof of the
lowering semantics; existing independent-model/CPU execution suites provide that
evidence. Arbitrary plan optimization is not enabled by this check.

Memory calls and the additional RMW-value call participate in lowering's import
signature conflict detection alongside generic helper declarations. The existing
CPU import-name restrictions also remain in force. `dump::mir` prints each plan,
its guard, native action, call arguments/signature and result policy next to the
remaining shared operations and block terminator.

Three focused tests check seventeen scalar/vector/RMW access forms before and
after optimization, ENTER partial-store/late-fault behavior, ten unsafe mutations,
missing plans, stale HIR store lanes and conflicting memory/helper signatures.
They are part of `make ir-tests`; focused evidence is
`build/ir-mir-memory-contracts.log`.

## Remaining MIR work

This is an executed migration of memory selection and policy into lowering, not
completion of IR-03 or IR-09. MirRegion still shares a cloned HIR value/state/CFG
arena and the existing local allocator. Plans introduce no new SSA values, so
all their inputs/results/StateMap references already participate in HIR liveness.
RMW commit, segment/stack address adapters, access checks and SSE guards have
subsequently moved to [effect plans](ir-mir-effects.md). CMPXCHG8B and division
now use [arithmetic plans](ir-mir-arithmetic.md). Generic helper observations use
[call-site plans](ir-mir-calls.md). Other remaining operations still have emitter-side policy. Full
independent MIR instructions/CFG, explicit materialization/call operations,
proof lifetimes, dynamic budget/commit accounting and MIR optimization remain
necessary before production Tier integration and legacy retirement.

The full connected suite passed in `build/ir-mir-memory-full-suite.log`, with
90 warnings-as-errors Rust tests and all existing IR/independent-reference targets.
This executes the new scalar, RMW-read, partial-store and XMM plan paths under
native RAM, MMIO, callback changes, faults and reentry. Experimental Wasm
compilation passed in `build/ir-mir-memory-check.log`; production export isolation,
generated catalogue and whitespace checks also passed. ISA coverage is unchanged.


Control dispatch subsequently moved into a [lowered MIR CFG](ir-mir-control.md),
including entry selection, branch conditions, budget recovery and typed scheduled
edge copies. The graph preserves HIR topology; independent graph transforms,
standalone MIR verification and the shared instruction/state arenas remain work
in progress.
