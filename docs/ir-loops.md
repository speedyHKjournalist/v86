# Experimental counter branches

LOOPNE/LOOPE/LOOP (E0/E1/E2) and JCXZ/JECXZ (E3) now use native SSA and the
same terminal branch exits as Jcc/JMP. A shared relative-branch frontend owns
condition selection and taken/fallthrough StateMaps. This implements individual
counter-branch instructions; it does not complete CPU regions with internal cycles
or their dynamic guest accounting.

## Counter, FLAGS and target semantics

Address size selects CX or ECX independently of operand size. LOOP variants
subtract one modulo the selected width and retain the upper ECX half for CX.
They do not update arithmetic FLAGS or retained `last_op1`. LOOPNE additionally
requires ZF clear, LOOPE requires ZF set, and LOOP needs only a nonzero result.
An initially zero counter wraps before that test. JCXZ/JECXZ tests the selected
counter without decrementing it.

The signed byte displacement is added to decoded next PC. The pinned CPU uses
operand size for taken target wrapping: 16-bit targets are masked to FFFF.
Untaken fallthrough retains the decoded next PC, including a 64 KiB crossing.
CPU exit materialization adds CS base exactly once and accounts for the completed
instruction. Standalone output writes the guest-relative selected PC.

A taken branch commits before the following target fetch. It introduces no target
page access of its own. A subsequent #PF therefore saves the selected target and
already-updated counter, and does not undo or recount the branch. The frontend
still requires these branches to end the supplied linear snapshot; online region
discovery, internal cyclic CPU graphs and linking remain separate unfinished work.

## Evidence

`make ir-loop-tests` generates 960 counter-branch fixtures in optimized/unoptimized
CPU and standalone forms, plus six composed CPU regions. Tests cover both decode,
operand and address modes, positive/negative/zero displacements, logical PC zero,
64 KiB boundaries and 32-bit wrap, with nonzero/wrapping CS bases.

- 138,240 exact CPU comparisons with boundary ECX values and both materialized
  and lazy FLAGS, also checked against an independent counter/target reference.
- 69,120 standalone executions checking all GPRs, FLAGS, guest PC, retained
  operand and committed count.
- 1,048,576 exhaustive cases spanning every CX value, both ZF inputs, all four
  branch kinds and both optimization configurations; ECX high halves are retained.
- 432 composed arithmetic/counter/branch regions, including INC ECX, DEC CX,
  constant counter definitions and FLAGS produced within the region.
- 64 real following-target #PF cases, verifying exact frames after the branch
  has committed, across independent widths and nonzero CS bases.
- Rejection of an interior branch in a linear snapshot and LOCK-prefixed forms.

Existing Jcc/JMP tests run through the extracted shared frontend. The experimental
catalogue gains 16 TerminalBranchHIR forms (152 total). Production Pending remains
3,728; full IR tiers, internal CPU loops, OS/performance acceptance and legacy
emitter retirement remain incomplete.
