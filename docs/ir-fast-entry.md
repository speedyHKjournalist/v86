# Bounded fast IR entry validation

The experimental core enables fast admission by default. This changes validation
work, not guest compilation policy or exception semantics. The existing cold
entry ABI and architectural state materialization remain in place.

## Reuse certificate

A published record remembers the non-wrapping admission epoch in which its bytes
were fully checked. Reuse requires that epoch, the current VM generation, the
exact CPU entry context, and every source page's CPU-visible TLB mapping and CPL
read permission to match. It does not skip page visibility or eagerly translate
secondary pages. Saturating the epoch permanently disables reuse.

Certificates cannot cross a CPU batch boundary, an interpreter/legacy fallback,
reset invalidation, or automatic publication's host call. Known physical writes
retire affected records without revoking unrelated byte certificates. A separate
continuation epoch still stops fused activations on those writes. Generated guarded
CPU artifacts also revoke them before slow memory and generic helper calls,
including helpers that return through CpuReload. Segment-address checks and RMW
ticket-value reads are the narrow successful-call exceptions: neither observes
the host. Audited register-only SSE calls synchronize only used XMM operands
and reload the destination; real/VM86 MOV Sreg and successful CLI checks preserve
SSA GPR/FLAGS state. Fault paths retain complete materialization and revocation.
Mapping identity and CPL permissions are still checked independently on every
entry. A terminal/fault/budget return without an ordinary link request revokes
the interval after activation. This retains detection of raw unnotified host
code writes; it does not assume every host write has a JIT notification.

Ordinary native stores still check TLB_HAS_CODE. Published code pages retain the
existing physical dependency watches, including all cached virtual aliases.
Dirty notifications retire dependent records and revoke the admission interval.
This is a bounded synchronous certificate, **not** a persistent page-version
registry. Snapshot `CodeDependency.version` is not used as a substitute for byte
validation across arbitrary host observations.

## Fetch and lookup

After a successful warm pre-fetch check, `get_phys_eip` uses an already-visible
translation, with no page walk, A-bit write or device callback. The post-fetch
stage verifies the same published owner and unchanged epoch, reusing that check.
Cold fetches retain full post-fetch validation, including code/PTE aliases changed
by accessed-bit writes. Missing secondary translations still decline execution.

A 64-slot direct-mapped entry cache avoids repeated BTreeMap lookups. Each hint
contains the complete entry key and record index. Retirement checks remain;
record compaction clears all hints, and post-fetch admission also checks the
publication identity. No hint owns a Wasm slot or bypasses validation.

## Diagnostics and tests

`get_jit_info().ir` adds `cache_fast_checks`, `cache_full_checks`,
`cache_post_fetch_reuses`, `cache_target_hits`, and `cache_fast_validation`.
The corresponding `ir_cache_stat` fields are 18 through 22. Existing field 8
counts successful cached admission checks, including certificate reuse.

The cold-only diagnostic export `ir_cache_set_fast_validation(0|1)` controls
byte-certificate and post-fetch reuse; disabling retains the entry hint cache.
It is not a hot backend switch. XP A/B measurements can use the same core:

```sh
IR_FAST_VALIDATION=0 IR_BOOT_MS=60000 node tests/ir/performance/xp_boot.mjs /absolute/path/to/xp.img ir
IR_FAST_VALIDATION=1 IR_BOOT_MS=60000 node tests/ir/performance/xp_boot.mjs /absolute/path/to/xp.img ir
```

The cache matrix now compares both modes with recording on/off and independently
checks finite two-region loop retirement. It requires fast mode to eliminate
more than half of the full checks. Continuing MMIO callbacks directly change a
previously executed entry's code bytes or remap its page; both must reject that
stale successor. Existing tests retain cold fetch A-bit/code aliases, secondary
pages, fault ownership, physical aliases, same-byte writes, reset/restore, slot
ABA, capacity eviction and synchronous host invalidation coverage.

## Local XP diagnostic (2026-09-19)

One sequential pair used the supplied 4 GiB XP C: image, 2 GiB RAM, 16 MiB VRAM,
the same production experimental core, recording off and in-memory disk writes.
No builds or other benchmarks ran concurrently. Both modes retained the new entry
hint cache; the switch isolates certificate/post-fetch reuse, not the entire patch.

| Observation | Fast validation off | Fast validation on |
| --- | ---: | ---: |
| First 60 s average, mIPS | 28.75 | 30.06 |
| First 800×600×32 transition | 51.39 s | 48.20 s |

Throughput rose about 4.6% in this pair. Fast mode performed 161.1 million full
checks over 166.6 million activations, plus 5.5 million pre-fetch certificate hits
and 166.6 million post-fetch reuses. It removed roughly half of the full checks per
activation, but this does not mean half of CPU time was spent checking bytes.
The workload visits different boot phases during each fixed time window; this is
a net diagnostic comparison, not a sampled attribution of total validation time.
It remains far below legacy-level throughput and is not a repeated-run median or
complete browser/Worker XP acceptance result.

Logs: `build/xp-boot-fast-entry-off.jsonl`, `build/xp-boot-fast-entry-on.jsonl`.
The pre-change core is retained at `build/v86-ir-runtime-before-fast-entry.wasm`.
220 native tests and the cache/automatic matrices on production, debug/release
invariant builds passed; the portable cache matrix and browser main-thread/Worker
integration also passed.
