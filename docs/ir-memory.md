# Experimental CPU memory path

This is an executable IR-06 subset, not completion of IR-06 or live Tier 1.
`compile_cpu_region` / `lift_cpu` / `emit_cpu` create artifacts that run against
actual CPU memory outside the legacy `in_jit` frame. They are not automatically
published into the CPU's tier/cache/linking system. Normal builds still use the
legacy backend; the adapters compile only with experimental IR or tests.

## Native path and precise slow path

MOV, MOVSX/MOVZX, integer ALU/CMP/TEST, INC/DEC/NEG/NOT, SETcc and CMOVcc use explicit
`SegmentAddress`, `GuestLoad` and `GuestStore` nodes with effect tokens and fault
maps. Widths are 8/16/32; effective address arithmetic includes 16-bit wrapping,
SIB and segment overrides. The path also supports [stack operations](ir-stack.md),
[shifts/rotates](ir-shifts.md), [multiply/divide](ir-multiply.md),
[bit operations](ir-bits.md), and [exchange/compare-exchange](ir-exchange.md).
Audited LOCK forms carry explicit paired ordering under the nonshared, single-owner
CPU ABI described in the exchange contract. [ENTER](ir-enter.md) adds frame-chain
loads between partial writes with explicit baseline fault/device policies. [Moffs/XLAT](ir-misc.md) use the same precise path. Wide RMW and other
memory families remain pending.

Segmentation checks the actual null-segment state and adds the actual base.
A null segment invokes the audited CPU adapter and exits after its #GP delivery.
The native RAM guard reads the CPU TLB and checks validity, MMIO exclusion,
CPL3 permission, width/page containment, and for stores writable/dirty validation
and code-page exclusion. The guarded host address and its load/store are emitted
together with no intervening helper or safepoint. No general reusable RAM proof is exposed
to optimization yet; GVN/DCE still preserve every memory/effect operation.

Missing translation, cross-page access and MMIO call the existing CPU safe-memory
semantics. Read/segment adapters return `(outcome << 32) | u32_result` as one Wasm
i64; store adapters return an i32 outcome. They are Wasm-to-Wasm imports and do not
need a JS multivalue shim. `safe_*` owns real exception delivery. A delivered fault
returns ControlTransferred and generated code exits immediately without restoring
the pre-fault state. Narrow data and outcome fields are decoded separately.

The initial implementation exits after **every successful committing store**, including a
native RAM store through an alias of its own code. Its explicit `commit` StateMap
specifies the successful next PC and state independently of the pre-store fault
map; it is verified and remains live through DCE and local allocation. This avoids
executing following stale instructions while IR publication/invalidation is still
pending. Stores cannot yet stay inside optimized loops/regions. Existing legacy
code-page dirtying remains in the CPU slow write semantics.

## CPU state ABI

CPU layout comes from `global_pointers`: GPRs, architectural/backing FLAGS, instruction
counter, CS base, current IP and previous IP. Entry reads lazy FLAGS through
`get_eflags`; raw ZF is also loaded separately. CPU materialization preserves that
backing bit and its lazy marker (only ZF can remain lazy) (see [selector queries](ir-selector-query.md)); all
other arithmetic bits are materialized. StateMap EIP is CS-relative,
whereas CPU current/previous IP is linear. Before a device callback, current IP
reflects fully decoded operands (next PC), while previous IP retains the fault PC.

Accounting adds the difference between the last materialized count and the new
committed count once per invocation. Faulting instructions are not committed.
CPU graphs with cycles are rejected until dynamic guest commit accounting exists;
standalone SSA-loop fixtures remain supported. CPU entry asserts it is not inside
an existing legacy JIT frame. No change to the old JIT entry/exit ABI is implied.

## Evidence and remaining scope

`make ir-memory-tests` generates fixtures and executes them on a real Wasm CPU:
2,040 optimized/unoptimized integer memory comparisons against exact interpreter steps,
340 instrumented native warm-RAM paths, 36 real #PF/#GP comparisons (including a
cross-page store with no partial write and a single exception frame), 340 MMIO
callback/state comparisons, non-zero CS exits and self-modifying aliases. A
supervisor-TLB/CPL3 negative check confirms that access is routed to the slow path;
it does not claim full ring3 exception delivery coverage.

Remaining: complex stack/other integer memory forms, complete segment/mode and ring3
fault matrix, wide/FP memory, explicit reusable guard proofs, region-wide memory
optimization, dynamic commit counters, stores continuing under validated
invalidation dependencies, code publication and full Tier 1/Tier 2 integration.
The production coverage gate remains Pending.

## RMW read/commit protocol

`RmwLoad` validates write permission for all accessed pages before reading any
RAM or device. It produces data plus an affine `RmwTicket`; native ALU operations
compute the new data and FLAGS in SSA, and `RmwStore` consumes the ticket once.
The verifier rejects ticket parameters, ticket use by other operations, mismatched
width/order/fault maps, uncommitted tickets, and any intervening ordered operation.
The ticket is a distinct runtime i64 type, with typed local allocation; it is not
an integer a pass may fold or an architectural value a StateMap may restore.

The warm path keeps a guarded native host pointer. The slow path stores the
pretranslated physical first/last addresses in the ticket. This is necessary:
a device read can modify page tables or TLB state, but the baseline RMW writes
the already-selected physical address. Tests deliberately remap the address from
MMIO to RAM during the read and require the subsequent write to hit the original
device; noncontiguous cross-page physical mappings are also compared.

The slow read returns its ticket directly as i64 and publishes the data in a
private scratch scalar only after all read callbacks return. Codegen immediately
reads that scalar with a non-reentrant getter. The scratch is not a guest snapshot
field and no pointer to it escapes. The ticket encoding distinguishes native,
slow same-page and slow cross-page addresses; the all-ones failure sentinel cannot
be a canonical ticket. On failure the CPU has delivered the fault and IR exits.

Before a slow physical/device write, the successful ALU/register state is made
observable while the committed-instruction count still excludes the RMW. The
write uses saved physical addresses and cannot introduce a new translation fault.
After writing, the explicit commit map accounts for the completed instruction and
exits the region, as for other stores. No generic instruction interpreter helper
performs the ALU computation.

The cross-page fault differential exposed a CR2 mismatch: the baseline dword RMW
queries the aligned second-page word. The adapter now uses that exact lookup
address, then restores the low offset when constructing its physical ticket.
MMIO permission/cross-page failure tests require zero read callbacks, unchanged
FLAGS/memory, and exactly one fault frame. A false CMOV condition still reads its
memory operand and therefore still faults on an inaccessible address.


Multi-access PUSHA now has explicit noncommitting `PartialStore` operations before
its final committing store. A verified sequence cannot cross the guest instruction
boundary, and the final write still exits. `GuestCheck` preflights page permissions
without a data access; it grants no reusable guard proof. See [ir-stack.md](ir-stack.md).
