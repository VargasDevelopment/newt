# NEWT-31 Part A: Library open attribution

A2 follow-up (2026-10-22): the header dependency repair and full Linux gate
are complete. Current clean images are from `4593c9f`; see the A2 section below.

This branch instruments `ab2f910` without implementing performance fixes. All
books used here are authored synthetic fixtures. Host work, clean C3 resource
builds and physical latency are different evidence. The Lead still needs to
measure the DEV images on hardware; a sub-100-ms device residual is a target,
not a result established by a host clock.

## Firmware path and timing contract

The Library Activate press enters `loop`'s `beginAction(micros())`, then
`dispatchAction` → `LibraryReaderSelection::dispatch` → `openSdSelection`.
The action finishes after paint and housekeeping, before idle producers. A
second consumed action marks `MultipleActions`; do not use that sample to rank
one book open.

Schema 2 preserves the order of the original nine `Reader action us: v=` positions and adds
one named `Reader open us` row. It replaces the old phase call-count row, not
any validation. The 76-byte timing owner and existing diagnostic allocation
stay the same size. Eight enclosing 4-bit phases live in the existing owner,
so scopes remain empty and do not extend callers' register lifetimes. Excessive
nesting is marked invalid. Nested scopes suspend their enclosing timer and restore it
on return, cancellation and failure. All durations below are exclusive and in
microseconds; nested work rows are explanatory subsets, not additional time.

| Row / phase | Actual work included |
| --- | --- |
| `open.select` | Complete `openSdSelection` transaction outside its nested phases: cover handoff, resume-authority load, receipt-presence lookup, reader/source release, `select`, candidate rollback and final status. `select` opens the EPUB, loads/checks the receipt, computes the full current source SHA even for a warm Library selection, compares all 256 bits, or imports the complete source/block digest receipt. Filename SHA is only the per-book cache route. |
| `open.progress` | Both `loadReaderProgress` calls: durable per-book progress loading/validation, saved-presence decision and restore authority. The first callback defers Home projection while candidate title is private; the immediate open uses `projectHome=false`. |
| `open.source` | `prepareSource`: returning-card/held-source checks, stale-handle reopen, source/session recreation, durable selection comparison and diagnostic projection. |
| `open.bind` | Remaining `openReader` setup: recovery selection, heap admission, optional session binding, lending/withdrawal of the display frame and final publication guards. A strict heap fallback can rehash the source here. |
| `v[0]` receipt | `warmOpenSelected` receipt load/validation and exact digest agreement. Source/receipt work during initial Library selection remains in `select`. |
| `v[1]` chapter | Complete verification of the selected compressed chapter range after canonical spine selection. |
| `v[2]` reader / ordinary | Initial reader attempt: package/admission and semantic/canonical selection, first-page restoration/layout outside the nested receipt, chapter, cache and projection timers; also ordinary fallback on a missing/unusable page cache. |
| `v[3]` projection | Verified semantic/index/projection lookup after first-page staging; missing optional metadata retains the ordinary reader's facts. |
| `v[4]` metadata | Accepted title/Home metadata publication, including the post-selection call and any optional title-record read/write/flush/close. Covers are handed off before reading; no cover decoder or idle producer runs as part of a normal open. |
| `v[5]` progress publish | The complete `saveReaderProgress` checkpoint transaction, including full-D selection/peer comparisons, protected progress save/readback, destination intent save/readback, checkpoint identity and optional estimate work. Library open calls it after acceptance; the inner publication also saves when its existing screen/recovery conditions require it. |
| `v[6]` cache | Cache presence and page queries, including whole-file CRC validation on a newly acquired immutable page-cache lease. Cache misses do not build a page cache in the foreground. |
| `v[7]` composition | First-page UI/native-frame composition, plus the cold opening working frame when no receipt exists. |
| `v[8]` display | Blocking display calls for the cold opening frame (when present) and final reader frame. Thus a cold sample can contain two refreshes. |
| `open.commit` | `SelectedBookStore::saveProtected`: commit/verification of the explicit selected book while preserving the previous acknowledged book. |
| `open.device` | `readDeviceInfo`, including battery sampling immediately before final paint. |
| residual | Dispatch/queue/housekeeping glue outside these scopes. `total` equals the sum of all fifteen durations plus residual. |

The earlier immediate `Reader open: phase=...` prints are removed from main;
their replacement uses fixed 160-byte formatting, TX-capacity checks, exact
write acknowledgements and no retry/wait/allocation. Require `schema=2`,
`flags=0`, `actions=1`, and `Reader action rows: emitted=7 expected=7` before
using a sample. A missing row or short write means incomplete evidence. Existing
`Reader work`, route, admission/member/hash/IO and producer rows remain available.
A release build has no diagnostic recorder, clock calls or timing storage.

## Host evidence

The focused wiring regression compiles the actual firmware `openSdSelection`,
`openReader`, progress loader, candidate transaction and source preparation with
real reader/parser/storage policies and authored hardware ports. It covers both
orientations. Removing each progress/source/setup/selection-commit/device-info scope fails
the phase oracle. Its incrementing clock proves wiring, not milliseconds.
Full-width durations, wrap, nested/early-return scopes, disabled diagnostics,
startup allocation failure, partial writes, TX backpressure and maximum decimal
rows have separate focused controls.

The reproducible host matrix uses four authored shapes: 3/14/3/20 spine
items respectively; each large shape includes a 2,097,152-byte inert stored
resource. Each shape exercises cold import, sealed-receipt reopen without a
page cache, and sealed-receipt reopen with a valid cache. Fixture creation and
warm preparation are excluded. Every case opens successfully. Counters measure
work, not elapsed time or SD speed.

| Shape / EPUB bytes | Route | Source + derived read bytes | Native open attempts | CRC bytes | Selection SHA bytes | First-page layouts |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| small-few / 7,055 | cold / miss | 264,687 | 20 | 1,440 | 14,110 | 1 |
| small-few / 7,055 | reopen / miss | 86,708 | 18 | 1,440 | 7,215 | 1 |
| small-few / 7,055 | reopen / hit | 71,439 | 17 | 8,763 | 7,215 | 0 |
| small-many / 21,370 | cold / miss | 458,674 | 20 | 1,440 | 42,740 | 1 |
| small-many / 21,370 | reopen / miss | 113,346 | 18 | 1,440 | 21,658 | 1 |
| small-many / 21,370 | reopen / hit | 96,131 | 17 | 6,812 | 21,658 | 0 |
| large-few / 2,135,255 | cold / miss | 2,564,363 | 20 | 1,440 | 4,270,510 | 1 |
| large-few / 2,135,255 | reopen / miss | 2,290,547 | 18 | 1,440 | 2,152,055 | 1 |
| large-few / 2,135,255 | reopen / hit | 2,345,153 | 17 | 102,827 | 2,152,055 | 0 |
| large-many / 2,236,143 | cold / miss | 3,104,334 | 20 | 1,440 | 4,472,286 | 1 |
| large-many / 2,236,143 | reopen / miss | 2,394,505 | 18 | 1,440 | 2,253,711 | 1 |
| large-many / 2,236,143 | reopen / hit | 2,405,467 | 17 | 43,037 | 2,253,711 | 0 |

Every selection reads exactly the complete EPUB once. Cold selection SHA
updates total twice the EPUB length: whole-source plus block digests on that
same read, not two source passes. Warm selection still hashes the entire EPUB
plus receipt metadata. Admission and consumed-chapter verification add more
SHA work; those bytes are separate in the phase evidence.

These are ranges across all twelve cases. The host phases combine some of the
finer firmware phases above; `receipt_admission` includes session preparation,
admission and a `stored()` cache hint which can acquire the CRC-checked lease.
Therefore cache CRC appears there even when `first_page_cache.crc_bytes` is zero.

| Host phase | Source read bytes | Derived read bytes | Native open attempts | CRC bytes | SHA bytes | Layouts |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| selection | 7,055–2,236,143 | 0–17,696 | 1–3 | 0 | 7,215–4,472,286 | 0 |
| progress | 0 | 0 | 0 | 0 | 0 | 0 |
| receipt_admission | 53,029–826,847 | 928–118,967 | 1–3 | 0–101,387 | 53,189–844,415 | 0 |
| first_page_cache | 0–32,768 | 64–2,743 | 2–4 | 0 | 0–32,768 | 0–1 |
| metadata | 0 | 340 | 1 | 656 | 0 | 0 |
| selection_commit | 0 | 0 | 0 | 0 | 0 | 0 |
| checkpoint | 0 | 428 | 11 | 784 | 0 | 0 |
| final_paint | 0 | 0 | 0 | 0 | 0 | 0 |

Each case holds one separately counted host source handle. Native open counts
above include failed attempts and exclude that handle; they are host adapter
operations, not physical FAT open counts. Progress/selected-book and NWBR intent
backends are modeled slots: progress load uses one logical backend transaction;
selection commit uses one transaction and one selected-book write; the accepted
checkpoint uses three per-book transactions, two progress writes and one NWBR
intent write. Their physical SD bytes and syscalls are **not measured**. Zeros
in those native columns cannot establish zero device I/O. Native derived
checkpoint work separately makes 11 open attempts, 6 writes and 3 sync calls;
metadata publication makes 1 open attempt, 18 writes and 1 sync. Final paint
adds zero source reads/layout runs in these cases. No measured work remains in
`other`; the ESP display and battery ports are outside this host work matrix.

The host-only CRC hook instruments the actual storage adapter in a temporary
overlay; SHA accounting wraps the host mbedTLS adapter. FreeInk PageCache uses
FNV for its layout-generation hash; storage CRC certification is counted
separately. No production counter hook is added. Reproduce with
`python -B tools/book-open-work.py` using the approved interpreter.
Exact per-phase counts, fixture hashes and command lines are archived in
[`newt31-host-book-open.json`](../artifacts/newt31-host-book-open.json),
[`CSV`](../artifacts/newt31-host-book-open.csv) and
[`stdout`](../artifacts/newt31-host-book-open.stdout.txt).
Raw gate/build commands and logs remain in `/var/tmp/newt31-evidence/`.

## Exact firmware cost

Four clean measurement lanes on `1470349` compile successfully. The two DEV
measurements fail only their inherited flash ceilings; these raw reports are
preserved as `artifacts/newt31-raw-code-health-{variant}.json`. All four initial
post-review clean lanes PASS the exact record-50 budgets on clean
`a8c997595d9a1c54ad5dee8332aafc52efdccae3` (`dirty: false`). Their compiler-input
hashes, flash/SRAM/RTC and every compiler frame match the reviewed raw reports.

| Variant | Main flash B | Measured flash B | Delta B | SRAM B (unchanged) | RTC B (unchanged) |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 2,173,904 | 2,173,872 | -32 | 133,232 | 300 |
| dev-usb | 2,316,624 | 2,317,040 | +416 | 134,064 | 304 |
| regional | 2,180,576 | 2,180,560 | -16 | 133,728 | 300 |
| regional-dev-usb | 2,325,312 | 2,325,728 | +416 | 134,560 | 304 |

All 357/386/359/393 selected compiler frames and their ceilings remain exact.
The full reports differ only by removing the old 32-byte DEV phase-print
lambda. Empty scopes and the packed parent-phase stack keep the owner at 76
bytes; existing diagnostic allocation and release storage are unchanged. The
former immediate phase lambda also evaluated `millis()` in release; removing
it explains the small release shrink. No floor, protected history, selected
entry, exclusion, adapter, partition or reserve changes. The usable flash limit
stays 7,016,448 B. Standalone record 50 (now renumbered 51) tightened the two
release ceilings relative to record 49. In the integrated history it follows
main’s record 50 and lowers all four formal policy ceilings to that historical
standalone measurement; this rechaining is not incremental Part A attribution. Individual frames, link SRAM and
RTC totals do not establish runtime heap, call-chain or hardware margins.

Fresh independent bounded resource-policy review is **CLEAN** and accepts
original record 50’s exact costs (now live record 51); see
[`review r1`](../artifacts/newt31-resource-review-r1.md). This is resource
acceptance only. The final four-lane run and software evidence are archived
below.

## Initial Part A verification and handoff

PASS: `tools/check.py --suite quick --profile linux` (347.051 s), focused action
owner/release-none diagnostics, actual firmware wiring in both orientations
with all five scope-removal negatives, real native source/cache/receipt paths
including corruption/seal failure/cancellation/strict fallback, and producer
lease attribution with historical/removal controls. The twelve-case host work
matrix passes ASan/UBSan. The resource-policy suite passes all 25 tests after
the accepted record/pin, and the structural/boundary gate passes. The focused
wiring clock is authored; none of these tests establishes physical latency.

PASS: four clean native health lanes, exact RTC/NVS/sleep/link guards, all four
`tools/code_health.py --variant V --adopt ab2f910` checks, and the pinned
release gate on `fcca87ad1cb792a3ca2bfafb7c7e5bae3ff14b0d..HEAD` through the
resource commit. Final-HEAD history is checked again after this receipt commit;
its output is retained at `/var/tmp/newt31-evidence/history-final.log`.

Committed evidence: [`quick`](../artifacts/newt31-quick-linux.log),
[`initial measurement`](../artifacts/newt31-measure-four-clean.log),
[`final four lanes`](../artifacts/newt31-final-four-clean.log), and
[`focused/adoption checks`](../artifacts/newt31-focused-adoption.log).
The initial per-variant resource, firmware-identity and link-verification
receipts remain in Git at `408dc81`; canonical receipts are now superseded by A2. The archived raw reports bind
the cost record separately from the post-review images. No firmware input
changed after the measured source; this handoff only adds evidence and docs.

Current Lead DEV images from clean `4593c9f`:

| Variant | Image in this worktree | SHA-256 |
| --- | --- | --- |
| dev-usb | `.pio/dev-usb/build/newt_x3/firmware.bin` | `dd788dbb201de6f8a9678129f5d7d94c90b6135cd79174bacc0d69da7ea465c5` |
| regional-dev-usb | `.pio/regional-dev-usb/build/newt_x3/firmware.bin` | `a9358c3eff412404a5f644b46507e741fa591a24285c16cb0af345a663210ee8` |

The worktree was created from the supplied local `ab2f910`; no fetch was made
because the shared rules prohibit network access. No optimization, dependency
acquisition, push, flashing or hardware access was performed. Hardware timing,
residual, memory margins and owner acceptance remain pending. Part A still stops
before optimization; the A2 repair and updated verification follow.

## A2 header dependency repair and full Linux gate (2026-10-22)

The Lead's full gate on `408dc81` exposed a missing direct dependency:
`library_cover_producer.h` used `ReaderActionScope`/`ReaderActionStage` while
relying on the main translation unit's transitive includes. Its exact-source
host wrapper intentionally replaces the hardware-heavy Library selection with
a port fixture, so those declarations were absent. The existing test reproduces
the unknown-type/identifier errors on `408dc81` and passes DEV=0 and DEV=1 after
the fix in `007c5a5`.

The production header now includes `reader_open_profile.h`. The same audit
found three other scope users relying on transitive definitions:
`library_reader.h`, `library_book_open.h` and `reader_revision_session.h`; they
now include it directly too. All seven scope-using headers own this dependency.
The wrapper keeps hardware ports first in its include search and the real
firmware directory second, resolving the production timing definition without
substitute types or a test-only timing shim. Executable reader behavior is
unchanged.

FULL Linux gate **PASS, rc 0, 1,106.624 s**:

```sh
/usr/bin/env TMPDIR=/var/tmp/newt31-a2 \
  PATH=~/worktrees/newt-x3-port/.build/python-bootstrap/python/python/bin:/usr/bin:/bin \
  python -B tools/check.py --profile linux --keep-going
```

No phases failed or were skipped. The wrapper, EPUB probes, chapter-cache
producer, next-chapter and DEV serial phases all ran and passed. Every-page
parity passes all 18 authored fixtures, both orientations/directions and state
journeys. The Linux profile retains its documented separate Mac ImageIO decode
requirement; these are host results, not device evidence. Resource-policy tests
(25) and the structural gate were repeated after the record/pin refresh and pass.

Four first clean lanes on `007c5a5` and four post-review clean lanes on
`4593c9f9d636fe955c6b0025dc70f7cfbbb99ff9` **PASS**, all `dirty: false`.
Flash remains **2,173,872 / 2,317,040 / 2,180,560 / 2,325,728 B**; SRAM remains
**133,232 / 134,064 / 133,728 / 134,560 B**, RTC **300 / 304 / 300 / 304 B**
(default / DEV / regional / regional DEV). Every compiler stack frame equals
the `408dc81` receipt, including all 357/386/359/393 selected entries. No ceiling,
floor, exclusion, adapter, protected record, reserve or partition changed.

The first adoption checks correctly rejected the old record's compiler-input
binding despite unchanged costs. At A2, original candidate record 50 kept its before/after measurements while
refreshing the raw report/input/core hashes and source to `007c5a5`. It is now
renumbered 51, preserving its measured after/source/evidence; its formal
policy predecessor is rechained as documented below. Its complete earlier version remains in
[`A1 record 50`](../artifacts/newt31-a1-flash-record-50.json), Git history and
the original raw reports/review; main's records 1-49 and all protected history
remain intact. Fresh bounded
[`A2 resource review`](../artifacts/newt31-a2-resource-review.md) is **CLEAN**
and accepts these exact unchanged costs and bindings. Final receipts match the
reviewed corrected-header inputs. All four final `--adopt ab2f910` checks pass.
The pinned release gate is checked after the closing receipt commit; its
final-HEAD output is `/var/tmp/newt31-evidence/a2-history-final.log`.

Committed evidence: [`full Linux`](../artifacts/newt31-a2-full-linux.log),
[`first four lanes`](../artifacts/newt31-a2-measure-four-clean.log),
[`post-review four lanes`](../artifacts/newt31-a2-final-four-clean.log),
[`before/after regression, audit, policy and adoption`](../artifacts/newt31-a2-regression-adoption.log),
plus the canonical four resource, firmware-identity and guard-link receipts.
No C++ input changed after `007c5a5`; later commits refresh evidence and docs.
No device was accessed. Part B remains stopped pending the Lead's device data.

## Lead device measurement

Use a DEV image from the committed clean receipts. Capture the complete serial
stream around one Library Activate and the first reader display. For several
structurally different books, compare first open after reset, Library reopen,
and existing/missing page-cache states; record power/reset conditions and route
flags. Preserve the action summary, all fifteen exclusive durations, route/work
rows, and every emission mask. Identify cold opening-frame refreshes separately
from the final page refresh. Verify total accounting, then require residual
below roughly 100,000 us. Any larger residual needs another attribution pass,
not a performance claim. Repeat samples and rank costs from device time together
with host work counts. USB timing does not establish battery sleep/wake or cold
power-on behavior. No hardware was accessed by this implementation worker.

Part A deliberately stops before optimization. The Lead requested the full
Linux gate in A2, including its every-page parity phase; that gate now passes.
The separate full turn-cost matrix and full adversarial/maintainability review
remain Part B work pending device data.

## Three fix directions to test in Part B

These are ranked hypotheses for the Lead's DEV measurements, not claims of
measured device speedups. No fix below is implemented here.

1. **Whole-source identity work on every Library selection.**
   `ReaderRevisionSession::selectRevision` deliberately recomputes the complete
   current digest even when the receipt is valid. A missing receipt imports the
   whole source and its block hashes. This is the strongest source-size-scaling
   suspect for the formerly unattributed time. Optimize generic sequential
   reads/hash chunking with the existing lent frame and amortize repeated
   descriptor/path checks only inside a checked immutable scope. Keep the
   complete full-source digest comparison, every consumed-block verification,
   cancellation and final source/receipt checks. Do not trust filename/stat
   hints as identity or reuse a certification across a changed source epoch.
2. **Synchronous selected/progress/destination publication.**
   Device facts in the brief put an existing `v[5]` sample at 1,435 ms, and #89
   reports roughly two seconds for a progress save. `open.commit` and `v[5]`
   now distinguish selection publication from the checkpoint transaction. Look
   for repeated peer/path reads and immutable-record reopen costs inside one
   transaction. Consolidate only work whose checked generation proves it is
   identical; retain protected peers, flush/promotion/readback, explicit failure
   and unconfirmed acknowledgements, and the ordering that makes destination
   intent the authority. Moving a required commit past acknowledgement is not
   an acceptable shortcut.
3. **First-page cache certification or ordinary chapter layout.**
   A valid page cache removes ordinary layout but still requires full-file CRC
   when its immutable lease is acquired (#63). A miss uses the complete ordinary
   chapter renderer; foreground open does not build a cache. Use the host CRC,
   read and layout counts with `v[6]`/`v[2]` to decide which route dominates each
   structural case. Candidate work is to order mandatory state-file operations
   before acquiring the page-cache lease, avoiding unnecessary displacement and
   reacquisition, or to improve bounded ordinary layout work generically. Keep
   first-lease CRC, identity/style/generation checks, full verified fallback,
   descriptor limits, exact pixels and stale/corrupt/change failure coverage.

Cold admission also checks the ZIP central directory and local headers, enforces
member/expanded-size limits, compares duplicate names, expands required package
members and parses canonical metadata. Its duplicate-name loop is quadratic in
member count, while directory/local-name reads scale with member count and name
length. Existing `Reader stages`/member rows identify this work inside `v[2]`;
the heterogeneous host matrix separates larger payloads from more spine items.
It remains a possible third-place cost if the device timings put it ahead of
cache certification or layout. Preserve all admission checks when investigating.


Main integration renumbers this document from 068 to 069 and Part A’s live
flash record from 50 to 51. Its measured after/source/evidence remain exact;
the formal policy predecessor is rechained to main’s time-zone record 50.
The original full object, including its original before values, is preserved in
`artifacts/newt31-original-part-a-record-50.json`. These are standalone Part A
measurements; the combined time-zone/Part B tree receives its own record 53.
