# NEWT-31 Part B: cached first page and verified publication

Owner's item 5 replaces the original timing restriction. Size and FAT modified
time are change hints only. A matching fixed-schema, CRC/full-digest-bound
sidecar plus admitted receipt permits a provisional cached page. Full SHA-256
still runs on every open, including a retained Home → Read. Missing, damaged,
changed or unreadable hints take the synchronous complete-source path.

The pending SHA context reuses the existing producer retry ledger. It holds no
additional idle allocation or input buffer; the C3 session remains 1,952 bytes.
Idle steps borrow 4 KiB of the native display buffer and restore that prefix
from the published logical frame before returning. Queue/USB input, the
existing quiet interval and producer power conditions gate each step. A due
checkpoint may advance these bounded idle steps at low or unknown battery too.
There is no progress/selection transaction at idle completion. Periodic saves
wait for quiet, an empty input queue and completed verification; continuous input
may leave them due beyond 30 seconds. DEV exit rows report checkpoint age.
An uncached foreground turn or explicit save/exit finishes any pending digest using the existing 32 KiB loan, under the existing
20-second opening budget, then follows the normal verified transaction.

Progress, selected identity, destination intent and derived publication are
barred until full verification. Cancellation pauses a digest; verification
failure remains unverified after close. A digest mismatch revokes receipt and
cache authority, discards the window's tracker, and reopens the changed source
through full admission. Existing Reading and Library UI carry a change notice.
A failed save or cancelled reopen still permits Back/Sleep. A refused durable save
retains its exact live page and unconfirmed warning in existing RAM/RTC recovery,
except for a merely deferred accepted open. A mismatch revokes the old RAM
checkpoint before any reimport attempt; it cannot return through a failed save.

The deferred window keeps separate RAM debts for an explicit selected-book
commit and the accepted-open checkpoint. Verification alone grants no right to
rewrite a different durable selection after RTC wake. A matching restored
selection is confirmed by readback. The first verified save retains accepted-open
permission to repair a missing acknowledged peer; only a verified checkpoint
clears that debt. The foreground candidate uses the clock after verification,
since a mismatch can replace the reader with a new revision during that action.

One canonical page gate admits pending page-cache reads. Cache fallback itself
refuses ordinary parsing while pending, including CRC-valid inner-cache faults.
An uncached or faulty-cache Next/Previous returns its frame loans, completes
verification through the bounded foreground path, and retries the turn in the
same action. A mismatch reopens the changed source before retrying the action,
retaining the changed-book notice. A real book boundary remains a clamp without
forcing a digest. Only cancellation can pause that barrier; it then requests a
repaint of the pending state instead of silently refusing the turn.
A terminal digest read failure shows “Could not check book” and Back remains
available. Periodic saves defer while verification is pending. Bounded idle
steps detect a mismatch; the separate changed-source service reimports and
reopens it, then paints the changed page and notice. Failed reimport or
selection publication keeps an accurate changed-book outcome. A changed-hint Home open preserves its freshly
verified import before binding the obsolete durable identity and reuses that
import when admitting the new identity. A Library open that detects an equal-hint changed source revokes its
old receipt and retries the full changed-book path once. The verified import is
consumed directly by that open instead of hashing the new file twice.

The current periodic save policy and coalesced chapter metadata behavior are
documented in [073](073-idle-progress-save.md).

Larger sequential source reads preserve all full/block SHA values and consumed
block validation. The pinned C3 build already enables hardware SHA; its
sha256_alt object acquires/releases the peripheral within each processing call
and stores intermediate state in the context, so idle steps and page checks
can interleave. No dependency or hardware change is involved.

For the 2,135,255 / 2,236,143 byte fixtures, warm payload reads fall from
522 / 546 to 66 / 69. Total warm source calls, including probes, fall from
1,047 / 1,095 to 135 / 141; cold total calls fall from 1,567 / 1,639 to
721 / 754. Every synchronous selection still reads exactly the source length.
These are host work counts, not ESP SD latency measurements.

Unchanged durable records already avoided writes. This change retains the exact
checked peer snapshot within one transaction, invalidates a written target for
fresh readback, and carries verified peer locators to derived checkpoint work.
No RAM hint authorizes a no-op. Unknown schemas/extents, ambiguous peers,
protected checkpoints, lost acknowledgements and write/readback errors retain
the existing behavior. Native unchanged progress reads fall 12 → 10; changed
progress reads fall 15 → 13 with one write, sync and target readback. Complete
unchanged accepted-open reads fall 54 → 38 and opens 6 → 5. Full Back reads fall
57 → 41 and opens 6 → 5, retaining its one Home-destination write/sync/readback.
The complete checkpoint counter stubs derived hooks; their real files and crash
cuts are checked independently by the checkpoint identity transaction suite.

DEV schema 3 keeps the existing 76-byte exclusive timing owner. Close labels
attribute Home painting, progress, reader release, exit and destination work;
raw slots remain available. First-page and digest-finish rows provide absolute
millisecond markers, with action microseconds for interactive first-page opens.
Telemetry uses bounded nonblocking writes. Device residual remains a physical
measurement, not a host assertion.

The extra cold refresh is the existing cancellable working screen followed by
the first real page. It remains for sources without a usable receipt/cache.
Cold title metadata previously zero-filled through 16-byte writes; it now reuses
the already-written 84-byte prefix. Canonical bytes/CRC/atomic commit stay the
same; complete/title-only native writes are 11 / 7. No book is privileged.

Expected device effects: matching cached opens remove the old complete-source
selection delay (about 1.2–3.7 seconds in the Lead's five USB observations) from
the first-page critical path, subject to receipt/chapter/cache checks. Digest
work moves after the page and may pause for input. Larger reads also shorten
forced foreground completion. The checkpoint change removes one transaction
and 16 reads in the measured unchanged paths; physical saving must be measured.
Metadata uses fewer small writes; no proportional latency saving is claimed.
The cold working-screen refresh remains. Close residual should move into the
new exclusive labels, with the Lead checking residual below 100,000 us.

The full turn-cost matrix passed 1,248 cells across 24 heterogeneous fixtures,
with 48 cold/produced exact pairs per fixture and the uninstrumented controls.
The focused deferred-window, startup, storage, metadata, source-work and crash
regressions pass, including removal controls. Final R7 Opus 5.5 high adversarial
and maintainability review is CLEAN with no High, Medium or Low findings.

Four clean measurements of implementation `b2e2a6b` bind append-only flash
record 52 and protected record 27. Raw reports and the resource review are in
`artifacts/newt31-b-raw-code-health-{variant}.json.gz` and
`artifacts/newt31-b-resource-review.txt`. Earlier records, memory floors,
exclusions, file allowances and partition reserve are unchanged.

| Variant | Flash before → after | SRAM before → after | RTC before → after |
| --- | ---: | ---: | ---: |
| default | 2,173,872 → 2,186,256 | 133,232 → 133,232 | 300 → 300 |
| DEV USB | 2,317,040 → 2,330,736 | 134,064 → 134,064 | 304 → 304 |
| regional | 2,180,560 → 2,192,928 | 133,728 → 133,728 | 300 → 300 |
| regional DEV USB | 2,325,728 → 2,339,424 | 134,560 → 134,560 | 304 → 304 |

Protected selected frames rise from 357/386/359/393 to 391/420/393/427:
34 new selected keys, 6/8/6/8 inherited increases, eight inherited
reductions per lane. Growth is bounded transaction/source work, with the
largest inherited increase 32 bytes. Every inherited key remains selected. The additions include canonical helper
bodies and the legacy exit wrapper, alongside the new 112-byte exit body. These individual static frames are not a whole-task peak or
measured runtime margin. No extra memory is held while the feature is unused.

Quick Linux passed in 245.609 seconds. All four final clean resource gates pass
with clean firmware identities from `4fa0c7f`; flash, SRAM, RTC, selected frames
and compiled-input digests exactly match the reviewed raw reports. All four
`--adopt ab2f910` checks and the pinned `fcca87a..HEAD` release gate pass.
The full Linux gate passed in 875.920 seconds on clean `557742a`, including
full every-page parity across 18 fixtures, both orientations/directions and
state journeys, every probe phase, and DEV serial tests. Its log and parity
receipt are archived in `artifacts/newt31-b-full-linux.log.gz` and
`artifacts/newt31-b-parity.json.gz`. Interrupted earlier attempts are not passing
evidence. Linux covers portable image decoding; the exact-source Mac ImageIO
and all physical device acceptance remain separate.
No Part B device, battery, sleep/wake or cold-power behavior is claimed.

The pending footer is refreshed by the next normal paint after idle verification;
completion does not force an extra display refresh ahead of input. A Sleep-time
mismatch follows the same changed-book reopen/save rule, but its transient notice
is not a new durable preference and is replaced by the sleep scene. A rare Home Read after source replacement may first paint an empty reader,
then the working frame and the new page. This extra refresh remains an accepted
changed-book residual. The existing
verified reopen may repeat an idempotent checkpoint; this correction keeps that
proven transaction flow rather than widening the save state machine.

Lead device measurements: compare `Reader first page: ms=... us=... pending=1`
with the later `Reader digest done: ms=... verified=1 changed=0`; check that
queued input and cached page turns win during the gap. Measure forced
foreground completion on Back/Sleep and each `Reader close us` exclusive phase,
with residual below 100,000 us. Check missing/different hints, an equal-hint
changed source, cold power-on with RTC discarded, and unchanged-page saves.
USB power, unplugged battery sleep/wake and cold battery power-on are separate
checks for the Lead. This implementation used no device and makes no physical
acceptance claim.


Main integration renumbers this document from 069 to 070 and standalone
Part B’s flash record from 51 to 52. All original measurements and their
source-bound proof remain unchanged; the exact original record is preserved in
`artifacts/newt31-original-part-b-record-51.json`. Protected record 27 remains
byte-identical. Main’s record 50 is preserved as the destination history prefix.
The integrated measurements and validation are recorded below; standalone
Part B evidence above remains separate.


## Integrated time-zone main

Merge `b06a20a` has both parents `4f06932` and `9a6753e`. The clock, zone
codec and routing preserve main behavior. Settings rows are 0 orientation,
1 slim bar, 2 refresh, 3 zone, 4 Save and 5 Cancel. The new runtime regression
uses the actual merged Settings dispatch and actual `TimeZoneStore`: row 3
only edits the draft, row 5 discards it, and row 4 verifies the independent
zone record while retaining the full-digest barrier before book-state writes.
The reader UI and time-zone conversion unit tests and all seven focused
startup/runtime suites pass. The merged full matrix passed all 1,248 cells
and its uninstrumented/removal controls.

NEWT-32 later moved the zone to the device Settings page
([071](071-device-settings.md)). Reading settings rows are now 0
orientation, 1 slim bar, 2 refresh, 3 Save and 4 Cancel, and the runtime
regression checks that Save and Cancel never touch the zone record. The row
mapping above is this merge's historical evidence.

| Variant | Main record 50 | Merged record 53 | Increase over main |
| --- | ---: | ---: | ---: |
| default | 2,177,328 | 2,189,648 | 12,320 |
| DEV USB | 2,320,000 | 2,334,112 | 14,112 |
| regional | 2,184,000 | 2,196,320 | 12,320 |
| regional DEV USB | 2,328,672 | 2,342,800 | 14,128 |

These four clean compile measurements bind flash record 53 and protected
record 28. SRAM and RTC are unchanged from both parents. All inherited
selected frames remain exact; five time-zone commit/restore/conversion/store
boundaries are newly selected, giving 396/425/398/432 entries. The formal
record-53 predecessor is 52 under the unchanged linear validator; the table
and record reason give the requested main-50 comparison. Historical branch
measurements and originals are retained, with no memory-policy exception.

The fresh closing merge-resolution adversarial and maintainability review is
CLEAN after the historical attribution wording fix. The independent installed
resource-record review is also CLEAN. All four final clean native resource
gates pass with clean firmware identities from `6af311f`; flash, SRAM, RTC,
selected frames and compiled-input hashes exactly match the reviewed reports.
All four adoption checks against main `9a6753e` and the pinned full-history
release gate pass.

On clean merged-tree commit `7ecf3de`, quick Linux passed in 273.607 seconds
and the full Linux gate passed in 925.308 seconds. Full every-page parity
passed across 18 fixtures, both orientations/directions and state journeys
(592.327 seconds). Every probe phase, the deferred-verification runtime suite,
merged time-zone/clock wiring and DEV serial suite completed successfully.
The full 1,248-cell matrix and its removal controls also pass. Logs and parity
receipts are archived as `artifacts/newt31-merge-*`; standalone evidence
above remains separately identified. Linux omits the exact-source Mac ImageIO
cover check. Neither standalone nor merged software evidence proves hardware
behavior; the Lead's separate power-condition checks remain pending.


## Final-probe cancellation correction

Lead review of `701855c` found that input arriving during the final source
probe consumed the completed SHA context and permanently marked verification
failed. Finalization now probes and checks cancellation before consuming SHA;
a paused verifier at EOF resumes directly at finalization without a zero-byte
payload read. Cancelled payload reads still contribute no digest bytes.

The focused regression injects input at the final probe for both unchanged and
equal-hint changed sources, then requires Verified/Changed after resuming with
no payload reread. Its frozen `701855c` negative control fails the pending-state
oracle. Resource and complete gate results for this correction follow below.

Clean measurements at correction source `79f0eb4` are 2,189,648 / 2,334,128 /
2,196,320 / 2,342,816 B (default / DEV USB / regional / regional DEV USB):
changes of 0 / +16 / 0 / +16 B from `701855c`. Per the Lead's instruction,
record 53 is refreshed exactly for all four lanes. Its original complete
version remains archived. Protected record 28 keeps identical before/after
and selected frames; only its source/evidence binding is refreshed for
adoption. Its original complete version is also archived. SRAM, RTC, selected
stack measurements, floors and every earlier record remain unchanged.
The closing source/regression/resource review is clean; its F1 installation
item is addressed by setting the matching protected-history source pin.
All four final clean native resource gates pass with clean `21ede33` firmware
identities and exact reviewed measurement/input matches. All four adoption
checks against `9a6753e` and the pinned full-history release gate pass. The
closing review is CLEAN after F1 closure, with both review reports archived.
On clean `30a0f55`, quick Linux passed in 255.942 seconds and full Linux passed
in 841.243 seconds, including this regression/old-code negative, deferred
runtime cases, every probe and DEV serial tests. Full every-page parity passed
all 18 fixtures in 530.569 seconds. `gate_stop_test.py` passed in 13.913 seconds;
the Lead's stop-timeout failure did not reproduce, with no concurrent suites
or reviewer tests in this worktree. Evidence is under `artifacts/newt31-finalize-*`.
Exact-source Mac ImageIO and all Lead device acceptance remain separate; no
device was used and nothing was pushed.


## Pending uncached turn correction

The Lead's USB regional DEV run at `9d0a0b6` measured cached first pages at
2.0–2.5 seconds versus 4.2–5.9 seconds on full-check opens. Idle verification
finished 10–16 seconds later and forced Back verification took about 1.9 seconds.
That run also exposed a silently dropped uncached Next during the pending
window. The UI now uses the same bounded foreground verifier after a cache-only
miss, then retries Next/Previous in the same action. Changed-source reimport,
failed-verification notices and Back availability retain their existing paths.
A refused durable selection does not stop a verified read turn; every durable
selection/progress guard still applies.
The verification interval is charged to the source-selection DEV phase.

The focused runtime regression covers unchanged and changed-source uncached
turns, a truncated current cache, and a source-read failure with Back available.
Removing the retry call reproduces the previous no-visual-change failure.
Ordinary parsing remains impossible until the full digest verifies.

For the secondary cache cost, the idle digest reads only the source and borrows
then restores a native-buffer prefix; it does not close the page-cache port.
The pending page gate did issue a separate `stored()` query before each actual
turn and repaint. Those queries add checked boundaries and can reacquire a
full-CRC lease when another page-port operation has released it. Live pages
now skip these redundant presence probes. Actual cache queries keep every CRC,
source, path and publication check. A first-page open keeps its presence probe.
This is a small generic reduction, not a claim to explain every measured
millisecond; the Lead should remeasure the first cached pending turn's v[6].
Resource measurements and closing gates for this correction follow below.

Clean measurements at final correction source `64583e0` are 2,190,016 /
2,334,560 / 2,196,688 / 2,343,248 B, deltas +368 / +432 / +368 / +432 B
from `9d0a0b6`. Record 53 is refreshed exactly for all four variants, with its
complete previous version preserved in `artifacts/newt31-turn-original-flash-record-53.json`.
Protected record 29 appends after unchanged records 1–28: every selected key
remains, pageGate tightens from 32 to 16 B, and the new transient turn helper
is selected at 96 B in each lane. Selected counts are 397/426/399/433. SRAM,
RTC, all other selected frames, floors and the other policy are unchanged.
Closing review R2 is CLEAN after restricting retry to an open cache-only miss,
so a genuine failed chapter remains visible instead of being skipped.
All four final clean native resource gates pass with clean `52dc5f6` firmware
identities and exact reviewed measurement/input matches. All four adoption
checks against `9a6753e` and the pinned full-history release gate pass. On clean
`a91ebad`, quick Linux passed in 273.278 seconds and full Linux passed in
858.450 seconds, including the focused regression and old-code negative. Full
every-page parity passed all 18 fixtures in 541.039 seconds. `gate_stop_test.py`
passed in 13.945 seconds without the reported timeout reproducing; no other
suites or reviewer tests ran concurrently in this worktree. Evidence is under
`artifacts/newt31-turn-*`. Exact-source Mac ImageIO and Lead device acceptance
remain separate. The Lead should remeasure pending cached-turn v[6] and verify
uncached turns during the pending window on the device. No device was used
and nothing was pushed.

## Foreground turn input ordering correction

The Lead's review of `0f22ae6` found that a second navigation press could pause
an uncached turn's foreground digest and discard the in-flight turn. The turn
barrier now leaves later navigation queued while it finishes verification and
the accepted turn. Dispatch then applies the queued press once, in order. This
also preserves opposite-direction pairs, without a replay flag or retained RAM.
Physical Back and Power keep their input-task cancellation tags and supersede
the interrupted turn. A non-consuming queue peek also recognizes an exit
queued before the budget began or by DEV USB. The existing 20-second open
budget remains effective;
there is no ordinary parsing before verification. DEV USB bytes that have not
yet been decoded remain buffered until the foreground action unwinds, rather
than cancelling an action before their command is known. Idle verification and
save/exit verification retain their existing input-preemption policy.

The focused runtime cases inject input during an actual foreground source read:
Next+Next, Next+Previous, Previous+Next, physical Back, physical Sleep, deadline
expiry, read failure and an untagged queued Back. Exact rendered frames and
page positions are compared
with sequential ordinary-reader turns. The existing tagged Back consumer and
Sleep exit path finish verification and exit without replay. A negative control
restores the `0f22ae6` turn-verifier policy and must fail the first-turn oracle.
Clean source `c884c07` measurements are 2,190,176 / 2,334,720 / 2,196,864 /
2,343,408 B (default / DEV USB / regional / regional DEV USB), changes
+160 / +160 / +176 / +160 B from `0f22ae6`. Flash record 53 is refreshed
exactly; its complete previous version is archived as
`artifacts/newt31-input-original-flash-record-53.json`. Flash records 1–52
and protected records 1–29 remain exact, with prior archives preserved. New
protected record 30 retains every selected key: the old no-argument verifier
becomes a zero-frame tail call, its core remains 64/80/64/80 B and the new
cancellation callback is 32 B. Selected counts are 399/428/401/435. SRAM stays
133,232 / 134,064 / 133,728 / 134,560 B; RTC stays 300 / 304 / 300 / 304 B.
All other selected frames, floors, reserve, exclusions, adapters and allowances
are unchanged. No replay flag, retained RAM or new idle buffer is added. These
compiler frames do not establish peak stack or hardware margin. Fresh bounded
GPT-5.6 Sol high closing review R1 is CLEAN for source, tests and the exact
four-lane resource proposal. All four final clean native resource gates pass
with clean `520e57a` firmware identities and exact reviewed measurements and
compiled-input matches. All four adoption checks against `9a6753e` and the
pinned full-history release gate pass. The first full run found a DEV event-type
mismatch in the shared host fixture; `5071aee` makes that fixture use the actual
DEV `ButtonEvent` or release byte payload. Firmware inputs and measurements
remain unchanged, as confirmed by all four subsequent adoption checks. Fresh
fixture follow-up review R2 is CLEAN. On clean `5071aee`, quick Linux passed
in 292.945 seconds and full Linux passed in 984.933 seconds. Attribution passed
in 113.840 seconds and the eight-case input runtime plus old-policy negative
in 106.504 seconds. Full every-page parity passed all 18 fixtures in 639.235
seconds. The stop test passed in 14.062 seconds; the reported timeout did not
reproduce, with no other suites or reviewer tests running in this worktree.
Evidence, including the first failed invocation and its correction, is under
`artifacts/newt31-input-*`. The Lead should verify burst Next presses, opposite
directions and Back/Sleep during foreground verification on the device.
Exact-source Mac ImageIO and physical acceptance remain separate. No device
was used and nothing was pushed.

## Timeout outcomes and saturated exit delivery

The R4 review of `83c9804` found two remaining input outcomes. A foreground
turn that reaches the existing deadline now paints “Book check timed out.
Press again.” before dispatch can consume another queued press. This chooses
an explicit visible failure rather than retaining a replay action. Verification
remains resumable; the next press completes its own turn once. The notice sits
above the footer so an unconfirmed-save warning cannot hide it, and the next
reader action acknowledges it. Ordinary parsing still requires verification.

A full physical input queue now retains a Back/Power cancellation request and
waits for a slot to insert that exit at the front. The separate input task can
wait while cancellation unwinds foreground work and the UI frees a slot.
Rejected navigation only increments its loss counter; it cannot consume an
exit's cancellation. Capacity remains 32 and no retained state is added.
Focused tests combine queued navigation with timeout, and fill the actual
32-slot queue before Back and Sleep. Both old-code negative controls fail.
Fresh bounded source review is CLEAN. Resource measurements and closing gates
for this correction follow below.

Clean correction source `9dffd8d` measures 2,190,496 / 2,335,024 / 2,197,184 /
2,343,712 B (default / DEV USB / regional / regional DEV USB), deltas
+320 / +304 / +320 / +304 B from `83c9804`. Record 53 is refreshed exactly,
with its complete previous version preserved in
`artifacts/newt31-r4-original-flash-record-53.json`. Flash records 1–52 and
protected records 1–30 remain exact. Protected record 31 adds the transient
64 B timeout renderer; every previous selected key/frame remains unchanged.
Counts are 400/429/402/436. The selected cancellation-consumer boundary remains
measurable through noinline after its overflow caller was removed. SRAM stays
133,232 / 134,064 / 133,728 / 134,560 B; RTC stays 300 / 304 / 300 / 304 B.
No idle memory, queue capacity, floor, reserve, exclusion, adapter or allowance
changes. Compiler frames do not establish peak stack or hardware margin.
Fresh bounded closing source/resource review R2 is CLEAN, including exact
four-lane reports, hashes, history and pins.
All four final clean native resource gates pass with clean `8335b78` firmware
identities and exact reviewed measurements and compiled-input matches. All
four adoption checks against `9a6753e` and the pinned full-history release gate
pass. On clean `c6d1367`, quick Linux passed in 322.126 seconds and full Linux
passed in 1,152.200 seconds, including the focused combined queued-timeout and
32-slot Back/Sleep cases, both old-code negative controls and the shared UI
tests. Full every-page parity passed all 18 fixtures in 796.898 seconds. The
stop test passed in 13.983 seconds; the reported timing failure did not
reproduce. No separate suites or reviewer tests ran concurrently in this
worktree. Evidence is archived under `artifacts/newt31-r4-*`. The Lead should
check the timeout notice followed by a queued turn, and saturated navigation
followed by Back/Sleep on the device. Exact-source Mac ImageIO and physical
acceptance remain separate. No device was used and nothing was pushed.

## NEWT-45: bounded idle batches and lifecycle marks

The idle verifier now repeats the same 4 KiB step for at most a 20 ms
admission window, stopping before another step whenever physical or USB input
is pending. This leaves about 10 ms for the last SD read/hash and native-prefix
restore toward a roughly 30 ms input target. An individual SD call cannot be
preempted: the target still needs device measurement. The completed pass restores
only the same 4 KiB native prefix, retains the previous cancellation hook, and
allocates nothing. A cancelled read still retries its offset; the SHA, mismatch,
I/O-failure and durable-publication rules are unchanged. Ready passes no longer
pay a 10 ms delay per 4 KiB; they yield 1 ms only when no input is pending.

Power-on shares one setup-local resume-intent snapshot between selected-peer
arbitration and checkpoint authorization. It does not retry a failed read under
another owner. Empty, corrupt, unknown and unreadable records keep their existing
Home/failure behavior. Warm reset/RTC policy and all progress, preference and
resume-intent writes are unchanged.

Optional identity/estimate writes remain on Back and Sleep. Cold Home reads the
identity for the exact acknowledged locator and uses the saved estimate when
complete counts are unavailable (`sd_home_progress.h`). The cold-start and exit
regressions require the canonical chapter label after an acknowledged exit.
Moving those writes to a later idle would lose that guarantee if power is lost
between exit and that idle. The existing periodic-save deferral is deliberately
a weaker contract and does not authorize weakening explicit exits.

DEV adds two rows, with stable field names:

- `Sleep phase us:`: `press_wait`, `saving_frame`, `verify_finish`, `save`,
  `stats_fold`, `render_lsb`, `upload_lsb`, `render_msb`, `upload_msb`, `refresh`,
  `resume_record`, `panel_sleep`.
- `Boot phase ms:`: `nvs`, `panel`, `app`, `progress`, `prefs`, `parse_storage`,
  `selection_restore`, `resume_load`, `sleep_restore`, `device_clock`,
  `first_paint_start`, `first_paint`.

The Sleep row also reports `order=paint-save` for a book exit and `order=home`
for the ordinary Home path.

Sleep's `press_wait` measures the power-button release wait after suspending the
input task, not time waiting in the input queue (the event carries no clock).
`verify_finish` is the foreground verification barrier, including selection
reconciliation; `save` excludes that barrier from the explicit save's total.
Saving-frame and stats times are separate. Plane uploads include selector
conversion; `refresh` is the final refresh, excluding the gray-base admission.
A monochrome fallback uses the MSB render/upload fields. Repaints after a storage
warning add to the plane fields. `resume_record` covers the SD destination checkpoint; `panel_sleep` covers panel/rail shutdown, excluding
source close. The single Sleep row is written before the final `Serial.flush`.
It requires the existing DEV diagnostic owner, whose allocation can already fail.

Boot fields are sequential durations except `first_paint_start`, which is elapsed
setup time at the boot open/first paint boundary. `resume_load` includes the
selected book's progress reload, and `device_clock` includes input-task startup.
`first_paint` includes an exact-page boot reopen when needed. The row is emitted
at the end of setup. ROM/bootloader time is outside these marks.

Both rows use fixed stack formatting buffers; they add no resident allocation.
Sleep's stack sink borrows the scheduler's existing observer pointer and survives
nested reader profiles without changing its 96-byte target diagnostic owner.
USB backpressure can truncate a diagnostic write; accept a measurement only with
a complete newline-terminated row. These marks do not establish hardware latency.

Host coverage uses a multi-MiB authored source, checks fewer idle passes,
interrupts a batch after three reads, resumes to the same SHA, restores both
frame orientations, rejects equal-size/equal-hint corruption and source I/O
failure, and keeps idle verification read-only. The actual startup fixture
counts one power-on intent load through the cold/failure matrix. Removing the
batch or shared boot snapshot fails their respective work/read-count oracles.
The telemetry fixture checks nested profile routing, reset/unwind lifetime,
verification subtraction and the boot first-paint boundary.

### Owner-approved early book Sleep image (2026-10-27)

A book's Sleep action now suspends physical polling, drains queued cancellation
tokens, and paints the sleep image while the reader is open. It then completes
the unchanged verified progress transaction, folds statistics, prepares the
sleep record and enters the existing resume-record/panel-shutdown path. Only
Back keeps the saving frame. The final loop skips the already completed Sleep
paint; a changed page-save warning is repainted immediately, and the existing
resume-record failure repaint remains in physical sleep entry. Home Sleep and
all wake paths keep their existing order.

A second physical power press during the post-paint save is ignored: the input
task stays suspended until power-off (or the existing abandoned-entry recovery
resumes it). Already queued physical cancellation tokens are retired before the
first paint. New DEV bytes cannot cancel the foreground check during Sleeping;
they wait for the existing sleep completion. The source-check deadline still
applies, and power-off is reached only after save success or an honest failure.
No progress or preference acknowledgement is moved ahead of durable readback.

The order fixture executes the actual exit, physical-input barrier and loop
paint/entry wiring with device and save boundary ports. Removal controls catch
missing early paint, missing failure repaint, premature power-off and missing
input quiescence. The separate real-reader cold-boot matrix continues to execute
actual progress write/sync/readback and startup authorization. The owner must
see one physical Sleep with serial captured; USB, battery sleep/wake and cold
power-on remain distinct hardware evidence, all pending at worker handoff.

The phase-2 measured record is flash 68 / protected 44, compiled from
`db138a23d8a0884a98b8adbdb7ba52a73afd9fe0`: flash 2,393,456 → 2,395,712 B,
SRAM 133,440 → 133,440 B, RTC 300 → 300 B. Additional held reading heap is
0 → 0 B; the existing DEV diagnostic owner remains 96 B. Seven selected frames
increase by 16–80 B; the new boot/Sleep row formatting frames are selected at
416/448 B. All older selected keys, records, floors and reserves remain.
Reviewer: GPT-6 Astra (xhigh), Lead-run cross-family review 2026-10-27: MERGE; no P0-P2; P3 stale sleep-order comment fixed before merge. Runtime heap/stack
margins and all physical power conditions still need device evidence.
