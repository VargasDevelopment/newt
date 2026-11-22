# Reader page reuse and certified chapter prefixes

**Historical source/build record:** the later parent-authorized physical phase
is complete. [036-reader-efficiency-physical.md](036-reader-efficiency-physical.md)
and [its receipt](../artifacts/m1-reader-efficiency-physical.json) supersede this
document's pre-install hardware/progress statements and the frozen build
manifest's hardware-planning fields. Installed build is
`f22f72044de2-regional-experiment`; latest observed anchors are 3442/1600.
All prejourney backups are historical and must not be restored.

The reader now repaints its owned, successfully copied current page without
metadata reconstruction, XML/inflate work or payload I/O. A new page in the
current chapter uses previously certified exact chapter totals and parses only
the prefix needed to select it. No extra page allocation, resident parser,
whole-book index, filesystem cache or SDK change was introduced.

The implementation starts from the passed fixed-reading baseline 7d0672a;
implementation commit is f1b95a23af8682b7670eccfb2f4740c192b2dc05. The baseline
tag/image/evidence and latest observed primary 2607/sibling 710 remain untouched.
The device remains last-confirmed awake Home with serial closed. This source
candidate has not been installed or tested physically; M1 acceptance is pending.

## Identity and validation

The reuse identity is one PhasedReader object's immutable source session,
settings and current owned spine selection. Layout parameters/font/style policy
are fixed by that object and this compiled implementation; parsed text styles
are preserved in copied runs. The owner must close before rebinding/replacing
the source. A source pointer, filename, size or zero-byte probe never establishes
content identity. Every actual SD reopen retains the full SHA identity path.

The first successful layout in a chapter completes actual XML EOF and
ChapterSource::finish before certifying exact page/character totals. A chapter
change/revisit requires a complete layout. Later same-chapter page/anchor
requests can break the outer step loop after their candidate is determined;
they never ask the sink to stop or mistake partial session counts for totals.
Anchor requests retain the last candidate at/below the requested anchor, wait
for a strictly greater page start or actual EOF, and handle chunk overshoot.
The parser is aborted and both arenas reset before the borrowed display storage
is reclaimed and copied data painted.

Current-plan validity is cleared before every layout attempt that could replace
its text. Any failure clears both plan and chapter certification; public page
info retains the last successful value, and staged frames publish only on
success. SampleReader's failed status remains latched until explicit reopen.
Close/new open/settings and lease reconfiguration invalidate reuse. The
no-plan reader retains a correct prefix-layout fallback for repaint.

Pre/post reuse checks call the actual source's zero-length read with a valid
byte destination. FileBookSource verifies cancellation, handle and live file
size before returning, including a new cancellation check after size() itself
may yield. Prefix teardown also checks the live source before publication.
Cancellation from a payload read, lease release or post-paint size callback is
covered through the production UI path. These probes do not detect same-size
in-place substitution outside the immutable-owner contract, and prefix turns
do not discover a newly failing I/O operation in an unread, previously verified
suffix. Prefix cost grows with page position; late pages can approach complete
chapter work. Exact selected-page UTF-8/glyph checks and empty-chapter handling
remain in force.

## Authored host evidence

The same common SampleReader/reader_ui benchmark was compiled against an
isolated 7d0672a reference and this implementation. Three interleaved pairs,
44 operations each, cover both orientations and both bar settings on a
deterministic styled deflated EPUB (94 portrait/98 landscape pages). All 132
paired rows match page/spine/anchor/character totals and numeric pixel digests;
separate production tests compare complete frame bytes to fresh full-layout
oracles. Source/fixture/raw-run hashes and min/max timings are recorded in
artifacts/m1-reader-efficiency-host.json.

| Operation, host ASan/UBSan | Baseline median ms | New median ms | Payload reads before → after |
| --- | ---: | ---: | ---: |
| Current-page repaint | 16.034 | 0.560 | 15 → 0 |
| Next and resulting UI render | 32.0345 | 1.6795 | 30 → 2 |
| Previous and resulting UI render | 32.0735 | 1.5645 | 30 → 2 |

Repaint/menu/focus/return operations use zero payload reads/seeks,
metadata/layout calls and XML allocations. With deliberately short 31-byte
file reads, full layout uses 290 reads and a prefix turn 33. Next/previous
operations use one layout instead of two and no metadata calls. Requested XML
peak falls from 14,445 to 10,318 bytes on these turns, with zero live parser
allocation at return; repaint has zero XML peak. This measures the parser's
requested payload, not total heap/RSS/allocator overhead. Existing modeled
owned/borrowed allocation totals remain 99,517/61,179 bytes and largest external
allocation 49,167 bytes. Arenas stay 48 KiB, copied page 10,820 bytes and XML
ceiling 32 KiB. These are host measurements, not SD/panel/device speedups.

Production tests cover mixed styles/entities/supported UTF-8, exact noninitial
resume, new book/source/settings invalidation, both geometries/bar settings,
middle, penultimate and final-page fresh full-layout oracles, backward spine
restoration, zero-I/O boundary clamps, late malformed XML, truncated deflate,
live read failure, cancellation and
atomic frame/info/progress rollback. The existing full metadata/late-I/O sweep
explicitly reconfigures the lease to invalidate reuse, keeping its complete
parse coverage. The admission-profile observer accounts nonpayload session
probes outside parser phases separately; payload reads still require a phase
and all original admission/member/semantic controls remain required.

## Reviewed resource exception

All four preliminary clean resource reports failed the old flash ceiling by
exactly **848 bytes** and pass the existing RAM and original selected-stack
ceilings. RTC remains 300 bytes, loop task 16,384 bytes, layout frame 1,248
bytes. New repaint/probe frames are 96/32 bytes; FileBookSource::readAt is 32
bytes. The default retry had zero changed input paths; an earlier compilation
was correctly rejected when its build inputs changed and is not accepted evidence.

The exception is limited to these measured flash costs. Independent adversarial
and maintainability reviews at 100a6b4 explicitly approve at most +848 bytes
per variant, with mandatory tightening to smaller final clean measurements. No safe simplification found in the early source review
removes the owned-page reuse, exact chapter certification and live guard code
while preserving their contracts. The measured benefit removes redundant
whole-chapter rendering work without increasing runtime allocations or keeping
XML alive. This is a feature-specific tradeoff, not a general margin increase.

| Variant | Old flash ceiling | Final clean bytes / exact ceiling | RAM ceiling unchanged |
| --- | ---: | ---: | ---: |
| default | 1,239,648 | 1,240,496 | 133,264 |
| dev-usb | 1,254,896 | 1,255,744 | 134,080 |
| regional | 1,243,456 | 1,244,304 | 133,776 |
| regional-dev-usb | 1,258,736 | 1,259,584 | 134,592 |

All four final clean builds at `f22f72044de2f20bac68051263ddf2e7f9ffa4d2` exit 0.
Their measured flash sizes equal the approved maxima; no slack remains. The
regional development image is 1,259,584 bytes and fits the same 308-sector app1
span with 1,984 padding bytes, preserving the partition/NVS boundary. Preserve all
original selected functions and add the repaint, source-probe and live-file-read
frames at their measured limits. No file exclusions, external adapters, line
thresholds, RAM/RTC/task ceilings or dependencies change. Final reports and complete output archives are recorded in
`artifacts/m1-reader-efficiency-builds.json`; both independent final identity audits are CLEAN.

## Integration and remaining gate

Root owns the reader source/header, FileBookSource live guard, reader tests,
host profile accounting and firmware/main lifecycle integration. Cover owner
remains separate: cover adapter/helper/tests, optional bounded decoration only;
its parent-approved source-only generator may own up to 64 KiB working storage
only while the reader is closed and ZIP discovery is released. The 16 KiB
raster-reuse cap is separate. Generator/cache storage must be fully released
before opening-message paint and book open, with real free/largest-block and
admission-reserve checks and placeholders when insufficient.
The cover owner must propose renderer API changes and hand back synchronous cache disable/
release hooks before root integrates lifecycle calls. This implementation
does not edit Library renderer, cover adapter, catalog/admission lifecycle,
firmware/main, SD migration or the separate tooling checkout.

The complete host gate and all four clean native gates pass. The exact proposed
image is build `f22f72044de2-regional-experiment`, SHA-256
`5b06b341a17f27b9b33a59c5286253236de7307f5d6d5c3d43c28d25eb501dd0`, at
`.build/efficiency-candidate-f22f720/firmware.bin`. Both independent final native audits are CLEAN, including current identities,
all 334 canonical/archived outputs per variant and independently rederived
resources/NVS/sleep/RTC/task guards. Hardware stays closed until the
parent grants that reviewed image. Before promotion, repeat all 23 unchanged
physical stages and compare latency, heap/largest block and reader-task stack
against the frozen baseline. Never restore older NVS for test setup. Owner
optics/buttons, both orientations/settings, sleep/wake, battery and broader
book acceptance remain manual; the 3–5 day trial freeze has not started.

The host preparation and production cover adapter use the same nonnegative
mtime rounding to even seconds. Actual FAT timestamp preservation and card
transfer remain pending; stat/CRC decoration never establishes book identity.
