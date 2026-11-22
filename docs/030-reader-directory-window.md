# M1 reader: scoped central-directory window

The physical diagnostic in `029-reader-admission-physical.md` identified ZIP
inspection as the next narrow source target: the sibling spent 10.700 seconds
in inspection and 12.143 seconds in file calls across admission. The 431 seeks
were measured across admission, not separately inside inspection. This change
reduces alternating central/local reads without skipping member validation.
It does not establish stable physical reading or authorize a device write.

## Ownership and behavior

`ZipDirectoryWindow` borrows exactly 4,096 bytes from the existing parse arena
for one synchronous inspection of an immutable open source. It retains no
filename, catalog, image, source generation or persistent cache. Its arena mark
is released before the inspection-end observation, catalog opening and inflater
work, on success and every error return. Both 48 KiB arenas and their owners
remain unchanged; there is no new heap allocation or global raster/buffer.

Only central headers and central filename chunks use the window. EOCD, local
headers/names and member streams keep their existing paths. Refills never leave
the previously validated directory range and request at most 4,096 bytes. Reads
may cross a window boundary and accept bounded positive short reads. A failed
active window stays failed; it never retries or switches to direct reads.
Insufficient parse space or an unsupported initial zero-byte probe keeps the
legacy direct-read path.

Every cached logical read checks source size and the production adapter's
cancel/handle/size guard before and after its scheduler callback; a refill checks
again after scheduled physical reads. Zero-byte probes use one centralized valid
scratch-byte address, retaining compatibility with generic sources that call
`memcpy` unconditionally. Production probes perform no file seek/read. Full SHA
identity, opening deadlines, cancellation, ZIP structural checks, duplicate
checks and all member streaming/expanded-length checks remain unchanged. Current
admission still does not compute ZIP CRC32.

The existing source owner must keep bytes, binding and cursor ownership immutable
through synchronous inspection. Same-size replacement or rebinding during that
scope is prohibited by this existing contract and is not detected by a cheap
size probe. A new window after a source change always reads fresh bytes. No cache
survives a changed source or its arena lifetime.

## Source verification

The implementation checkpoint is
`b739b6722ecd9d509b2908df624f598066cc32ba`, based on physical-evidence head
`bf9cce7ab8913899b2ac0301cdad38fe75fd99ee`. Production-adapter tests cover header
and 511-byte name crossings, directory extras/comments, 31-byte short reads,
cancel/handle/size invalidation on hits and refills, detach, sticky failures,
physical truncation, oversized counts, small-arena and unsupported-probe
fallback, malformed directories and unchanged rejection status. A legacy generic
memory source with unconditional `memcpy` exercises non-null probes under
ASan/UBSan. Tests also verify release before inspection-end on success/failure
and fresh bytes after between-scope mutations or rebinding.

The authored three-entry ZIP inspection uses **19 reads / 9 seeks**, versus
**29 reads / 29 seeks** on its uncached path. Seven authored causal fixtures
preserve frozen page/status, complete compressed-member coverage and exact
fixture hashes; random reads across admission fall from 42 to 22. This is host
work evidence, not an X3 latency estimate. Detailed and phase-only observer runs
retain identical work. Zero-byte validity probes and probe-backed logical yields
are counted separately from physical reads, so expanded-stream callback counts
remain independently checked.

Two independent bounded source review rounds finished clean after fixing the
generic null-destination probe class. The full host/native results and exact
candidate are recorded with the final build manifest after source freeze.

## Frozen native candidate

All four clean resource builds at
`57ba552281813d6979b71c9714b6a715537b800d` exit 0. The two independent final
native audits verify current source/vendor/config/procedure/interpreter bindings,
all 334 outputs per variant, fresh output remeasurements and exact selected
frames. The compiled/tool/test scope bytes remain identical to the freeze.

The fresh complete `python3 -B tools/check.py` gate exits 0, including
ASan/UBSan, source/policy fixtures, both orientations and exact PBM/PNG previews.
After it completes, all four canonical resource receipts are reverified against
current identities and actual outputs. Tightened capacity and structural checks
also pass. No compiled, tool or test bytes changed after the freeze.

| Variant | Shared SRAM | App image | App headroom |
| --- | ---: | ---: | ---: |
| default | 133,264 | 1,239,648 | 6,559,136 |
| dev-usb | 134,080 | 1,254,896 | 6,543,888 |
| regional | 133,776 | 1,243,456 | 6,555,328 |
| regional-dev-usb | 134,592 | 1,258,736 | 6,540,048 |

RTC remains 300 bytes. All 31/33 selected frames equal their ceilings; the three
cache helper frames are 32/32/48 bytes. These individual static frames do not
certify runtime stack safety. All linked NVS/sleep guards and the 16 KiB reader
task remain verified. Final regional DEV is 16 bytes smaller than its dirty
preliminary cost image, so its ceiling tightens by 16 bytes.

The concrete source-test proposal is build `57ba55228181-regional-experiment`,
image **1,258,736 bytes**, SHA256
`e39bcbd491853aedee03bc2d6c77f2e15904cb5d01b97d9badc61585d437111d`.
Its hypothetical exact app1 span remains 308 sectors, `[0x780000,0x8b4000)`,
with 2,832 FF padding bytes. This arithmetic is proposal evidence, not a grant
or device operation. A live action would require its own exact-image scope and
fresh preservation/recovery checks against current progress.

`artifacts/m1-directory-window-builds.json` records final logs, receipts, image/
ELF identities and authored host proof. The original authored comparison report
is preserved byte-for-byte at `artifacts/m1-directory-window-frozen-baseline.json`
(SHA256 `2057bf73d4e4160e68a94a1cd5fcbf2f480454a1e3e27d95bd4e13e10acaa27f`).
To repeat that optional comparison in another checkout, seed
`.build/admission-profile/profile.json` with this report before running the host
gate. No real books, artwork, private backups, card data or device state are
contained in either authored report.

## Physical checkpoint at source handoff

The first parent-approved install stopped before write. A separately authorized
ROM recovery and newly reviewed attempt subsequently passed the complete
23-stage journey. See `032-directory-window-physical.md` for current preservation,
measurements and authoritative positions; `031-directory-window-install-stop.md`
preserves the original failure boundary. The following records the earlier
source-only checkpoint.

The installed diagnostic remains `4a31ec31750b-regional-experiment`, image SHA256
`e7678f6c02ebbc137e881f3a219174ef0a0ebc2228e352ebe4424f5b2a3aaf1c`.
The latest observed primary anchor **1708 remains authoritative**. Post-journey
durable NVS has not been recaptured; historical snapshots must not replace it.
No USB, device input, sleep, card transfer, private book or backup action is part
of this source change.

A future exact-image parent grant must follow final source/resource/procedure
verification and fresh live preservation/recovery checks. Retained numeric
instrumentation can then compare physical inspection, seeks, whole admission,
hashing and opening. Both books must complete the actual reading/return/resume
journey within the unchanged budgets before any stability claim. Optical focus,
ghosting, both orientations, sleep/recovery and the proposed 3–5 day trial freeze
remain separate M1 acceptance evidence.
