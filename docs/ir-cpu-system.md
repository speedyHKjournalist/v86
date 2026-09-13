# Experimental system transfers and halt-state adapters

SYSENTER/SYSEXIT, HLT, CLI, CLTS and WBINVD now lower to separate named CpuExit
helpers. Each ends the current cold CPU artifact. LOCK, standalone execution and
a following instruction in the same linear artifact are rejected. These are
explicit semantic adapters; no arbitrary InterpretOne or legacy JIT emitter is
used. The catalogue adds twelve CpuSystemHelper forms.

## Commit and exception ownership

The BeforeInstruction map materializes prior GPR/FLAGS/last_op1 changes and counts
completed prefix instructions. It supplies the exact previous instruction PC and
decoded next PC, including a nonzero CS base. The adapter returns Invalidated (4)
and increments instruction_counter once only after a successful semantic body.
Permission failures call trigger_gp(0) once and return ControlTransferred (2),
without committing the failed instruction. Both outcomes exit with CPU state
authoritative; the old SSA snapshot is never restored after a transition.

SYSENTER requires protected mode and a nonzero masked SYSENTER_CS. SYSEXIT also
requires CPL zero. The audited bodies have no further guest-fault path after
these guards. They construct fixed CS/SS caches, set CPL and 32-bit code/stack
widths, change EIP/ESP, update cached state flags and invalidate last_virt_eip.
SYSENTER clears VM/IF; SYSEXIT preserves the baseline FLAGS policy. Selector masks,
16-bit selector wrap and full 32-bit target values retain CPU behavior. Successful
transfers do not read GDT descriptors or access the destination instruction/stack.
A later target fetch can fault with the already-committed new execution context.

CLI uses instr_FA_without_fault's explicit success predicate, including the pinned
CPU's disabled VME/PVI virtualization branches. CLTS/WBINVD require CPL zero.
CLTS clears CR0.TS; WBINVD retains the baseline no-op semantics. Other CPU cache or
feature policy is not changed. In VM86 with CR4.VME set, the shared exception
dispatcher currently panics; tests preserve and catch that existing behavior.
This is not newly implemented VME support or a successful #GP delivery.

HLT checks CPL before any halt/timer action. On success, the CPU sets in_hlt,
then either notifies halt with IF clear, or reads the clock, runs timers and
handles pending IRQs with IF set. An IRQ can clear in_hlt and transfer execution
before the helper returns. The adapter commits after these synchronous
observations and preserves their final state. Invalidated denotes a terminal
artifact exit; the host must inspect authoritative CPU halt state before any
future entry. This is not an implemented online idle/scheduling loop.

STI remains unsupported: its baseline semantic body executes the following
instruction under an interrupt shadow. Calling that arbitrary execution path
would bypass IR lowering and require distinct accounting/fault semantics.
These six adapters do not claim that shadow or general interrupt polling support.

## Verification

`make ir-cpu-system-tests` generates 84 fixtures with INC ESI before the terminal
operation, both 16/32-bit modes and seven prefix combinations. Optimized and
unoptimized Wasm run against the corresponding real debug CPU interpreter. State
comparison includes all GPRs, FLAGS/last_op1, CRs, EIP/previous_ip, CS/SS and other
segment caches, CPL, code/stack width, cached state flags, SYSENTER MSRs, halt state
and exact fault/interrupt stack bytes.

- 672 ordinary state, nonzero CS-base and stack-width cases.
- 3,024 CPL/IOPL/VM86/VME/PVI cases, including checked baseline VME aborts.
- 504 zero/masked/wrapped selector and extended-FLAGS cases.
- 168 real-mode cases, including SYSENTER/SYSEXIT real IVT #GP frames.
- 252 full-width EIP/ESP boundary values with no premature target access.
- 56 subsequent target-fetch #PF cases after a successful system transfer.
- 112 HLT IF/halt-event/timer/PIC sequences with complete observer state.
- 56 successful system transfers with absent/MMIO descriptor pages and no access.
- 84 #GP IDT/GDT/TSS MMIO sequences with exact state and access ordering.

The HLT harness supplies deterministic clock/timer/halt/stop-idling imports through
the existing wasm_fn interface only after boot. Real PIC delivery is exercised
with pending IRQs and IRQs raised from the timer callback. Actual host device
scheduling is outside this controlled test. Independent assertions check target
registers, selector arithmetic, CPL/width/cache changes, IF clearing, TS clearing,
halt state and event order. Shared semantic-body comparisons establish integration
consistency rather than independently proving every architectural policy.

Production Pending remains 3,728. Full system/ISA coverage, general MIR/regions,
online tiers/publication/invalidation, scheduling, optimization and XP/performance
acceptance remain incomplete; the production backend remains legacy.
