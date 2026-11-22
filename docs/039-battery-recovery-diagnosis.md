# Installed battery recovery diagnosis

Installed runtime source is `f22f72044de2f20bac68051263ddf2e7f9ffa4d2`,
with evidence checkpoint `c505be373e25d75e1e5ec41eb016ca7423d6db0d`.
The owner reports a first-spine transition error, then physical sleep/wake
appearing to lose the selected book and position. Source reproductions below
establish general defects; they do not establish the device's earlier reset,
record contents, or the original chapter error's status.

## Storage ownership

| Location | Installed use |
| --- | --- |
| SD | Read-only EPUB bytes under `/Newt/Books`; selected filename is opened directly and the entire current source is SHA-256 checked on actual reopen. |
| NVS | `newt_selected` holds bounded filename/full SHA; per-book `nb…` namespaces hold full digest and two progress slots; `newt_resume` holds two destination-intent slots referencing an acknowledged progress checksum. Reader preferences remain separately stored. |
| RTC | One-shot SleepRecord and selected SD identity for eligible deep sleep or explicitly marked rejected-entry restart. Cold power-on ignores RTC. |

NvsProgressBackend already requires `nvs_set_blob`, `nvs_commit` and bounded
readback. Both durable stores validate schema, checksum and slot selection.
The startup preservation guard remains unchanged and essential; unreadable,
unknown or uninitialized storage never authorizes erasing existing records.
This fix moves, migrates and erases no records. SD storage adoption requires a
separate coordinated design; changing the medium alone leaves the demonstrated
state-machine bugs intact.

## Reproduced error followed by sleep

A failed chapter turn leaves `SampleReader::hasProgress()` true and `opened()`
false, with the last successful frame, locator and tracker intact. The actual
Reading exit branch successfully saves that checkpoint. It only arms pending
recovery on a failed save. Installed SleepSession nevertheless requires an open
reader or pending recovery, changes the requested Reading destination to Home,
and produces a Home record without book/position. Actual post-arm sleep entry
then commits a Home destination. Both deep-sleep and power-on restoration land
on Home while selected full SHA/filename and per-book progress remain intact.
Manual exact-page reopening reproduces the last good frame.

The narrow fix allows an acknowledged checkpoint with retained reader progress
to keep an intended Reading destination. Intentional Home, failed initial open,
unsaved checkpoints and unverified selections keep their previous gates. Source
identity, reset eligibility, checksum matching and reader admission are unchanged.
No pending recovery is injected into an intentional Back/Home route.

`tests/failed_chapter_sleep_test.py` uses the actual reader with borrowed display
parse storage and verbatim firmware save/exit/post-arm entry/setup branches.
Seven failures (malformed XML, unsupported glyph, XML budget exhaustion, missing
chapter, live read failure, parser allocation failure and source cancellation)
run across both orientations and both deep/cold recovery: 28 exact page, frame,
settings and statistics checks. Actual unsaved progress/selection abandons sleep
before hardware admission; Back and failed initial open remain Home. The old
condition is compiled in an isolated copy and fails the specific destination
assertion. Injection of cancellation does not prove a physical Power button
interrupts an ordinary turn.

Two independent source reviews are CLEAN. Complete host/native measurements
and physical battery acceptance are recorded separately; this source result
alone is not a release approval or a remedy for the chapter parser failure.

## Separately reproduced periodic-save gap

An active reader advances, and actual `saveReaderProgress()` commits its newer
anchor/statistics/sequence. The durable Reading intent still references an older
progress checksum. Actual power-on setup loads the new progress but refuses its
wake destination due to that mismatch, leaving Home. A new selection also has
no Reading destination until its first deliberate sleep. These are destination
and acknowledgement gaps, not demonstrated record deletion.

This gap remains unresolved in the narrow fix. Sequential progress then intent
writes require interruption coverage: loss or failure between commits can
invalidate the previous reference even when both old/new records survive.
Do not claim atomic battery recovery merely after adding an intent write.
Intentional Home tombstones, full digest authorization, ambiguous/unknown-record
rejection and preservation of existing records must remain explicit.

## Returned-device observation

After the owner explicitly returned the connected unit, one bounded read-only
observation reverified the original X3 fingerprint and exact installed HELLO
build. STATUS reported idle Home, generation 1, ready, no dropped input; DIAG
reported USB reset (11), no active reader/opening, and progress read permission.
`current_book=0` describes a closed reader workspace. It does not expose or prove
absence of a durable selected record. The prior battery reset cannot be inferred
from a later USB reset.

Home FRAME returned `ERROR unsupported-format`: its four-gray selector frame
cannot be labelled monochrome by the installed bridge. The connection was
closed immediately. No input, explicit reset, flash, card/NVS write or recovery
operation occurred. Raw transport/capture attempts remain private and untracked.
Current durable records have not been recaptured; historical prejourney backups
must never be restored or treated as current evidence.

## Focus latency and remaining gates

Historical installed evidence measured Library focus median 1.4921195 seconds
and page turns 0.734668 seconds; full Home paint was roughly 2.307 seconds.
These numbers include composition and panel work. Source-only inspection finds
whole-Home rendering/rotation before a 960-byte cursor-island transfer and
whole-Library recomposition on every focus. Each valid optional thumbnail adds
377 small reads; missing thumbnails have no payload reads. Actual card contents
and current isolated rendering/panel timing were not measured. Library already
requests FAST_REFRESH after entry; Home island acceptance remains unverified.

Release blockers remain actual first-spine error diagnosis, periodic/cold-start
transaction recovery, owner battery sleep plus distinct cold-start evidence,
and Home/Library focus latency/ghosting acceptance. Preserve the expanded
27-stage chapter journey; its USB evidence cannot stand in for battery use.
See `040-battery-recovery-acceptance.md`. No trial freeze or new flash is granted.

## Completed source checkpoint

The source fix is `7f44c25a78bc6e146c4717eeb42e4f9c5496281b`; exact reviewed
resource adoption is `06248a911bb8b7adae9d6ef1faffa4ef3ae877e5`. Full
`python3 tools/check.py` exits 0, including ASan/UBSan, both orientations, exact
previews and the 28-case failed-chapter recovery regression with its compiled
old-condition negative control. Post-adoption structural and resource fixtures
pass (21 and 39 tests). All four final clean native builds exit 0.

Two independent final native audits are CLEAN. They check current source,
procedure, toolchain and configuration identities; all 2,672 canonical/archive
output hash comparisons; ELF/map/image remeasurement; and linked NVS, sleep,
retained RTC and strong 16 KiB task guard proofs. SRAM, RTC and all original
34/36 selected frames remain unchanged. Compiled/tool/test scopes are identical
between the source fix and measured checkpoint; only exact resource provenance
and documentation changed. See `artifacts/m1-battery-recovery-builds.json`.

The reviewable regional DEV candidate identifies as
`06248a911bb8-regional-experiment`: 1,259,600 bytes, SHA-256
`6170154d938c1ccdecfd7f53451d4da1bf0fe078e3b30c933674906c56fe59aa`.
It is not installed or authorized for flashing and does not resolve the other
release blockers. Integrating the source fix into a newer combined checkpoint
requires that checkpoint's own clean measurements and exact image proposal.

Library checkpoint `0993cd82133138fc7adfa4cd2aabe717b0104485` is already
integrated through `0dec6a69ac9530f8007a714e8f2c412d834cde40`. The installed
read-only adapter matches the host's nonnegative epoch-second timestamp rounded
down to two seconds and exact source size. Actual FAT/card timestamp agreement
has not been tested. No cover preparation or transfer was performed during this
battery investigation. Automatic covers and future chapter-cache wiring remain
paused while the installed failures are diagnosed.
