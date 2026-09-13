# Experimental scalar and string port I/O

IN/OUT with immediate or DX ports and non-REP INS/OUTS now have explicit HIR
operands and audited CPU adapters. Byte/word/dword widths retain the baseline
port-dispatch behavior. These instructions terminate the cold CPU region.
REP INS/OUTS use the separate [batch ABI](ir-rep.md); online scheduling and
production tier integration remain pending.

## Permission and device ownership

`ir_in` and `ir_out` call the pinned `test_privileges_for_io` before accessing a
port. This preserves real/protected/VM86 distinctions, CPL/IOPL, TSS form and limit
checks, I/O bitmap reads, page-table side effects and CPU-owned #GP/#PF delivery.
DX is captured as its low word; immediate ports are zero-extended. Scalar repeat
prefixes retain the pinned ignored-prefix behavior. LOCK stops compilation; repeated string I/O uses the REP frontend.

IN writes AL/AX/EAX after the port read, preserving untouched accumulator bits.
OUT receives the captured accumulator fragment as an explicit argument. The
adapters use the CPU's actual width-specific port primitives, including the
existing Rust-handled byte ports; no JS-side device bypass is introduced.
Both adapters commit once only after success, and return through CpuExit.

OUTS resolves DS or its override first, checks permission through the normal
returning `ir_io_check`, then performs its source GuestLoad through the native
RAM/MMU/MMIO path. `ir_outs` writes the captured value to the port and only then
commits the advanced SI/ESI. It does not repeat the permission lookup after the
source read. The frontend contract fixes this check/load/device order; a general
helper registry and permission-proof verifier are still pending.

INS resolves ES, then `ir_ins` checks permission and the complete destination
write range before reading the port. It performs the actual safe write after the
read. The preflight is not a reusable RAM proof: a port callback can remap or
unmap the destination. A later write fault retains the already-observed port read
and the old DI/EDI, without committing this instruction. Successful execution
advances DI/EDI and commits once. INS destination writes remain inside this
explicit ordered I/O adapter; no native destination-fast-path claim is made.

The frontend computes DF-based pointer advancement in SSA, with address16 low-word
wrap and high-word preservation. Both string adapters preserve ECX and FLAGS.
On failure, the CPU owns exception delivery and the caller exits without restoring
its old snapshot. Device callbacks observe the pre-instruction GPR/FLAGS state,
completed-prefix accounting, previous_ip and decoded next instruction_pointer.

## CPU helper observation point

The device differential exposed a pre-existing gap for normal returning CPU
helpers: only CpuExit calls prepared decoded next IP. A BeforeInstruction CPU
helper now uses the same call preparation, retaining previous_ip as its fault
PC. This also covers TSS/bitmap MMIO during `ir_io_check`. Standalone helper ABI
behavior and caller-owned fault restoration remain unchanged. Other resume kinds
retain their explicit StateMap PC. The existing helper suites are part of the
full regression for this change.

## Evidence

`make ir-io-tests` generates 312 optimized/unoptimized fixtures with a preceding
INC. Dimensions include both code/address widths, three data widths, all string
segment overrides, and ignored scalar F2/F3 prefixes. Actual port fixtures use
E8, 0500, 0507 and FFFE; wide permission masks include adjacent-byte and high-port
cases without changing the pinned CPU model.

- 6,336 CPU and device-state comparisons, including 1,008 confirmed native OUTS
  source-memory paths. INS uses its documented safe-write adapter.
- 2,688 CPL/IOPL/VM86 permission cases and 480 individual bitmap byte-lane and
  adjacent-bit checks, including permission denial before source data access.
- 272 string segment/permission/source/destination/tail-page fault cases, with
  real ring3-to-kernel exception stacks, precise CR2 and no premature port access.
- 720 TSS16, short-header, short-bitmap, missing-header-page and missing-bitmap-page
  faults. The TSS16 fixtures provide a valid 16-bit kernel stack for #GP delivery.
- 144 combined TSS/bitmap/data MMIO and port-observer comparisons.
- 48 INS callback remaps or write faults after successful preflight and port read.
- 288 string page/address16/DF boundary cases and 144 real-mode comparisons.

Comparisons include GPRs, complete FLAGS and last_op1, IP, CR2, CPL, stack/code
mode, segment selectors/bases, operand memory, exception frames and device event
order. Rust checks require terminal CPU emission, reject unsupported prefixes,
and verify OUTS segment -> permission -> GuestLoad -> device ordering.

The catalogue gains 36 CpuIoHIR coarse forms: scalar port I/O plus non-REP string
I/O. These are contracted helper capabilities, not standalone native arithmetic
or completed production coverage. Production Pending remains 3,728. Online REP scheduling, remaining ISA, full MIR/regions, online tiers/invalidation, OS/performance acceptance
and legacy-emitter retirement remain incomplete.
