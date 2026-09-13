# Experimental exchange and LOCK contract

XCHG (86/87), XADD (0F C0/C1) and CMPXCHG (0F B0/B1) use native SSA for
8/16/32-bit register and memory operands. This extends the experimental CPU
frontend; it does not publish IR into live CPU tiers. CMPXCHG8B remains pending.

## Source capture and observable state

Operands are captured before any architectural destination changes. XADD writes
the old destination into its source register before writing the sum into the
destination, including identical registers and AL/AH aliases. Its arithmetic
FLAGS and retained `last_op1` match ADD. XCHG preserves FLAGS and `last_op1`.

CMPXCHG compares the appropriate accumulator against the old destination, computes
SUB FLAGS, selects either the captured source or old destination, and updates the
accumulator on failure. A failed memory comparison still validates write permission
and performs the physical write cycle, as the pinned CPU does. MMIO read callbacks
observe pre-instruction registers/FLAGS; write callbacks observe the updated state.
The committed count advances only after the successful write.

Memory forms use the checked affine RMW ticket. All write pages are validated
before any data read, and the saved physical addresses are reused at write time.
Page/segment faults preserve the old state and dispatch once. Successful stores
commit and exit under the current conservative invalidation policy.

## Explicit ordering and audited execution model

`RmwLoad` and `RmwStore` both carry `RmwOrder::Plain` or `RmwOrder::Locked`.
The verifier rejects mismatched order, width, ticket or fault map, and forbids
any helper, poll or other ordered operation between paired read and write.
Tickets cannot escape or cross block parameters. Memory XCHG is implicitly Locked.
LOCK is accepted only for memory-destination ADD/OR/ADC/SBB/AND/SUB/XOR, INC/DEC,
NOT/NEG, BTS/BTR/BTC, XCHG, XADD and CMPXCHG forms that have this complete protocol.
Other forms compile-stop, including register LOCK, CMP, BT, MOV and shifts.
This does not replace the pinned interpreter's still-incomplete LOCK legality
checks with new architectural exceptions.

The locked emission has been audited against the current CPU ABI:

- `src/rust/wasmgen/wasm_builder.rs` declares an unshared memory import. An actual
  shared `WebAssembly.Memory` fails IR module instantiation with `LinkError`.
- `src/cpu.js` uses the CPU module's memory. `src/main.js` is the sole production
  caller of `cpu.main_loop()`; execution and Wasm-to-host calls are synchronous.
  Browser worker runtime commands are queued messages and do not transfer the
  guest Wasm memory buffer (`src/browser/cpu_worker_runtime.js`).
- Native RAM accesses have no intervening call/suspension. Slow MMIO retains the
  baseline synchronous read/write callbacks and checked physical ticket. A test
  schedules a microtask from the read and verifies the write finishes first.
- The CPU ABI requires callbacks not to recursively execute the guest CPU. Such
  reentrancy could also overwrite the slow-read scratch state and is unsupported.

Under this nonshared, single-owner execution model, another CPU/device event
cannot interleave guest execution between the paired accesses. Emission therefore
uses the existing native loads/stores and synchronous MMIO protocol. This is an
explicitly bounded ordering argument, not an implementation of hardware atomics
for shared memory, multiple CPUs or reentrant custom imports. Adding any of those
requires a new ABI and lowering audit before accepting Locked pairs.

## Verification

`make ir-exchange-tests` generates 2,016 exchange fixtures and 24 LOCK-family
fixtures, each optimized and unoptimized. Exact CPU interpreter steps and an
independent BigInt oracle verify:

- 48,384 exchange/alias/FLAGS executions across decode modes and operand widths,
  including equal/unequal CMPXCHG and source/accumulator aliases.
- 20,736 instrumented warm native memory paths, with no slow data-RMW imports.
- 5,616 real missing-page, readonly, null-segment and cross-page faults, requiring
  no MMIO callbacks before failure and preserving the uncommitted count.
- 1,296 MMIO read/write state observations and 864 ordinary cross-page successes.
- 576 legal LOCK arithmetic/unary/bit-family CPU and MMIO comparisons.
- Verifier rejection of mismatched pair ordering and unsupported LOCK forms,
  shared-memory import rejection, and synchronous locked-pair scheduling.

The catalogue gains 12 NativeHIR and 18 CpuMemoryHIR forms; prefix combinations
are not separate coarse catalogue forms. Production Pending remains 3,728.
