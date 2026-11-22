# M1 UX proposal and physical acceptance

This document records the upstream UX-only PR #3 proposal and evidence. Its
fixture identity, allocation and image measurements describe that branch. For
the combined SD reader candidate, use `015-m1-library-trial.md`, the current
SD verification artifact and `CODE_HEALTH.md`; combined hardware acceptance
remains outstanding.

The installed UI baseline remains source `f6c6792bf8e450f4ea8f3f77d1ba05db50f31fae`.
The owner accepted the frosted Home, quiet focus/tenth-move cleanup, Pocket Pause
preferences, footer, reader orientations, fresh sleep backdrop and canonical logo.
USB-connected reader wake initially passed; at 15:45 UTC battery-only wake returned
Home, and at 16:05 UTC the owner repeated USB PASS / battery FAIL with no agent USB
access. At 17:45 UTC the owner repeated Read→text→Sleep→correct CCW frosted reading scene→Wake
on installed f6: USB returned the same text, battery returned Home. Both sleep
scenes were correct, so the observed divergence follows destination rendering.
Treat that sequence as the acceptance fixture, prioritizing wake/reset/restore or
later input. Reader resume acceptance is FAILED/intermittent; the cause is unobserved.

At 19:36 UTC the owner reconnected the X3 and spare SD adapter and released the
coordinated hardware window for reviewed builds. This UX worker has not opened
either device, sent inputs, reset or flashed. Parent coordination, a single hardware
owner, fresh identity/layout and private backup/recovery verification must precede
any write. Avoid the old unsafe diagnostic opens. Source tests do not establish
battery resume reliability, physical refresh quality or energy savings.

## Upstream UX-only uninstalled wake-evidence proposal

Compiled source `829f8c834568c351a99350483c9f90e24bc15f5b` combines the minimal DEV-only
awake RAM receipt with C10 actual procedure/driver runtime binding and the
C11 complete compiler-output-tree preflight. All selected build-root descendants
are validated before child dispatch; unsafe file/directory/dangling aliases,
hardlinks and special nodes fail closed while lawful dependency links remain. Your X3 shows
immutable boot/initial-open/first-paint/consumed-input evidence alongside storage
warnings; routing, initial screen, USB schema, ADC reads, NVS and 68-byte RTC sleep
contract remain unchanged. Initial exact/anchor policy and typed failure freeze
before settings rollback or later retry. This is a cause-measurement candidate;
battery-only resume remains FAILED/intermittent, with cause unmeasured.

Full host ASan/UBSan, runtime10/resource39/cache7 and existing regression suites
PASS on this source. Four clean native compilations and linked NVS/sleep guards
PASS. Two source reviews per feature, two rebase reviews and two fresh actual
four-variant resource/cost audits are CLEAN. C11 adds two fresh source reviews,
168 output rejection cases, eight valid trees and 24 actual old-builder controls.
The complete host gate and all four fresh clean native builders exit0 under the
unchanged exact limits; two fresh actual receipt audits are CLEAN. Structure and
all four live resource checks also PASS after provenance refresh. The original
e171 diagnostic builds rejected four old DEV caps each before reviewed adoption;
that historical result is retained separately from these current successful builds.
Nine compiled source scopes remain frozen.

All four flash/SRAM/selected-frame measurements are unchanged from reviewed e171.
Fresh image hashes include current compile-time metadata, ELF/image digests and
DEV identity; byte equality with historical images is not claimed.
Ordinary measurements are unchanged. DEV flash increases 3,584 bytes, regional
DEV 3,600; both SRAM +16 and selected loop/setup frames +16 each (240/192).
Other four selected frames remain 736/80/80/912. The 80-byte receipt replaces64-byte
backing state; SleepSession72→88. The DEV-only info pointer consumes existing data
padding. Main546→556 is the separately reviewed ten-line observation exception.
No pins, adapters, exclusions or compiler ownership expand. Actual Python runtimes
and the isolated offline driver relocation are bound in current receipts; the
historical toolchain digest is not claimed identical. Static metrics do not prove
runtime peaks or physical behavior. Live private PR3 provides remote review status.

| Variant | Bytes | SHA256 | DEV | Sectors / app1 end |
| --- | ---: | --- | --- | --- |
| default | 1,234,848 | `791db7209f03cca2693435a7517b696bdc6cba6db49188326af4aa3607e4deae` | — | 302 / `0x8ae000` |
| dev-usb | 1,244,528 | `8464b4d6398d4525a54d13e8a0bdd04b72a1d6a8d8c29aa863bed5c905434806` | `829f8c834568` | 304 / `0x8b0000` |
| regional | 1,238,672 | `66777067d63dacd9f30d5883ee0c19dfdf05f347e9ff43b4adcfad5e0035a2e8` | — | 303 / `0x8af000` |
| regional-dev-usb | 1,248,384 | `911e7b6965c100cd47fb2d0e1374d8523e1e80275bdfeec932de2a81608267cf` | `829f8c834568-regional-experiment` | 305 / `0x8b1000` |

Every image starts at app1 `0x780000`. The proposed regional development image
spans `[0x780000,0x8b1000)` (305 sectors). The [current manifest](../artifacts/m1-wake-evidence-builds.json)
binds exact source, both runtimes, procedure/cache/compiler/toolchain inputs,
measurements, guards and archives. Canonical compiled workspace is
`newt-wake-evidence`; the C9 [historical manifest](../artifacts/m1-status-progress-builds.json)
was compiled and archived in the separate `newt-ux` workspace.

This manifest covers the upstream fixture-only UX proposal. The combined SD
candidate uses [its own verification artifact](../artifacts/m1-sd-reader-verification.json)
and source-bound resource receipts in [CODE_HEALTH.md](CODE_HEALTH.md).
The owner window/new dongle is available without contention. This worker has
performed no device access or installation. Parent coordination must name this
exact reviewed image, a single hardware owner, fresh identity/layout/active app1,
a new private full backup and recovery slice, the whole sector bound and full
readback. Preserve every byte outside that interval, including NVS, OTA selection,
partition data and app0. Never reuse the consumed journal or old unsafe DIAG opens.

After coordinated installation, perform only the [matched owner comparison](battery-wake-evidence-plan.md):
same noninitial passage/page/orientation, USB with no serial session versus USB
disconnected before sleep. Photograph first awake screen and then Your X3 Wake
details before USB reattachment; record owner presses and stop after the pair.

The installed regional development image remains 1,251,168 bytes, SHA256
`cd3fc8a9e03253a594c761c152595a9fb795ee4e54c18d4adae98c489841b3b0`,
DEV `f6c6792bf8e4-regional-experiment`, app1 `[0x780000,0x8b2000)` (306 sectors).
Historical image identities remain in [its manifest](../artifacts/m1-book-wake-builds.json).

## Recovery and storage

An actual open reader sleeps to its semantic page. All other screens wake Home;
reader overlays use committed settings and discard drafts. Only genuine deep wake
or the checksummed SDK-confirmed rejected-entry software restart can restore RTC.
Cold, panic, USB and unrelated resets stay Home. Wake-source arming failure cancels
the record and retains the workspace. Preserve early RTC snapshot, rail ordering,
returning sleep API and the NVS startup/mutation preservation guards.

Pending reader recovery owns the immutable newer bookmark, original geometry,
statistics, restore policy and durable-save outcome separately from storage write
permission. Temporary heap, allocation, parser and unclassified open failures retain
it. Success consumes it; verified identity/bounds or exact-page mismatch rejects it.
Home Read retries revalidate storage permission before selecting the protected checkpoint.
Books always browses Library; a pending checkpoint blocks replacement selection
with guidance to recover through Read. See [the route correction](048-books-recovery-routing.md).
Known live reflow uses a strict validated anchor and never falls back to page one.
Settings first verifies pending recovery and stays paused through failed preference
commits. Failed live exits retain the actual checkpoint before workspace release.
If wake-source arming rejects sleep, a verified live reader resumes ownership and
retires the old pending snapshot; later saves and exits cannot replay it. An
unavailable reader retains pending recovery for exact retry.

RTC v6 stays 68 bytes and explicitly carries pending exact/validated-anchor recovery
while Home still wakes Home. Another sleep retains the original checkpoint; later
Home Read retries it. v5 records fail closed. Durable progress stays 52-byte v1. Confirmed
absence differs from unreadable, ambiguous, foreign, invalid and corrupt storage;
unmapped records and present stores without a valid generation block ordinary writes.
A valid alternate generation recovers. First Home projects validated fixture progress
from generated semantic chapter lengths without allocating a reader workspace.

## Home and reader UI

The owner-approved Home contains the current-book card and three evenly spaced
navigation rows: Books, Apps and Settings. Morning Page's duplicate Pocket
Pause placeholder route is removed. The frosted shell, small wordmark, center clock,
seamless four-gray backdrop, square cards and 4px/1px gray85 outlines are preserved.
Focus bounds and sleep rendering use this layout. Good 16px body insets, responsive
reader/footer layout, both orientations and canonical settings wordmark remain.

Changed battery values attempt the whole right-aligned status text's native
`{8,30,24,134}` region. Unsupported/rejected timer painting defers until an ordinary
user paint; identical visible values do nothing. A disjoint gray activation retains
cursor eligibility only when both complete uploads match the current on-panel cursor
checkpoint. Accepted cursor DU updates that proof. Battery painting preserves the
nine-DU cleanup count. The tenth focus move requests full cleanup. Regional status
activation still uploads full planes with XTH4; bounded region tests do not measure
physical flash quality or energy savings.

Sleep freshly renders the intended reader or Home, applies the 25x25 box frost using
the two existing framebuffers and bounded scratch, then overlays the approved crisp
logo with transparent exterior and white halo. Original mascot art remains the
Easter egg. Display failures fall back to fresh Home; incomplete selector planes
cannot activate as a gray image.

## Evidence and next coordinated device window

The fresh source-frozen combined host gate passes with ASan/UBSan, CLI/pixel
checks, real EPUB page/prose comparisons, allocation/parser/heap faults, pending
retries, failed Settings commits/live exits/repeated sleep, both orientations,
reset rejection, guarded frames, display adapters and storage codecs. Verbatim
boot/open/Settings tests cover opposite RTC and persisted geometry, bars and
refresh cadence, semantic rejection and immediate repeated Save. Temporary parser
failure and exact success retain RTC settings. Old-code controls reproduce stale
settings/drafts, unbound procedure changes and altered valid helper bytecode.
An actual child-launcher control proves inherited external cache-prefix behavior.
All 21 structural-policy, 39 resource and 7 executed-cache fixtures pass.

Four sequential clean native compilations pass linked NVS/sleep guards and the
existing resource ceilings. Two fresh combined source reviews and two independent
actual resource audits are clean. Both audits bind configurations, compiled inputs,
authored procedures and applicable caches, compiler/toolchain/interpreter and 303
artifacts per variant to the frozen images; independently verify measurements,
pins, display identities, guards and DEV labels. The adopted exact policy and all
four read-only receipt checks pass. Reader allocation remains 69,968 bytes natively
and 70,088 bytes in the host gate. Static occupancy and frames do not prove runtime
stack/heap peaks.

The installed source has no USB/battery resume-policy branch. Pinned ESP-IDF 5.5.2
rejects manual deep sleep using configured wake triggers; Newt enables only GPIO3.
USB alone does not choose the tagged software restart. RTC FAST remains powered in
ordinary deep sleep and the retained record is reserved. These source/ELF facts do
not measure battery voltage, actual reset, retained-state validity or physical input.
Input starts before exact reader reopening; only Power has a release gate. Two stable
startup ADC Back readings can enqueue Back, but none were observed on this device.
Do not broaden reset eligibility or substitute stale durable state speculatively.

Before a future reconnect, Your X3 distinguishes cold/reset boot, unavailable wake
state and successful restoration. It does not expose requested Book versus Home,
and a storage warning replaces its footer. The [bounded evidence proposal](battery-wake-evidence-plan.md) freezes reset/cause,
record status, requested destination, initial exact-open outcome and first paint,
then records the first physical event and first Reading-to-Home transition. Your X3
shows it alongside storage state without USB or NVS writes. This is proposed
instrumentation, not an implemented or measured explanation. Parent coordination
is required before any device test.

Physical acceptance must cover both USB-connected and battery-only cycles at the
same recognizable noninitial passage, in both orientations. Record the first screen
and passage before further input or USB attachment. Repeat non-book Home wake and
the agreed UI checklist. Lost passage, stale imagery, reset loops or storage warnings
remain failures; never silently restore or retry a write.

The read-only development DIAG reports boot/wake, RTC validity, target/anchor,
restore/save outcome and first physical action. Its host client proves stable boot,
advancing sessions and fresh random-ID STATUS before optional operations, and emits
bounded connection receipts through failures and interruption. Older firmware stays
compatible. An earlier diagnostic attachment coincided with a reader-to-Home jump;
USB reset remains a hypothesis and old-client replay weakness is not observed device
replay. Fresh transcripts do not prove non-disruptive attachment. A later coordinated
USB test should first establish attachment behavior at idle Home.

## Milestones and integration boundaries

SD/library/migration remains separately owned. This branch uses a bounded fixture
identity and does not translate external-book anchors. SD integration must
reconcile RTC v6 content identity/locator and pending-recovery semantics, preserving
migration and per-content state, then run fresh reviews/gates/exact builds.

M1 requires actual readable books, reviewed actual-book resume, reliable pages,
settings/sleep, coherent UI, efficient refresh and tested recovery on the combined
device candidate. Both power states must pass. Only afterward propose a 3–5-day
reading trial freeze, allowing crash/data-loss/severe-usability fixes. M2 apps SDK
build/install/try follows the trial; new Pocket content remains separate work.

PR1 was normally merged as `aace103b46f3f769c7e11933c2195eeb2aae8ff1`.
Private draft PR3 targets main, preserving the same merge-base
`9a669b1bb02e749d6d9d5c427c05821ac3beaaa6` and compiled source. No merge or public publication is
authorized. The owner has released a coordinated reviewed-build hardware window;
this worker has performed no device access or installation. Raw backups, private books/photos, identities and secrets remain
outside Git and connected storage.
