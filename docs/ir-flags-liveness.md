# IR-10: CPU-only partial FLAGS liveness

This increment is stacked on the complete lazy-FLAGS provenance work. It keeps
generic HIR and standalone StateMaps complete while allowing optimized Tier 2
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

## Supported exact backing

The first exact lazy-backing set is deliberately small:

- ADD;
- SUB and CMP;
- AND, OR and XOR;
- TEST and NEG through the same audited arithmetic forms;
- XADD/CMPXCHG where they reuse those arithmetic semantics.

Logical operations clear the eager CF/AF/OF bits and make only PF/ZF/SF lazy.
ADD makes all arithmetic flags lazy. SUB/CMP additionally carry the baseline
`FLAG_SUB` marker.

ADC/SBB, INC/DEC, shifts/rotates, bit operations, multiply, SAHF/BCD and other
partially eager flag layouts invalidate the proof. Once invalidated inside a
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

The generic CFG, allocations and standalone emitter are unchanged. The CPU
emitter consults the certificate only for pure value programs.

## Tier policy

The certificate is disabled by default. It is enabled only for optimized Tier 2
CPU compilation with nonzero optimization rounds. Tier 1 and standalone output
remain unchanged. `passes.cpu_values_elided` reports the static number of pure
CPU value programs skipped by emission.

## Validation

Focused tests require an ADD -> ADD -> JNZ region to produce a smaller CPU Wasm
module after CPU liveness while producing byte-identical standalone output.
INC/JNZ is a negative case and must retain canonical recovery.

The existing reachable-CFG CPU differential also enables the certificate. An
additional ADD/SUB/AND/CMP/JNZ fixture compares raw flags, `flags_changed`,
`last_result`, `last_op1` and `last_op_size` against interpreter execution
at the same budget exit.

## Still open

The validity model is intentionally conservative. Extending exact backing to
ADC/SBB, INC/DEC, shifts, rotates and other mixed eager/lazy operations can
unlock more partial FLAGS elimination. General dirty-state merging across
resumable helper/MMU callbacks, memory LICM, remaining ISA coverage, system and
performance validation, and IR-14 retirement remain separate work.
