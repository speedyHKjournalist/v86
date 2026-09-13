# IR-11 continuation: bounded metadata and no-motion paths

This continuation is based on `ir` at
`75a9d125aec9f4c19dcde5c1ddef17b9cb949f86`, which already contains the
transactional Tier 2 LICM from PR #5. It preserves that implementation's
`Config { max_work, max_hoisted }`, statistics, pipeline and recovery tests.
It does not replace them with the earlier competing implementation.

## Added guarantees

Before verification or cloning, LICM now checks not only top-level arena
counts but also the aggregate size of variable-length metadata. This includes
block parameters/instruction lists, edge arguments, instruction operands and
results, XMM/x87 recovery vectors, helper names/signatures and optional
fault-delivery names. A shared cap of 131,072 items/bytes rejects oversized
inputs without following possibly invalid IDs. Entry and helper arena counts
are bounded as well. The existing 4,096-state cap is preserved.

The work counter still measures loop discovery and candidate processing,
not elapsed time or the verifier's internals. Metadata bounds constrain the
input to those separate operations; they are not a wall-clock guarantee.

Acyclic regions and loops without legal preheaders return after analysis,
avoiding a full HIR clone. If loops exist but no instruction moves, the original
arena allocations remain in place and redundant final verification is skipped.
Input validation remains mandatory, and all transformed candidates still pass
final verification before any modification is committed.

## Regression coverage

`src/rust/ir/passes/licm/limits.rs` adds tests for allocation preservation,
zero-work/invalid-input rejection and eleven oversized-metadata cases,
including several small containers whose aggregate exceeds the cap. Existing
LICM/CPU/Wasm tests remain unchanged and must also pass.

The CI jobs additionally execute selected CPU CFG, loop, memory and stack
differentials. Report actual per-commit results, not merely configured jobs.
No XP/application speedup or completion of IR-00 through IR-14 is claimed.
The legacy default and ISA acceptance gate remain unchanged.
