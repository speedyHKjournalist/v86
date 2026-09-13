# IR encoding and lowering coverage

Run `make ir-coverage` for counts and `make ir-generated-check` to validate that
`ir-coverage.json` and the Rust decode catalogue match the source opcode table.
The stable encoding ID is `(opcode << 4) | (fixed_g + 1)`, using zero for an opcode
without a ModRM group. It does not depend on table ordering.

`lowering` is the **production** status. `experimental_lowering` separately records
experimental capability. Of 3,830 coarse forms, 102 explicit invalid reg/mem forms
are BaselineUD and **3,728 production forms remain Pending**. Current experimental
capabilities are:

| Category | Forms | Contract / evidence |
|---|---:|---|
| NativeHIR | 838 | [Design](ir-design.md), [shifts](ir-shifts.md), [multiply](ir-multiply.md), [bits](ir-bits.md), [exchange](ir-exchange.md), [scalar/BCD](ir-misc.md) |
| CpuSimdHIR | 450 | [Moves](ir-simd-moves.md), [integer arithmetic](ir-simd-integer.md), [packing/shifts](ir-simd-permute.md), [shuffles](ir-simd-shuffle.md), [half/scalar transfers](ir-simd-transfer.md), [word lanes and sign masks](ir-simd-lane.md), [masked stores](ir-simd-masked.md) |
| CpuMemoryHIR | 622 | [RAM/MMU/RMW](ir-memory.md) and the instruction-family contracts above |
| CpuStackHIR | 156 | [Stack](ir-stack.md), [ENTER](ir-enter.md), [FLAGS/segment stacks](ir-system-stack.md) |
| CpuRepHelper | 84 | [REP HIR and terminal batch ABI](ir-rep.md) |
| CpuInfoHelper | 8 | [CPUID, timestamp and MSR terminal adapters](ir-cpu-info.md) |
| CpuSystemHelper | 12 | [SYSENTER/SYSEXIT, HLT, CLI, CLTS and WBINVD](ir-cpu-system.md) |
| CpuControlRegHelper | 8 | [Control/debug register transfers and mapping changes](ir-control-regs.md) |
| CpuDescriptorHelper | 56 | [Descriptor tables, machine-status word and INVLPG](ir-descriptor.md); includes explicit invalid-register #UD paths |
| CpuTaskRegHelper | 32 | [SLDT/STR, LLDT/LTR and task busy-bit state](ir-task-regs.md) |
| CpuSelectorQueryHelper | 32 | [LAR/LSL and VERR/VERW query, fault and raw-ZF policy](ir-selector-query.md) |
| CpuIoHIR | 36 | [IN/OUT and non-REP INS/OUTS](ir-io.md); contracted device/permission adapters |
| CpuStringHIR | 30 | [Non-REP MOVS/CMPS/STOS/LODS/SCAS](ir-strings.md); repeat forms use CpuRepHelper |
| CpuStateHIR | 28 | [Segment MOV and far pointer loads](ir-segments.md) |
| CpuControlHIR | 28 | [Near control transfers](ir-control.md) |
| CpuArithmeticHIR | 14 | [DIV/IDIV](ir-multiply.md), [AAM](ir-misc.md) |
| TerminalBranchHIR | 152 | [Jcc/JMP design](ir-design.md), [counter branches](ir-loops.md) |

These counts are not completed production coverage. `make ir-default-gate`
rejects nonzero production Pending. No helper coverage is claimed simply because
an old CPU helper or emitter exists. The release runtime still uses the legacy
backend; [ir-progress.md](ir-progress.md) tracks the full migration requirements.

The forms expand catalogue operand size, address size and reg/mem selectors.
Implicit widths retain the opcode table's fixed-width classification; Rust lowering
selects the actual register fragments. Privilege/mode constraints, LOCK legality,
repeat-prefix combinations, nested x87 subopcodes, runtime feature checks and
helper-specific invalid encodings still need a more complete acceptance matrix.

The `tests` list points to the relevant fixture generator and executable suite
for each implemented family. The coverage check requires attributed paths to
exist. This navigation is not exhaustive per-form proof; actual tested dimensions
and deliberate policies are recorded in the linked contracts and
[validation report](ir-validation.md). Pending experimental forms carry no test
claim; invalid forms reference the decoder suite.

Decode facts distinguish `fetch_modrm` (the opcode family's fetch before mandatory
prefix dispatch) from `e` (the selected form's EA semantics), retaining the pinned
baseline's invalid-form lengths. Production analysis uses the catalogue's `custom`
attribute to retain helper observer boundaries. Both are covered by the actual
legacy-analyzer differential, independently of experimental lowering capability.

[CMPXCHG8B](ir-cmpxchg8b.md) adds four experimental memory forms with native
RAM execution and ordered CPU slow paths; production coverage is unchanged.
