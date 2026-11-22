# Next-chapter idle preparation (NEWT-27)

For the current bounded lookahead past short spines, see
[Short-spine lookahead (#231)](091-short-spine-lookahead.md). The history below
records the original one-spine policy and its later segment support.

This candidate has host and compile evidence only. Physical boundary latency,
input response, SD behavior and power acceptance remain unverified.

After the displayed chapter's complete cache is validated or produced, the
Reading idle owner may prepare exactly its immediate next spine. It uses the
same two-second post-paint quiet interval, power/card/view gates, cancellable
admission census, three-try per-spine session ledger, 120-second attempt limit,
and eight foreground-duty pauses. Terminal current-chapter refusal ends both
current and next work for that view; it does not grant speculative work or
spend the neighboring chapter's independent ledger. A packed view verdict
distinguishes a complete cache from an exhausted retry count; entering another
chapter resets that verdict and validates any existing cache before lookahead.

The next chapter is selected into the inactive canonical selection. Metadata,
CSS and pagination read through the producer cancellation wrapper. Styles and
complete totals settle before any writer opens: the admission credit reserves
one maximum-size candidate, so style retries cannot spend several candidates.
A final settled pass writes the cache and checks totals again. A successful next
cache therefore takes at least two complete parses: one unwritten settled
certification pass and one write pass, plus any bounded style-settlement retries. The existing
leased display workspace and its tail hold all parser, writer and index state.
The displayed frame, copied page plan, navigation, progress and resume intent
remain unchanged. The derived style slot is dropped on return; a later ordinary
cache miss resolves its own styles. No parser, writer or producer survives
an idle call. A validated immutable Pages read lease may remain open. Existing descriptor suspension lets due durable saves run before
the provisional writer resumes; input and view changes latch cancellation.

A boundary turn previously bypassed even a prepared destination. It now stages
and verifies the destination's canonical selection and compressed source range,
then validates and reads the cache. Cold entry and turns read a page file
alike: its checked query reads the directory and consumed blocks under the
read scope's time and attempt budget, with no byte cap (NEWT-87; the 128 KiB
opening cap bounds only full-CRC records). Only a successful cached page publishes its
selection. A missing or invalid cache uses the existing ordinary renderer;
no foreground turn builds a cache. The public measured cache-show entry point
retains the actual query body for both routes; a synchronous staging bit uses
existing cache-owner padding. The import-policy query is out of line to keep its additional
boundary arguments outside the existing public session-show stack frame.

Backward turns consume the previous chapter's existing cache and select its
last page by the maximum anchor. There is no speculative previous-chapter
builder: preparing an uncached previous chapter would require the same complete
selection, style settlement and pagination as next preparation, plus another
admission/write and more eviction pressure.

At the last spine there is no next candidate or extra next probe. Since #196 a
next chapter above 128 pages gets its first segment and its map (see "Long next
chapters" below); no partial file becomes a hit. Empty/malformed chapters are refused, without guessing a later destination.
Many tiny chapters do not cause a cascade: at most current and next are considered
for one displayed spine. The existing 32 MiB Pages quota, 128-entry census limit,
validated inventory and eviction rules remain unchanged. Reaching the inventory
limit denies admission rather than scanning more entries. The next admission
rechecks the current complete cache before the census; storage pressure may evict
old unleased caches, so hits are opportunistic. Closing/leaving the book retains
no background work and a fresh session gets its own ledger.

DEV USB retains per-action route bits: CacheHit is 0x8000, CacheMiss 0x10000.
PrefetchComplete is 0x800000 and PrefetchStopped 0x1000000, describing the
last next-chapter attempt in that session (Stopped covers interruption and
terminal failure). 0x400000 is the internal Running sentinel: it must never
appear on a foreground turn. Investigate if it does; use the PC target/attempt
rows to observe producer activity. `PC target: from=N next=1` identifies a next build for spine N+1;
`next=0` identifies current-chapter production. Its following `PC attempt`
row records status, stop reason, pages, writes, elapsed time and housekeeping
pauses. Complete means that attempt finished successfully, not a permanent
promise that quota eviction or later corruption cannot invalidate its cache.
Producer and turns run synchronously, so a foreground turn cannot overlap a
running producer: input first stops it and the turn's route records its result.

The Lead should capture PC target/attempt rows followed by the boundary action's
route bits and layout counters. Measure a cold miss, a completed-prefetch forward
crossing, a backward crossing into the previous cached last page, and an input
press during a next build. Confirm that the cached crossing has CacheHit and no
chapter-layout work, that input cancels before foreground dispatch, and that due
progress saves still obtain their descriptor during a long build. Record the 16 KiB reader task's stack high-water mark during a long DEV build
and its due-save pause. Confirm that the Running sentinel never appears on a
turn. Record USB sleep/wake, unplugged battery sleep/wake and cold power-on
separately, including power condition and reset evidence. No device was touched
for this candidate.

## Round 1 host measurements and software coverage

The full authored matrix has 24 fixtures (4 KiB, 64 KiB, 256 KiB and 1 MiB;
plain, styled and list/table; stored and deflated), both orientations and three
samples: 3,744 observations and 1,248 summary cells. Of 288 prefetched turns,
144 eligible successful preparations hit with zero chapter layout, expanded
chapter bytes and cache writes; 144 capped/refused preparations correctly use
the ordinary path. Eligible cached crossings have a 1.438 ms median against
2.847 ms for the paired cold crossings. The 118-page deflated plain fixture
specifically measures 3.990 ms cached versus 17.327 ms cold across six samples;
its cache exceeds the old 128 KiB cold-opening hint. The control that
rejected it retired with that hint (NEWT-87).

Across 144 successful next builds, elapsed host time is 4.754–149.332 ms,
median 10.348 ms. These times exclude quiet settling and the admission census.
For scale, a **20-second page dwell is an assumption**, not an observed human
reading rate: the median and maximum build are about 0.052% and 0.747% of that
assumed dwell. Host scheduling can greatly change wall time; none is a device
latency bound or latency-admission result. The Lead must compare actual PC
build time with actual page dwell on the device.

Full every-page equality passed 13,408 frame/anchor/state comparisons across
18 fixtures, both orientations and directions. The five-fixture actual-idle
regression and its removed-target/removed-boundary controls pass, as do the
opening-only-hint control, queued/mid-build input, due-save-before-writer,
current/next attempt independence, cap/malformed/last-spine, stable displayed
state and cached backward-last-page tests. The existing 42 producer policy
controls remain intact.

The complete Linux gate passed at `42b4f26` (1,628.221 s), including the expanded
260-observation smoke matrix and all parity/failure controls. A subsequent
quick gate passed at `fdaa790`; actual-firmware idle/runtime and fake-serial
checks passed on final firmware source `0bbec2e`. The later firmware edits
only move DEV trace formatting out of the producer's live frame. Matrix
measurements bind algorithm source `616e229`; the tracked
[manifest](../artifacts/newt27-host-measurements.json) hashes the retained raw
matrix, preparations and equality data in `/var/tmp/newt27-evidence/`.
Exact-source Mac ImageIO and all physical acceptance remain pending.

## Round 1 native resource proposal (historical)

Flash record 47 binds clean builds of firmware source `0bbec2e` and the four
frozen `artifacts/newt27-next-chapter-raw-code-health-*.json` reports. Its image
cost is explicit; it changes no partition, reserve, RAM, RTC, selected stack,
exclusion, adapter, or heap-min-free floor. Four final clean resource gates pass
with `dirty=false` identity `ea23b1c12be0aa56abbb5d3ebb678aaee1eeac99`
in every lane; committed image identities distinguish those builds from the
frozen cost reports. No firmware source changes follow `0bbec2e`.

| Lane | Image bytes | Growth | Shared SRAM | RTC |
| --- | ---: | ---: | ---: | ---: |
| default | 2,169,952 | 3,104 | 133,232 | 300 |
| DEV USB | 2,310,528 | 3,504 | 134,080 | 304 |
| regional | 2,176,672 | 3,120 | 133,728 | 300 |
| regional DEV USB | 2,319,232 | 3,504 | 134,576 | 304 |

All 351/380/353/387 inherited selected stack measurements remain present and
within their unchanged ceilings. The actual cache-query body remains inside
its protected `ChapterPageCache::show` entry point at 208 bytes. A separate bare
C3 comparison of base and candidate measures a 96-byte cache owner and 192-byte
query frame in both; that compile-only configuration is separate from the full
firmware measurement. Packed private flags keep the session owner at 1,952 bytes,
asserted for every C3 build and confirmed in the default and DEV ELF type data.
These are static resource facts, not task-stack peaks or hardware evidence.

Unselected call frames also matter. Every external turn now passes through the
48-B out-of-line `RevisionImportSession::show`; a cached boundary additionally
uses 48-B `showBoundary`, 32-B `showStaged` and 96-B `captureStaged` frames.
Library pending/admission/production frames change from 16/80/48 to 48/128/80 B.
These are individual compiler frames, with inlining-dependent lifetimes, not a
measured task peak. DEV `ReadingCacheIdle::attempt` is now 224 B, below its
368-B base and 448-B first candidate. Target/result formatting has separate
208/256-B frames that return before or start after the producer/parser call.
The unchanged 16 KiB reader task still needs hardware high-water evidence.

Quota admission can evict an old unleased current cache, costing future speed;
normal rendering remains authoritative. Since #196, next work no longer spends
the destination's three-try ledger (see "Long next chapters" below).

Fresh review round 1 (Opus 5.5 high, `f6fba84..42b4f26`) found no correctness,
data-loss, cancellation or resource blocker. Its three low items were the DEV
formatting frame, unreachable Running signal documentation and incomplete
measurement/resource notes. Trace isolation and the notes above address them.
Fresh round 2 (Opus 5.5 high, `f6fba84..a818c71`, exact target
`a818c71a4723c3d134989168c48cb6f01ef395af`) is **CLEAN**: all three items
resolved, no blocking or actionable findings. It independently verifies the
record/report/receipt hashes, all preserved histories and resource bounds, and
accepts the measured flash cost as an engineering trade. No firmware, tool or
test file changes follow that review; this close-out records its result only.
Full review texts and software/native logs remain in `/var/tmp/newt27-evidence/`.
This is not owner, ImageIO, device-latency, battery or total-stack acceptance.


## Round 2: device findings and host attribution

The Lead measured `6ddbb82` over USB: next preparation completed and a cached
forward crossing had no chapter layout, taking 1,121–1,173 ms against a cold
1,567 ms crossing. Two runs each had one 3.2–3.3 s press, with about 2–2.5 s
before the action. A matching 43-press run on main-equivalent `116bb1a` had
no press above 2 s (maximum 1.41 s). This strengthens suspicion of prefetch;
1/43 versus 0/43 does not prove causality. These are the Lead's observations
on the earlier candidate, not acceptance evidence for this revised source.

**Repeated slow cached turns: confirmed on host.** Admission attached a producer
and closed the current validated Pages descriptor to probe the current cache,
then closed it again before census. A stopped census left the next ordinary
turn to re-read the whole NTCC for CRC. The actual idle/selection/session/storage
regression crosses into a cached chapter and compares the exact same page before
and after interruption. Its stored EPUB reads 2,197 bytes before and after each
of three key interruptions; its deflated EPUB reads 2,253 bytes before and after
three USB-byte interruptions, with identical frames. On `6ddbb82`, the stored
case reads 14,370 bytes after interruption: 12,173 extra bytes, exactly the
complete destination cache. The regression also checks exclusive query stages.

Read-only probes now reuse the same validated descriptor, with checked entry
and exit freshness. A missing destination's canonical and recovery names are
probed without opening or disturbing it; a present other name still requires
full CRC validation. Census counts the leased file's bytes but excludes that
exact generic cache name from eviction. No incomplete census or provisional
credit is retained. A granted admission still checks Pages close success before
its writer; durable state retains its unconditional `releasePagesForState`.
The acquired FAT provider allocates directory state separately from its regular
file-slot table; synchronous census closes its DIR before any writer/save.
Its separate 596-byte DIR allocation already existed. No fourth regular file
slot is needed. Descriptor/transaction and quota/cancellation fault gates remain
required; provider heap peaks still require device measurement.

Interrupted admission now spends one of the existing target's three tries,
including input arriving just after grant. Queued input before admission starts
spends none. Thus driver HELLO cannot restart a census after every quiet turn
for the whole session. Removing this accounting fails the no-more-I/O control.
This may exhaust optional preparation before a reader pauses longer; ordinary
rendering remains authoritative. Main's census after every quiet turn while
the current chapter is **uncached** is pre-existing, as the Lead confirmed.

**Rare pre-action gap: not causally reproduced.** New admission probes could do
an untraced full-file CRC before the first PC row; they are now avoided for the
retained current/missing-next case. Due durable progress saves and their Pages
release are pre-existing synchronous loop work outside an active action.
USB consumption acknowledgement is another pre-existing pre-action wait; its
1000 ms check happens after the driver returns, so it cannot bound a stalled
driver call. Producer completion and cleanup return synchronously before the
PC attempt row; there is no deferred cleanup 50 seconds later. The host cannot
identify which operation caused the observed device gap.

DEV now emits `BG slow: work=NAME ms=N` only when `N > 200`, after a synchronous
owner returns. Names are `pc-current`, `pc-target`, `pc-census`, `pc-receipt`,
`pc-admission`, `reading-idle`, `library-idle`, `producer-duty`, `progress-save`,
`battery` and `input-ack`. Nested rows identify an inner phase and enclosing
owner; their milliseconds must not be added. Rows use fixed formatting, an
80-byte cap and nonblocking TX admission. Full TX drops a row; missing telemetry
cannot prove no slow work. Default firmware has no background trace timers or
retained trace state. Foreground turns acquire no background timer; input-ack
uses its existing clock reads. Host checks cover 199/200/201 ms, wraparound,
void/value results, full TX and a slow actual acknowledgement driver.

**Cached crossing residual: confirmed attribution gap, validation retained.**
`RevisionImportSession::show` probes `ChapterPageCache::stored` before the old
query timer. The first probe validates the complete NTCC. Its new DEV query
scope moves that work into v[6]; the removal control restores 12,173 read bytes
outside query, against zero with the scope. This changes attribution, not
latency. First-open cost is payload plus a 12-byte footer, with
`ceil(payload/4096)+1` read calls and checked seek/stat work, before the page
query. Payload is capped at 4 MiB; unchanged read-authority limits may refuse a
large first hit and fall back. Removing CRC would weaken integrity; issue #63
remains separate. Metadata sharing and periodic durable saves can still displace
a read lease and cause a later first-open CRC, like main's periodic slow query.

For the next device run, repeat the 43-press/three-second-silence pattern and
capture the BG rows before any slow press. Compare the cached chapter's steady
v[6] after stopped admissions with the previous cached chapter, and confirm
census ceases after three interrupted admissions. Measure boundary total and
query separately: residual should now contain no first-hint CRC, while query
includes it. Also press during next production and census, exercise a due
progress save, and record the reader task's stack high-water and heap minimum.
All original power-condition checks remain distinct and pending for this source.


Review of `f6fba84..dc7692f` (fresh Opus 5.5 high) found no blocker and accepted
those exact preliminary costs. It requested consistent trace-contract wording
and explicit round-two unselected-frame notes. Idle rows intentionally include
any repaint their owner requests; the battery and progress-save scopes now
cover those complete background branches too. A row identifies its synchronous
owner, not a separate display/core duration. Foreground dispatch and page turns
remain outside these background timers. This telemetry correction is remeasured
before the final audit record and receives a bounded follow-up review.

Round-two preliminary unselected frames matter despite unchanged selected
actuals: DEV admission grows 128→208 B, idle admission 128→144 B and stored-cache
probe 208→240 B; new timing wrappers add 32–48 B. Default admission grows
128→144 B, census 1,232→1,248 B and the stored-cache probe 208→240 B. The new
missing-name probe is 416 B and retained-page probe 48 B in every lane. DEV
census-path prefix growth is about 208 B before children. During a producer
pause, a slow duty also puts the 32-B timing destructor and 208-B row formatter
under the live parser. These are individual compiler frames and partial sums,
not task peaks. The Lead must record task high-water and heap minimum during a
slow progress save while the build is paused. The final telemetry-scope frames
and exact image sizes are recorded with the integrated audit below.

## Round-two integrated audit (historical, before clock integration)

Main `5493f5da72e5763264e5fbe2550ffa832d5e5f58` is integrated with both
parents preserved. At that checkpoint, branch record 48 was the sole NEWT-27 flash record after
main records 1–47. The clock integration below supersedes that current-chain
record with record 49 after main’s accepted clock record 48. The four clean measurements of firmware `9cd7315` are:

| Variant | Flash B | Growth B | SRAM B | RTC B | Selected stacks |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 2,172,656 | 4,304 | 133,232 | 300 | 351 |
| DEV | 2,315,184 | 6,672 | 134,080 | 304 | 380 |
| regional | 2,179,376 | 4,320 | 133,728 | 300 | 353 |
| regional DEV | 2,323,888 | 6,688 | 134,576 | 304 | 387 |

All selected actuals equal main; no memory ceiling, floor, reserve, exclusion,
adapter or protected history changes. The unselected frames above are unchanged
by the final scope correction. Whole-task peaks remain unmeasured.

Fresh Opus 5.5 high final review through `f9530c6` is CLEAN and accepts these
exact costs. It confirms both earlier low findings are resolved. Full text is
in `artifacts/newt27-r2-review-r2.md`; it performed read-only review, not builds
or device verification.

The round-two full matrix has 3,744 observations, 1,248 cells and 24 authored
fixtures. All 144 eligible prefetched turns hit; 144 over-cap or unsupported
cases safely refuse. Host eligible median is 1.629 ms versus paired cold
2.958 ms; next-build median is 9.394 ms (4.097–79.426 ms), excluding settling
and census. The 118-page boundary subset has 18 observations: median 2.000 ms
cached versus 16.613 ms cold, with zero layout, expansion or cache writes.
Every-page parity covers 13,408 comparisons across 18 fixtures. The host
manifest and raw-evidence hashes are in `artifacts/newt27-r2-host-measurements.json`.
These runs precede the final main-only timer correction, which their compile
does not include. Host times do not establish device latency.

The focused regressions and Linux quick gate pass, including the former
`6ddbb82` lease failure and removal controls. The quick invocation ran at
`f9530c6`, before record 48 and final receipts. The complete Linux gate covers
final test candidate `7822cce`: it passes in 1,171.309 seconds, including full
18-fixture parity, all portable renderer frames and the corrected queue fixture.
All four clean passing receipts, resource adoption against `5493f5d` and the
pinned full candidate history pass. Physical, battery, cold-boot and Mac ImageIO
acceptance remain pending for this exact source.

Final clean builds of `b50e3cd` pass all four native health gates at exactly the
table's sizes. Each final identity is clean and cites that compiled revision;
firmware inputs equal the reviewed raw measurements. All four `--adopt` checks
against `5493f5d` pass. The separately hash-bound final receipts and preserved
memory/history checks are summarized in
`artifacts/newt27-r2-final-resource-audit.json`.

The first keep-going full Linux invocation exposed one additional extracted
queue fixture with the old acknowledgement signature. Its seam now accepts the
trace callback and compiles it against the real bounded background trace helper;
the focused BookOpenScope/queue regression passes. No firmware changed. The
failed invocation was stopped after the other reported phases completed, with
only parity still running; separate full parity already passed. Its log is
preserved as `r2-full-linux-first.log`. The fresh full invocation passes at
`7822cce`, including every-page parity and the corrected fixture.


Fresh Opus 5.5 high close-out review of `f9530c6..7822cce` is CLEAN. It
verifies exact record/proposal equality except the requested reviewer update,
all preserved histories and memory values, clean receipts, source identity,
gate hashes and the queue seam. Its three bookkeeping requests are resolved
above. See `artifacts/newt27-r2-review-r3.md` and the command/source/log-hash
manifest `artifacts/newt27-r2-gates.json`. Later close-out changes are only
documentation and review/evidence artifacts; compiled firmware remains
`b50e3cd`, with unchanged firmware inputs since `9cd7315`. The final pinned
history and all four adoption checks are rechecked after that close-out commit.
No device has been accessed or flashed.

## Clock integration onto main 7c6dec6

Merge `667a41abe7142aacdd9de7c452fb15555b6e0ed1` retains both `8a4e9e3` and
`7c6dec6`. Conflicts covered 12 generated receipts, the resource baseline,
history pin and appended health documentation. Main's receipts were retained
as provisional evidence, then replaced by fresh clean builds at close-out.
Main's complete resource history is preserved: flash record 48 belongs to the
Home clock, protected entry 26 is unchanged, and NEWT-27 now has sole current
flash record 49. No new protected entry is added.

The automatic `main.cpp` merge retains clock boot initialization after device
info, clock publication/cleanup around Home painting, and all NEWT-27 background
owner timers and input acknowledgement. The status-only battery repaint retains
the shown time without a clock read. `gate_plan.py` retains clock and both
next-chapter registrations. Fresh Opus 5.5 high merge-resolution review is CLEAN
and accepts the exact costs; full verdict is
`artifacts/newt27-integration-review-r1.md`.

| Variant | Flash B | Growth above main B | SRAM B | RTC before→after B |
| --- | ---: | ---: | ---: | ---: |
| default | 2,173,904 | 4,320 | 133,232 | 300→300 |
| DEV | 2,316,624 | 6,672 | 134,064 | 304→304 |
| regional | 2,180,576 | 4,304 | 133,728 | 300→300 |
| regional DEV | 2,325,312 | 6,688 | 134,560 | 304→304 |

Every inherited selected stack actual matches main. All memory ceilings,
selected entries, tighter values, floors and exclusions remain unchanged. Record 49
cites clean firmware merge `667a41a`; the archived raw reports match its input
hashes. Final passing clean receipts cite `b7f7c1b`, with those same firmware
inputs after the reviewed pin/record commit. Focused clock/cache/power
tests and quick Linux pass at `667a41a`. Full matrix passes at `b7f7c1b`; full
Linux passes at `04ad323` in 899.234 seconds. All four post-full adoption checks
against `7c6dec6` and pinned full candidate history pass. No change addresses #89 or post-save CRC #63. No device is accessed;
physical and exact-source Mac ImageIO acceptance remain separate.

Four final clean `--health-report` builds of
`b7f7c1b3959a0e6cec64ca14e994fed2689a6856` pass at the exact table values.
All identities are clean, with the reviewed firmware inputs. All four
`--adopt 7c6dec6` checks and the pinned full candidate-history gate pass.
The audited preservation of 48 main flash records, 26 protected entries and
all other baseline fields is recorded in
`artifacts/newt27-integration-final-resource-audit.json`. The complete turn
matrix passes 3,744 observations / 24 fixtures / 1,248 cells; its raw hashes are
in `artifacts/newt27-integration-host-measurements.json`. The complete Linux
invocation on receipt commit `04ad323` passes in 899.234 seconds, including
13,408 every-page comparisons across 18 fixtures. The Linux portable profile
also passes all 20 reviewed renderer frames; exact-source Mac ImageIO decode
and physical acceptance remain separate.


Fresh Opus 5.5 high closing merge-resolution review of `82698ef` is CLEAN.
It verifies the source union is unchanged from `667a41a`, every main record and
protected entry is preserved, only the reviewed flash ceilings change, all
receipt identities and selected memory measurements agree, and all 23 recorded
passing invocation hashes plus five host data hashes match. See
`artifacts/newt27-integration-review-r2.md`. The final close-out commit changes
only documentation and this archived review. Final-HEAD pinned full history
and all four adoption checks are rechecked afterward. No device is accessed;
physical and exact-source Mac ImageIO acceptance remain separate.

## Long next chapters (#196)

Before, prefetch wrote only whole chapters of at most 128 pages. On the page
before a longer chapter it parsed for about 3.7 s on the bench, returned
`Unsupported`, and that terminal outcome spent the destination's three-try
ledger. Entering the long chapter then cost an ordinary parse (5.5-6.8 s), and
admission denied its own production for the rest of the visit, so every turn
inside it stayed an ordinary parse.

Now, generic for any book:

- Prefetch counts the whole next chapter (up to the map's 4096 pages) and
  notes each segment's first character. Over 128 pages, its write pass stops
  after the first segment, certifies that the next segment starts where the
  counting pass saw it, and writes the chapter's map last. A crossing reads
  the first segment through the map, as #192 serves anchored reads.
- Admission for an unshown next chapter probes its whole-file name, then its
  map name, then the first segment that map names: both present mean a
  prefetched long chapter (Complete). The census evicts files one at a time,
  so a map can outlive its first segment; a missing segment or an unreadable
  map is repaired by the same prefetch, which rewrites both. Any prefetch
  reserves two census entries (a first segment and the map) and keeps the
  names it knows, as #193 accounts segments. An under-cap next chapter may
  free one entry it does not use.
- A prefetch never spends the next chapter's own ledger. Its outcome ends
  prefetch for the view; interruptions count in two spare bits of the view
  verdict, three per view. Once shown, the chapter still has its own three
  tries to produce what prefetch could not.
- A chapter shown from the cache publishes neither its selection nor settled
  styles. Current production now settles them itself in the inactive
  selection (as prefetch does), certifies the published totals again, writes
  the missing segments from that selection, and drops the styles afterwards,
  so no ordinary layout reuses them with the stale published selection. The
  #192 rule that denied such a chapter's admission is removed.
- A chapter over 4096 pages stays uncacheable: its prefetch is one counted
  parse per view that ends `Unsupported`.

