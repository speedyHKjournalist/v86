# Experimental CPU identification, timestamp and MSR adapters

CPUID (0F A2), RDTSC (0F 31), RDMSR (0F 32) and WRMSR (0F 30) lower to
separate named CpuExit helpers. Each requires a terminal CPU instruction boundary;
standalone emission, a following instruction in the same linear artifact and
LOCK are rejected. Operand/address and ignored repeat/segment prefixes retain
shared-decoder behavior. The coverage catalogue records eight CpuInfoHelper forms.

## State and exception ownership

The BeforeInstruction map materializes all preceding GPR/FLAGS/last_op1 changes,
completed-instruction accounting and the exact previous/next instruction PCs.
The helper owns architectural results. Success increments instruction_counter
once and returns Invalidated (4); the artifact immediately exits without restoring
the input map. These instructions do not resume with stale SSA GPR or system state.

RDMSR/WRMSR check CPL zero before entering their audited CPU bodies. RDTSC checks
CPL against CR4.TSD. Rejected accesses deliver #GP(0) once and return
ControlTransferred (2), without a successful instruction commit or clock access.
The audited bodies have no later guest-fault path after those permission checks.
CPUID has no permission check. Its result registers and the configured maximum
leaf remain CPU-owned.

The adapters call existing semantic functions for these four specific operations;
they do not dispatch an arbitrary opcode or use the old JIT emitter. MSR support,
CPUID leaves/features, SYSENTER register truncation, APIC policy and the timestamp
interpolation algorithm are preserved. Unknown MSRs still abort in debug builds;
release reads produce zero and writes are ignored. Unsupported APIC base/high-word
and x2APIC writes retain debug assertions and release enable-bit updates. These
are pinned v86 policies, not additional hardware behavior or newly delivered #GP.

## Deterministic verification

`make ir-cpu-info-tests` generates 56 fixtures with an INC ESI predecessor,
16/32-bit modes and seven prefix combinations. Both optimized and unoptimized
artifacts execute against the real CPU memory/state ABI. Separate debug and release
CPU instances compare with their corresponding interpreter.

For each build the suite passes:

- 3,920 CPUID leaf/subleaf/ACPI comparisons and 42 configured maximum-leaf cases.
- 2,870 recognized MSR read/write/no-op/APIC cases, including SYSENTER state.
- 224 deterministic timestamp/interpolation cases and four persistent six-step
  WRMSR/RDTSC/RDMSR sequences that retain timestamp state between instructions.
- 280 CPL/CR4.TSD/VM86 cases, including real TSS-switched #GP frames, and 28
  real-mode cases.
- 16 unknown-MSR and 16 restricted-APIC cases checking both IR variants against
  each build's abort or release behavior. Expected debug assertions are caught
  WebAssembly traps; they are not test failures.

The test supplies a deterministic microtick import through the existing V86
wasm_fn constructor, records CPU state observed by each clock callback, and
compares all six internal TSC fields using ir-test-hooks-only accessors. Production
clock code and imports are unchanged. Independent assertions cover the vendor
string, configured maximum leaf, first timestamp value, SYSENTER_CS truncation,
APIC enable bit and unknown release read results. Most leaf/MSR comparisons share
the semantic body and therefore verify integration rather than independently
proving every CPU model constant.

Online Tier publication/context invalidation and scheduling remain unconnected.
Other system instructions, full ISA/regions/MIR, optimizations and OS/performance
acceptance remain open. Production Pending remains 3,728; these adapters do not
establish IR XP boot compatibility or a performance result.
