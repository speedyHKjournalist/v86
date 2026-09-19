# IR-08 x87 register-form contract

This increment adds the first x87 execution path owned by the IR frontend/runtime.
It covers **register-only D8-DF forms** (`ModRM.mod == 3`) and deliberately keeps
all x87 guest-memory forms, MMX aliases, and further floating-point lowering out
of scope.

## Execution boundary

The frontend lowers a register x87 instruction only when it is the terminal
instruction of a CPU IR region. The generated HIR contains one audited
`CpuExit` helper call:

```text
decoded D8-DF / mod=3
  -> BeforeInstruction StateMap
  -> ir_x87_reg(opcode, group, r, operand_size)
  -> ControlTransferred on #NM/#UD
  -> Invalidated after one successful instruction commit
```

No x87 register operation resumes cached SSA after the helper. GPR/FLAGS state
that precedes the instruction is materialized through the normal StateMap path,
and the helper-owned CPU/F80 state is authoritative on exit.

## Architectural guard and F80 ownership

The helper preserves the interpreter's x87 ordering:

1. CR0.EM/CR0.TS are checked through `task_switch_test()`; either bit delivers
   #NM before any x87 operation.
2. After the guard succeeds, the legacy x87 f64 shadow cache is synchronized and
   discarded with `fpu_cache_barrier()`.
3. The operation executes against the canonical `F80` stack, tag/TOP, status
   word, control word and EFLAGS state.
4. Successful execution commits exactly one instruction and returns
   `Outcome::Invalidated`.

This is a backend-coexistence barrier, not a conversion of x87 to WebAssembly
f64. IR therefore keeps the same software-F80 semantics as the interpreter.

## Nested invalid encodings

Several D8-DF groups contain ModRM-register subopcodes whose validity depends on
the low three ModRM bits. Examples include D9/2, D9/4, DA/5, DB/4, DE/3 and
DF/4. The runtime validates those nested encodings **before** calling the shared
instruction body. Invalid combinations deliver #UD and return
`ControlTransferred`; they can never fall through to the success commit.

The CR0 task-switch guard runs first, matching the generated interpreter, so
#NM has priority over a nested #UD when EM or TS is set.

## Shared instruction semantics

The adapter dispatches to the existing `cpu::instructions::instr_*_reg`
functions for arithmetic, compares, stack operations, constants,
transcendentals, FCMOV, FCOMI/FUCOMI, FINIT/FCLEX and FNSTSW AX. It does not
reference the legacy JIT emitter or `x87_codegen`.

D9/DD operand-size variants share the same register semantics, matching the
existing 16/32 interpreter aliases.

## Coverage and tests

The experimental catalogue attributes **160 coarse forms** to
`CpuX87Helper`. This reduces experimental Pending from 1,110 to 950 without
changing production coverage: all 3,728 production forms remain Pending until
the later default-backend gate.

`make ir-x87-tests` builds optimized and unoptimized modules for every D8-DF
ModRM register subencoding, including 16/32 D9/DD variants. The differential
runner compares those modules with interpreter execution in debug and release
for:

- physical F80 register bytes;
- TOP/tag, status/control and x87 metadata;
- GPR, EFLAGS/lazy backing, EIP/previous EIP and retirement count;
- full and sparse/wrapped x87 stack layouts;
- FCMOV and FCOMI/FUCOMI flag conditions;
- nested #UD encodings;
- CR0.EM/TS #NM priority.

## Remaining IR-08 work

This increment does **not** complete IR-08. Remaining work includes x87 memory
loads/stores/environment/state images and their exact MMU/fault/partial-write
ordering, MMX aliasing and EMMS transitions, remaining SIMD/FP state/control
forms, no-SIMD fallback, and the later XP/application acceptance matrix.
