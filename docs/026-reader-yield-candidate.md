# Reader cadence candidate proposal

M1 remains blocked by the installed image's failed sibling open. Ten of 23
journey stages passed; the sibling exhausted the unchanged 20-second opening
budget. The primary reopened its exact saved anchor. The current device progress
is authoritative, and no new image has been installed.

The source correction replaces one wait on every reader callback with a shared
wrap-safe five-millisecond elapsed cadence. Every callback, cancellation check,
full source digest, admission step and parser reserve remains. The actual wait
is still delay(1), with its timestamp recorded after suspension. The separate
source-hash wait is unchanged. DEV phase counters expose elapsed time, callback
visits, wait requests and measured wait time for admission, metadata and layout.

Frozen authored host controls identify suspension as 99.3% of the large
expanded-stream control's previous whole-open cost. Both arms perform identical
reads and callbacks, preserve source and frame SHA, and produce identical pages
and characters. This is causal host evidence, not predicted X3 latency or a
successful private-book reading result.

## Exact candidate

- Implementation: `1ad90ad5cb05b65f6ff99cccd25040808cf15163`
- Compiled source: `3b1041e2f901251a0956670b96f55b09de86f728`
- Build: `3b1041e2f901-regional-experiment`
- Image: **1,253,536 bytes**, SHA-256 `05a667f7a924ed5f8b95792cbda3ba90cec5ce3eeaed1eb883fd2d99679b49db`
- ELF SHA-256: `1daa70fb065ac0d48d60e9a7c929ba31c957ac3a349822aa14148959b2180e8c`
- Partition table SHA-256: `334c9553e2799107ed33eb9057f8661c6dd757c662a661edac6c73bb99ae7d5e`
- Archive: `.build/m1-reader-yield-3b1041e2f901/regional-dev-usb`
- Proposed application span: **307 sectors, [0x780000, 0x8b3000)**, with **3936 FF padding bytes**.


This proposal grants no hardware access. The previous installation grant cannot
be reused for this image.

## Verification and costs

The full host gate passed, including ASan/UBSan, real compressed reader
cancellation in all three phases, both orientations, exact pending checkpoint
and settings preservation, frame rollback, clean retry, and existing admission,
zero-output deflate, parser ownership, NVS, wake and progress checks. All ten
production/config/test/tool scopes of the tested implementation match the
compiled freeze. Post-adoption policy fixtures passed 21 structural, 39 resource
and seven procedure-cache tests, plus the structural gate.

| Variant | RAM bytes | Image bytes | RTC bytes |
|---|---:|---:|---:|
| default | 133264 | 1237888 | 300 |
| dev-usb | 134080 | 1249696 | 300 |
| regional | 133776 | 1241696 | 300 |
| regional-dev-usb | 134592 | 1253536 | 300 |

All 24 selected frames remain measured: open grows 848 to 864 bytes and select
128 to 144; the other 22 stay unchanged. DEV openReader grows 144 to 224, with
local counters rather than persistent diagnostics. The named persistent
assertion remains strictly below 1024 bytes, including its four-byte borrowed
pointer. The report can transiently allocate up to 152 bytes through the existing
Print::printf after parser teardown. Callbacks do not log or allocate. Native
external-owner modeled allocations and SRAM/RTC occupancy are unchanged.
See 025-reader-yield-cadence.md and the cost artifact for exact supporting detail.

Four canonical clean builds bind source, configuration, approved local
compiler/toolchain, procedure interpreter and all 334 outputs per variant.
Every variant retains startup/NVS/sleep guards, RTC preservation and the owned
16 KiB loop task. Individual static frames do not prove physical stack safety;
a fresh observation after actual reader paint remains necessary.

Two independent final audits are CLEAN. Each verified all four current
canonical resource gates, all 334 outputs per variant and archive, source and
procedure bindings, exact image/span and linked startup/task guards. The
maintainability audit also independently verified the frozen authored causal
profile and failure-preserving proposal. The build manifest is
artifacts/m1-reader-yield-builds.json. The candidate is ready for coordinator
action review; this does not authorize installation.

## Next coordinated physical scope

After an explicit grant for this exact image, establish current device/layout
state using the approved route, capture fresh full-flash backups outside Git,
and preserve all current NVS/core bytes. Prejourney backups are historical and
must not replace the owner's latest progress. Write only the proposed application
span and verify the full flash against the fresh baseline, including every
outside byte. Verify both boot observations and first-startup low-flash
preservation using the reviewed procedure.

Then run one unchanged 23-stage journey on one connection, retaining distinct
accepted, consumed and terminal evidence plus emitted work-phase reports and a
stack observation after actual reader paint. Preserve the 20-second opening and
30-second terminal deadlines. Stop and close on failure, with no retry, arbitrary
input, automatic restore or deadline extension. No card/cover transfer belongs
to this proposal.

Library checkpoint 0993cd82133138fc7adfa4cd2aabe717b0104485 is included through
owner cherry-pick 0dec6a6. Sidecars remain optional decoration. Host and firmware
both bind source size and POSIX mtime rounded down to two seconds; exact length
and payload CRC are checked. Actual FAT timestamp preservation, cover transfer,
focus latency/ghosting, optical buttons, battery sleep/wake, reader resume,
remaining recovery/settings stages and owner acceptance remain physical work.
The proposed 3–5 day trial freeze and apps/SDK milestone follow M1 acceptance.
