# Combined Stage and cover candidate

## Current installed correction: durable Stage and one-DU focus (2026-10-11)

The rejected `670` image kept its explicit theme only in retained RTC memory;
its cold/external reset policy cleared that choice. This could not satisfy
theme persistence across complete retained-memory loss. The owner's earlier
unplugged wake reset class is unverified. This correction commits explicit Stage
or Frosted to the supported SD A/B workflow before publishing the choice and
restores the durable choice before first paint for every reset class. The record
`/Newt/State/newt-home.nrs` is independent of reading progress and resume intent.
RTC is only a validated warm fallback when the medium is unavailable. Missing or
corrupt mounted records retain Frosted, the product default.

**Installed engineering image:** source `ea4570aff5ad09199f8545acb825ca8d5086b84d`,
build `ea4570aff5ad-regional-experiment`, 1,619,664 B,
SHA-256 `814b202760f3031356e82620a5a89966bb91a2351da5b1e0f8aacf2edc58481d`. Later evidence commits are not
compiled into this frozen image. On 2026-10-12 the owner adopted it as the working
experimental baseline after approximately 24 hours of actual reading. Repeated
unplugged sleep/wake preserved reading position and Stage Home; those behaviors
and the installed Home UI are **owner-accepted**, not untested. The 50,468 B
boot-lifetime internal-heap minimum has a scoped owner-approved experimental
exception; the original 64 KiB requirement did not pass. Cold power-on and formal
performance admission remain open. Mac is owner-deferred and **untested**.
See [the acceptance scope and follow-up plan](061-experimental-baseline-acceptance.md).

Two fresh matching 16 MiB recovery reads preceded one application-only write.
Full preboot readback verified the payload, 2,352 B erased
padding and every byte outside 396 app sectors
`[0x780000,0x90c000)` unchanged. Boot answered the exact-image HELLO. No bootloader,
partition table, OTA selector, whole erase, card format or blocked migration was
written. Recovery assets were copied outside this worktree before the write.

### Behavior and measured comparison

The quiet selection uses an 8 px black straight top edge on the selected button,
fixed labels/icons/rounded frames and **no white connector strip**. An explicit
controller opt-in coalesces the two changed binary islands into one DU activation;
identical white-to-white OLD/NEW padding leaves gray gaps undriven. The prior
image used two activations. Changed pixel count still affects transfer/rendering,
but the roughly half-second second activation was the dominant focus overhead.
Existing controller plans keep their prior behavior. Gray baseline invalidation,
checkpoint synchronization, due device-wide cleanup and complete gray/mono
fallbacks remain guarded. Full Stage transitions use the existing Overlay
lifecycle with Direct/monochrome fallback.

These are USB request-to-terminal medians on the same X3 and saved current page,
not optical completion or an approved baseline. Focus includes its identifiable
periodic cleanup; ordinary Stage driver refreshes are about 393–432 ms, with one
2,633 ms periodic cleanup in this run. Kicked actions are excluded from comparisons.

| Journey | Rejected 670 ms | Correction ms |
| --- | ---: | ---: |
| Stage focus, 12 moves | 1,012.5 | 467.9 |
| Stage warm reading open, 3 | 2,872.0 | 2,126.0 |
| Stage to Library, 3 | 1,750.7 | 1,015.8 |
| Library to Stage Home, 3 | 1,443.1 | 1,159.0 |
| Frosted focus, 12 moves | 581.9 | 581.8 |
| Frosted warm reading open, 3 | 2,125.3 | 2,125.6 |

On the correction, Reader to Stage Home is 1,353.0 ms
versus 1,179.5 ms for Frosted. Library to Stage Home is
1,159.0 versus 983.9 ms. The remaining
return-to-Home cost and optical quietness/prominence required owner judgment,
which was subsequently received for this installed image as recorded in
[Decision 061](061-experimental-baseline-acceptance.md).
All four old Stage reader exits used reply-tail kicks, so there is no clean old
exit median. Pixel/host/controller evidence alone does not prove optical behavior;
the owner subsequently accepted the installed Home UI and improved navigation.

A normal external reset restored actual Stage focus without any selection
command. It changed the boot identity; it does **not** independently establish
RTC loss, unplugged battery sleep/wake or cold power-on. Actual preprocessed
startup/selector host regressions discard all RTC state, test stale RTC, explicit
Frosted, storage corruption/unavailability and ambiguous/failed commits. Five
compiled restore/selector negative controls catch removed or disabled wiring.
`AGENTS.md` now requires durable preference authority and separate USB, battery
and cold-power evidence; an RTC-only preference or USB-only acceptance cannot
satisfy this contract. The later owner report supplies battery sleep/wake
acceptance; cold power-on remains separately unconfirmed.

### Reading, covers and resource evidence

First reading open after the confirmed normal restart:
2,190.2 ms (0 reply-tail kick(s)).
Twenty paired Stage turns: 981.2 ms median;
same-image Frosted current-page control: 974.0 ms.
The owner's current saved page differs from the historical `670` reading run,
so its 857.7 ms median is not a matched comparison. The chapter-crossing journey
completed its bounded 15-page rewind and verified inverse restoration; its
boundary timings include reply-tail kicks and are retained in the artifact.
Cold derived-cache import: 4,905.6 ms (1 reply-tail kick(s)); post-import
warm open: 2,110.9 ms (0 reply-tail kick(s)). The cache purge
affects derived reader data, not books, covers, progress or preferences.

Actual Produced records took 16865, 19404, 4044 ms; measured valid monitor-window
minimum cover-local heap was 66004 B. The delivery-triggered publication
input was consumed in 12.0 ms; USB delivery can lag firmware/panel phase. Eight
scheduled interruption inputs were consumed in 11.2–12.2 ms. Pending input
suppresses producer cancellation logging, so active cancellation is not
independently established. The historical 841 ms repaint phase remains unmatched.
Last complete census: 51 records, 37 art-shaped / 14 no-art-shaped, 0 temporary names, 0 unknown entries and 0 errors. Shapes do not validate every cache checksum.
Post-generation reopening verifies the original saved page. A subsequent normal
restart and quiet shelf revisit check persistence using actual cached observations
with no cover writes; this is not a full filesystem or every-record checksum audit.

Generic same-input host early-unsupported fixtures retain valid no-art records
and reduce wrapped reads from 27,268,986 to 2,628 B for a 13,633,179 B member-limit
source, and 25,177,020 to 7,530 B for a CRC-valid XML-policy source. Relevant metadata
CRC and a second current-path definitive Unsupported proof remain necessary;
changed, inconclusive, I/O or invalid-metadata paths retain full validation. These
are host byte measurements, not a matched timing of the unidentified historical
13.2 MB source. No source is privileged and valid existing caches were retained.

The complete approved-Python Linux gate passed in 1,524.808 s
on clean exact source. All four authoritative clean resource builds passed with
fresh hash-bound receipts and unchanged input closures. Source/controller,
durable-startup, resource adoption and the single-write safety handoff received
independent review. Final sanitized evidence review is CLEAN: all 30 run
projections, image bindings, comparison arithmetic, privacy and qualifications
were checked against private receipts and raw logs.

| Clean variant | Flash B | SRAM B | RTC B |
| --- | ---: | ---: | ---: |
| default | 1,473,184 | 133,248 | 300 |
| dev-usb | 1,611,040 | 134,080 | 304 |
| regional | 1,479,840 | 133,744 | 300 |
| regional-dev-usb | 1,619,664 | 134,576 | 304 |

All four ELFs contain exactly one 365,597 B atlas. DEV contains the one 104,544 B
backdrop and 4 B RTC mirror; production contains neither. Reviewed flash growth
is 0 / 2,256 / 784 / 4,192 B over `670`. SRAM and all inherited selected stack
ceilings remain unchanged; new preference and focus guards are added. The reviewed
main-file exception is exactly five startup lines (681 to 686), with the policy
history pinned. No automatic ceiling, exclusion or adapter relaxation occurred.

Observed cumulative boot minimum heap: **50,468 B, below required 65,536 B**.
Minimum observed loop-stack free headroom: 7,052 / 16,384 B. Runtime observation
coverage is explicit in the artifacts; physical sleep-path peak is unmeasured.
The cover-local reserve is not a universal reader guarantee. Preserving the
transactional reader/repaint workspaces is retained. On 2026-10-12 the owner
accepted this measured shortfall as a scoped experimental exception, preserving
reserves, fragmentation protections and historical measurements. The original
requirement remains failed; no formal passing performance baseline is inferred. Issue #22 and
`reading-latency-policy.json` still require evidence-based owner admission.

Completed exact-image plans: after-final-stage-selection, after-frosted-entries, after-frosted-focus, after-frosted-reading, after-generation-interruptions, after-generation-publication, after-generation-reopen, after-passive-stop-probe, after-passive-verified-restoration, after-reading-baseline, after-reading-cold, after-reading-crossing, after-stage-entries, after-stage-focus, after-stage-probe, cache-restart-boot-probe, cache-restart-revisit, durable-cold-boot-probe, durable-cold-focus, exact-boot-probe, final-owner-position.
Stopped exact-image plans: after-generation-passive. Raw private logs and receipts are retained;
ambiguous presses are never retried. Diagnostic tail loss remains intermittent.

### Current rollback and combined physical check

Recovery location: `external-private-recovery-archive`, verified outside the
active worktree. Its local path is provided privately to the owner.
Original `0184bbb0da20-regional-experiment` restore: `0184-app1-restore-396-sectors.bin`,
1,622,016 B, SHA-256 `722141c07abd2cf24b245e848c63ffad6e9fc87837cd64c1d1738a1518805d22`. Immediate `670` restore:
`current-670-rollback-full-write-span.bin`, SHA-256 `5242185c5ae24fff71b5366292c8639fb7879c9d68e8ed15b0ad1752fb425d71`.
Both are the complete **396-sector** write span; older 394/395-sector restore
files are insufficient. Readback SHA-256: `180a501e17b72bbc558a7beba3c13859ed2ad1ffc5bb594a37b59f6d2fb58577`.

Rollback follows the supported app-only procedure: close/save the reader, freshly
verify device identity/layout/active slot and restore size/hash, read current
flash, write only the entire verified span at `0x780000` without booting, and verify
the payload span plus every outside byte against that fresh read before normal
boot. No whole-device restore or card/NVS overwrite. Recovery copies/hash checks
and offline rejection controls passed; an actual rollback write is untested.
The owner explicitly waived a new whole-SD backup; existing SD contents were
preserved apart from normal supported state/cache writes.

The owner is continuing to use the installed image. Approximately 24 hours of
actual reading and repeated unplugged sleep/wake faithfully retained reading
position and Stage Home. Those behaviors and the installed Home UI are accepted;
no repeat is requested merely for paperwork. Cold power-on remains unconfirmed.
Remaining genuinely physical checks are consolidated in
[Decision 061](061-experimental-baseline-acceptance.md): cold power-on, targeted
verification of future fixes, formal reader/latency evidence, instrumented
sleep-path stack and supported powered-down card handling. Mac remains untested.
This acceptance pass is repository-only; no device or SD access is authorized.

The owner authorized merging #37–40 with merge commits after review
reconciliation, with documented exceptions and follow-up issues kept open.
Release publication is not part of that authorization.
Cleanup remains the earlier recoverable Git worktree removal: 24.73 GiB net,
with active/unique/dirty work, rollback, evidence and shared tools retained.
The completed [legacy audit](062-baseline-review-reconciliation.md) found no lost
unique implementation. After the replacements merge, close only #18→#37,
#19→#38 and #20→#39 with replacement/merge links; preserve all branches. #29 is
a separate Stage issue and remains open for its residual product scope.

## Historical rejected 670 evidence

The records below preserve earlier images. Their word “current” refers to the
recorded historical image; their smaller rollback spans must not be used for
the correction above.

### Structural cover and USB correction (2026-10-11)

Installed source `670f3790e5c59ec2e9be9def8d9fb64a2c98938b`, build `670f3790e5c5-regional-experiment`,
1,615,472 B, SHA-256 `7607f740a2992e88ca31f68c68e548c72cc144e56e38714c712e2876ba9adb83`. Exact readback
and a later HELLO/DIAG probe establish that this image booted. Automated journeys
are classified individually below; a stopped journey is never presented as a
completed sample. This remains an experimental installed candidate: the whole-boot
heap requirement and complete diagnostic reliability remain open. The device is left awake with the reader closed on selected
Stage Home. Owner acceptance remains pending. Later evidence commits do not
change this frozen firmware identity. Frosted remains the product and
cold/external-reset default.

The complete approved-Python Linux gate passed in 1,470.258 s,
log SHA-256 `837fa8a2c0a352591e4b7df2a3554967e73baf4772f0b73019d72ed357ddd200`. All four authoritative clean resource builds
passed on the same clean source, with fresh hash-bound receipts and no changed
build inputs. The Mac ImageIO/renderer gate is explicitly deferred and
**untested**; this is not full cross-platform acceptance.

| Clean variant | Flash B | SRAM B | RTC B | Image SHA-256 |
| --- | ---: | ---: | ---: | --- |
| default | 1,473,184 | 133,248 | 300 | `41e99e6793c6b661d94467b128019eeae14facba9eae7155082ee8949bd2e531` |
| dev-usb | 1,608,784 | 134,080 | 304 | `554c81d910873fbf82ee80b7d593bd423c048632a124f8ce65bd630b74c7cfca` |
| regional | 1,479,056 | 133,744 | 300 | `0baf61592e053d771f2897e59d90f38cf2c29691690943da39fa07f6cabfc37a` |
| regional-dev-usb | 1,615,472 | 134,576 | 304 | `7607f740a2992e88ca31f68c68e548c72cc144e56e38714c712e2876ba9adb83` |

Every ELF has one 365,597-byte atlas. DEV links one 104,544-byte backdrop and
one four-byte retained opt-in token. The generic early-rejection correction
costs 16 flash bytes per variant over its predecessor. The later USB correction's
exact four-variant cost is recorded in the protected resource audit. RAM remains
unchanged; selected stack budgets are preserved or tightened and the transport
boundary gains explicit guards. Exact post-install comparison confirmed payload,
padding and unchanged bytes outside app sectors. Two scoped reviewers approved the hash-bound final evidence snapshot; owner and remaining engineering requirements are still open.

### Exact-image engineering results

Completed exact-build runs: final-awake-stage-home, final-reading-baseline, final-reading-cold, final-reading-restart-persistence, final-stage-entries, final-stage-focus, final-stage-generation-48, final-stage-generation-52, final-stage-interruptions, final-stage-publication, final-stage-reopen.

Stopped exact-build runs: final-cover-cache-persistence, final-reading-crossing. Stopped runs and verified recovery
evidence remain in the artifact; their partial timings are excluded below.

All times below are USB request-to-terminal milliseconds, not optical timings.
The e30 controls use the same saved reading position but are not an approved
performance baseline. First-restart preparations differ, so their difference
is observed, not attributed to the correction. Raw stopped runs are retained.

| Journey | Rejected e30 control ms | Final combined ms |
| --- | ---: | ---: |
| First open after initial normal restart | 3,597.3 | 2,448.4 |
| Three warm opens | 2,136.6 / 2,265.7 / 2,138.7 | 2,117.3 / 2,116.6 / 2,121.2 |
| Twenty paired page turns, median | 862.2 | 857.7 |
| Cold import | 4,853.6 | 4,746.6 |
| Post-import warm open | 2,120.6 | 2,116.6 |
| Previous chapter crossing | 2,097.3 | stopped; no completed sample |
| Next chapter crossing | 1,864.8 | stopped; no completed sample |

Individual completed boundary actions remain measurable despite the stopped
journey: previous crossed in **2,091.481 ms** (control 2,097.3 ms), and the verified
inverse next crossed in **1,862.7 ms** (control 1,864.8 ms). The latter used a fresh
HELLO before each restoration action, outside the measured PRESS interval. These
are individual action measurements, not a full crossing-run pass.

Stage warm opens were **2,856.2–2,863.4 ms**, approximately 0.73 s above the same
image's Frosted control because entering/leaving the gray Home requires cleanup.
The input requested after USB delivery of actual Produced was consumed in
**20.1 ms**, versus the earlier e30 delivery-triggered sample of 52.4 ms. Each is
one unpaired sample; USB delivery can lag firmware/panel phase. The historical
841 ms phase is not matched, so it is not claimed solved by this measurement.
Cover production took 15,654 ms in the publication journey and 17,033/18,468 ms
in the quiet generation journey. Reopening preserved the saved reading position.
Normal restart first open was 2,465.7 ms; three warm opens were 2,129.0–2,134.2 ms.
The stopped restart-cache journey still observed cache reuse with zero writes,
but it does not validate every cache or constitute a completed navigation pass.

Exact event-level samples, refresh classifications, reply-tail kicks, input
consumption, cover byte counts, cache census, stack and heap evidence are in
`artifacts/combined-x3-engineering-measurements.json`. Private receipts verify
the saved owner position and the inverse restoration after the stopped crossing.
Only completed journeys support behavior claims here. Final Stage focus completed with 11 binary island updates, 1 periodic full cleanup and 1 full style-selection update; this classification does not prove optically quiet focus. Ordinary
focus is designed to keep the backdrop, rounded frames and labels fixed and to
change only the selected straight black top edge. Device-wide periodic cleanup
remains allowed, plus the documented proof/gray fallbacks.

Observed whole-boot minimum heap: **50468 B**; this measured evidence **does not meet the requested 65,536 B floor**. Cover generation's separately enforced reserve is not a
universal reader guarantee. Minimum recorded cover-local free heap:
66004 B. Minimum
loop-stack free space: 7052 B of 16,384 B. Physical sleep-path stack peak
remains unmeasured. Issue #22 and `reading-latency-policy.json` remain without an
approved baseline or owner review; no thresholds or performance admission are
invented.

Earlier interim images lost DIAG replies after completed Home transitions. The
exact candidate completed 11 no-kick journeys, but its first chapter-crossing
attempt still lost a DIAG reply after a completed PRESS/STATUS. The saved position
was verified on the same boot and restored through the known inverse path. The
restart cache journey also lost a STATUS tail after a completed navigation action;
a same-boot probe and six verified inverse moves restored Library origin and Home.
The correction did **not** eliminate all tail loss or establish USB reliability.

Active producer cancellation on this image is not independently established:
the scheduled presses completed, but pending input suppresses the producer's
own cancellation observation. Missing Cancelled records therefore prove neither
absence nor successful cancellation. Host tests cover cancellation semantics.
The clean completed census reached 48 records
(34 art-shaped, 14 no-art-shaped),
with 0 temporary names, 0 unknown entries,
0 errors and 0 truncation flag. These counts
are structural census results, not a full checksum audit of every record.

### Generic early unsupported-cover proof

Large sources (at least 8 MiB) may now short-circuit fixed ZIP member/XML policy
rejections only after validation of the relevant metadata and a second current
pathname proof of definitive Unsupported. Invalid metadata CRC, inconclusive
lookup, I/O errors and changed source keep ordinary full validation. JPEG/gray
production, cancellation and stable CJB2 valid no-art caching remain intact;
no source identity receives special treatment. Existing valid no-art records
were retained across the update rather than deleted to force a benchmark.

Same-input authored host fixtures: the 13,633,179 B member-limit archive fell
from **27,268,986 to 2,628 wrapped bytes**; the 12,584,745 B CRC-valid XML-policy
archive fell from **25,177,020 to 7,530 wrapped bytes**. Both retained valid no-art
records, with fault, cancellation and same-size/mtime replacement coverage.
Those are host measurements, not X3 elapsed times for the unidentified historical
source. Completed hardware publication/interruption samples are in the artifact;
stopped samples remain explicitly labeled. The historical repaint phase has no
matched phase trace here, so input improvements do not constitute optical acceptance.

### Current recovery and owner checks

Recovery location: `external-private-recovery-archive`.
The original rollback file is `0184-app1-restore-395-sectors.bin`, 1,617,920 B,
SHA-256 `b745050ee72ca72fd5c62ad099b80e7323b283d523fbd808db794f82524c528c`; the immediate predecessor is
`current-baa-rollback-full-write-span.bin`, SHA-256
`877affbfa283d7dae8914d4ca4e1ac56fce0d36662542af4f9d4fcce7ddbe658`. Current full-flash readback SHA-256 is
`0e04a276a3df3d4616206427cc5d8b41f57f7272cbea87238e8025d739e67b7b`. The current image touches exactly 395 sectors
`[0x780000,0x90b000)`; **the historical 394-sector restore below is insufficient**.

Rollback uses the supported application-only procedure: close the reader/save
progress, freshly verify device identity, selected app/layout and rollback
size/hash, read current flash, write only the full 395-sector restore span at
`0x780000` without reset, then verify the span and every outside byte against
the fresh read before normal boot. External recovery copies and offline rejection
controls were hash-verified by the private publisher; a rollback write is untested.
The new whole-SD backup was explicitly waived.

Pending owner checklist for this **one combined session**:

- Left/Right on Stage: judge top-edge clarity and no ordinary gray-area flash;
  a periodic whole-device refresh is expected.
- Open/resume, turn pages/cross a chapter, return Home/Library and navigate while
  another shelf generates; confirm covers, responsiveness and saved position.
- Sleep from Stage Home, then wake with Power: inspect the selected Stage sleep
  backdrop and restored Stage theme. Physical deep-sleep RTC behavior is pending.
- Follow supported powered-down card removal/reinsertion checks; no formatting
  or library migration.

Final evidence review is recorded, but owner visual/sleep/card acceptance, merge and release are not claimed. The historical cleanup inventory below remains unchanged.
Superseded PR closure stays conditional
on acceptance and merging #37–40; #29 is an issue, so its requested PR closure
needs clarification at that point.

## Historical interim f73 evidence

An interim corrected combined candidate is installed on the experimental X3. Its
source is `f73a8f95d0a6336cb513098050ce5bb89ba6f3b2`, build ID
`f73a8f95d0a6-regional-experiment`, image size 1,615,392 B, SHA-256
`00386f9aa08309ce6c45560c364b5816ac9fe3ac1af6e137fb21fc33b8b874bc`.
It passed the complete Linux gate, all four clean resource gates and fresh
independent review before one app-only write. Whole-flash readback verified its
exact payload, erased padding and equality of every byte outside the 395-sector
write span `[0x780000,0x90b000)`. It booted and answered the exact-build handshake.
Owner optical and sleep/wake acceptance remains pending. Later evidence commits
must not be confused with the frozen source revision above.

## Historical pending structural cover correction

The exact f73 passive run completed two further covers, but also exposed an
Unsupported result at 53,179 ms with 23,893 reads / 24,460,196 wrapped bytes.
Its no-art record was durable, and the final census found 44 records (31 art,
13 no-art), zero temporary/other files and zero errors. This image is not the
final handoff: a generic fixed ZIP/XML admission rejection still fell through
to whole-source CRC validation. The pending correction recognizes that
structural verdict early and requires the reopened current pathname to prove
definitive Unsupported again before publication. Invalid metadata CRC, missing
data, I/O and unproved verdicts keep ordinary validation. Linux/resource gates,
independent review and exact-image device measurements must be repeated for
the final frozen correction. Physical acceptance remains pending.

## Corrected owner defects (2026-10-11)

The first image `e30e2638bf55-regional-experiment` failed owner acceptance:
ordinary focus visibly scrubbed the gray dock, and sleep/wake showed Frosted.
The correction keeps backdrop, labels and rounded frames unchanged. Selection
adds two pixels to the straight black top edge; the owner authorized trying
this variant. Three immutable binary regions use the existing 480-byte
checkpoint storage (324 B used); compile-time binding excludes mutable and
short-lived plans. Only the old/new regions take stock binary DU. The existing
periodic device-wide cleanup and guarded full Direct/monochrome fallbacks remain.
A moving halo or bold label is no longer part of firmware focus.

Home sleep now composes the selected Stage layout before the existing blur and
Sleep stamp. A separate exact versioned four-byte DEV RTC token preserves
opt-in for deep-sleep and software-reset causes. Independent final-ELF review
verified alignment, placement, token/zero stores, reset eligibility and invalid
clearing in both DEV variants. Cold power-on and external/USB reset still default
to Frosted; durable selection across power loss is not implemented. The old
68-byte sleep and 140-byte SD contracts are unchanged. Physical RTC survival is
not established by ELF or host proof.

The complete Linux gate on the frozen source passed in 1,487.237 s. Its log
SHA-256 is `096a60668467430679a4121f76a1c283e854de77d4d016e35bbb6baafe64d0e9`.
The owner-deferred exact-source Mac ImageIO/renderer check remains **untested**;
full cross-platform acceptance is not claimed. Two earlier failed Linux runs
(system-Python provenance and a stale wake-test include) are retained separately;
the final pass reran every required suite using the approved project Python.

| Clean variant | Flash B | SRAM B | RTC B | Image SHA-256 |
| --- | ---: | ---: | ---: | --- |
| Default | 1,473,168 | 133,248 | 300 | `843488b41aeff886e9865b090fd04e502bf3a53b528eac9eaeb56b32b8aa2855` |
| DEV USB | 1,608,720 | 134,080 | 304 | `4a1d8d9504ee6cd387bdae8798c3bba11cac9d2b78bc814dbed6ddb4f0726bb5` |
| Regional | 1,479,040 | 133,744 | 300 | `d8741ea0e79388006198b6fe7bd93059da956de5d4a27bd77f2c41e5c5e8d6f3` |
| Regional DEV USB, installed | 1,615,392 | 134,576 | 304 | `00386f9aa08309ce6c45560c364b5816ac9fe3ac1af6e137fb21fc33b8b874bc` |

All four final ELFs contain exactly one 365,597-byte shared atlas. DEV alone
links one 104,544-byte backdrop and the four-byte token. Inherited selected
stack guards remain present and no ceiling was raised. The reviewed exact
flash costs and RTC exception are recorded in CODE_HEALTH.md and protected
budget history; regional SRAM ceilings tightened by 16 B.

## Interim f73 corrected-image measurements

Fresh before/after journeys use the same saved reading position; the controls
are the rejected e30 image, not an invented approved performance baseline.
These are USB request-to-terminal samples, not optical endpoint measurements.

| Journey | Before e30 ms | After f73 ms |
| --- | ---: | ---: |
| First after normal restart | 3,597.3 | 2,451.1 |
| Three warm opens | 2,136.6 / 2,265.7 / 2,138.7 | 2,114.1 / 2,121.6 / 2,116.6 |
| Twenty paired page turns, median | 862.2 | 857.4 |
| Cold import | 4,853.6 | 4,874.2 |
| Post-import warm open | 2,120.6 | 2,114.8 |
| Previous chapter crossing | 2,097.3 | 2,095.4 |
| Next chapter crossing | 1,864.8 | 1,854.4 |

Cold import and chapter crossings each needed a USB reply-tail kick on both
images; they are transport-affected samples. The restart difference is observed,
not attributed causally to code: reader-cache preparation differs across these
journeys. Every completed reader journey returned to the exact saved place.
The latency policy and issue #22 still have no owner-approved baseline/review.

On f73, 11 of 12 Stage focus moves used binary-region updates and one used the
periodic full cleanup. Binary driver refreshes were 934–975 ms, compared with
about 1,367 ms on the rejected e30 gray path; full cleanup was 1,437 ms. No
unexpected first-focus full pass occurred. Driver classification and timing do
not prove visually quiet focus. Minimum observed loop-stack free space is
7,052 B of 16,384 B. Settled reader heap is 65,640 B, largest block 53,236 B.
Whole-boot heap minimum is 50,468 B: it does **not** meet a universal 64 KiB
floor. The separate 64 KiB cover-generation reserve was measured at 66,004 B on
f73; physical sleep-path peak stack remains unmeasured.

A Stage navigation journey stopped on a missing DIAG after completed
RENDER_RETURN and STATUS on Home. A normal ROM/reset operation recovered USB;
a Frosted-only journey later reproduced the same boundary. Both failed
receipts/raw logs remain failures. A 120-second passive trace received exactly
256 bytes of the missing DIAG without a newline/CRC/tail—the pinned HWCDC TX-ring
capacity. Fresh read-only HELLO/DIAG then succeeded on the same boot without
reset. Independent investigation found a transport partial, not a Stage-specific
transition fault. A delayed SD operation alone cannot explain the missing tail.
The underlying SDK transport is not fixed by this feature pass. Spacing
read-only diagnostics by 50 ms, outside the timed PRESS interval, allowed the
complete Stage Reader/Home and Library/Home journey to pass. The original
stopped journeys are not relabeled. Source and flashed image did not change.

Three Stage warm opens were 2,856.5 / 2,861.0 / 2,857.2 ms. Leaving the gray Home
still requires the existing cleanup; these are distinct from Frosted's warm
open measurements and from the optional 44–47 ms styled-reader overhead.
Eight timed input controls on a production-eligible fresh shelf were consumed
in 3.6–32.7 ms, with no reply-tail kicks. Cancellation observations may be
suppressed when USB input is pending; these timings alone do not prove an active
job at every sampled press. A real Produced record on f73 reported 13,870 ms, 5,891 reads / 4,432,344
wrapped bytes, 174 writes / 174,096 B and 11 opens. The input sent immediately
after delivery of that record was consumed in 20.0 ms and returned in 982.9 ms
(939 ms driver refresh), with no kick. The IDF local monitor start/stop both
succeeded; the generation-local free-heap minimum was 66,004 B, 468 B above the
65,536 B cover reserve. Sampled largest block was at least 90,100 B and the
whole-boot stack watermark remained 7,052 B free. This is a delivered-publication
edge probe comparable to the earlier e30 edge probe (52.4 ms), not proof of the
exact historical 841 ms firmware/panel phase. The 180-second passive run completed two further covers at 20,585 / 19,361 ms
and the structural rejection above, with the same 66,004 B generation heap
minimum and no stray temporaries. Reopening after generation restored the saved
reading position. Cache persistence still requires the final-image restart
journey.

## Corrected-image recovery

Two fresh matching 16 MiB prewrite reads and an externally copied recovery set
were verified before the write. The prewrite image remained exactly e30 and the
partition table/OTA selection matched the prior verified layout. Preboot
readback SHA-256 is
`708600754176e8397b3ff4df99702bb82ff37deafb4acd1bfeaee316d3c6ef89`.
Recovery assets are outside the T3 worktree at
`~/newt/.build/private/x3/combined-20261010/correction-20261011/`.

Use the supported app-only workflow after fresh device/layout identification:
restore `0184-app1-restore-395-sectors.bin` at `0x780000` to return to original
0184 (SHA-256 `b745050ee72ca72fd5c62ad099b80e7323b283d523fbd808db794f82524c528c`),
or `current-e30-rollback-full-write-span.bin` to return to the rejected e30
(SHA-256 `c3ab26ec210889831e27e7bd56a68504f5c5c6c50d782c5a991b0b498103189f`).
Both restore all 395 touched sectors. Verify full readback and unchanged bytes
outside that span before normal boot. Do not restore old whole-flash data/NVS,
erase the device or use the earlier 394-sector rollback on f73. No rollback
write was exercised. SD backup was explicitly waived; app-only writes preserve
books, hidden settings/progress and card contents. No library migration occurred.

## Historical first-image evidence

The remaining measurements below describe the rejected e30 image or its older
0184 control. They are retained for provenance and must not be promoted to
corrected-image evidence. Older recovery commands below cover only e30's 394
sectors; the corrected-image recovery section above supersedes them for f73.

## Starting state (2026-10-10, Linux host)

The open replacement stack was verified through GitHub and local refs:

| PR | Base | Head |
| --- | --- | --- |
| #37 | main | 1772e3bf7d24af64e9b69e283d699288924c0302 |
| #38 | port/18-reader-styles | 0e3ff5fb503a0ed9ec616d5d9c9fccca0119a503 |
| #39 | port/19-home-cover-restack | 441836355a4e8a594b027bf8b64fdc3c4fb6175f |
| #40 | port/20-cover-producer | f7d64aaed0d2707414d55bbde3f7b742599a7882 |

The clean T3 integration worktree was advanced from f008308 to the existing
stack head by fast-forward. No historical feature branch was reset or replaced.
No separate Opus report was supplied. Decisions 057–059 and STAGE_HOME.md plus
the current PR descriptions are the implementation records.

Read-only device identification found an ESP32-C3 revision 0.4 with 16 MiB
flash, working ROM access, secure boot disabled and flash encryption disabled.
The live bridge reported `0184bbb0da20-regional-experiment`, on Home with no
reader workspace. Private identity, recovery and raw timing evidence stays
under ignored `.build/private/combined-x3`; no raw flash/card data belongs here.
Two fresh 16 MiB flash reads matched SHA-256
`db1533a79f7a31a4cb2df15fcfb0566694156251b8f7c0e13d03d2c2d082329e`.
The active app1 is at `0x780000`, capacity `0x770000`, selected by valid OTA
sequence 6 (state 2). A third full-flash copy and the complete app1 rollback
span are preserved outside the T3 build tree. The active image is 1,335,824 B,
SHA-256 `0581864f01ac015ec5c11be300d13a1b11b3f168fff187a9c4f3910b3b52b0e7`.
The owner explicitly waived a new SD backup: the device is experimental and
its books have multiple independent backups. App-only writes must still
preserve the bootloader, partition table, NVS and card contents.

## Required evidence

- Existing host gate on the Linux host uses `python3 tools/check.py --profile linux`.
  The exact-source Mac ImageIO check,
  `python3 -B tools/check-mac-covers.py --expected-commit FULL_SHA`, is explicitly
  deferred by the owner for this experimental X3 candidate. It remains untested;
  neither cross-platform software acceptance nor Mac renderer parity is claimed.
  A concrete dependency affecting image correctness or safety must still be
  investigated if found.
- Selected native safety diagnostics and all four clean
  `tools/firmware-build.py --clean --health-report` variants are required.
- RAM ceilings and every inherited selected stack entry remain protected.
  New flash costs require measured four-variant attribution and independent
  review, with no automatic baseline update.
- Exact combined-image bridge measurements cover opens, turns, chapter
  crossings, navigation, gray focus, a real publication edge and generation
  heap/stack. Detailed receipts and limitations appear below. Sleep/wake,
  card removal/reinsertion and optical judgment require owner participation.
- `docs/reading-latency-policy.json` has no approved baseline or owner review.
  Issue #22 remains open; measured samples cannot invent that approval.
- Frosted remains the product/boot default. Stage's physical appearance and
  gray-focus strategy require the owner's judgment before a default change.

## Worktree preservation

The initial inventory found uncommitted source/test changes in `newt-fw`.
They are preserved in place. The cover worker verified that their useful
path-polling, temporary ownership and cache-probe changes already exist in the
stack head. `newt-home-visual-refresh` and `newt-scratch` have commits outside
the stack and main and must be retained or archived before removal.
`newt-x3-port` owns the managed Python runtime reused by existing toolchains;
it cannot be treated as obsolete build output while that runtime is in use.
Cleanup follows a safely installed candidate, recoverable archiving and a fresh
process/dirty-state audit.

## Superseded PR disposition (conditional)

After owner acceptance and merging #37–40, inspect unique changes and remaining
requirements before closing #18 → #37, #19 → #38 and #20 → #39, with a brief
replacement-link comment and no branch deletion. Their current descriptions
still require physical acceptance; that requirement carries into this candidate.
No inline review comments or submitted reviews were found on #18–20 initially.
GitHub number #29 is an open issue, not a PR. The owner's requested #29 → #40
closure is flagged for clarification; do not close a different object silently.

## Initial baseline measurement

The exact installed baseline image above was measured with the historical private
bridge driver, preserving the saved reader position. One restart open completed
in 2,479.6 ms; two warm opens completed in 2,141.1–2,143.4 ms; twenty paired
forward/back turns completed in 654.7–1,027.6 ms (median 719.15 ms). Two validated
reader exits completed in 1,287.7–1,291.6 ms. Reader heap was 65,624 B, largest
block 53,236 B. These are host USB request-to-terminal observations. No optical
endpoint was measured, and driver refresh classification was unavailable.

The run stopped on a damaged DIAG protocol record after a later reader exit's
terminal and STATUS replies. The full failed/partial receipt and raw log were
preserved. No press was retried. Completed individually checked actions above
are provisional measurements; the overall journey did not pass. The latency
policy and its unapproved baseline/review fields remain unchanged.

Two validated chapter crossings completed in 1,796.4 and 1,867.6 ms, returning
to the saved chapter/character pair. Both needed the historical driver's USB
reply-tail kick, so they are transport-affected samples, not clean panel latency.
The corresponding warm open was 2,145.6 ms.

A later baseline cold-import run completed after the supported DEV `purge-cache`
action. That action removes only derived reader receipts/pages and closes their
handles; books, reading state, covers and settings are retained. The cold open
completed in 4,689.0 ms with one USB reply-tail kick and returned to the saved
position. Its device refresh receipt was 835 ms. The following warm open was
2,065.7 ms without a kick. Reader heap remained 65,624 B. The changed timing
between sessions is retained as observed variation, not attributed to Newt code.

## Combined implementation

Large archives (at least 8 MiB) can now receive an early negative verdict for
non-JPEG artwork after the complete chosen member passes its ZIP CRC. Container
and OPF staging retain their integrity checks. Publication closes the original
descriptor, binds the current pathname again, and independently repeats the
content decision. Supported JPEGs and uncertain cases keep the existing
whole-source path. The route identity includes the new member helper, and
temporary ownership, cancellation and valid no-art persistence remain covered.
An authored 9 MiB heterogeneous fixture required 264 book reads / 12,350 bytes.
The historical 28,704 ms / 13.2 MB unsupported-art source was not identified or
invalidated on the card for a matched rerun. Existing valid no-art caches are
preserved. This host result proves the generic early-rejection read boundary,
not a before/after timing for that unidentified physical source.

DEV cover diagnostics describe only their actual observation boundaries:
`reads`/`read_bytes` include all reads through the file wrapper, `path_stats`
counts pathname checks (not descriptor `fstat` checks), and largest-block values
are sampled rather than a runtime watermark. A successfully started/stopped IDF
local heap monitor supplies `min_free_local`. The bounded directory census emits
`art_shape`, `noart_shape` and `tmp_names`; sizes and suffixes do not establish
NLC2 validity. Cache persistence must be checked through the producer's actual
cache validator and repeated cache hits. The host regression deliberately counts
invalid same-size files while verifying that the cache validator rejects them.

Cover publication retains a deferred repaint while input is queued. A fresh
review found that clearing it at input dequeue loses art for no-op or rejected
input. The structural fix clears it only after a completed composition including
the cover, or after painting a different screen. Status-only Home paints retain
it. Queue arrival after publication and during the final quiet wait is tested.

Stage shares the existing atlas arrays instead of linking another 365,597 B.
Its 104,544-byte backdrop is a directly linked typed object with generator and
assembler size checks. The DEV selector travels through the existing session
and input queue gates; Frosted remains the product and boot default. Gray focus
uses the union of old/new dock halos only when the SDK accepts the regional
Direct path; otherwise a complete Direct pass, then monochrome fallback, is
available. Physical quality remains pending; exact combined-image timings appear below.

## Native resource audit

Four clean builds of `80817575968ebef07d5675cf33da75992014bd02` compiled and
passed linked NVS/sleep preservation. They passed every inherited RAM and
selected stack ceiling; their only resource failures were the previous exact
flash ceilings. The manually authored budget update was independently reviewed and committed
before the final clean builds. Reports, image/map/ELF/partition hashes and selected
frames are bound in the baseline's flash/protected histories; private archived
outputs are retained for independent inspection.

| Variant | Previous flash | Measured flash | Increase | Shared SRAM |
| --- | ---: | ---: | ---: | ---: |
| Default | 1,470,928 | 1,473,136 | 2,208 | 133,248 |
| DEV USB | 1,490,768 | 1,607,968 | 117,200 | 134,080 |
| Regional | 1,475,152 | 1,477,360 | 2,208 | 133,760 |
| Regional DEV USB | 1,494,992 | 1,612,960 | 117,968 | 134,592 |

Production leaves Stage's renderer and backdrop unlinked. Its 2,208-byte growth
covers the generic early-artwork verdict, input/publication corrections and
shared-atlas boundaries. DEV growth beyond production is 114,992 / 115,760 B:
the backdrop accounts for 104,544 B, and the remaining 10,448 / 11,216 B includes
the Stage renderer, selector, gray-focus path and bounded diagnostics. These
residuals are combined attribution, not isolated per-component measurements.
The regional DEV ELF has exactly one each of `kHomeRuns` (150,383 B),
`kHomeGlyphs` (141,120 B) and `kHomeKerns` (74,094 B), plus one backdrop object
(104,544 B), all at flash addresses. The shared atlas totals 365,597 B.
The largest image leaves 5,403,488 B below the generated usable app capacity
7,016,448 B; the 7,798,784-byte partition and its reserve remain unchanged.

DEV SRAM is unchanged; production SRAM is 16 B lower. Reducing the clock cache
from 64 to 48 bytes funds IDF's 16-byte local heap-monitor state in DEV without
raising a ceiling. All inherited selected stack entries remain. Exact new guards
cover 29 / 44 / 29 / 46 Stage, shared-atlas, cover-member/verdict, paint-transaction
and DEV observation functions. The new non-Home transaction is 80 / 96 / 80 / 96 B;
DEV Home is 96 B. The dispatcher is tightened to 64 / 48 / 64 / 48 B, and native
epilogues restore its stack before transaction tail calls. The inherited stage
and locate wrappers remain selected at 48 and 0 B, with their substantive bodies
separately guarded at 1,200 and 32 B. No exclusion or adapter is added. The loop
budgets remain 80 / 96 / 80 / 96 B, and its runtime stack remains 16 KiB. Static
frames do not prove call-chain peaks; the exact-image device observations below
provide separate runtime evidence.


## Exact-source verification and installation

On clean `e30e2638bf55a91c9b03f55649563e28873b9b76`, the approved project-local
Python ran these authoritative commands:

```text
python -B tools/check.py --profile linux
python -B tools/check-safety.py --clang-tidy .toolchain/llvm/bin/clang-tidy
python -B tools/firmware-build.py --clean --health-report
python -B tools/firmware-build.py --clean --health-report --dev-usb
python -B tools/firmware-build.py --clean --health-report --regional-home
python -B tools/firmware-build.py --clean --health-report --regional-home --dev-usb
```

The complete Linux gate passed in 1,498.854 s, including sanitizers, renderer,
cover integrity/failure/cancellation controls, host fixtures and the read-only
code-health ratchet. Log SHA-256:
`d2a0abd1ab7cea14c4710cee1f76987e312341cc139165266dff053679f7fc61`.
Selected native safety passed (14 of 39 default translation units; 17 bad
fixtures rejected). That command does not analyze DEV/regional conditional
branches or establish runtime compliance. The actual Mac ImageIO/renderer gate
was not run; owner deferral is explicit, and no cross-platform acceptance is
claimed. No concrete Mac dependency affecting this native image's correctness
or safety was found.

Earlier Linux attempts exposed a stale paint fixture and long absolute fixture
paths/ambient umask. The fixes share the implementation's actual paint paths and
run ROOT-contained fixture paths with a child-only private umask; rejected
outside-root paths and existing path limits remain covered. Those failed runs
are preserved and are not gate passes. A first final resource attempt correctly
failed when the pinned Arduino component helper rewrote `idf_component.yml`.
The rewrite preserved its parsed dependency data; no dependency, SDK or gate was
changed. A clean rerun, with identical input hashes before/after each build,
passed all four variants. This input-drift failure is retained separately.

All final resource receipts are fresh, source-clean and bound to this revision.
The four images have these hashes:

| Variant | Image SHA-256 |
| --- | --- |
| Default | `a9e8d7f05326c75fbbf7398fee9ae31a153236198d0dece089653e9f9dddd8cf` |
| DEV USB | `4bf62443b06b843dfecb258b014902cfc850c97d1913805a4a0c72282095f822` |
| Regional | `803f3a22d7d9792cf69dcc317b8e8fb36e01ea560eee76675d5df4b2f36bf96a` |
| Regional DEV USB (installed) | `c6e5475d5230c9ec37959f48c2d86e4249ec5d533bfdc8464a4f5ef75f67b476` |

Fresh bounded adversarial/maintainability reviews ran through seven rounds;
actionable findings were fixed, with the final source/host delta clean. The
publication latch and diagnostic boundaries were corrected as shared invariants.
The final reviewer independently checked all 164 archived artifact hashes per
variant, selected stack presence/ceilings, one shared atlas, DEV-only backdrop,
prewrite recovery and whole-flash readback. No actionable P1/P2/P3 finding
remained. This is engineering review, not owner approval.

Immediately before the write, a fresh whole-flash read matched both recovery
reads and the external copy. ROM identity, selected app1, image digest/checksum,
partition/header compatibility and sector extent were verified. One supported
application write at `0x780000` erased/wrote exactly 394 sectors, ending at
`0x90a000`. No retry, whole-device erase, bootloader/table/NVS change, card
format or library migration occurred. A 16 MiB readback before boot verified the
exact image, 864 B of erased padding, and byte-for-byte equality everywhere
outside that sector extent. Readback SHA-256:
`de1da88d4b05c218ec7978be1f52d02c93da23872bb7c1a40f586515e1ba0952`.
A normal reset booted the exact expected build, verified by two consistent HELLO
responses and live diagnostics. Stage was selected through the existing DEV
input queue.

## Engineering measurements on the installed image

`artifacts/combined-x3-engineering-measurements.json` contains sanitized metrics
and hashes of private receipts/raw logs. Raw serial, flash, device identity,
book IDs and saved anchors stay private. The raw decoder reconstructs split USB
chunks before accepting full telemetry records; a fragmented line is not a
valid measurement. Request-to-terminal times below are host USB observations,
not optical latency. Samples with a reply-tail kick are marked. The old
baseline journey stopped on a damaged DIAG; individually validated actions are
retained without declaring that whole journey a pass.

| Operation | Installed 0184 baseline | Combined image | Qualification |
| --- | ---: | ---: | --- |
| Actual restart open, Frosted | 2,479.6 ms | 2,457.3 ms | Same saved position; no kick |
| Cold derived-cache import | 4,689.0 ms | 4,767.8 ms, Frosted | Each has one reply-tail kick; same saved position, refresh 835/836 ms |
| Warm opens | 2,141.1–2,143.4 ms | 2,111.2–2,113.8 ms before restart; 2,124.6–2,227.7 ms after restart, Frosted | Three samples each, no kick; session variation retained, baseline post-import control also varied to 2,065.7 ms |
| Warm opens with Stage | — | 2,851.4–2,854.2 ms | No kick; gray Home exit forces destination display cleanup |
| Twenty paired page turns | 654.7–1,027.6 ms; median 719.15 | 647.6–1,022.7 ms; median 714.65 | No kick, same two anchors, net-zero movement |
| Chapter previous/next | 1,796.4 / 1,867.6 ms | 1,790.7 / 1,868.7 ms | Each crossing has one reply-tail kick; saved anchor restored |
| Home → Library | — | Frosted 1,010.5–1,016.3 ms; Stage 1,756.8 ms | No kick; same 62-book library |
| Library → Home | — | Frosted 984.7–989.0 ms; Stage 1,453.5 ms | No kick |
| Stage focus driver refresh | — | Regional 1,367 ms; full 1,409 ms | 10 regional / 2 full Direct of 12 moves, including cadence cleanup; first full-pass cause not isolated |
| Frosted focus driver refresh | — | Binary island 506–546 ms | Also one 2,458 ms cleanup and one 1,035 ms rebase in 12 moves |

The first open after combined-image boot, following Stage selection, completed
in 2,879.0 ms. A private driver label in the Frosted style-switch run says
`read-after-restart`; that 2,138.6 ms action was warm, with no intervening
restart, and is not restart evidence. The actual later restart has a separate
receipt. Stage's gray-to-reader cleanup adds about 0.74 s compared with the
same image's Frosted warm control. Gray regional focus saves only 42 ms of
recorded driver refresh. Frosted stays selectable and remains the production
and boot default; no fast but unsafe cleanup bypass is introduced.

The historical cover trial reported 15 of 16 consumed inputs at 3.1–46 ms and
one 841.1 ms delay during an unsolicited 693 ms repaint. The exact image's 16
scheduled shelf inputs at 2.4–5.2 s quiet offsets completed at 4.7–47.7 ms,
without kicks. No complete cover records were delivered in that scheduled run,
so it cannot prove active generation or cancellation at those instants. Pending
input can intentionally suppress diagnostic logging.

A separate passive probe captured an actual `Produced`: 6,093 ms, 2,348 reads /
1,954,659 wrapped bytes, 77 writes / 76,012 B, and 11 opens. The host issued one
press about 0.11 ms after USB delivery of that Produced record; it was consumed
in 52.4 ms, with no kick, and returned in 993.7 ms (917 ms driver refresh).
An idle repaint lies between the preceding and following render generations.
This supports responsive handling at a delivered publication edge; USB delivery
may lag publication, so it does not establish the exact firmware/panel phase of
the historical 841 ms event. The old 13.2 MB unsupported-art source was not
matched for a hardware rerun; no-art caching is intentionally preserved.

During that active generation, the IDF local monitor started/stopped successfully
and reported 65,984 B minimum free heap, 448 B above the documented 65,536 B
cover reserve. Settled reader heap remains 65,624 B, largest block 53,236 B,
identical to the installed baseline. The whole-boot global heap watermark reached
50,636 B at first reader open and 50,452 B at a chapter crossing; it is not a
per-cover minimum and does not satisfy a universal 64 KiB runtime floor. The whole-boot loop-task
stack watermark was 7,052 B minimum free of 16,384 B, reached during reading
and unchanged through generation. No optical endpoint, universal heap
floor, approved performance threshold or owner review is inferred from this data.

A subsequent real cache validation completed in 118 ms with zero writes.
An uninterrupted 180-second passive window produced three additional covers in
41,602 / 3,834 / 14,656 ms. Wrapped read bytes were 17,085,809 / 934,450 /
5,764,585 B respectively; these successful JPEG paths retain complete source
validation. All three local heap monitors reported the same 65,984 B floor.
Delivered directory censuses grew from 38 to 40 entries (26 to 28 art-shaped,
12 no-art-shaped), each with zero temporary names, other names and errors,
and no limit/interruption/skip flags. Shapes alone do not establish NLC2 validity.
Reopening after generation preserved the saved anchor and reader heap, completing
in 2,902.4 ms from Stage with no kick. One later normal reset changed the boot ID
without changing the build. The actual Frosted restart open completed in
2,457.3 ms; three warm controls were 2,130.4 / 2,227.7 / 2,124.6 ms. The isolated
higher warm sample is retained without inventing a regression attribution or
cherry-picking it away. Revisiting the same catalog/offset after that restart exercised the real cache
validator: 201 ms, 60 reads / 13,648 B, four opens/path checks and zero writes.
The catalog and offset were stable; telemetry does not expose book identities or
prove cache-file byte equality. This supplies validator-based persistence
rather than relying on shape counts. Issue #22 stays open; `reading-latency-policy.json` retains its null
approved baseline and review. No performance acceptance is invented.


## Completed worktree cleanup

After the image passed gates, was installed/read back and finished engineering
device checks, a fresh HEAD, branch, dirty/untracked/ignored, unique-commit,
archive and live-process audit preceded supported removals. Five confirmed
obsolete worktrees were removed with `git worktree remove --force` (one force is
required for their initialized submodules): `newt-main-ro`, `newt-port-19`,
`newt-port-20`, `newt-port-20-review` and `newt-port-29`. Every removal succeeded;
no manual directory deletion or extra prune was needed. Branches were retained.

The active T3 implementation, `~/newt`, dirty `newt-fw`, unique
`newt-home-visual-refresh` and `newt-scratch`, and Python-owning `newt-x3-port`
remain. No shared runtime, unrelated project, Docker volume, backup or unique
work was removed. The earlier source snapshots/branches and original all-refs
bundle also remain. Fresh private all-refs bundle
`newt-all-refs-final-e30e263-20261011T0141Z.bundle` includes this candidate and
all obsolete HEADs; SHA-256
`26707424f8a6398447712b7b38cd2f0db95e1cf5e39d7ae46de88ef9b4523ce9`.
Full worktree/ignored-build/Git-administration archives are retained under
`~/newt/.build/private/newt-cleanup-20261011` (directory 0700, archives
0600). Their digests and removals are bound in
`artifacts/combined-x3-candidate.json`; each passed compressed integrity,
SHA verification and fresh GNU tar comparison before removal.

Filesystem available space increased by 40,793,153,536 B (37.99 GiB) during
removal. Recovery archives occupy 14,237,536,256 allocated B (13.26 GiB), giving
approximately 24.73 GiB net attributed recovery after retaining archives.
The latter is an attribution estimate, not an isolated filesystem before/after
measurement across the entire implementation session.

To inspect/recover, verify the archive digest, extract with GNU tar's ACL/xattr/
sparse options into a new private staging directory, then recover desired files
into a Git-supported detached worktree at the archived HEAD (or a separate
repository from the verified bundle). Do not extract over live directories.
The review archive also contains the retained X3 port's SDK Git administration;
never overwrite that retained path merely to restore the obsolete review tree.
The private final audit documents original-path repair preconditions as an
alternative, including absent target/admin paths and `git worktree repair`.

The Linux gate's 21 rewritten fixture images were likewise copied to a verified
private archive before restoring only our generated tracked outputs. Current
exact-source native receipts are committed; no owner work was reset or stashed.

## Historical rejected-e30 recovery (394 sectors; do not use for current image)

Recovery root:
`~/newt/.build/private/x3/combined-20261010/prewrite-20261011`.
It contains the matching whole-flash baseline, original app1, exact candidate,
whole-flash preboot readback and recovery receipts. The minimal app-only restore
payload is `app1-restore-394-sectors.bin`, 1,613,824 B, SHA-256
`dc1b04a589e1c4b5f9ec95bfcd99101b3d87ebe173b97449136d7f3348480410`.
It is exactly the baseline's original bytes at `[0x780000, 0x90a000)`; bytes
outside that extent were unchanged by installation. The backup and restore span
were independently verified; a rollback write was not exercised.

Use the same supported application workflow if rollback is requested:

1. Close the reader and save progress. Verify the expected X3 ROM identity,
   current selected app1/partition map and the restore file's size/hash; stop
   on a mismatch. Take a fresh current flash read for outside-span comparison.
2. With the approved project-local Python and the verified stable serial path,
   write only the archived restore span:

   ```text
   .toolchain/platformio/penv/bin/python -B -m esptool --chip esp32c3 --port VERIFIED_X3_PORT --baud 460800 --after no-reset write-flash 0x780000 ~/newt/.build/private/x3/combined-20261010/prewrite-20261011/app1-restore-394-sectors.bin
   ```

3. Read flash before boot and verify the restore span and all outside bytes
   against the fresh current read. Do not restore the old whole-flash image:
   that would replace current NVS/reading state and unnecessary partitions.
4. Perform one normal reset and verify `0184bbb0da20-regional-experiment` boots.
   Stop on any write/readback failure; do not retry blindly, erase, repartition,
   change a bootloader or format/migrate the card.

The SD was not backed up anew by explicit owner waiver. Device tests retained
books, settings and saved reading position; the cold test purged only supported
derived reader caches. Raw backups and SD/serial evidence remain owner-private.

## Historical rejected-e30 owner checklist (superseded)

The rejected e30 image was left awake on Stage Home. On that image Stage was boot-local: a later boot/wake
returns to Frosted; the existing DEV `PRESS home-stage` action selects it again.
No merge, release, default change or visual/performance acceptance is claimed.
The owner has not yet provided the physical Power wake/card steps, so no
unsupervised sleep command was issued.

- Judge Stage's gray tones, text, cover, dock halo, focus moves and cleanup
  ghosting. Decide whether its measured focus/entry costs are acceptable.
- Open/resume, turn pages and cross a chapter; confirm saved position, styling,
  generated covers and Library/Home behavior, including navigation while a
  fresh shelf is generating.
- With the owner ready to press Power, run sleep/wake and confirm progress/Home;
  perform the documented safe powered-down card removal/reinsertion checks and
  confirm recovery without formatting or migration.

Mac ImageIO/renderer remains untested by explicit deferral. Matched timing of the
historical unidentified 13.2 MB unsupported-art source, optical publication
phase, physical sleep/wake/card behavior and independent performance-policy
admission remain unverified. These are explicit limits of this installed
experimental candidate, not inferred passes. Conditional superseded-PR closure
stays after owner acceptance and merge; #29 remains flagged as an issue.
