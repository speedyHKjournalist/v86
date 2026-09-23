# IR-10: CPU-only partial FLAGS liveness

This increment is stacked on the complete lazy-FLAGS provenance work. It keeps
generic HIR and standalone StateMaps complete while allowing optimized
CPU emission to omit pure value programs that are not required by CPU recovery
or explicit guest semantics.

## Why CPU-only

Every guest instruction boundary can be a budget/recovery point. Removing an
arithmetic flag merely because a later instruction overwrites it would be wrong
if recovery at the intermediate boundary still needed the concrete flag value.

The CPU recovery plan can instead preserve v86's exact lazy backing when it is
known precisely. For the first supported scalar ALU set that backing consists of:

- raw EFLAGS backing;
- full `flags_changed`;
- `last_result`;
- `last_op1`;
- `last_op_size`.

The six concrete arithmetic SSA flags remain present in generic HIR. Standalone
emission therefore keeps its existing behavior.

Incoming arithmetic flags are separate `ReadFlag(bit)` roots. CPU emission calls
the matching canonical getter only when that bit is demanded; a plain ADD needs
none, INC/ADC need CF, and an incoming JZ needs ZF. The standalone ABI extracts
the same bit from concrete FLAGS. `ReadSystemFlags` loads only non-arithmetic
bits, so retaining DF/IF or another system bit no longer roots `get_eflags`.
System and raw backing keep distinct entry SSA identities for CFG grafting.
Their origin proofs also distinguish a masked system value from full FLAGS:
extracting CF from a system-only value cannot prove incoming CF equivalence.
CpuReload still uses its established full FLAGS ABI; its system bits are sourced
from the accompanying raw reload value.

## Supported exact backing

The first exact lazy-backing set is deliberately small:

- ADD;
- SUB and CMP;
- AND, OR and XOR;
- TEST and NEG through the same audited arithmetic forms;
- XADD/CMPXCHG where they reuse those arithmetic semantics;
- ADC/SBB, with eager CF/AF/OF stored in raw flags and PF/ZF/SF left lazy;
- INC/DEC, with the incoming architectural CF stored eagerly and the remaining
  arithmetic flags left lazy;
- SHL/SHR/SAR and SHLD/SHRD, with CF/OF eager and PF/AF/ZF/SF lazy, including
  count-zero preservation of the complete incoming backing;
- ROL/ROR/RCL/RCR, which update only eager CF/OF and preserve the previous lazy
  mask, last_result, last_op1 and last_op_size;
- BT/BTS/BTR/BTC, which update only eager CF;
- BSF/BSR, with eager CF/ZF and the baseline undefined-flag lazy policy;
- POPCNT, whose arithmetic flags are fully eager;
- MUL/IMUL, with eager CF/OF and the baseline undefined PF/AF/ZF/SF lazy policy;
- CLC/STC/CMC and CLD/STD raw-EFLAGS control-bit updates.

Logical operations clear the eager CF/AF/OF bits and make only PF/ZF/SF lazy.
ADD makes all arithmetic flags lazy. SUB/CMP additionally carry the baseline
`FLAG_SUB` marker. ADC/SBB use the baseline mixed mask that excludes
CF/AF/OF, while SBB also carries `FLAG_SUB`. INC/DEC exclude only CF from the
lazy mask and DEC carries `FLAG_SUB`.

SAHF/BCD and other not-yet-audited partially eager flag layouts invalidate the proof. Once invalidated inside a
region, later ALU instructions do not guess the backing valid again.

## Liveness certificate

Lowering creates a bounded CPU-only certificate. Its roots are:

- values used by CPU StatePlans;
- count and decoded-next materialization;
- explicit conditional-branch conditions;
- ordered/effect/helper/poll operations and their arguments.

For a live block parameter, only the matching predecessor edge argument is
traced. Edge arguments are not all rooted unconditionally. This is what allows
dead concrete FLAGS phis and their producers to disappear from CPU emission.

The generic CFG and allocation proofs remain complete. CPU emission also rebuilds
parallel-copy schedules from only demanded destination block parameters. It
filters the original SSA assignments before scheduling, so removing a dead phi
cannot discard a scratch save needed by another move. Owned-MIR verification
reconstructs the schedule from the retained SSA edge facts; a work-budget fallback keeps the generic
schedule, and later machine reallocation invalidates cached CPU edge schedules.
Standalone emission continues to use the complete generic schedule.

The Wasm emitter declares allocated locals on first use. Unused CPU values and
phi destinations therefore need no Wasm local, and fresh declarations rely on
Wasm's entry-zero initialization instead of explicit zero stores. These
declarations never reuse freed temporary slots, whose old values could otherwise
survive along a control-flow path. Legacy builder allocation APIs are unchanged.

## Tier policy

The certificate is disabled during lowering, then enabled for optimized CPU
compilation in both tiers when optimization rounds are nonzero. Tier 1 computes
one demand mask after enabling only entry-equivalence state certificates already
checked by lowering. It does not run Tier 2 post-observer backing analysis,
helper-state trimming, stack scheduling, machine reallocation, RAM forwarding or
LICM. This removes otherwise eager concrete FLAGS calculations from cold code
whose recovery plans carry exact lazy backing. Standalone output is unchanged.
The FLAGS and state-write pass switches remain independent.
`passes.cpu_values_elided` reports the static number of pure CPU value programs
skipped by emission.

## Validation

Focused tests require an ADD -> ADD -> JNZ region to produce a smaller CPU Wasm
module after CPU liveness while producing byte-identical standalone output.
Dedicated ADC/SBB/INC/DEC/JNZ and SHL/ROR/BT/POPCNT/IMUL regions require every
audited recovery state to remain eligible for exact lazy backing.

The existing reachable-CFG CPU differential also enables the certificate.
ADD/SUB/AND/CMP/JNZ and mixed ADC/SBB/INC/DEC/JNZ fixtures compare raw flags,
`flags_changed`, `last_result`, `last_op1` and `last_op_size` against
interpreter execution at the same budget exit. The mixed fixture runs in both
16/32-bit default modes and has an additional byte-width variant.

Additional regression cases reject arithmetic-bit origin claims derived from
system-only FLAGS, check incoming getter demand (ADD none, ADC/INC CF, JZ ZF,
SAHF OF), and preserve separate system/raw values across STD/JZ/CLD joins.
A mixed live/dead phi cycle is simulated against the complete parallel assignment;
forged demand/schedules, exhausted work budgets and later reallocation are checked.
Builder execution tests poison and free I32/I64/V128 temporaries, declare fresh
locals across index 255/256, and require zero initialization on repeated calls.

The 2026-09-23 follow-up passed 284 native tests (four explicit benchmarks ignored)
with warnings denied and the full standalone Wasm execution suite. A single
immutable XP replay compiled all 2,540 captured inputs without failure and
emitted 34,236,297 bytes. This is a compiler-corpus check; it does not establish
an XP speedup or isolate the contribution of this pass.

## Conservative fallback and completion boundary

The validity model remains intentionally conservative. SAHF/BCD and remaining
special/undefined flag layouts canonicalize at recovery instead of inventing an
unproved lazy backing. This is a supported negative/precondition path of the
completed IR-10 FLAGS pass, not a correctness gap in its liveness analysis.

IR-10 also adds audited helper-state trimming for state-independent, nonfaulting
helpers. Broader dirty-state specialization across resumable helper/MMU
callbacks, memory LICM, remaining ISA coverage, system/performance validation
and IR-14 retirement remain later work. See [IR-10 completion](ir10-completion.md).


## Extended backing bundle

The next bundled increment broadens exact CPU recovery without changing generic
HIR or standalone materialization:

- variable and immediate shifts preserve complete backing when the effective
  count is zero, and otherwise update eager CF/OF plus last_result/op-size;
- rotates and through-carry rotates update only raw CF/OF and clear those bits
  from the previous lazy mask;
- bit tests update only raw CF; scans use eager CF/ZF with the pinned baseline
  undefined-flag lazy representation; POPCNT makes arithmetic flags fully eager;
- MUL/IMUL use eager CF/OF while preserving the baseline lazy undefined flags;
- carry and direction control instructions update the corresponding raw EFLAGS
  bit instead of invalidating the whole certificate.

Dedicated differential suites compare the raw CPU backing fields to the
interpreter, not only architectural get_eflags().
