# Experimental near control transfers

The cold CPU frontend now lowers relative near CALL (E8), indirect near CALL/JMP
(FF /2 and /4, register or memory), RET (C3) and RET imm16 (C2). These use native
SSA values, stack arithmetic and the existing RAM/MMU/MMIO path. The frontend does
not call the old instruction emitter or a generic interpreter helper. The current
linear snapshot must end at the control transfer. Full region discovery, far
control transfers, interrupts, task changes and live tier linking remain pending.

## Dynamic destination state

`StateMap.next_value` optionally holds an i32 SSA CS-relative destination. It is
allowed only for AfterInstruction state. `next_pc` still describes the sequential
PC used when preparing memory/device calls; a successful exit uses `next_value`
when present. CPU emission adds the current CS base once, since the CPU's stored
IP is linear. Standalone emission writes the logical destination directly.

The destination participates in StateMap value enumeration, dominance/type checks,
DCE roots, optimization replacement and local allocation. A test whose computed
destination is used only by StateMap checks that DCE preserves it and emitted Wasm
writes the expected EIP. Fault maps cannot carry a future dynamic destination.

CALL captures the target before changing ESP or writing the return address. This
covers CALL ESP, CALL [ESP], and a memory operand that overlaps the return-address
slot. The pushed return address is the sequential CS-relative EIP, truncated to
the operand width. The store's fault map retains the original ESP and instruction
PC; its success map commits both ESP and the saved target. CALL rel16 wraps its
target at 16 bits. All current stores already end the region, so a completed CALL
cannot execute following stale code.

RET reads from the old stack pointer. Only a successful read commits the loaded
destination and stack adjustment. RET imm16 adds its unsigned byte count after
popping the operand-sized return address; the selected stack width controls pointer
wrapping independently. Indirect JMP commits its loaded/register target without
changing the stack. Narrow targets are zero-extended to the StateMap i32 value.

Memory/device callbacks run before the control transfer commits. They observe the
sequential EIP and original ESP, matching interpreter operand-decoding order.
A source, stack, or segment fault exits through the CPU-owned delivery path without
restoring the success target. Fetching the target instruction is a separate step:
the transfer and any CALL push/RET adjustment have already committed when that
fetch faults.

## Evidence

`make ir-control-tests` generates 102 fixtures and compares real CPU execution
with exact interpreter steps:

- 1,632 ordinary comparisons across 16/32-bit decoding, operand and stack widths,
  zero/nonzero CS bases, cold/warm TLB and both optimization settings.
- 816 warmed runs with no slow memory calls and 48 loaded-target width/wrap cases.
- 200 faults during the instruction, including memory/segment/RET stack faults and
  ring3 CALL write faults delivered through a real TSS onto a separate kernel stack.
- Six subsequent target-fetch faults, after the near transfer has committed.
- 192 MMIO callback/state comparisons, including a CALL target whose source slot
  is overwritten by the pushed return address.

Comparisons include EIP, GPRs, FLAGS, stack bytes, exception frames, CR2, selectors,
segment bases and CPL. The pushed return address and committed instruction count
also have explicit checks. These tests do not establish the full far/system,
privilege, segmentation or OS matrix. The 28 experimental `CpuControlHIR` catalogue
forms remain production Pending, and the default CPU backend remains legacy.
