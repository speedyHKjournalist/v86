# CPU entry specialization and admission

`compile_cpu_region` and `compile_cpu_cfg_region` now produce CPU artifacts with an
explicit `EntryContract::Cpu(CpuEntryKey)`. The key records the decoded guest EIP,
linear entry address and default 16/32-bit width. Their wrapping difference is the
CS base. These are compile-time inputs: instruction recovery PCs and branch targets
must not silently be reused under another logical/linear association, even when
both mappings refer to identical physical code bytes.

`CompiledArtifact::current` requires an entry contract, publication key, physical
dependency versions and ordered linear-to-physical code mappings. All four must
match (see [live code capture](ir-live-compile.md)). A standalone state-layout
artifact cannot be admitted as a CPU artifact, and vice versa. A future cache caller
must supply the current entry context, rather than copying the artifact's own key
as evidence that the running CPU matches it.

CPU artifacts additionally validate the live context in the generated Wasm entry.
The single external entry index must be zero; nonzero values return before any
imports run. Then `ir_entry_matches` checks linear IP, CS base and full-width mode,
and rejects a legacy `in_jit` frame, an active instruction prefix or a halted CPU.
It only reads CPU fields: it performs no translation, lazy-FLAGS evaluation, helper
state mutation or exception delivery. Failure returns before `ir_enter`, which
otherwise changes previous IP and clears REP progress metadata, and before any
state materialization, guest memory access or semantic helper call. Successful
admission continues through the existing CPU emitter and its exact recovery rules.

Stack width, non-CS segment bases/selectors, CPL, flags and other CPU fields remain
runtime inputs to the existing plans/adapters. They have not been added to the key
as speculative constants. Additional future specialization must extend the key or
supply another verified guard. The compile-request entry currently supports one
external entry; a multi-entry request needs a separate key for each entry.

The lower-level `emit_cpu` API still emits an unguarded cold function for existing
MIR/unit fixtures that establish their own execution context. It is not an online
cache admission API. Normal production builds continue using legacy JIT and do not
export the experimental guard. The `ir_test_entry_in_jit` test hook is restricted
to `ir-test-hooks` builds.

The normal CPU dispatcher maintains `in_jit` in both debug and experimental IR
release builds. Previously it was debug-only, so merely checking that flag could
not reject a real release legacy frame. A real guest OUT/LOOP workload now checks
admission from inside compiled I/O callbacks with recording disabled and enabled,
and verifies that the marker clears when control returns to the outer CPU caller.
Ordinary release builds without IR retain their existing compile-time behavior.

This guard does not validate code bytes, physical page versions, translation-cache
visibility or a table slot's lifetime. It is not a replacement for transactional
publication/invalidation. Rejection retires no guest instructions; an online caller
must select a matching entry or return to interpretation/compilation, rather than
repeatedly calling an incompatible slot and treating that as execution progress.
The experimental [explicit cache](ir-cache.md) now checks these keys in CPU lookup;
production default IR selection and IR links remain pending. Future link lookup
and failed-compilation suppression must distinguish this context too; the legacy bridge's physical-page
and cached-flags suppression key alone is not an IR entry key.

`make ir-entry-tests` checks 176 generated CPU modules across linear/CFG compilers,
optimization, both widths, CS/EIP wrapping and two linear aliases of one physical
code page. In each debug/release CPU build, 1,760 rejected calls leave CPU fields,
RAM and existing REP results unchanged and invoke no state/access/semantic helper.
The suite compares 176 admitted executions with the actual interpreter, including
integer load/store, CPUID, a loop and SIMD; 32 admitted page faults match the
interpreter's destination, CR2, stack frame and previous IP without retiring the
faulting instruction. Publication checks separately reject wrong entry context,
artifact kind, page version and slot generation.
