# Reader efficiency: historical bounded physical gate

The parent-authorized efficiency image passed the unchanged physical reading
journey: 23 stages, 245 operations and 646 CRC/order/ACK-validated responses.
Median request-to-terminal page-turn latency fell from 1.607892 to 0.734668
seconds across six turns. Library focus latency stayed comparable. Both final
independent evidence reviews are CLEAN. Owner M1 acceptance remains pending;
the 3–5 day trial freeze and SDK/apps milestone have not started.

This historical 23-stage journey did **not require a chapter crossing**. Its
successful short page-turn and reopen checks do not establish later-chapter
readability or book completion. The owner subsequently reported an unavailable
reader while reading; the exact failed position and cause remain unknown.
[038-chapter-transition-diagnosis.md](038-chapter-transition-diagnosis.md)
records the source diagnosis and the stricter 27-stage chapter gate. That new
gate has host negative-control coverage and has not run on hardware. The frozen
physical receipt below retains its original historical scope and bytes.

The private-content-free receipt is
[m1-reader-efficiency-physical.json](../artifacts/m1-reader-efficiency-physical.json).
It supersedes the hardware-planning statements in
[034-reader-efficiency.md](034-reader-efficiency.md) and the pre-install hardware
fields in [m1-reader-efficiency-builds.json](../artifacts/m1-reader-efficiency-builds.json).
That frozen build manifest remains byte-identical for provenance.

## Exact installed image and preservation

- Compiled source: `f22f72044de2f20bac68051263ddf2e7f9ffa4d2`.
- Reviewed source: `e3acb5d731b51f649a11242d8f0a970cd53e98e3`.
- Build: `f22f72044de2-regional-experiment`.
- Image: 1,259,584 bytes; SHA-256
  `5b06b341a17f27b9b33a59c5286253236de7307f5d6d5c3d43c28d25eb501dd0`.
- App1 write span: `[0x780000, 0x8b4000)`, 308 sectors, including 1,984 bytes
  of `FF` padding.
- Complete 16 MiB preboot readback SHA-256:
  `86fc3ff1f91ab711f3cd39b30b4457b4bd1d611913fe82a2b8d2123d4580b91a`.

Three complete prewrite reads matched. The forensic proof reconstructed every
NVS byte, explained all six logical changes across 103 live records, and passed
114 negative controls with zero unknown changes. The exact padded app image
matched the entire readback; every byte outside the approved app span was
unchanged before boot. Two planned 35-second boots were clean, with the NVS
startup guard active. The first 64 KiB after startup, including NVS, matched
the prewrite snapshot.

Baseline source `7d0672a2d5d02c1e9358718e8970291bf79a2e6a`, its tag, portable
bundle and known-good image remain immutable. No historical snapshot was
restored. All four clean native build archives and their 334 outputs per
variant remain intact; the full host gate already passed.

## Physical comparison

| Measurement | Baseline | Efficiency image |
| --- | ---: | ---: |
| Six turns: median request to terminal | 1.607892 s | 0.734668 s |
| Six turns: median whole paint path | 1,038.5 ms | 541 ms |
| 170 Library focus moves: median request to terminal | 1.490322 s | 1.4921195 s |
| Reader post-stage minimum free heap | 69,624 bytes | 69,624 bytes |
| Reader post-stage largest free block | 51,188 bytes | 51,188 bytes |
| Observed minimum free loop-task stack | 5,752 / 16,384 bytes | 5,752 / 16,384 bytes |

Turn median latency is 54.31% lower. All 11 reader-stage whole-paint measurements
and five measured open paths improved. Whole-journey elapsed time fell from
376.317 to 367.733 seconds, about 2.28%; Library navigation and cold-source work
still dominate that fixed journey. Library focus measurements show no material
change. Reader free-heap maxima varied slightly, so only the observed minimum
and largest-block result are equal. There are 186 valid task-stack observations,
including one after the final reader paint.

These are one fixed journey per image, with identical commands, stages and
plan. Preserving current progress means paired stages can show different pages.
This is not a repeated same-page statistical trial. `refresh_ms` includes
rendering and the blocking panel call; it does not isolate waveform, panel or
repaint latency. Post-stage heap readings do not establish transient allocation
peaks, and observed stack minima do not bound every possible input. No extra
menu, repaint, sleep or card operation was issued for timing.

## Final state and next gate

The historical journey ended awake at normal Home, with the reader closed,
opening false and serial closed. Its last observed anchors were **primary 3442 /
sibling 1600**. The owner has since used the device; current progress is unknown.
Durable NVS was not recaptured after this journey. **All prejourney snapshots
are now historical and must not be restored.** Any future hardware phase needs
fresh matching backup and reconciliation that preserves this newer progress.

Owner checks remain: optical quality/ghosting, physical buttons, settings in
both physical orientations, physical sleep/wake, battery behavior and broader
actual-book checks. Passing this fixed gate does not start the trial freeze or
complete owner acceptance. This exact hardware phase is closed; no further
device action is in progress or authorized by its consumed grant.

The owner reports CrossInk cold-open around five seconds and subsequent
Martian reopen effectively instant, and finds Newt's approximately 19-second
open unacceptable. This comparison is owner-reported, not controlled evidence
from this journey. Newt's measured five open paths still range approximately
6–19 seconds. Immediate canonical Newt Opening/Indexing feedback before long
hash/admission work, working cancellation preserving the saved checkpoint, and
cold/warm open latency are the next usability gate. Do not invent progress or
expand the cache framework. The parent separately coordinates a read-only
implementation and license comparison before any larger transplant decision.
Read-only source inspection finds `openSdSelection()` already calls `paint()`
with an Opening message before selection work; the current Library renderer
places it in a small footer line. A dedicated visible working state and its
cancel behavior still need shared-renderer and owner verification.

The four-cover Library presentation is already in the accepted baseline.
The later automatic-cover handoff is source `c2df314`, evidence HEAD `236e17e`,
and is not integrated in this installed image. Root owns lifecycle wiring.
Generation must run only with the reader closed and discovery released, then
cancel, unwind and synchronously disable/release before opening-message paint,
book open, sleep, Library exit, detach, rescan or error. Its 64 KiB work cap and
64 KiB reader reserve need actual combined stack/heap/resource proof; partial
component measurements do not establish incremental firmware cost. Two source
CRC passes plus FAT path rebinding can read up to 192 MiB at the 64 MiB source
limit, so latency and battery behavior need a bounded integration decision.

NLC2 remains optional decoration: exact 3,412-byte sidecars, 144×188 1bpp,
18-byte rows, source size, nonnegative mtime rounded down to two seconds, and
CRC validation. Host and native rounding contracts agree. Missing, stale or
corrupt input produces placeholders; same-stat collisions never authorize a
book open. Actual FAT timestamp preservation and card transfer remain pending.
The parent coordinates any next combined image and its exact hardware grant.

Raw captures, backups, device/book identities and private inputs stay in ignored
private local directories. This report commits only numeric evidence and
source/build hashes.
