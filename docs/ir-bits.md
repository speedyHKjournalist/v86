# Experimental bit operations

The frontend supports BT/BTS/BTR/BTC register-index forms (0F A3/AB/B3/BB) and
immediate forms (0F BA /4–7), BSF/BSR (0F BC/BD), POPCNT (F3 0F B8), and BSWAP
(0F C8–CF). Normal calculations use pure HIR and native Wasm scalar operations.
POPCNT requires the decoded mandatory F3 form; bare 0F B8 remains unsupported/UD
according to the pinned encoding. Memory BTS/BTR/BTC also accept LOCK under the
[audited single-owner RMW contract](ir-exchange.md); LOCK BT and register forms
remain compile stops.

## Bit strings and memory ordering

Register bit indices are masked by operand width. Immediate memory indices are
also masked by operand width, exactly as the pinned CPU helpers do. Register-index
memory forms sign-extend the 16-bit operand (or use the signed 32-bit operand),
shift it arithmetically by three, and add that byte displacement after effective
address wrapping. The final sum is not wrapped to address-size 16 again. Segment
addition preserves the same modulo-32 linear address as the interpreter.

Memory forms access exactly one selected byte, rather than reading a full word
or dword around it. Thus the selected byte can be readable while the nominal base
page is absent. BT uses a read-only `GuestLoad`; BTS/BTR/BTC use the affine byte
RMW ticket. Write permission is checked before a device read, CF records the old
bit, and the physical write reuses the checked translation. MMIO sees the old
state at the read and updated CF at the write. Successful modifying forms commit
and exit under the current conservative store policy.

## Scans, counts and FLAGS

BSF/BSR select the old destination for zero input and set ZF according to the
source, not the bit index. CF is cleared, and the other undefined flags follow the
pinned lazy-result policy: PF/AF/SF use the scan index (zero for a zero source), and
OF uses the retained `last_op1` plus that result. The source of undefined AF/OF
therefore remains explicit SSA state; it is not replaced with arbitrary constants.
POPCNT writes zero for a zero source, sets ZF accordingly, and clears all other
arithmetic flags. Both kinds retain `last_op1` for later instructions.

BSWAP exchanges the complete register dword and preserves FLAGS. This intentionally
matches the pinned implementation even with operand-size override / 16-bit decoding;
it is a baseline compatibility policy for the otherwise undefined 16-bit form.

`CountLeadingZeros`, `CountTrailingZeros` and `PopulationCount` are typed pure HIR
operations restricted to I32/I64. They emit the native Wasm instructions and fold
with identical zero/full-width semantics. BSF/BSR adapt their zero case explicitly.
Byte swapping uses native extraction/shift/OR nodes. No CPU instruction helper
performs these calculations.

## Verification

`make ir-bit-tests` checks optimized and unoptimized Wasm against exact CPU
interpreter steps and independent bit-position enumeration:

- 54,720 ordinary bit/string/scan/POPCNT/BSWAP cases; 26,720 confirmed warm native
  memory paths without slow data-memory imports.
- 480 actual memory/segment faults, 168 MMIO observation comparisons, 168 lazy
  FLAGS paths and 384 signed-index/page/address16 boundary cases. The latter
  include an absent base page with an accessible selected byte.
- 393,216 exhaustive 16-bit BSF/BSR/POPCNT cases (all 65,536 sources, optimized and
  unoptimized), including zero-input destination preservation.
- 768 dword single-bit and complement patterns for all 32 positions, both decode modes.
- 28 standalone I32/I64 bit-count executions across zero, top bits, all ones and
  32/64-bit boundaries, with constant-folded and runtime-input artifacts.
- Verifier/contract checks for count operands, one-byte bit-string memory accesses,
  and the mandatory POPCNT prefix.

This adds 60 experimental `NativeHIR` forms and 44 `CpuMemoryHIR` forms. Production
Pending remains 3,728. The broader integer/ISA surface, wide locked RMW, online IR tiers and
OS/performance acceptance remain part of the full implementation goal.
