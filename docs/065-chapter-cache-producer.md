# Current-chapter cache production

Owner decision: build caches inside NEWT-8. Fresh review of `de5cade` selects
**D + hardened A**: a capped display-tail writer in one synchronous, abortable
idle attempt. No resident parser job or parser-heap peak increase (step 5 below adds a fixed
256-byte session ledger). This
supersedes the earlier feasibility stop; it does not approve resource growth,
format/identity changes or changes to ordinary reading. Implement S1–S5 below,
with a Lead commit and fresh review between slices. No device proof yet.
Design references use `de5cade`; S3 entry/storage references use the working
revision after `7cfcb26`. `SDK/` means
`vendor/freeink-sdk/libs/book/FreeInkBook/`.

## Trigger, outcome and limits

Consider the current external chapter after accepted open/reopen, crossing or jump.
Start only in Reading/Page after **2 s quiet**, no pending input or DEV byte,
eligible battery and no due foreground duties. Owner decision (2026-10-20,
NEWT-21 step 5): **at most three producer attempts per chapter per Reading
visit**. Input/DEV interruption discards the provisional output and retains the
chapter's remaining tries, with a new full quiet interval. Completion and every
terminal refusal exhaust that chapter immediately. Style discovery still aborts
without a style retry. Admission interrupted before a producer starts spends no try.
Each uncached chapter runs a fresh idle census; no provisional allowance survives
an attempt. Turns, jumps and Chapters selection rearm through the existing idle
observation of the current spine, including return to an exhausted chapter.
An actual Reading reopen (including settings reflow or wake) starts a new visit;
merely opening a panel or changing the displayed generation does not.

A verified complete cache makes later same-chapter turns hits. No next-chapter
prefetch, no first-crossing optimization (b), no lease/index amortization (c),
and no capacity expansion (a) in these slices. Visits without a compatible file
and first crossings fall back;
even an existing destination file cannot bypass today's `ready(spine)` gate
(`firmware/chapter_page_cache.h:302`). **128 pages maximum per file**; page 129
of one file aborts it, never publishes a partial. Since #175 a chapter above
that cap is cached as 128-page segments (see "Chapters over 128 pages" below);
`produceNext` still refuses them. Recheck the cap against actual output too.
The decision is generic, never book identity.

The build costs one complete chapter parse/layout plus serialization and full
candidate validation: O(compressed bytes + expanded bytes + layout work + output
bytes). A style-policy retry is not part of a producer attempt. Round-1 full
incoming crossings are a full-parse proxy, not producer timing:

| Portrait/plain/stored | Full parse: bytes / emitted runs | Middle/Next fallback: bytes / runs | Completed prepared hit: bytes / runs |
| --- | ---: | ---: | ---: |
| 4 KiB | 4,096 / 522 | 4,096 / 447 | 0 / 0 |
| 64 KiB | 65,536 / 8,911 | 34,816 / 4,692 | 0 / 0 |
| 256 KiB | 262,144 / 35,696 | 133,120 / 18,057 | **not produced: 473 pages** |

First/Next fallback uses 2 KiB/224 runs at each size. Last/Previous uses the
whole 4/64/256 KiB and 447/8,835/35,660 runs. Eligible hits remove parsing/layout,
not source/receipt checks, cache CRC/index/page reads or display work. The authored
4/64-KiB shapes fit 128 pages; all authored 256-KiB/1-MiB middle chapters exceed
it. Their flanking chapters remain eligible. Do not report the earlier unlimited
prepared 256-KiB hit as a producer benefit. Completion begins no earlier than
2 s quiet and requires the complete work; there is no SD wall-time guarantee.

## Chapters over 128 pages (#175)

One file holds at most one SDK writer index chunk, 128 pages: the display
tail fits exactly that index. A longer chapter whose totals the current visit
has certified (an ordinary parse of it) is cached as consecutive files of 128
pages (the last may be shorter). Each is an ordinary canonical page file; its
name hashes the whole chapter's name with the segment index and the certified
chapter page count and character total. Names of chapters at or under the cap
are unchanged, so existing caches stay valid.

Production is one parse of the chapter that writes every missing segment in
order. A segment is finished and published (temp, then rename) between parser
steps, never inside a parser callback. One 2 KiB feed can emit several pages,
so a segment whose first page went by while its predecessor was finishing is
skipped; a second pass parses the prefix without writing and fills the skipped
ones (never adjacent, so two passes always suffice). Published segments stay
after a cancel or fault; the next attempt skips every verified one (`exists`
is full CRC verification on the card) and writes only what is missing. The
final segment is published only after the pass certifies the chapter totals.
One census grant covers all segments: the producer spends its credit as it
writes (payload and envelope bytes) instead of reserving 4 MiB per file, and
a write beyond the grant fails.

Reading: a non-anchor page of the current chapter with certified totals maps
to segment `page / 128`, opens and verifies it exactly as a whole file, then
requires its stored character total to equal the certified one and its page
count to equal `min(128, pages - 128k)`. Any missing, corrupt, stale or
mismatched segment falls back to the ordinary parse for that page only; other
segments still hit. A whole-chapter file (an import build) is still tried
after a segment miss.

Anchors (#192): an open, wake, Chapters jump or crossing has a character, not
a page, and no certified totals, so it cannot name a segment. The producer
therefore also writes one small map per segmented chapter (`ChapterSegmentMap`,
at most 156 bytes plus the card envelope). Its name hashes the chapter's name
with a reserved index, so it binds the same digest, settings and layout
generation. It holds the chapter's page count and character total and each
segment's first character, with its own check. An anchor that misses the
whole-chapter file reads the map, picks the last segment starting at or before
the anchor, and reads that segment only if its name (which binds the map's
totals) exists and it agrees with the map: the same totals and page count, and
its first page starting where the map says. The map cannot vouch for where a
segment ends, so an anchor on a non-final segment's last page parses (1 in 128
anchors). With that rule any wrong map gives a refusal or the exact page.
A cold open holds no page file to the 128 KiB opening cap, which bounds only
full-CRC records: a checked query reads the file's directory, index and
consumed blocks, never the whole file. Segments of prose exceed that cap (a
host segment holds 158 KB), so with it every cold open parsed (#192). So do
whole files of dense prose from about 87 pages: until NEWT-87 the cap still
held them, and their cold opens and wakes hashed the book and parsed.
An open served from a segment settles no style slot and publishes no
selection. Since #196 the producer settles both itself in the inactive
selection, certifies the published totals, writes the missing segments and
drops those styles again (docs/067, "Long next chapters").
Serving publishes the certified totals, so later turns read segments directly.
A missing map is written by the next attempt alone (one parse, no segment
rewritten); this covers caches from before the map. Admission counts the map
as one more file of a segmented chapter's need and never evicts it for that
chapter. Crossing hints and cache-only opens treat a map as a stored chapter.

Card: each segment is an inventory entry and an eviction candidate of its own;
temps left by power loss are reclaimed by the census like any page temp. A
chapter of P pages uses ceil(P/128) of the 128 inventory entries. The census
keeps 8 of them free. Admission tells it how many new files the attempt
writes: the current chapter's missing segments, at most a batch of 4, else
one. While the count plus that need is over 120 it reclaims derived records
left in `Pages/` from before #179 (oldest first), then the oldest pages, but
never one of the chapter's own segments (#191). The batch bounds what the
census frees for an attempt that stops early, for example at the attempt
limit. It grants at most 120 minus the count as new producer files, which
storage spends one per write (temp included). An attempt that runs out (after
its batch on a crowded card, or when only pinned pages or backups remain)
stops; its published segments stay and the next census grants again. So segments can
never fill the directory past 128 names, where a folder of pages alone grants
nothing and the census evicts no page. On the
invented host fixtures a page costs 0.65-1.24 KB by content, so a full segment
is 83-158 KB and the measured 640-page chapters are 0.54-0.79 MB. A layout generation
writes each chapter's bytes once: a resumed attempt rewrites no published
segment.

Host tests (`chapter_cache_producer_test.py`) cut power in a child process
during a segment write, just before and just after its rename, and between
segments. A fresh process that keeps only the card's files serves only the
complete segments, the census reclaims the temp, and production resumes with
only the missing segments; a control that skips temp reclaim fails. A
test-only type-setting build (36 px instead of 30 px; Newt has no type
setting) moves page counts and segment edges: the old segment set misses on
every page and is rebuilt under the new layout's names, and the
`reader_turn_cost_test.py --segment-curves` retyped rows hit at p01/p25/p50/p90
and the last page.

## Bounded abort, not a resident pump

Retain the existing loop and scheduler cadence: about 5 ms work followed by the
existing 1 ms wait (`firmware/reader_yield.h:9`, `reader_scheduler.h:70`). A scoped
producer latch polls before/after every source/decoded-chunk boundary, metadata
operation, transfer/retry, index batch, CRC chunk and promotion/cleanup boundary.
Poll again after a scheduler wait. Classify returned failures before any later
poll; lower layers carry an already observed cancellation cause into the same
attempt latch. EINTR and short transfers use the existing bounded retry policy.
No foreground dispatch or source close inside callbacks; unwind first, then let
the next loop iteration consume input. No chapter-completion wait after a press.

The attempt can remain on the UI-task stack while quiet. Source/decoded units
are bounded; a no-page chapter still polls every decoded 2 KiB through
`ChapterSource` (`src/epub_admission.cpp:204`). SDK `step(1)` alone is not the
bound (`SDK/src/layout/ChapterLayout.cpp:1939`). Index/CRC writes also call the
same latch/cadence, not just the parsing stage. S3 adds these boundaries inside synchronous storage finalization; S4 wiring
remains separate.

**HIGH 1 — parser processing, including canonical metadata.** Patch the pinned
SDK only through `tools/apply-freeink-patch.py` with source/patch/result hashes
and MIT provenance. Producer sources opt into the parser policy; ordinary
sources do not. Before expansion, reject internal DTD subsets (including OPF,
container and encryption metadata); refuse external entity callbacks and tokens
that can retain more than 4 KiB including the next feed. This conservatively
rejects some valid producer inputs, which still read ordinarily. Forward the
policy through scheduled and decoded sources so chapter and canonical parsing
cannot escape it. No additional global/parser state or heap allocation.
Ordinary reader frames, anchors and behavior must match the pinned baseline.

The deterministic press bound is one in-flight source/authentication unit, or
one <=2-KiB XML feed with <=4-KiB retained-input processing, plus bounded
paragraph/page/EOF work and abort. SMALL paragraph text is 3,072 B, 176 lines,
304 page runs; HTML entity buffers are fixed. Include authenticated-block work
and XML/filter callbacks in the measured bound; do not call 2 KiB a complete
bound by itself. S2/S3 certify numeric maxima for metadata, index batches and
abort syscalls, including retries. A FAT syscall/fsync may block in real time;
only device evidence can bound observed milliseconds. Fail the work-bound tests
if a boundary disappears; wall-clock time never decides a host pass.

**MEDIUM 6 — cancellation is not source corruption.** Use a scoped latch above
authenticated reads, distinct from `BookOpenControl` (which is inactive outside
open). Keep its borrowed policy/reference until all parsers unwind. Once latched,
no further work, `settleStyles()`/style retry, `finish()` or `suspend()` is allowed.
Do not invalidate or rebind the live revision to signal cancellation. Genuine
source/receipt failure retains its current fail-closed behavior.

### Cancellation site inventory (S2 review fix and S3)

The review of `de5cade..adfb825` reproduced 16 decoded reads after cancellation
inside one buffered inflater/filter operation. Raw-source and XML-feed checks
did not cover that boundary. `ChapterSource::readAt` now checks the optional
policy before reading and after its yield; scheduled raw reads and the decoded
EOF probe have the same two-sided contract (`src/epub_admission.cpp:191`).
With no policy the ordinary result is unchanged. S3's SDK preprocessed these
checks away; S4 enables support in every lane, with no policy on ordinary reads.

| Reachable site | Poll / deterministic unit | Evidence or remaining obligation |
| --- | --- | --- |
| Producer raw reads | Latch before/after source call; at most 2 KiB plus the admitted source's authentication unit | Poll matrix `raw`; S1 authenticated-source cancellation tests |
| Scheduled source yield | Policy before read and after yield | `scheduled`, `reader-wait` |
| Decoded read/yield, including buffered inflater output | Policy before each read and after yield; at most one 2-KiB decoded chunk | `decoded`; compiled omission control fails |
| Latch/scheduler wait | Stop before wait and again after it | `scheduler-wait`; S1 cadence/latch controls |
| XML open/feed/filter and start/end/text callbacks | Policy at feed boundaries; DTD/token limits bound processing. A callback can finish its in-flight feed, not start another | `xml-feed`, `xml-callback`; S1 parser-policy tests. This is a feed bound, not one callback |
| Page/anchor callbacks and layout step | Latch in capped writer and after `step(1)` | `page`, `anchor`, `layout-step` |
| Decoded EOF and style handoff | Policy around EOF read; no style passes/retries; one bounded return of the lent settled slot during unwind | `eof`, `style-return`; S2 styled ordinary-turn checks |
| Initial/final source validation | Latch before/after validation, above source invalidation | `validation`; S2 initial/final zero-read cancellation controls |
| Writer begin/blob writes, index/anchor records, footer | Store wrapper before/after every SDK storage operation; index/anchor record is 8 B | `writer-begin`, `writer-write`, `index`, `footer` |
| Abort | Owned provisional cleanup still completes after latch | `abort`; S2 continuous lease/abort tests |
| SD transfer/short-I/O/EINTR retry; metadata, CRC/readback validation | Scoped page-port policy before/after open, read, write, seek, stat/fstat, mkdir, truncate and sync; <=1-KiB CRC chunks, preserved result/errno | S3 table: open, fstat, stat, seek, mkdir, read/write, read/write-short, read/write-EINTR, sync, crc; compiled EINTR and CRC omission controls |
| SD promotion/rollback/lost acknowledgement and cleanup | Poll around rename/close/unlink; record successful ownership even if post-poll stops; mandatory owned cleanup still runs | S3 close/rename/unlink rows, strict FAT replacement, existing/late backup refusal, lost-ack old/new validation and shared-receipt isolation |
| Idle-only directory census, temporary cleanup, quota/eviction | Stop before/after each folder read, at each entry and around each unlink; the folder read always closes; bounded scans; no directory in the producer | S3 open/next/stat/unlink/close rows, interrupted census at every step and convergence; actual idle/entry zero-work regression |

`chapter_cache_producer_poll_test.py` runs one 19-row table through the actual
producer and generated observations of the actual source/SDK units. It requests
cancellation at each implemented site, permits at most one in-flight unit,
requires no commit or new finish entry after the request, checks exact frame/plan/public state and a still-valid source,
then repaints. The deflated unknown-entity fixture exercises repeated decoded
reads without raw input. Separate compiled controls remove the pre-read check,
the post-yield check and both: entry cancellation must do zero decoded-read
work, and yield cancellation must deliver zero bytes toward the filter. All
three fail their own work bounds. No production probes or SDK provenance change.
The separate S3 storage table exercises real POSIX transfers and injected faults,
with complete old/new NTCC validation and a still-readable receipt/source route.
A post-promotion cancellation may leave a complete committed new file; it must
never start another mutation or discard that final. Mandatory close/unlink of
owned provisional state is the explicit exception to stopping all work.

## One continuous display loan; frame and plan remain live

**HIGH 2 — preserve logical pixels.** Never call foreground
`publishLendingFrame()` or lend logicalFrame as receipt scratch from the
producer (`firmware/frame_adapter.h:20`). It must use the receipt owner's existing
256-byte scratch. The accepted logical frame, plan, selection, page/anchor and
tracker remain intact even if cancellation is followed by a failed foreground
repaint or rejected Home/sleep transition. Reconstructing the plan is rejected:
that would require parsing and could fail.

**MEDIUM 3 — retain the tail through finish/abort.** X3 display allocation is
52,272 B; parser/shared arena demands remain 49,152 B each. Aligning the parse
base leaves at least **3,105 B** beyond its 49,152 B, versus **2,564 B on C3**
for 192 anchors plus one 128-page writer-index chunk. Acquire ONE lease covering
parse and tail before writer initialization, and release only after writer,
XML, inflater and index pointers die on finish or abort. Check current raw
pointer, alignment, capacity and non-overlap before touching bytes. Never use
receipt selection's tail scratch concurrently. The old `streamChapter()` returns
the lease before `writer.finish()`; do not reuse that lifetime
(`firmware/chapter_page_cache.h:256`, SDK display `returnBuildStorage():447`).

The published 9,616-byte plan and logical frame are excluded from every loan.
Parser/layout/style buffers reuse the existing arenas; bounded writer scratch
uses the existing tail, and control/latch state lives only on the attempt's stack.
No resident context, new buffer, task or larger Expat budget. Releasing the loan
clears display staging; it does not touch logical pixels or the plan.
**Static RAM/heap-capacity delta 0 and selected-stack delta 0 are acceptance
requirements**, proven per slice by four clean health builds. Do not rename or
re-sign guarded symbols (`build`, `streamChapter`, `PhasedReader::layout`, `loop`).
Any measured increase stops the slice; no ceiling/adapter/exclusion expansion.

## Storage, retry footprint and namespace

Keep FIBP/NTCC format and full source/font/SDK/settings/orientation/style identity
(`firmware/chapter_page_cache.h:309`). Full CRC/index validation and old-or-new
atomic promotion remain required. Never publish partial/suspended files. A lost
post-promotion acknowledgement may leave a complete new file: reconcile by
bounded checked readback or independently validate old/new later, never declare
it definitely unsaved or delete a complete final as a failed temporary.

**MEDIUM 4 — match actual names.** In shared `Pages/`, page canonical basename is
`<64-hex book-route>-<64-hex chapter-key>.fibp`; siblings are `.bak` and
`.<8-hex nonce>.tmp` (`firmware/sd_cache_file_port.h:67`, `:146`). Strictly match
lowercase hex, full extents and regular files, never links. Cold boot ignores
provisionals for reads. The bounded idle census below reclaims all recognized
page temporaries within its cap, including all 32 O_EXCL collision slots.
Preserve every `.bak`, including lone recovery copies. The reading-time producer
never enumerates a directory or explicitly unlinks a pre-existing name.

**MEDIUM 5 — mixed namespace quota.** Account every physical name exactly once,
including page canonicals, backups and temporaries, and semantic metadata and
progress-estimate records and their siblings (`firmware/reader_semantic_metadata.h:19`,
`firmware/progress_estimate_record.h:15`). Those non-page records are protected
from eviction/cleanup. Since #179 they live in `Meta/`, not `Pages/`; see
"Derived records beside Pages/" below. Unknown entries/overflow/stat errors prevent production;
never silently omit their footprint. Bound totals with checked uint64 arithmetic,
reserve the maximum candidate before creating it and invalidate inventory after
any write, promotion, cleanup or eviction. No persistent ledger/new authority.
Quota is 32 MiB managed logical bytes; existing excess disables generation until
bounded cleanup/eviction can establish space. Evict oldest unleased page
canonicals (name tie-break), protecting active/transaction/recovery dependencies.
Logical quota is not a FAT-sector or free-space guarantee.

### S3 idle census and boot allowance

The amended Lead decision prioritizes fast opening: run inventory only in
settled Home/library idle, composed with the cover owner. Choose **(a)**: without
a valid allowance this boot, decline production for the visit. Cold resume never
runs a census, cleanup or eviction. The common **and worst open-path delta is
zero directory entries, directory sectors, path lookups, stats, unlinks and heap
allocations**. New session creation adds only bounded scalar allowance handoff;
existing warm owner reuse stays intact. Existing admission/receipt/layout work
is unchanged. This trades optional cache coverage for predictable opening.

`serviceIdleLibraryWork` calls the unchanged guarded cover service first
(`firmware/library_cover_producer.h:170`); main invokes it after input and
housekeeping (`firmware/main.cpp:685`). It
runs the census only in Home/Library after the same 2-s quiet gate, with no input,
cover job/resume, reader progress/workspace or retained revision session
(`firmware/page_cache_idle_budget.h:13`, `:19`; `SampleReader::hasProgress`
tests actual workspace ownership at `src/sample_reader.cpp:398`). It does
not discard a warm reader to create an idle opportunity. Browsing has already
closed the reader/source; its catalog may remain. The census DIR closes before
the next owner can run. The acquired FAT `opendir` heap is 596 B, below the browser census's
1,877-B DIR/readdir heap (plus catalog); browser DIR lifetime is
`firmware/library_directory.cpp:8`/`:34`. This is a **heap-only** comparison:
the caller-owned 260-B `readdir_r` entry and census object occupy task stack,
accounted separately below. No directory allocation overlaps a reader/session
or a cover job/resume. Main's open path stays free of
census calls. No framework, mount ownership or SD downcast changes.

Historical S3 policy (replaced by Reading admission in step 4 and the
per-chapter retry policy in step 5): seven spare selection flag bits hold 0 (unattempted), 1 (denied/consumed), or
65..127 conservative 64-KiB credit units. One attempt per idle opportunity spends
the flag even on stop/error; no retry every loop tick. Known card loss, failed
storage identity, card return and reset invalidate credit. A new Library scan
reopens one idle opportunity (`firmware/library_reader.h:197`). Normal selection
carries it into the next session, then marks it consumed (`firmware/library_reader.h:460`,
`firmware/page_cache_idle_budget.h:28`). Boot/cold identity
restore starts without credit (`firmware/library_reader.h:220`); no RTC continuation. A retained live session
prevents another census, so its own page port remains the only write authority.
New metadata/progress writes use that port and debit its credit conservatively.
An unseen external filesystem writer is outside the existing single-owner mount
contract. The existing undetected closed-handle card-swap limitation also applies:
without a card identity, a swap never observed missing can carry stale credit
(`docs/056-reader-reliability.md:849`). Known loss/return invalidates it; this
slice does not change mount ownership. Quota is not source authority or a FAT
free-space guarantee.

`PageName` recognizes exact lowercase full page, semantic and progress names,
including canonical, `.bak` and eight-hex temporary siblings. Accounting counts
each physical name's bytes once with checked uint64 totals. All semantic/progress
siblings and page backups are protected. Unknown names, nonregular files,
errors, overflow or a 129th name deny credit. Select page temporaries first, then
the oldest eligible canonical (mtime/name). Close DIR, recheck the chosen file
and any backup dependency, unlink one, fully rescan. All backups, including lone
recovery copies, remain available to ordinary validated reads. Interruptions are
repeat-safe; cold leftovers wait for the next eligible Home/library idle.

Each inventory has a bounded name-only dependency pass and one accounting pass
on the same DIR, with a polled rewind between them. A 256-bit conservative backup
membership filter removes the former per-canonical backup stat: collisions can
only protect extra files and decline eviction. A selected canonical's dependency
is checked again before unlink. Cap: 128 names per pass, 128 mutations and 129
inventories; stop polls surround open/next/rewind/stat/unlink/mandatory close.
There is no resident inventory or reading-time directory scan. The authored
32-temporary case takes 33 inventories/66 passes, 1,254 next calls, 593 stats and
32 unlinks, reclaiming all 32 O_EXCL collision slots.

**Directory cost and evidence limits.** The acquired ESP FAT `dirent` has only
inode/type/name (`newlib/platform_include/sys/dirent.h:37`); no size or physical
offset. Archive disassembly confirms `telldir` counts returned names, not FAT
slots. The pinned FAT archive SHA256 is
`b388e7c93d0d348795a3020b77b40d04e9622d99bf1303588c998bfece2813ba`.
The supported API therefore needs one size/type stat per physical name. Opening
each file/fstat also repeats path lookup; private DIR layouts or guessing the
mounted FatFs drive are not permitted substitutes. Complete statless accounting
needs an additional supported interface, outside this slice. The remaining
N stats can still be quadratic on FAT; moving them off opening does not erase
that cost.

For a clean N-name directory, measured host calls are 2(N+3) next calls, N+1
stats, one open/rewind/close, zero unlinks and N+2 path lookups. Previously the
canonical-only case needed 2N+1 stats. LFN names consume one short slot plus
ceil(name characters/13) long slots. For a compact directory of page canonicals,
each uses 12 32-B slots; D=ceil((3+12N)/16) 512-B sectors. A conservative
**modelled Pages-sector visit bound**, with uncached last-component searches,
is (N+2)D; ancestor lookups, FAT/cluster traversal and mutation writes are extra.
Physical sector reads are **unobserved**, not inferred from host POSIX timings.

| N page canonicals | Host next / stats / path lookups / unlinks | Compact D sectors (model) | Pages-sector visits, upper model |
| ---: | ---: | ---: | ---: |
| 0 | 6 / 1 / 2 / 0 | 1 | 2 |
| 8 | 22 / 9 / 10 / 0 | 7 | 70 |
| 32 | 70 / 33 / 34 / 0 | 25 | 850 |
| 128 | 262 / 129 / 130 / 0 | 97 | 12,610 |

For mixed names substitute actual LFN slots; protected semantic temporaries can
use 17 slots each. Deleted slots can make D arbitrarily larger than this compact
model even below the 128-name cap. For inventories i with N_i and physical D_i,
and U removals, path lookups are sum(N_i+2)+2U+E, E<=U canonical backup rechecks.
Pages-sector work is bounded by sum((N_i+2)D_i) plus the selected recheck/unlink/
backup searches; FAT allocation writes and ancestor directories are additional.
NEWT-9 must bind Pages/ visible names **and physical directory sectors**, record
these lookups/stats/unlinks, and measure SD transfers separately; the allowed
VFS supplies no physical-sector counter. A single FAT syscall can still block
in real time. No device milliseconds or physical-sector measurement is claimed.

Reserve the full 4-MiB payload +12-B footer under a 32-MiB logical quota. Credit
rounds DOWN to 64-KiB units; at least 65 units are required. The page FilePort's
existing receipt-only serial scalar holds the transferred credit. Ordinary
page/metadata writes debit requested bytes including failed transfers; producer
creation charges the full candidate before O_EXCL and never refunds. At most
one maximum candidate fits the 127-unit carry. Metadata remains writable after
credit exhaustion. Static/RTC storage does not grow.

Only the page port borrows producer policy, in its unused token union/spare
flag bit. A discriminator-first accessor owns all token reads; receipt authority
and cancellation stay separate. Producer preflight and promotion both refuse a
pre-existing backup on replace-capable and strict FAT paths. A backup appearing
after preflight is also protected by FAT's fallback check. Explicit cleanup
unlinks only the visit's own O_EXCL provisional. Complete CRC/readback remains;
successful syscalls preserve result/ownership across cancelling post-polls.
Lost promotion acknowledgements retain complete old/new/recovery output.

The storage table covers full, positive-short and one-shot EINTR transfers,
CRC, metadata, promotion and cleanup, including success and bounded stop, exact
old/new validation and provisional removal. Independent compiled controls cover
namespace/filter protection, the stat work bound, reservations/cap, CRC and both
normal/cleanup post-polls, token isolation and both promotion backup checks.
The entry/idle regression holds its no-inventory guard throughout preparation,
session binding and actual `openExternal`, for browser, warm, cold restore and
card-return entries. Compiled controls insert a census at preparation, session creation
and binding and must fail. It also checks boot/card invalidation and absence of
reader/session/XML ownership when DIR opens. `PageCacheIdleBudget` is a normal
stateless collaborator with explicit borrowed inputs, not a class-body include;
credit stays in the selection's existing padding. FilePort likewise declares its
whole API/access explicitly, with producer definitions in a normal after-class
header. Registered tests reject include directives inside either owner's class
declaration, closing the repeated hidden-member extraction pattern.
The receipt-path probe lives
with `ReaderRevisionSession`; its original guarded selection wrapper remains.
Final amended-source evidence: Linux quick rc 0, 171.240 s. Focused storage,
actual idle/entry, cover, cold/warm reader and lifecycle tests pass. Six fresh
local review rounds concluded clean in round 6. Four clean lanes compile in 142.377 s, health rc 1 **for flash only** (no ceiling edits):

| Lane | Flash B (above clean ceiling) | RAM B / delta | RTC B / delta | Selected frames changed |
| --- | ---: | ---: | ---: | ---: |
| default | 1,477,104 (+3,920) | 133,248 / 0 | 300 / 0 | 0 / 314 |
| dev-usb | 1,614,976 (+3,936) | 134,080 / 0 | 304 / 0 | 0 / 343 |
| regional | 1,483,760 (+3,920) | 133,744 / 0 | 300 / 0 | 0 / 316 |
| regional-dev-usb | 1,623,600 (+3,936) | 134,576 / 0 | 304 / 0 | 0 / 350 |

DEV includes the 16-B dirty ID; feature growth is 3,920 B in every lane. Every
pre-existing measured frame also matches. Guarded build/streamChapter/layout
remain 240/384/448 B; loop remains 80 B or 96 B on DEV. New noinline frames:
census owner 1,232 B, scanNames 368 B, run 16 B, idle wrapper 48 B and idle
preparation 64 B. The unlinked storage preflight is 128 B in both C3 probes.
New functions have no same-symbol baseline. The C3 compile-only census guard
sums **all** authored census/namespace frames (even mutually exclusive branches)
plus 256 B for callers (actual linked maximum 96+48+64=208 B) and
256 B for authored callbacks: 2,304 B against an
authored-only 3,072-B bound, both DEV modes. A separately compiled census with
a 2,048-B volatile enlargement measures 4,336 B and is rejected. These sums
exclude libc/VFS/FatFs/drivers and interrupts: the acquired FAT stat provider
alone has a 336-B frame and `f_stat` 624 B, with deeper calls unmeasured. The
existing 16-KiB task reservation is unchanged; neither this guard nor static
frames prove its runtime high-water or a complete provider-inclusive peak.
Device stack low-water remains required. Flash review/record belongs to the
Lead. SDK producer linking and combined patch copies remain S4.

For text pages F = 48 + 18P + 10R + T + 8A + L bytes per chapter, including FIBP
and NTCC overhead, run text T and link records/strings L. A book's footprint is
sum(F); the file payload cap stays 4 MiB plus 12 B. A successful single pass
writes F and reads it for CRC; aborted prefixes count too. Reserve the full file
cap; the entry allowance conservatively charges it before each attempt. Low/unknown battery blocks
writes (20% threshold; confirmed charging exempt). The current three-try per-chapter visit bound
limits rewrite loops, not lifetime writes across deliberate repeated visits.

## Lifecycle and foreground priority

On entry to Reading explicitly end the cover idle owner
(`firmware/cover_idle.h:147`); merely skipping its service can retain its resume
allocation. Do not alter #47 cover policy. Cover work, reader production,
metadata/checkpoints and display have exclusive owners.

Abort on input/menu/Home/sleep, chapter/book/settings/orientation change, DEV
purge, due checkpoint/housekeeping, low battery or SD loss/error. Abort before
any paint or state transaction. Pending production is not activity and cannot
extend sleep policy. Rejected Home/sleep transitions must leave the live reader
usable. After wake/reset restart only after normal source admission and accepted
page publication, then 2 s quiet; no retained continuation, pointer or watermark.

S4 must run actual firmware startup/selector/idle/sleep wiring on host with all
RTC/global state discarded; complete/corrupt/provisional caches; durable state
beating stale RTC; explicit fallback theme replacing old opt-in; missing/corrupt
storage and failed/lost-ack preference/checkpoint/cache commits. A derived cache
is not a preference or resume authority. Compile negative controls omitting the
actual durable restore, idle hook and provisional refusal. Add cancellation then
rejected Home/sleep and failed foreground repaint, checking exact prior pixels,
plan, anchor and live-source authority. Hardware USB sleep/wake, unplugged
battery sleep/wake and cold power-on with provisionals remain separate NEWT-9
checks, each bound to image, source, SD, power/reset and display conditions.

## Slices and evidence

The original order was docs, then S1–S5. File lists below are intended
production ownership (tests/tooling listed separately). Estimates are not resource
allowances. Each slice is independently gate-green; the Lead commits before the
next. Full host gate is mandatory on S5; quick/focused and four clean health
builds on every firmware-affecting slice. No private numbers/labels in artifacts.

| Slice | Files / scope | Production estimate | Tests and negative controls |
| --- | --- | --- | --- |
| S1 | `patches/freeink-producer-policy.{patch,json}`, `tools/apply-freeink-patch.py`; `firmware/chapter_cache_producer_policy.h`, `firmware/reader_scheduler.h`, policy forwarding in `include/newt/epub_admission.h`, `include/newt/reader_style_watch.h`, `src/reader_styles.cpp`, `src/reader_navigation.cpp`. Patch-copy and cancellation seams only; no idle/display/writer wiring or firmware SDK-route switch. | 150–220 lines; opt-in flash +1–3 KiB, currently unlinked. | `chapter_cache_producer_policy_test.py`: actual SDK canonical/OPF entity fan-out, tokens/chunk boundaries/encoding, cancellation and retry/cadence seams, admitted source still usable; every-page renderer parity across orientations/compression/markup. Independent compiled controls remove DTD refusal, token bound and latch; ordinary pinned/patched parity and sizeof proof. Patch destination/provenance tests. Register quick smoke + full next to reader turn-cost tests. |
| S2 | `firmware/chapter_page_cache.h`, small producer-attempt header, SDK-copy build routing only as needed; retain guarded signatures. Tail writer/128 cap, continuous lease, no style retries, no logical-frame loan. | 180–300; +1–3 KiB. | `chapter_cache_producer_test.py`: 128 succeeds/129 refuses, misalignment/short lend, every cancellation/EOF/style boundary; mutations release before finish or overwrite plan must fail. Prove RAM/selected frames unchanged on linked code. |
| S3 | Idle census/namespace helpers, boot allowance/session handoff, SD page/FilePort policy and allowance. Poll transfers/retries/CRC/promotion/cleanup. | About 510 added production lines including helper extraction; linked census/idle flash measured below; producer-only storage remains unlinked. | `chapter_cache_producer_storage_test.py`: exact temp grammar; mixed namespace/siblings, overflow, inventory invalidation/reservation; write/readback/commit/lost-ack, SD removal/replacement. Mutate namespace protection, reservation, EINTR poll and CRC poll. |
| S4 | Small idle/lifecycle owner plus existing `main.cpp`, session/selection and cover-handoff seams; no main/adapter allowance expansion. | 180–300; +1–3 KiB. | `chapter_cache_producer_runtime_test.py`: actual loop/setup/selector/sleep extraction; all cold/durable/preference/transition tests above, finite attempt budget, no background DEV interference. Remove idle/handoff/durable restore/provisional refusal independently. |
| S5 | `tests/reader_turn_cost_{test.py,matrix.h,probe.h}`, equality harness, `docs/064*`, this record; no new optimization. Add a `produced` condition on actual device-path turns. | 0 production; test-linked only. | Every page/frame/anchor, both orientations/directions/crossings/reopen/settings/sleep-wake, all authored shapes/storage methods. Report actual before/after work incl. 128-cap fallbacks, n/variance; old unwired-path and missing producer-policy controls must fail. Full host gate + four builds. |

S2–S4 tests register in the full reader/storage/lifecycle phases of
`tools/gate_plan.py`, with bounded quick smoke and exclusive build resources.
Counters cover source/expanded/inflater/XML/events/layout/pages/runs, writes,
index entries, transfer retries and producer cancellation. Count residual abort
work and assert a deterministic upper bound; no timing pass/fail. Device work
must additionally measure button response during XML, index and final CRC,
matched pre/post position sawtooths and observed hit/miss, SD footprint/writes,
and the three separate power conditions. Host pixels/compile/work do not establish
physical latency, optics, FAT blocking time or battery acceptance.

D + hardened A remains selected. S3 implements the idle-only census and
transactional storage seam; S4 still owns idle/lifecycle linking and deterministic
SDK composition. Resource evidence above is compile evidence, not device timing.
No cache-format/identity, source authority or resource ceiling changes are allowed.

## S2 implementation boundary and SDK composition

S2 adds an explicit `ChapterCacheProducer::produce` entry, not an idle hook.
It accepts only the published current chapter, known 1–128 pages and settled
rules. It uses that selection and rule slot directly: no canonical reselection,
style discovery or retry. `returnProducerStyles()` returns the loan with bounded
memory work only, including after cancellation. The existing `build` and
`streamChapter` bodies and signatures remain intact.

One extended parse lease precedes writer initialization and outlives finish or
abort, all writer/parser pointers and the explicit attempt destructor. The
display tail also holds the attempt's name, writer, arena and source/storage
wrappers. A compile-time assertion fits those controls plus the 2,564-byte C3
index in the conservative 3,105-byte tail; no task-stack buffer, published plan
or logical-frame loan is added. A short/misaligned/overlapping workspace or
failed extended lend refuses before writing. The sink enforces page 129 refusal
even when given a larger test arena. Only complete output with the published
page/character totals may finish. SDK-operation polls are present; S3 supplies the internal transfer, CRC,
promotion, retry and cleanup boundaries before S4 linking.

`chapter_cache_producer_test.py` checks exact 128/129 boundaries, leased writes
and teardown, cancellation at source/EOF/blob/index/footer/commit boundaries,
complete public progress/selection and frame/plan/repaint preservation, and
every-page/anchor equality against ordinary fallback in both directions/
orientations. After each exit it forces an ordinary turn on the same reader,
including styled success/cancellation, without metadata/style retries. Controls
release the lease early, remove the cap, overwrite the plan, omit style return,
clear progress, omit post-validation cancellation checks and enlarge the native
producer frame. Zero-length source probes can request cancellation; an observer
at the actual SDK writer's `finish()` entry (generated test copy only) proves
that neither initial nor final source-validation cancellation enters finish.
The compile-only C3 probe covers both DEV settings and bounds the new entry/
stream by the existing builder/stream's unchanged 240/384-byte frame budgets;
it is separate from linked-image and runtime stack evidence.

The producer remains unlinked through S3. Current firmware lanes therefore retain
their existing SDK routing:

| Lane | Book/font/hardware inputs | Display input |
| --- | --- | --- |
| default | pinned `vendor/freeink-sdk` | pinned `vendor/freeink-sdk` |
| dev-usb | pinned `vendor/freeink-sdk` | pinned `vendor/freeink-sdk` |
| regional | pinned `vendor/freeink-sdk` | `.build/freeink-sdk` (regional patch) |
| regional-dev-usb | pinned `vendor/freeink-sdk` | `.build/freeink-sdk-dev-usb` (regional patch) |

S2/S3 host and compile-only producer probes use the producer-patched Book copy
`.build/freeink-producer-sdk`; they do not establish linked firmware behavior.
**Before S4 links the producer**, replace the table above with one deterministic
copy per lane and route *all* SDK libraries in that lane to it:

| Lane | Required future copy | Ordered patches |
| --- | --- | --- |
| default | `.build/freeink-reader-sdk-default` | producer |
| dev-usb | `.build/freeink-reader-sdk-dev-usb` | producer |
| regional | `.build/freeink-reader-sdk-regional` | regional, then producer |
| regional-dev-usb | `.build/freeink-reader-sdk-regional-dev-usb` | regional, then producer |

Only `tools/apply-freeink-patch.py` may prepare these copies. Verify the pinned
source, each ordered patch's input/patch/result hashes, unchanged-file inventory,
license and final tree; retain the ordered manifests and tree hashes in one
provenance receipt. Code-health identity and compiler-dependency checks must bind
the exact copy actually used. Print that copy and patch order for every lane.
By S4, tests must reject a dropped regional or producer patch, wrong order,
changed results/provenance and any mixed/pinned library route in a producer
lane. No partial migration or silent fallback to an unpatched SDK is allowed.

## S4 SDK composition, before Reading wiring

All eight SDK libraries use one copy per lane, with official pin
`cd6f5b831c8fad899723b6d406586f00c575f021` unchanged:

| Lane | All SDK libraries | Ordered patches |
| --- | --- | --- |
| default | `.build/freeink-reader-sdk-default` | producer |
| dev-usb | `.build/freeink-reader-sdk-dev-usb` | producer |
| regional | `.build/freeink-reader-sdk-regional` | regional, producer |
| regional-dev-usb | `.build/freeink-reader-sdk-regional-dev-usb` | regional, producer |

Default/DEV content trees hash to
`da80ff829069157f827a9f5380402dbedfab15728c3df322bd0f598f292a53b3`;
regional trees to
`f5e8bab7285359d09b4d6ec19b6f271e4770cf7ee9c0306f38a36e0775cfb51c`.
Structural provenance binds 871 blobs, 225 directories and Git owner-execute
classification, including the archived gitlink's empty directory. All lanes
preserve structure SHA256
`50a4e4f2aae6e31a671249b721fa219f86d16dc6fb5683196448c81b5b548632`.

Canonical validation runs at generation and every resource identity capture
and verification. One independent reviewed pin authority verifies actual HEAD
blob bytes and owner-execute modes, bypassing mutable index/status hints.
Every FreeInk Git read/archive uses one sanitized literal-object boundary
(`GIT_NO_REPLACE_OBJECTS=1`), and source validation rejects replacement refs.
Origin authority requires one raw local/no-includes official URL and one
matching effective URL, comparing exact output bytes without text/newline
normalization and rejecting aliases, rewrites and duplicate origins.

The validator snapshots exact patch/manifest bytes, replays ordered patches in
`/var/tmp` against verified pinned blobs and the canonical archive structure,
and derives actual changed files, hashes, intermediate/final trees and modes.
Manifest outputs, receipts, final inventory/content and unchanged LICENSE must
agree with replay. SDK receipts bind structural digests alongside content
hashes. Reviewed patch files remain hash-bound authored inputs, so future
reviewed updates are checked by replay without a second hardcoded output map.
No cache format or identity changes.

Controls cover joint SDK/receipt/manifest forgery with an unchanged patch;
source identity, license, intermediate and inventory drift; dropped/reordered
patches; empty directories; all eight execute-bit masks and mode drift hidden
by `core.fileMode=false`; replacement-aligned trees; origin aliases/duplicates
and raw/effective control-character suffixes;
and missing validation/procedure bindings. Genuine composed-copy controls
cross direct validation, `build_identity` and `check_resources`. Actual compiler
`.d` evidence requires all eight libraries in the lane's copy and rejects
missing, other-lane, mixed pinned/composed and undeclared subtree routes.
Mutation tests use isolated `/var/tmp` trees; official source stays read-only.

The first migration candidate stopped: guarded `FilePort::commit` grew from
32 to 48 B in every lane. Disassembly and GCC IPA identify the SDK macro enabling
the capturing `producerCleanup` closure around CLOSE: result and errno survive
two outlined policy calls, increasing saved-register pressure (s0–s7 plus ra,
`addi sp,sp,-48`). Switching only the SDK include reproduces the change.
Typed noinline `producerClose(int)` keeps this closure outside commit; inactive
calls go directly to CLOSE, active calls preserve mandatory cleanup, both polls,
actual result and errno. Commit now saves s0–s6 plus ra and uses
`addi sp,sp,-32`; its signature, symbol and definition file are unchanged.
Shared typed post-result processing also preserves full result width and saves
416 B/lane against typed-close alone. Close and int/long post-result helpers
each measure 16 B. Cancellation/errno/cleanup controls exercise both wrappers;
a C3 control restoring the original capturing closure rejects its 48 B frame,
while current plain and policy paths measure 32 B.

Rejected scratch trials: generic cleanup noinline left commit at 48 B and grew
release 272 to 288 B; boundary/final-name outlining did not restore commit.
After typed close, bypassing inactive generic wrappers increased controlled
object text by 464 B, and outlining producer I/O increased it by 738 B and grew
other frames. A cold boundary saved no useful flash. Shared post-result code
was retained after all four lanes preserved every selected frame.

Four clean native lanes compile (150.710 s); health rc 1 is **flash-only** against
unchanged ceilings. Against saved `f20d185` reports, all 314/343/316/350 selected
frames are byte-identical: commit 32 B, build 240 B, streamChapter 384 B, layout
448 B and loop 80/96 B. Static RAM and RTC deltas are zero.

| Lane | Flash B / delta from S3 | RAM B / delta | RTC B / delta |
| --- | ---: | ---: | ---: |
| default | 1,480,032 / +2,928 | 133,248 / 0 | 300 / 0 |
| dev-usb | 1,617,904 / +2,928 | 134,080 / 0 | 304 / 0 |
| regional | 1,486,688 / +2,928 | 133,744 / 0 | 300 / 0 |
| regional-dev-usb | 1,626,560 / +2,960 | 134,576 / 0 | 304 / 0 |

Images remain below the 7,016,448 B usable partition limit. The Lead will make
one reviewed S3+S4 flash record at NEWT-8's end; this stage edits no ceiling,
resource record, exclusion or adapter. Focused provenance, source, resource,
procedure, lane and storage regressions pass; Linux quick passes in 168.480 s.
The SDK stage was committed as `0dacfb2`; the Lead's fresh exported Astra review
is clean. Its three existing unselected +16-B frames (`allowed`, `sync`, cache
`write`) are included in the S4 loop-task chain accounting below. No device
latency or owner hardware acceptance is claimed.

## S4 Reading idle and lifecycle wiring

Historical S4 evidence follows. The later NEWT-21 steps 4 and 5 supersede its
pre-open allowance, single-attempt and cold-resume trigger rules.

`ReadingCacheIdle` uses only attempt-local references/snapshots. The existing
cover idle owner's `quietFrom` is reset by the existing consumed-input hook;
its `view` tracks the completed display generation during Reading. A new paint
starts the two-second quiet interval after publication, including open, turn,
menu, jump and reflow. No producer or census runs inside open/turn/render.

The loop invokes the Reading owner after input, battery/progress housekeeping
and the DEV automation gate. The Home/Library wrapper yields ownership in
Reading. Before any reader/selection allocation, `handoffCoverToReading` moves
the existing cover screen-change cleanup earlier: zero directory/source/cache
I/O and zero allocations, at most one existing cover-resume release; repeated
handoffs are scalar no-ops. Turning adds no producer hook or I/O. This is not a
claim of zero CPU instructions for the required ownership transfer.

Start requires an opened external Page, the matching verified session, eligible
battery (known >=20%, or confirmed charging), no queued input/DEV byte or due
checkpoint/battery read, and the S3 allowance. The scoped latch polls these
conditions plus owner/source/chapter/settings/display identity throughout.
Callbacks never dispatch or close the reader: press/menu/Home/sleep/settings/
orientation/book changes interrupt first, then the next loop handles them.
Known source/card failures abort through the existing authenticated source;
cancellation alone does not invalidate it. No continuation survives unwind.

The session attaches the S3 transactional page-port policy, runs the real capped
tail-loan producer, withdraws the policy, and discards all remaining credit on
every attempted success/refusal/cancellation. No new resident attempt flag is
needed. Without a fresh allowance, later chapters/settings/reopens remain
fallback; they never inventory or replenish credit while Reading. Cold boot
starts without credit and therefore does no cache work on its resume path.

The press bound starts when input is queued. The next poll latches interruption;
no following work unit starts. The in-flight unit is one <=2-KiB raw/decoded
read or XML feed (<=4 KiB retained XML processing), one <=1-KiB SD transfer/CRC
chunk, one 8-B index record, or one metadata/promotion syscall. An admitted raw
read can straddle two 4-KiB authentication blocks: conservatively <=10,240
canonical bytes (8,192 authentication + 2,048 requested), plus bounded receipt/
path checks. It is not a one-physical-read bound. Finish the bounded paragraph/
page callback already in flight, then unwind; SMALL paragraph/line/run limits
remain 3,072 B/176/304. Mandatory cleanup may close the owned fd and unlink its
own temporary; it never enumerates a directory or completes the chapter after
the latch. The existing source/decoded/XML/index/SD/CRC/promotion omission
controls certify these boundaries. Blocking FAT/fsync and input acquisition
still require device measurements; these work units are not milliseconds.

Four clean lanes compile in 141.724 s; health rc 1 is flash-only against
unchanged ceilings. Compared with `0dacfb2` (and S3 for RAM/RTC/selected stacks):

| Lane | Flash B / delta from SDK stage | RAM B / delta | RTC B / delta | Selected frames equal |
| --- | ---: | ---: | ---: | ---: |
| default | 1,484,960 / +4,928 | 133,248 / 0 | 300 / 0 | 314 |
| dev-usb | 1,622,832 / +4,928 | 134,080 / 0 | 304 / 0 | 343 |
| regional | 1,491,616 / +4,928 | 133,744 / 0 | 300 / 0 | 316 |
| regional-dev-usb | 1,631,504 / +4,944 | 134,576 / 0 | 304 / 0 | 350 |

The newly emitted first-party unselected frames are identical across lanes:

| Function/group | Frame B |
| --- | ---: |
| main idle wrapper; Reading service; stop poll; cover handoff | 32; 176; 80; 48 |
| selection eligibility/delegate; revision eligibility/delegate | 16/32; 0/48 |
| producer entry/stream/workspace/overlap | 64/352/32/0 |
| capped writer page/anchor; its two destructor entries | 32/32; 0/0 |
| Store exists/fileSize/readAt/beginWrite/write | 32 each |
| Store endWrite/remove; its two destructor entries | 16/0; 0/0 |
| producer source readAt/size/policy; its two destructor entries | 32/0/0; 0/0 |
| cancellation stopped/checkpoint/cancelled | 16 each |
| session policy bridge; Reading yield/stop bridges | 16; 16/0 |

That is 36 new frames (24 positive, 12 zero). The existing unselected idle
allowance helper moves to the ordinary after-class definition header with the
same 64-B frame; no guarded symbol/signature/definition file moves. The live
session's size is unchanged. These are individual compiler frames, not new
ceilings or an absolute runtime stack measurement.

The deepest known producer chain is **3,392 B non-DEV / 3,408 B DEV**, including
`loop` and the complete cancellation callback. Its three parts are the caller/
producer prefix 784/800 B, SDK/Expat text-to-page chain 2,048 B, and writer/
storage/poll tail 560 B. The prefix runs through main's wrapper, Reading service,
selection/revision delegates and producer entry/stream. The middle reaches
`placeLine`'s 1,536-B frame. The tail includes page serialization, `write`,
`stable`, `allowed`, both latch frames, the 80-B stop poll and its deepest 16-B
reader/settings leaf. The scheduler-yield alternative ties this bound.

The comparable existing cover chain is **6,352/6,992 B**: Decision 059's 5,936 B
minus its separately counted 48-B idle step, plus actual cover/observation/
library wrappers and `loop`. Producer margins are 2,960/3,584 B. Other producer
branches are lower: final validation 2,000/2,016 B, null image-prescan setup
1,584/1,600 B, inflation 1,328/1,344 B, writer begin 1,184/1,200 B and sync
976/992 B. It reuses the selected entry, so it does not call the 4,240-B ZIP
directory locator, 2,192-B plain-text convenience layout or 8-KiB miniz helpers.

For the SDK review's three **unselected** increases, choose option **(b)**:
`allowed` 0->16 B and cache `write` 32->48 B occur on the producer chain above;
`sync` 16->32 B occurs on its separate finish branch. Audit ordinary routes too:
their highest chains are respectively 2,928/3,904 B (open/cache), 1,488/2,336 B
(metadata write), and 1,552/2,416 B (metadata/receipt seal). Ordinary final
validation is 2,576/3,424 B. Thus these inherited increases and the newly linked
producer do not exceed an existing chain in the **same known-component scope**.

These sums use pinned C3 `.su`, source edges and linked disassembly; they exclude
precompiled libc/provider, FAT/VFS/filesystem/SPI/driver, RTOS and interrupt
frames. They establish that scoped comparison, not unchanged absolute runtime
stack use or a task high-water measurement. USB sleep/wake, battery sleep/wake,
cold power-on and on-device stack/press timing remain separate NEWT-9 evidence.

Runtime regressions extract the actual idle class, main loop/handoff and durable
startup/selector wiring. The real selection/session/storage leg proves ordinary
open/turn add no writes, idle creates a matching cache, and the next turn hits;
corrupt canonical and recognized temporary files still fall back exactly. Each
non-Page panel blocks an attempt without spending credit, and in-flight Menu/
Settings changes cancel. Real producer cancellation followed by rejected Home/
sleep and failed repaint publication preserves the admitted source, semantic
locator and subsequent byte-identical repaint. Independent compiled controls
remove the idle hook, handoff, panel guard, durable restore, CRC validation,
temporary rejection and destination guards. Scheduling fakes make no pixel or
plan-preservation claim; those assertions use the real reader and transactions.

Final Linux quick gate passes in 160.927 s, including the focused producer,
storage, runtime and power controls. Three fresh runtime adversarial/
maintainability rounds finish **clean in round 3**: the first two found three
test-oracle gaps, all fixed with production-path assertions and independent
mutation controls. Four clean firmware builds above are the final unchanged
production code; subsequent changes only strengthen tests/documentation. Full
host equality/work-matrix validation remains S5, not established by this slice.

## S5 production-path work and equality

The turn-cost harness retains `cold` (derived files absent) and historical
`warm` (the legacy explicit builder and its existing index capacity), and adds
`produced`. After a real
revision/session open, `produced` grants the existing census allowance and
calls that live session's capped transactional producer. The measured turn then
uses the ordinary UI/session path with `buildOnMiss=false`. Preparation and its
write/validation work are reported separately from PRESS. The explicit grant
isolates an eligible idle outcome; S4's actual loop/census/handoff regressions
establish the trigger wiring. It does not measure the two-second wait, SD time,
display work or physical power transitions.

Each produced cell starts with no destination page files and prepares only its
origin chapter. Positions are the origin's first/middle/last page, forward and
back, plus both incoming crossings. First/Previous and last/Next are outgoing
crossings. Thus a produced origin does not count as a destination hit. The
report retains both orientations, three markup shapes, stored/deflated members
and three observations per cell; the existing 1-MiB refusal sweep is retained.

Misses remain expected before eligible idle completes, without a fresh census
allowance, after cancellation/refusal, above 128 pages (since #175 only anchor
restores and the first crossing page there), or after source/settings
identity mismatch. First cross-spine requests still miss even when a compatible
destination file exists. Under step 5, a reached chapter becomes eligible after its fresh quiet interval
and census, and settings reopens start a new visit. Cold Reading resume uses
the same idle admission; an already valid persisted file is never rebuilt and
may still serve compatible same-spine turns.
No hit-rate percentage here predicts a library or a reader's actual idle gaps.

The full authored report is
[065-produced-turn-cost-matrix.csv](065-produced-turn-cost-matrix.csv): **24
fixtures, 1,152 cells, n=3 per cell / 3,456 samples**, with median, nearest-rank
p90, max and population standard deviation for every work counter and
informational host microseconds. Producer status, origin spine and actual page
count accompany the observed hit/miss counters. An independent calculation
reproduces every CSV statistic from the retained raw samples. Of 1,152 produced
turn samples, 288 hit and 864 miss; 4/64-KiB cohorts each have 144 hits and 144
crossing misses, while the 256-KiB and 1-MiB cohorts each have 288 misses. These
are deliberately balanced cells, not a steady-reading hit rate.

Representative **portrait/plain/stored**, n=3, chapter bytes / emitted runs
before -> after production:

| Expanded chapter | First/Next | Middle/Next | Last/Previous | Observed outcome |
| --- | --- | --- | --- | --- |
| 4 KiB | 2,048 / 224 -> 0 / 0 | 4,096 / 447 -> 0 / 0 | 4,096 / 447 -> 0 / 0 | hit, 7-page cache |
| 64 KiB | 2,048 / 224 -> 0 / 0 | 34,816 / 4,692 -> 0 / 0 | 65,536 / 8,835 -> 0 / 0 | hit, 118-page cache |
| 256 KiB | 2,048 / 224 -> unchanged | 133,120 / 18,057 -> unchanged | 262,144 / 35,660 -> unchanged | miss, 473 pages exceeds cap |

Every observed hit has zero delivered chapter bytes, XML bytes, emitted runs
and Layout phase entries; every measured turn writes zero cache bytes. Both
incoming crossing directions retain full destination work: 4,096/65,536/262,144
bytes and 522/8,911/35,696 runs in those same cohorts. Other positions,
directions, markup/compression and orientations are in the CSV.

Preparation is not free: 720 successful attempts serialize 6–118 pages and
4,514–146,079 payload bytes through 368–18,060 SDK writer calls, then validate
the candidate. These are logical writer calls/bytes, not SD sectors or physical
writes. The 432 known-over-cap attempts refuse at 345–1,893 pages with zero
source/storage/XML/write/index work inside the preparation interval.

Removing parse/layout work does **not** establish an overall I/O or latency
reduction. These isolated first-hit cells reacquire a validated lease. For the
64-KiB example, first/middle/last cache storage reads are
149,040/149,040/149,067 B, versus fallback's 32/288/544 B plus source reads of
4,096/36,864/69,632 B. The hit rereads 118 index entries. Consecutive stable
leases can amortize the whole-file CRC; this experiment does not credit that
future saving to its isolated cells.

There is also a measured refusal cost: the same-chapter 256-KiB examples retain
identical source/parse/layout work but read **740 additional storage bytes** on
the following turn (32->772 B first, 1,056->1,796 B middle,
2,080->2,820 B last). Producer boundaries close the shared read lease and clear
absence knowledge (`firmware/sd_reader_cache_storage.h:184–191`). Thus the
zero-foreground-producer/census guarantee is not a claim of zero incidental
validation I/O after idle. S5 reports this consequence and makes no production
optimization. Device stage rows must decide whether saved CPU/layout outweighs
SD validation/index costs; no physical speedup or noise threshold is approved.

S5 changes no authored production code or native build procedure. Four final
clean firmware lanes all compile;
health exits rc 1 for flash alone against the untouched main ceilings (longest
lane 119.539 s). Compared with `73c5ed0`:

| Lane | Main flash B | Final flash B | Delta B | RAM B | RTC B | Selected frames equal |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| default | 1,473,184 | 1,484,960 | +11,776 | 133,248 | 300 | 314 |
| dev-usb | 1,611,040 | 1,622,832 | +11,792 | 134,080 | 304 | 343 |
| regional | 1,479,840 | 1,491,616 | +11,776 | 133,744 | 300 | 316 |
| regional-dev-usb | 1,619,664 | 1,631,504 | +11,840 | 134,576 | 304 | 350 |

Every final flash value equals S4; RAM/RTC and selected frames equal both main
and S4. DEV values include the uncommitted `-dirty` build-ID overhead. These are
measured sizes, not a flash-budget approval; the Lead owns the final reviewed
record. No resource ceiling/history, exclusion or adapter changes are included. The S4
known-component stack-chain scope and separate device high-water requirement
remain unchanged.

Final verification: the full Linux host gate passes (rc 0, 929.505 s). Its
production-path equality sweep checks **13,408 exact frame/semantic-anchor
comparisons across 18 fixtures**, both orientations/directions, crossings,
reopen, durable close/readback/new-session wake, settings changes, cancellation
and absent allowance. Independent unwired-producer and missing-policy controls
fail their intended hit oracle. The full producer, polling, storage, runtime
and power suites also pass. Local review findings were fixed; two subsequent
reviews found no remaining defect. Legacy foreground fixtures now declare the
cover handoff seam, and receipt proofs snapshot all owned firmware headers.

The style-source digest guard records reviewed producer-only forwarding/loan
changes under the **same policy identity**. Independent comparison with
`73c5ed0` finds all 14 marked policy regions unchanged and identical resolved
rules, issues, ordering and content hashes across 73 authored fixtures; no
cache identity or format changes. Earlier failed/partial gate runs and their
repairs are retained in the commit handoff. These checks establish host
correctness/work evidence, not device timing or owner acceptance. Exact-source
Mac ImageIO coverage and physical USB, battery and cold-power journeys remain
pending, as do device stack high-water and latency measurements.

## NEWT-21 step 1: card census and one-session observation

The Pages inventory now recognizes every current firmware writer. All four
use `SdChapterCacheStorage` and its shared `CacheRoute`/`FilePort` transaction:

- `ChapterCacheProducer` through the SDK `PageCacheWriter`/`ChapterPageCache`:
  `<64-hex route>-<64-hex page identity>.fibp`.
- `ReaderSemanticMetadata`, published by `ReaderRevisionSession::publishMetadata`:
  `<route>-sm-<64-hex digest>-<40-hex SDK>-<8-hex policy>.ntsm`.
- `ProgressEstimateRecord`, published by `publishEstimate`:
  `<route>-pe-<64-hex digest>.ntpe`.
- `ChapterIdentityRecord`, published by `publishCheckpointIdentity` after a
  reading checkpoint: `<route>-c[0-2]-<52 base32hex digest>.ntci`.

The chapter identity writer and census share `ChapterIdentityName`: all 256
bits are preserved in lowercase base32hex, with the final four padding bits
zero (last character `0` or `g`). `FilePort::begin` creates an O_EXCL
`.xxxxxxxx.tmp` suffix with exactly eight lowercase hexadecimal digits;
`FilePort` replacement can leave `.bak`. These siblings are recognized for
all four kinds. Canonical identity files, backups, incomplete temporaries and
other metadata count their **physical** sizes, including storage footers,
against quota. They never become eviction candidates. Unknown names, malformed
identity encodings, nonregular entries, over-cap inventories and storage
failures continue to deny credit. No book-specific exception, deadline,
receipt deferral, I/O batching, retry or foreground producer policy changes.

DEV-USB emits best-effort numeric rows only, using the same bounded
`traceReaderRow` helper as reader diagnostics. No filenames, titles, labels or
book text are emitted. No operation emits a row or waits for TX capacity.
The census row follows directory closure; the attempt row follows full producer
unwind and is outside its measured interval. Row emission can be dropped if TX
is full. A census row without a later attempt row indicates that no attempt
was observed; TX drops can also leave that gap.

Exact wire formats (decimal integers, one newline each):

```text
PC census: o=<observed> r=<result> n=<unknown>,<page>,<semantic>,<progress>,<identity> b=<physical_bytes> ms=<elapsed_ms> u=<units>
PC attempt: o=<observed> t=<start_after_quiet_began_ms>,<end_after_quiet_began_ms> d=<progress_ms_left>,<battery_ms_left> s=<BookStatus> r=<first_stop> p=<pages> w=<write_calls>,<written_bytes> stat=<path_stats>,<receipt_path_stats>
```

`o=1` means the scoped observer was attached. The existing DEV scheduler's
startup diagnostic heap owner grows by 4 bytes on C3 (80 to 84), retaining its
one-pointer static cost; failed diagnostic allocation leaves `o=0` and counters
unavailable. No runtime allocation or retry is added for diagnostics. Its extra
field holds only the scoped observer pointer. Attempt times use the same
`idle.quietFrom` clock as admission: quiet began at a screen/view change, input
or interruption. Display receipt completion is identical to main.

Census results: 0 granted, 1 unknown name, 2 entry cap, 3 cancelled, 4 storage
failure, 5 quota, 6 invalid root. Counts describe the final scan's name pass;
bytes describe that scan's accounting pass. On failure either may be partial
(or bytes zero if the name pass failed). Rescans after page eviction replace,
rather than accumulate, these observations. Units are the census's actual
return value, not the selector's consumed/denied state `1`.

Attempt stop reasons: 0 none, 1 queued input, 2 unread DEV byte, 3 progress
save deadline, 4 battery refresh deadline, 5 unsuitable power/battery state,
6 screen/panel/view/source/owner change, 7 storage, 8 other refusal. The first
observed reason remains latched even if another failure follows. `s` is the
SDK's numeric `BookStatus`, so a failed attempt remains distinguishable from a
completed one. Storage syscall failures exclude ordinary ENOENT/EEXIST probes,
EINTR retries and cancellation; an otherwise unexplained IoError is classified
as storage after unwind, other refused statuses as other. Pages count successful
writer page callbacks, including work discarded on abort. Write calls include
retries/failures; written bytes count positive acknowledgements, including the
storage footer. Path stats count stat/lstat calls across the scoped attempt;
receipt stats are the subset under `Imported/`. They include cleanup and add no
new filesystem operation. End and start times use unsigned subtraction from
the quiet origin passed by `service()`; remaining deadlines clamp at zero and retain the existing
30,000/60,000 ms deadlines. These are instrumented DEV observations, not a claim
about production-lane timing.

Non-DEV lanes use the same census and admission/stop logic with no counters,
clock capture or rows.

Host regression and compile evidence remain separate from device evidence.
No card timing, USB session, unplugged-battery sleep/wake, cold power-on,
physical stack high-water or owner acceptance is established by this change.

The real runtime regression now checkpoints multiple authored books before the
Library census, transfers its granted allowance, produces through the actual
idle/session/transactional storage path and checks exact pixels on the next
turn with zero layout phases. It also checks physical accounting under quota
pressure and persistent metadata protection. Removing only identity grammar
recognition fails the grant oracle; genuinely unknown entries still deny. The
telemetry suite covers all seven census results, each stop reason and errno
exclusions, maximum row lengths, failed TX admission, actual display-completion
duration and wraparound, quiet-origin subtraction and wraparound, diagnostic allocation failure. Existing producer, polling, storage,
entry, corruption, power and wiring controls remain registered. Both Linux and
canonical plans grow from 235 to 236 full phases, and 135 to 136 quick phases.

Shared EINTR classification lives in `firmware/interrupted_syscall.h`, so
telemetry does not depend on the entire file port. The unchanged bounded
`InterruptedSyscall` retry state stays in `sd_cache_file_port.h`, preserving its
selected stack measurement and original caller behavior. The structural guard
permits exactly one production EINTR comparison inside the shared classifier
only; its regression explicitly rejects a raw loop in the file port. No
additional exclusion, comparison or retry is allowed.

Identity sibling regressions inject faults through the real checkpoint storage
syscalls, with no production-only hook: a payload physically written before an
injected EIO, followed by failed cleanup, leaves the actual O_EXCL-created
incomplete temporary alongside its prior canonical; FAT replacement collision,
canonical-to-backup rename, failed candidate promotion and failed rollback leave
the actual complete backup with canonical absent. Independent progress still
commits in both cases. The final multi-book inventory contains one canonical,
one backup and one incomplete temporary, all generated by the real writer.

The producer close path shares `producerResult` with ordinary and cleanup I/O:
a captured storage failure is classified before the post-operation cancellation
poll, and errno is restored afterwards. An actual failed-close/input-race host
regression checks this first-reason ordering; reversing it fails its oracle.
This changes observation ordering only, with the same close and cancellation
calls. Host and clean four-variant resource evidence are recorded below.

### Paint-stack investigation

The paint-note call itself costs 16 B in the DEV paint frame even when outlined;
removing it restores main's frame (lead isolation, confirmed by clean builds).
`display_receipt.h` matches main exactly; no paint observation remains.

### R6 evidence

The Lead removed the idle diagnostic row. Census and attempt telemetry remain;
the four handoff files match `e50a999` exactly, and the diagnostic visit bit and
foreground reset are gone. The reviewer attribution fix remains. Four clean `--health-report` lanes at
`2afe139ea5d5543ab6cac5ba6c0b3d905af7231b` pass, with linked NVS/reader-task
guards and every selected frame present.

| Variant | Flash | Change vs record 31 | SRAM | RTC | Open / paint / retry stack |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 2,153,072 | 0 | 133,232 | 300 | 144 / 80 / 16 |
| dev-usb | 2,292,784 | -224 | 134,080 | 304 | 560 / 96 / 16 |
| regional | 2,159,760 | 0 | 133,728 | 300 | 144 / 80 / 16 |
| regional-dev-usb | 2,301,488 | -208 | 134,576 | 304 | 560 / 96 / 16 |

All RAM/RTC/stack ceilings stay unchanged. Flash record 32 preserves the full
31-record prefix and tightens only the DEV ceilings to these exact sizes after
fresh independent resource review. Four post-adoption clean lanes at
`7721030` reconfirm these exact flash sizes, unchanged RAM/RTC/selected stacks
and linked guards. Quick Linux passes all 136 phases in 199.280 s; full Linux
passes all 236 phases at `b57aae7` in 780.790 s, using `/var/tmp` scratch with
no hard-coded `/tmp` quota failure. The approved release environment passes
`fcca87ad1cb792a3ca2bfafb7c7e5bae3ff14b0d..HEAD` at that same revision.
The exact final evidence commit is rechecked with both full Linux and pinned
release commands; the task handoff reports their results. A fresh independent whole-branch
adversarial/maintainability source review reports no actionable findings.

The earlier independent review found an indirect EINTR-ratchet bypass. The
ratchet now confines classifier type references to its declaration and the two
complete bounded retry/storage-observation bodies. Direct/qualified calls,
aliases, function pointers and loops in existing caller files are covered by
23 focused regressions. No additional exclusion, adapter or retry is allowed.

[Verification evidence](../artifacts/newt21-step1-verification.json) separates
host, compile and physical scope. Exact-source Mac ImageIO, card timing,
USB/battery/cold-power journeys and runtime stack high-water remain unestablished.
This candidate is not ready for owner acceptance.

## NEWT-21 step 2: bounded I/O and deadline pauses

Based on finalized step 1 (`a1a50f9`, PR #67). The producer now coalesces the
SDK's unchanged FIBP run/index serialization in a **1,024-byte stream buffer**
allocated, with checked failure, from the existing lent parse arena. It is
retained through writer finish/abort and the continuous display lease. This
adds no heap allocation, static/RTC storage, new task or larger arena. A 4 KiB
buffer was rejected during development because it exhausted the compressed
chapter parse arena. Ordinary cache builders and the pinned vendor SDK remain
unchanged; no additional SDK patch is necessary.

The receipt port has an exclusive immutable producer-read scope. It checks its
path and descriptor on first use and at checked exit; interior guards retain
owner generation, bounds, token and cancellation checks. A separate page-port
pre-promotion callback closes/checks that receipt scope **before rename**, then
reopens it for final source validation and checked unwind. Ordinary open/turn
receipt deferral and page CRC/index validation retain their existing behavior.
Cancellation still discards only the attempt's own provisional name.

Progress-save and battery-read deadlines are soft pauses of the same synchronous
parser/writer attempt. Deep cancellation polls continue to latch input, DEV
bytes, view/settings/source change, unsuitable power and a 120,000 ms total
attempt limit. Only the outer boundary after `ChapterLayoutSession::step(1)`
may run foreground housekeeping; no XML/layout/storage call is then on the
stack. The pause keeps its parser, arena, provisional output and consumed quota
credit, reads battery first and saves the durable progress core without optional
identity/estimate metadata. It rechecks hard stops afterward. Painting waits
until the display lease is released. There are at most eight pauses and one
charged attempt at a time. The former single-attempt visit policy in this
step-2 evidence is superseded by step 5 below. An unchanged or failed
housekeeping clock cannot spin. The final CRC/sync/commit is bounded and runs
without a nested save; a crossed deadline is serviced immediately after unwind.

The census row is unchanged. The final attempt row is:

```
PC attempt: o=1 t=<quiet-start-ms>,<quiet-end-ms> d=<progress-remaining-ms>,<battery-remaining-ms> s=<BookStatus> r=<first-stop> p=<total-pages> w=<write-calls>,<acknowledged-bytes> stat=<path-stats>,<receipt-stats> pause=<pause-count>
```

`p` includes all pages produced across pauses. `pause` counts in-attempt
housekeeping calls; it does not grant another candidate or reset the quiet
origin. Reasons remain 0 none, 1 input, 2 DEV byte, 3 progress deadline without
successful pause, 4 battery deadline without successful pause, 5 unsuitable
power, 6 view change, 7 storage error and 8 other (including elapsed bound).
Rows remain nonblocking and bounded to the existing 160-byte formatter.

Host operation counts, clean four-lane resource results and independent review
are recorded below when available. All X3 timing, minimum free heap and physical
power-condition evidence remain pending Lead measurement. Host tests and clean
compilation do not establish real-device completion or owner acceptance.

### Step-2 host work witness

`tests/chapter_cache_producer_io_test.py` compiles the actual SD FilePort,
revision session and producer, and a header-only `a1a50f9` control. All fixtures
are authored XHTML (65,536 B plain, 65,536 B styled/inline, 4,096 B list/table),
with deflate, native reader settings, and FAT non-replacing rename semantics.
Counts include the writer's complete NTCC CRC readback and promotion.

| Fixture / pages | Writes, before → after | Path checks, before → after | Receipt path checks, before → after | Cache/receipt fstats, before → after |
| --- | ---: | ---: | ---: | ---: |
| plain / 118 | 17,993 → 145 | 72,599 → 591 | 72,012 → 4 | 144,565 → 1,165 |
| styled inline / 101 | 9,235 → 86 | 37,325 → 355 | 36,974 → 4 | 74,259 → 693 |
| short list/table / 7 | 369 → 7 | 1,531 → 39 | 1,496 → 4 | 3,001 → 61 |

The observed per-page averages after the change are respectively 1.23 / 0.85 /
1.00 writes, 5.01 / 3.51 / 5.57 path checks, and 9.87 / 6.86 / 8.71 descriptor
checks. The mechanism depends on serialized byte count, not book identity;
larger records simply use additional bounded 1 KiB transfers. Primary port
operations (open/stat/fstat/seek/read/write) fall from 253,451 / 130,235 / 5,287
to 2,347 / 1,401 / 131. Payload receipt reads stay 150 / 90 / 8.

The advancing clock charges **every counted port operation** (including cleanup,
sync, rename and directory operations) 6 ms. It additionally models native
FileBookSource calls conservatively: one descriptor check per live probe, and
one descriptor check plus read plus possible seek per raw payload request.
That source-operation upper bound falls from 36,492 / 18,842 / 820 to 797 /
545 / 97. This is a host syscall-work model, not physical FAT sector evidence.
The resulting complete-chapter storage model is 1,739.700 / 894.504 / 36.684 s
before and **18.906 / 11.718 / 1.410 s** after. Scaling the plain fixture to
90 pages gives about 14.4 s of modeled I/O; adding the Lead's 10–25 s CPU floor
suggests **24–39 s**, pending device measurement.

The mid-chapter deadline fixture completes the same attempt at 22.206 modeled
seconds, including 100 ms per outer parser step, one acknowledged soft pause,
145 writes and one complete canonical. The actual idle/runtime fixture separately
checks main's callback wiring, two progress saves and one battery read without
restarting, rejects an unchanged clock, enforces the hard elapsed bound, and
fails with the pause handler removed. Removing buffering yields 109,435 primary
port operations; removing receipt scoping yields 3,579 and violates the path
check bound. Both controls fail independently.

Real partial write followed by ENOSPC is injected on the **second** write (inside
the 1 KiB buffer, after the header). Card removal, changed/removed/replaced
receipt, input, low power and sleep stops expose no partial canonical. Existing
storage tests retain successful short-transfer retries, FAT replacement/lost-ack
coverage, full CRC rejection and provisional-file refusal. Style cleanup now
follows the checked commit so the buffer stays owned; its poll regression
verifies that a late stop starts no additional parsing or I/O and cannot undo
an already complete commit.

Expat requested heap peaks are unchanged at 14,034 / 14,732 / 15,018 B before
and after. Buffering adds **0 heap bytes** and no session object capacity; it
spends 1 KiB of the existing arena. The 32 KiB XML budget plus 32 KiB admission
reserve remain unchanged. Deadline servicing can overlap the existing bounded
SD progress transaction with a live XML parser, at an outer step rather than
the deepest layout stack; optional metadata and final-name allocation do not
overlap that pause. The physical minimum-free-heap/allocator margin must still
be measured on the X3; the host requested-byte counts do not establish it.

Fresh GPT-5.6 Sol high reviews `newt21-step2-adversarial-r1` and
`newt21-step2-maintainability-r1` found no actionable source defects. The latter
independently accepted the approximately 1.1 KiB flash tradeoff provisionally,
requiring final committed-source reports and exact hashes for the one audit.
This is not owner review or hardware acceptance.

### Step-2 measured resource review

Fresh independent GPT-5.6 Sol high ROUND2 (`newt21-step2-adversarial-r2`,
2026-10-19) reviewed the completed test/source range and independently rehashed
all 376 artifacts per variant, including image/ELF/map/partition and NVS receipt
bindings. It found no actionable source/test defects and approved one exact
flash-only record for compiled source
`41749beb13fd63a36ef1946af4096b0af8a7b324`. That record is appended once in
`code-health-baseline.json`; all protected histories and ceilings are retained.

| Variant | Exact app bytes | Growth | Shared RAM / RTC bytes | Inherited stack entries unchanged | Usable flash headroom |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 2,154,144 | +1,072 | 133,232 / 300 | 348 / 348 | 4,862,304 |
| dev-usb | 2,293,904 | +1,120 | 134,080 / 304 | 377 / 377 | 4,722,544 |
| regional | 2,160,832 | +1,072 | 133,728 / 300 | 350 / 350 | 4,855,616 |
| regional-dev-usb | 2,302,592 | +1,104 | 134,576 / 304 | 384 / 384 | 4,713,856 |

The cost is stream flushing, pre-promotion receipt authority, bounded outer-step
housekeeping and DEV pause accounting. Static buffering would raise protected
RAM; new heap buffering adds lifecycle/fragmentation risk; SDK-wide changes
would broaden impact; restarting or durable partial-resume machinery would
waste work or increase complexity. No slack, partition/reserve change, larger
arena or protected ceiling increase is included. The 7,016,448-byte usable
partition limit is unchanged. Individual compiler frames are evidence about
those functions, not runtime call-chain/interrupt or physical stack proof.

A before/after open/ordinary-turn observation using the existing native Device
fixture measured identical cache/receipt + conservative source operation counts:
plain open 519 + 152, styled open 488 + 138, short open 453 + 122, and each
ordinary forward turn 121 + 20. All 96 overlapping rows of the existing
`docs/065-produced-turn-cost-matrix.csv` and the quick matrix's 64 KiB list/table
and 1 MiB stored-plain fixtures had **zero increases** in the 20 deterministic
work counters (both orientations and every cache condition/position/direction).
Timings were excluded from that comparison.

The quick Linux gate passed on `41749be` in 204.952 s. Focused I/O, storage,
producer/poll, telemetry, power and actual runtime/startup tests passed, including
the final unchanged-clock negative case. Final full Linux and resource-gate
results belong to the subsequent verification run. Mac ImageIO decode coverage,
X3 completion, minimum-free-heap and USB/battery/cold-power checks remain separate
pending evidence; this candidate is not described as hardware-ready.

The subsequent four clean `--health-report` lanes all passed on committed source
`4619ed79d0457adad210dc8cef6f2ca1c5503b1b`, with exactly the sizes above and
unchanged protected resources. The archived firmware identities identify that
clean source; the verification commit changes only receipts, documentation and
a host negative-control anchor. That control now targets the normal-save branch
of the producer-aware progress-save expression and passes independently. The
full Linux gate is rerun after committing these verification files so its result
is tied to the final commit.

### Step-2 R2 correction of pause ownership and parser bounds

Independent `newt21_step2_review_r1` reproduced two defects in `cbd4f18`;
its findings supersede the earlier clean-review claims for deadline servicing.
The receipt, source and provisional cache occupied all three mounted SD file
slots, and `step(1)` could parse an entire invisible-markup prefix before a
page appeared. The fixes retain the three-descriptor mount and source identity
rules, the existing arena, XML budgets and RAM/stack ceilings.

A due outer pause now syncs and snapshots the private cache writer, then closes
only its descriptor. Its generation, O_EXCL temporary name, payload length and
rolling CRC remain owned; source and authoritative receipt stay open. Progress
and battery work run with two SD descriptors live. Only a successful duty with
no pending stop permits an existing-only O_RDWR reopen. It checks the exact
retained length and snapshot before further appends at retained payload offsets.
The complete final CRC reread remains mandatory before publication, so a
same-size/time substitution cannot become authoritative. The buffer and parser
keep their arena ownership throughout. Producer save failure leaves
`lastProgressSave` unchanged, stops production without reopening the writer,
and suppresses the post-production catch-up retry; main's next loop handles it.
Ordinary callers retain their prior failed-save retry cadence. Only successfully
serviced and resumed pauses count in the unchanged `PC attempt` row's pause field.

The tracked FreeInk producer patch adds an optional feed-count bound to
`ChapterLayoutSession::step`, with the default preserving ordinary page-based
stepping. The producer requests one 2 KiB feed per outer step, including feeds
that produce no visible runs. No session fields or global policy state are
added; the provenance manifest binds both changed layout files and the patch.

`chapter_cache_producer_pause_test.py` runs the real selection, producer,
`SdReadingState`, main's `saveReaderProgress` body and checkpoint/intent stores
with a shared three-descriptor model for source, cache and state. Success reads
back the durable checkpoint and continues to a complete chapter. Injected
ENOSPC stops the candidate, preserves the pending deadline, then permits the
next save after production unwinds. Controls execute the old `cbd4f18` receipt
owner (with only a caller compatibility adapter) and old save body; each fails
its respective regression. The runtime fixture retains its cancellation,
startup/durable-preference and removed-pause controls.

Only progress saves and battery sampling execute inside a pause. Input dispatch,
sleep entry, display paint, cover/library work and card detection run after the
synchronous producer releases parser, buffer, file and display leases. Queued
input stops production before dispatch. The durable-core checkpoint deliberately
skips derived identity/estimate publication, so it never reenters the page port
or borrows parser/display storage. X3 battery sampling uses the BQ27220 gauge
on I2C, independently of the SD/display SPI bus and without file descriptors.
All these ownership conclusions concern source/host evidence; physical power
and bus behavior still require separate device checks.

The same paused fixture compiles the pinned `BatteryMonitor.cpp` against the
actual X3 board configuration in isolated host translation units. Scripted
`TwoWire` responses verify the three BQ27220 repeated-start I2C register reads
(SoC, voltage and current) while the parser loan and two SD files remain live.
No SPI/file operation is added by that real driver route. Receipt removal and
replacement during the pause also refuse publication. A same-size/time private
writer replacement is caught by the final complete CRC reread; bypassing that
check fails its private-publication control. Failed saves and input during the
pause never reopen the writer, and the retained source still serves a real turn.

Three synthetic nonvisible prefixes (empty spans, nested inline elements and
short comments) exercise the real reader/producer at 6 ms per modeled storage
operation/parser poll. First pauses occur at 1,642 / 1,702 / 1,702 ms for a
1,000 ms deadline, bounded below 2,000 ms. The `cbd4f18` page-only call control
(`step(1)`, otherwise identical) first pauses at 5,854 / 6,682 / 6,370 ms and
fails that bound. These clocks demonstrate bounded host work, not device time.

The fresh R2 adversarial/maintainability review additionally identified that
reopening the receipt using only FAT hints would lose its original authority.
Keeping that descriptor and releasing the private writer instead closes that
class without an unbounded receipt scan or invalidating the foreground reader.
The storage owner's retained rolling CRC and mandatory full readback protect
all resumed output before it becomes visible. No resident state, heap buffer,
parser-arena bytes or additional descriptor slot is added. Hardware timing and
minimum-free-heap evidence remain pending.

### Step-2 R2 final resource correction

Clean source `07f1b8bce7c5c83ba7e018f83e68669fc06282c9` supersedes the earlier
unmerged step-2 source/resource record. Its four clean compilations and NVS/RTC
checks pass; before adopting the corrected record, their only resource failures
are against the inherited flash ceilings. Record 33 is corrected in place in a
forward commit, retaining the exact first 32 records and the complete protected
history. Fresh independent adversarial/maintainability/resource review
`newt21-step2-r2-adversarial-maintainability-r2` found no actionable final-source
findings and approved these exact flash-only costs, independently matching all
376 artifacts in each report. The earlier resource review did not cover these
corrections. R1 found the receipt-authority defect; its writer-release correction
is covered by R2. This is not owner performance or physical acceptance.

| Variant | Exact app bytes | Growth over step 1 | SRAM / RTC | Selected guards unchanged | Usable headroom |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 2,154,544 | 1,472 | 133,232 / 300 | 348 | 4,861,904 |
| dev-usb | 2,294,320 | 1,536 | 134,080 / 304 | 377 | 4,722,128 |
| regional | 2,161,248 | 1,488 | 133,728 / 300 | 350 | 4,855,200 |
| regional-dev-usb | 2,303,024 | 1,536 | 134,576 / 304 | 384 | 4,713,424 |

No slack or protected budget change is included. The extra runtime code bounds
parser steps and transfers only the private writer descriptor around foreground
work. The SDK source/license pin remains intact with tracked patch provenance.
No fourth FAT descriptor, new heap buffer, larger arena or resident state is
added. XML requested peaks and normal open/turn/producer operation counts remain
unchanged from the step-2 table above. Physical minimum-free-heap impact remains
unmeasured. Final four passing clean lanes and full final-commit Linux/release
gates are still required after this audit is adopted.

The four post-adoption clean `--health-report` lanes all pass on
`663ca2c13adbd91de6f3299b60ca9fb1f7a533cd`, with `dirty: false` identities and
exactly the R2 sizes above. Quick Linux passed on the final runtime source in
211.737 s, including the real paused checkpoint/battery test, all three pause
controls, parser-work controls, unchanged turn matrix, producer/poll/storage,
power and zero-RTC runtime/startup regressions. R2 review is clean after fixing
the one R1 receipt-authority finding. `artifacts/newt21-step2-r2-verification.json`
records operation counts, review, clean identities and limits. This evidence-only
commit is followed by the full Linux gate and pinned full-adoption release gate;
their final-commit outcomes are reported in the task handoff. No hardware-ready
or owner-acceptance claim follows from these host/build facts.

## NEWT-21 step 4: Reading-session admission

Reading's existing idle wrapper is now the sole chapter-cache admission point.
After an accepted Page paint and two seconds of quiet, it releases the live
Pages descriptor before the bounded census can evict anything. The source and
receipt remain owned; admission rechecks the selected session, view, input,
power and receipt before lending provisional write units to the producer.
In the original step-4 policy a completed attempt spent the session and
quota/storage/power denials were terminal for that session. Step 5 below
supersedes this visit-wide limit with per-chapter accounting. Interrupted admission discards provisional units
and restarts the quiet clock while remaining pending. A new actual Reading
open rearms admission even when Home Read reuses the same revision owner.

The Home/Library census, pre-open allowance transfer and terminal empty-credit
sentinel are deleted. Home and Library idle work now handles covers only.
No census is added to open, turns or paint. The old seven-bit allowance field
becomes a two-bit pending/denied/spent state without adding a resident owner.
The session's Pages/receipt policy helpers live together in
`reader_revision_cache_policy.h`, keeping the existing source-line ceilings.

DEV builds emit one bounded numeric `PC admission` row per admission outcome:
`r=1` denied, `r=3` granted, `r=4` stopped; `s` is `PageCacheStop` and `u` is
verified allowance units. Quota and unknown/over-cap inventory have distinct
numeric reasons. Formatting and the TX-capacity check run in idle-owned
frames, outside open/turn/paint, and never wait for TX space. Existing census
and producer rows remain separate observations.

The new `chapter_cache_admission_test` uses the actual selection, SD owner,
revision session, reader, producer and idle wrapper with authored short, long,
multi-file and uneven EPUBs. It compares immediate and dwell opens across
Library import, warm reopening, spine-2 resume, Home Read/resume, opening
straight after boot and a second book in one boot. Failure probes exercise
queued input, DEV bytes, removed card, quota, low power, storage failure,
changed session and requested sleep. Its clock charges six milliseconds for
observed storage operations; these are modeled bounds, never device latency.

Host and compile evidence do not establish physical timing, battery sleep/wake,
USB sleep/wake, cold power-on or owner acceptance. No device was accessed or
flashed by this change; hands-on verification remains pending.

The final admission matrix produces **14/14 immediate and 14/14 dwell**.
The `c817f70` diagnostic produces **0/14 immediate and 14/14 dwell**; requiring
the fixed behavior fails there with exit 1 and the assertion
“Reading admission must produce once for all 14 immediate and all 14 dwell paths”.
Same-owner reopening rearms after both Spent and Denied. Known card loss is
Denied whether present before admission or introduced mid-census. Pending
membership identifies the unresolved live session; the common idle policy and
admission boundary own storage eligibility and explicit denial, preventing a
failed eligibility check from silently hiding unresolved card loss.

The response probe also charges the source descriptor's `fstat`, alongside
Pages close, directory operations and receipt validation. Input during census
leaves one following cleanup operation (6 modeled ms). Canonical receipt
validation leaves four following operations (24 modeled ms). The worst measured
whole-admission case is a recovered `.receipt.bak`: **six following storage
operations, 36 modeled ms**. Its two complete receipt checks each additionally
verify that the canonical name is absent. Including a triggering operation
already in flight gives **seven operations / 42 modeled ms** at 6 ms per
operation; adding the existing 10 ms Reading tick gives a conservative
**52 modeled ms** scheduling bound (46 ms after the triggering operation).
These are host-model bounds, not physical input/display latency. Stopped census
retains no credit and waits a new full two-second quiet interval.

Clean four-lane measurements of runtime source `2759c42` are:

| Lane | App bytes | Growth | Shared SRAM | RTC |
| --- | ---: | ---: | ---: | ---: |
| default | 2,157,968 | 512 | 133,232 | 300 |
| dev-usb | 2,298,064 | 848 | 134,080 | 304 |
| regional | 2,164,656 | 512 | 133,728 | 300 |
| regional-dev-usb | 2,306,768 | 848 | 134,576 | 304 |

Every selected stack measurement fits its inherited ceiling. DEV
`paintNonHomeScreen` remains 96 bytes and `openReader` 560 bytes. No protected
ceiling or exclusion changes. The single flash audit update binds the exact
four raw reports retained under `artifacts/newt21-step4-raw-code-health-*`.
Its reviewer field records independent review status without claiming owner
review. Final post-adoption clean lanes and final-commit Linux/release gates
are required after the audit is committed; their results belong to the handoff.

The fresh bounded `admission_review` (GPT-5.6 Sol high) found one actionable
card-denial gap, fixed and closed across before/during-admission cases. Its
final supplemental source/test/resource review is clean and independently
approves the exact flash-only audit above, including all four report/input/
artifact hashes and every inherited selected stack guard. Removing this one
record and restoring its four prior flash values reproduces the entire
`c817f70` baseline exactly. This is engineering review, not owner acceptance.

All four post-adoption clean `--health-report` lanes pass on `d9c5fcc7b45a1ad0093b5895f6eda82a0880c882` with
`dirty: false` firmware identities and exactly the sizes above. The quick Linux
gate passes on that commit in 271.145 seconds, including the strengthened
canonical-receipt four-following-operation clock, both card-denial cases, 28-session matrix,
unchanged 192-cell turn-cost regression, paused checkpoints and startup/zero-RTC
controls. The existing 10 ms Reading tick delay follows stopped admission work;
the modeled storage-operation bound is separate from that cadence and from
physical input/display latency. `artifacts/newt21-step4-verification.json`
retains the evidence and limits. This evidence-only commit is followed by the
full Linux gate and the pinned full-adoption release gate; their final-commit
results are reported in the task handoff.

The matrix clock explicitly advances 1,000 ms before immediate opens and
2,100 ms before dwell opens in both API modes; those labels never substitute
for the modeled pre-open delay. This test-only refinement changes no firmware
or compiled resource input. Final Linux validation follows its commit.

The full-suite cover wrapper fixture's obsolete census-credit expectations are
replaced by cover-only checks before/after quiet, with input, reader progress,
and Reading handoff. Its selection seam exposes no chapter-admission API.
Focused DEV/non-DEV wrapper checks and the explicit 1.0/2.1-second admission
matrix pass. These refinements affect tests/evidence only; a source diff proves
compiled firmware inputs remain identical to `d9c5fcc`. Final quick/full Linux
and pinned release gates run after this verification-only correction.

Round 2 on clean `67716b7` addresses the independent review's LOW finding:
the prior whole-admission bound covered canonical receipts only. The response
matrix now exercises canonical alone, canonical plus backup, recovered backup,
and recovered backup plus an incomplete temporary replacement, each with input
and a DEV byte at the first receipt stat. Both recovered layouts must demonstrate
exactly six following operations; both canonical layouts require four. Missing
canonical to `.bak` is the sole supported recovery route: `.tmp` never supplies
authority, and canonical takes precedence when both complete names exist.
Additional cases change either recovery name during validation, verify no credit
or output, and verify renewed admission rejects the stale authority. Stopped
valid cases require a new full quiet interval before spending one attempt.

Firmware remains byte-for-byte source-identical to `67716b7`; the corrected
52 ms conservative host-model bound does not warrant a runtime cancellation
change before device measurement. Round 2 runs the focused admission regression,
quick Linux gate and pinned full-adoption release gate. The passing four clean
lanes and full Linux gate on `67716b7` remain prior evidence; they are not rerun
for this tests/documentation-only correction. Device latency remains unmeasured.


## NEWT-21 step 5: chapter retry budget

The selector lazily clears one fixed 256-byte, two-bit-per-spine counter set in
its existing heap-owned revision session, after two seconds of quiet. It covers
the existing admission maximum of 1,024 spine items regardless of actual book
length; no allocation/table grows with the book, and open does not clear it.
The existing selector flag byte gains one visit-initialized bit without changing
its resident size. The session heap allocation grows by 256 bytes, counted by
its existing `ownedHeapBytes()` reserve-credit contract. No parser, arena, RTC,
static SRAM or selected stack ceiling is increased. Settings changes use the
existing actual close/prepare/open transaction, hence begin a new visit under
the new exact cache identity. A view/settings change during production interrupts that attempt; progress/battery deadline pauses retain the same attempt.

A fresh census precedes **each** attempt, including retries and newly reached
chapters. This is the simplest quota rule: interrupted writes never leave
reusable allowance, cleanup/eviction and metadata growth are reflected again,
and the ordinary turn path needs no hook or storage work. Before census, a read-only probe uses the existing page port with its scoped
producer cancellation policy. It validates the canonical or supported `.bak`
candidate's complete storage CRC/footer; confirmed corrupt output retains the
existing repair behavior. A found or uncertain candidate ends this chapter
without a production attempt. This probe grants no page-publication authority:
ordinary reading still checks source, receipt, generation, index and page data.
Every filesystem transfer, CRC chunk and cleanup boundary keeps the existing
input polling; there is no uncancellable cache CRC scan in admission.

The attempt's reasoned stop latch is the sole interruption authority. Its typed
hook distinguishes a real input/DEV/view/session interruption from terminal
power/card/deadline refusal. The pause hook returns a typed result, which is
settled before any later cancellation poll. An I/O boundary that already
latched interruption retains precedence; housekeeping failure alone never
creates a retry. The first latched reason cannot be replaced by a later poll.
After
all parser, receipt and page-port cleanup unwinds, the selector captures it once
in `ProducerOutcome`; try accounting and idle retry scheduling use that same
snapshot. A cancellation consumes one of the three tries irrespective of the
returned parser/storage status (including `Truncated` from `finish()` or a
suppressed transfer after seek). Completion exhausts the chapter. An outcome
without cancellation is terminal, including unsupported (before #175, over-128-page) or style
refusal, real truncation, stale source/receipt and storage errors. There is no
status whitelist or independent retry-refusal flag. A real failure coinciding
with cancellation remains bounded by the three-try cap; the next uncancelled
failure ends the chapter. Every retry reacquires admission and checks authority.

Admission has its own synchronous cancellation latch, covering candidate CRC,
census, mandatory directory close and receipt validation. A cancelled admission
stays pending and consumes no producer try; an uncancelled failure/uncertain
candidate or quota/card/power denial ends the chapter. Idle service still requires
a valid Reading view and power before any new admission. All attempt-local
references and provisional output are released before the renewed quiet clock.
The existing numeric telemetry rows and foreground frames are retained.

Round 3 also makes admission's latch carry its stop kind and names the census
latch `cancellationObserved_`: only its cancellation callback can set it. The
producer/admission/census audit finds no other mixed failure/cancellation latch
on this path. SD policy's `failed` flag records storage failure only and grants
no retry authority. The foreground open controller already retains explicit
queued/deadline bits; cover generation retains its separate heap-refusal cause.
Those independent lifecycles do not decide chapter producer retry eligibility.

Wear is bounded in acknowledged cache payload bytes, not physical flash-program or
FAT-sector counts. One attempt writes at most **4,194,316 bytes** (4 MiB payload
plus the 12-byte SD footer); at most three write candidates per chapter is
**12,582,948 bytes**. Retry overhead versus one try on that chapter is at most
**8,388,632 bytes**, two additional provisional creates/cleanup sequences and
two additional censuses. For N distinct chapters reached in a visit (N <= 1,024),
the total is <= 3N candidates / 12,582,948N bytes (12,884,938,752 at the maximum).
Compared with the old *single visit-wide* candidate this permits at most 3N-1
extra candidates / 4,194,316(3N-1) bytes. Each census retains its existing bound
of 128 deletions / 129 scans; interrupted censuses can repeat after quiet but
write no cache payload. Existing progress/battery duty service retains its
per-attempt eight-pause/120-second bounds. Filesystem metadata, failed requested
transfers (including the existing at-most-four EINTR attempts), ordinary progress
writes and card-internal amplification are separate
from the cache-payload ceiling; no physical wear measurement is claimed.

Host and compile evidence remain separate from physical acceptance. No device
is accessed or flashed. USB sleep/wake, unplugged battery sleep/wake, cold
power-on and physical responsiveness remain pending for this candidate.


The focused retry matrix uses actual successful SD writes and a six-ms-per-
observed-storage-operation clock. Current input and DEV interruptions complete
on renewed quiet; three interruptions produce no fourth attempt, including a
return after another chapter and the spine-3/4 counter-byte boundary. Turn,
direct jump and Chapters-menu crossings each complete the new chapter; all five
corresponding controls fail on a complete `dee4232` firmware shadow. Cached CRC
interruption, terminal over-cap/write errors, settings/reopen and sleep/wake
reopen cases pass. Round 2 replaces round 1's combined-fault precedence controls: cancellation
must win over an error status, while the same genuine fault without cancellation
must remain terminal. The exhaustive boundary sweep and exact results are
recorded with the round-2 verification below.
The prior lifecycle fixtures retain their actual startup, durable zero-RTC,
checkpoint and descriptor-lifetime controls with the new selector interfaces.

Modeled build interruption leaves three following observed storage operations
(18 ms); interruption during candidate CRC leaves one (6 ms). The heterogeneous
admission matrix still has the original worst case of six following operations
(36 ms), seven including the triggering operation plus the Reading tick:
**52 modeled ms**. These are deterministic host work/cadence bounds; real FAT
blocking time, physical input/display latency and device power acceptance remain
unestablished. The unchanged 192-cell turn-cost matrix is required by the final
Linux gate, along with all existing pause/poll/storage/source-authority controls.


Clean four-lane measurements of runtime source `30768c3` are:

| Lane | App bytes | Growth | Shared SRAM | RTC |
| --- | ---: | ---: | ---: | ---: |
| default | 2,159,424 | 1,456 | 133,232 | 300 |
| dev-usb | 2,299,552 | 1,488 | 134,080 | 304 |
| regional | 2,166,128 | 1,472 | 133,728 | 300 |
| regional-dev-usb | 2,308,256 | 1,488 | 134,576 | 304 |

Every inherited selected stack measurement fits its ceiling. DEV `openReader`
remains 560 bytes and `paintNonHomeScreen` 96 bytes. Some unselected producer
frames grow by 16 bytes; all selected guards and the authored census upper
sums (2,304 bytes non-DEV / 2,384 DEV) fit unchanged ceilings. These static
measurements do not establish runtime stack peaks. Static SRAM and RTC are
unchanged; the heap-owned revision object adds the fixed 256-byte ledger, and
producer source extent uses eight previously unused tail bytes. No protected
budget/history, selected entry, exclusion or adapter is changed. One exact
four-lane flash-only audit binds the raw reports under
`artifacts/newt21-step5-raw-code-health-*`. Its independent review status is
recorded in the audit, without claiming owner review or physical acceptance.

The round-1 review claims are superseded by the independent round-2
brief: two reproduced cancellation misclassifications (post-seek transfer and
chapter finish) belong to the same competing-authority defect. Round 2 removes
that authority split and adds structural cancellation-boundary coverage.

### Round-2 heap accounting

The ledger is an inline 256-byte member of `ReaderRevisionSession`, allocated
with the session's single `new(std::nothrow)` in `LibraryReaderSelection::makeSession`,
not at the first quiet tick. The first quiet admission only clears the member.
It lives until that session is reset/destroyed (selection/source close or replacement);
a reader close alone may retain the session, and reopening clears the visit in
idle. The allocation is made during selection/retained-source preparation,
before the reader heap guard. There is no separate ledger allocation or ledger
allocation-failure branch: session allocation failure returns null. Selection
falls back to full digest/strict reading as before; `bindSession` can admit the
strict path without a session, after rehashing and rechecking the heap. Optional overview preparation
also skips on allocation refusal. `ownedHeapBytes()` includes the extra 256 B
in the existing reserve-credit contract; that credit does not restore physical
free heap.

The allocation request and its live build footprint grow by exactly 256 B
(the session alignment is unchanged; no second allocator header). At a given
otherwise identical parser/SD peak this lowers free heap by 256 B, before any
allocator fragmentation differences. Relative to the reported approximately
800 B margin above the 50,468 B exception floor, the arithmetic remainder is
approximately **544 B**. This is an accounting bound, not a device measurement
or a guarantee against fragmentation; the floor remains 50,468 B. No new
physical heap-low-point evidence is claimed. The old 65,536 B product target
also remains unchanged.

No safe heap-free placement is established in the current storage layout:
static/RTC placement would need another 256 B against unchanged ceilings, and
the existing 256 B revision scratch is live source-verification storage. The
parse/display loan and page/style/selection slots are overwritten by foreground
work and cannot retain a per-visit ledger across arbitrary chapter revisits.
A separately proved lifetime-safe reuse could avoid the added heap cost, but
this change does not claim such a placement or reduce existing arena capacities.


### Round-2 regression and clean resource evidence

Committed runtime/test source `ae80b526ae483e9c4a57ae5f2c2217762af7115a`
passes the expanded real-wrapper regression. The dedicated six-paragraph
structural fixture retains the fourteen existing heterogeneous fixtures and
reaches every required seam. A successful trace is replayed with pure input
cancellation at **every** observed pre/post syscall, parser finish/EOF,
CRC/promotion, census/admission and live cancellation observation. All live
input/DEV observations, including XML/layout scheduler checkpoints, converge on
the instrumented `Poll::stopReason()`; the test numbers these observations too.
It does not rely solely on operation labels or one selected callback per class.

The build trace has **2,811** replay points: 2,804 stay pending and seven arrive
after complete cache settlement. The existing-candidate trace has **132**:
131 stay pending and one arrives after complete validation. Each pending case
checks zero tries for admission, exactly one for a started build, no new partial
artifact, a full renewed two-second quiet period, and eventual complete output
or validation. Already complete late cases retain a complete validated cache
without rewriting. Three interrupted builds still prevent a fourth attempt.

Eight explicit uncancelled fault classes (seek, sync, stat, rename, actual
chapter finish, confirmed EOF, CRC and promotion) are terminal with count three,
no partial output and no later retry. Existing write/zero-transfer,
underlying-source, candidate and all six census syscall failures also retain
uncancelled terminal coverage. Cancellation coinciding with a real source read
error rearms once; the retained source error then terminates the uncancelled
retry. This does not manufacture source recovery or publish stale output.

`chapter_cache_retry_test.py --baseline-controls --round1-controls` passes.
Separate `31a23c4` shadows fail independently at post-success seek and at actual
`ChapterSource::finish()` cancellation. All five `dee4232` input/DEV and chapter
crossing controls still fail as expected. Current input and DEV interruptions
stop after **30 modeled ms**, candidate interruption after **6 modeled ms**;
the separate heterogeneous admission suite retains its **52 modeled ms** bound.
The clock conservatively charges both sides of observed transfer calls; real
FAT blocking and physical input latency remain separate hardware evidence.

Four clean `--health-report` builds of the clean committed source above pass:

| Lane | Exact app bytes | Change from round 1 | SRAM | RTC |
| --- | ---: | ---: | ---: | ---: |
| default | 2,159,168 | -256 | 133,232 | 300 |
| dev-usb | 2,299,312 | -240 | 134,080 | 304 |
| regional | 2,165,872 | -256 | 133,728 | 300 |
| regional-dev-usb | 2,308,016 | -240 | 134,576 | 304 |

One four-lane flash record tightens the exact ceilings and binds archived raw
reports `artifacts/newt21-step5-r2-raw-code-health-*.json`. All inherited selected
stack measurements and ceilings, RAM/RTC, protected history, exclusions and
adapters remain unchanged. The admission latch uses existing Poll flag padding;
its frame remains 48 B non-DEV / 128 B DEV, and DEV open/paint remain 560/96 B.
The redundant eight-byte attempt-tail source extent is removed. The persistent
256-byte session ledger and its heap caveat above remain; no floor is extended.

The fresh bounded adversarial/maintainability/resource review and final committed
source gates are recorded in the task handoff after the audit. This is host and
compile evidence only. Mac ImageIO coverage, physical heap minima, USB/battery
sleep-wake, cold power-on and device/owner acceptance remain unestablished for
this round; hands-on verification is pending.


The fresh bounded independent GPT-5.6 Sol high adversarial, maintainability and
resource review `newt21-step5-r2-review-1` is **CLEAN**, with zero actionable
findings in `31a23c4..b25ac5e`. It independently accepts flash record 38, all four
raw report hashes, all sixteen live bin/ELF/map/partition hashes, clean build
identities, linked guard metrics and the unchanged complete protected history.
It also confirms the ledger lifetime/failure path and heap-margin arithmetic;
no safe heap-free placement is established. This is engineering review only.
The subsequent `9bf3a7b` test-only extraction moves the unchanged structural
cases into `chapter_cache_retry_boundaries.h` to satisfy the existing 700-line
ratchet; no production code or assertions change. Final host gates compile and
exercise that organization. The final four-lane rebuild and host/release gates
follow this review record; their results are reported in the task handoff.


All four post-review clean `--health-report` lanes pass on committed source
`00297bf939f921d9095c1c9bfc10d74722b2a29d` with exactly the round-2 sizes above,
unchanged SRAM/RTC and every selected guard retained. The current firmware
identity and health receipts bind those final clean builds. The archived raw
record-38 measurements remain bound to `ae80b52`; this rebuild verifies the
reviewed exact ceilings without adding another audit record. The final receipt
commit changes evidence/documentation only. Quick/full Linux and pinned adoption
history gates run on that final commit before handoff; Mac and physical evidence
remain separate and pending.

### Round-3 stop reason authority

The independent round-2 review found the reverse classification defect: a
failed pause used the same boolean latch as genuine cancellation. Thus a writer
sync/suspend/reopen or unacknowledged duty could incorrectly rearm a chapter.
`ProducerStop` now represents running, interrupted and terminal states in one
byte. A callback must name its stop kind; `interrupted()` is true only for the
interrupted value. The pause returns a typed result; terminal failure is
settled before any later poll. I/O that already latched interruption keeps
precedence, and the latch never changes after its first recorded reason.
The selector still captures one outcome after cleanup and uses that snapshot
for both try accounting and quiet retry scheduling.

No open/turn/paint hook, growing state, new allocation, persistent record or
telemetry row is added. Admission keeps its reason byte in the existing Poll
padding. The fixed 256 B session ledger, fresh census before every attempt,
three-try wear bounds and heap-margin caveat above remain unchanged. The
existing Card/Power priority when first observing simultaneous conditions is
retained; an already latched interruption cannot become terminal afterward.

The round-3 regression uses the production SD reading-state backend and real
`ExternalProgressStore` transactions, plus the pinned X3 BatteryMonitor with
scripted I2C transport. A successful live pause contains 212 observations,
including cancellation polls and repeated storage boundaries; all 212 are
replayed with input and DEV (424 cases). Each remains pending with one charged
try, no visible partial output, and completes after the full renewed quiet.
Seven genuine transport failures (writer sync/suspend/reopen and progress
open/write/sync/rename) settle at three tries with no later attempt or writes.
Fourteen same-site failure-plus-input/DEV cases exercise the observation
ordering. An already observed interruption retains precedence; a merely queued
request cannot override a failed duty acknowledgement.
All seven independent `47d5cdc` controls fail the terminal-settlement oracle.

Three real battery transaction failures retain valid power and complete, as the
existing product policy permits. Six pure battery-transaction cancellations
remain pending and later complete. Lost power, an unacknowledged battery deadline,
and simultaneous unobserved input with lost power settle terminally; the last
case preserves the existing Power-first observation order. This distinguishes
transport failure from an actual producer stop without inventing battery-driver
failure semantics. Host replay is not physical I2C, SD or power evidence.

Clean pre-adoption four-lane builds of `a1639b16d264dcca009528d90a1871ceb4cd5598`
compile successfully and pass all protected guards; they fail only the prior
flash ceilings. One flash record (39) binds their exact raw reports and all
sixteen bin/ELF/map/partition hashes, preserving the full 38-record prefix.
The reviewer field remains pending until the independent review completes.
The build driver publishes the firmware-identity receipt only after the health
gate passes, so the pre-adoption failures leave those receipts at round 2;
post-review clean lanes must replace them before handoff.

| Lane | Exact app bytes | Growth from round 2 | SRAM | RTC |
| --- | ---: | ---: | ---: | ---: |
| default | 2,159,344 | 176 | 133,232 | 300 |
| dev-usb | 2,299,472 | 160 | 134,080 | 304 |
| regional | 2,166,048 | 176 | 133,728 | 300 |
| regional-dev-usb | 2,308,176 | 160 | 134,576 | 304 |

Every inherited selected stack measurement is byte-for-byte unchanged, with all
349/378/351/385 selected guards retained. Admission stays 48/128 B and DEV
open/paint stay 560/96 B. SRAM/RTC, protected history, partition/reserve,
exclusions and adapters are unchanged. The archived raw reports are
`artifacts/newt21-step5-r3-raw-code-health-*.json`.

The combined focused run passes all original and round-1/round-2 controls.
The non-pause structural build trace now has 2,815 points (2,808 pending,
seven already complete), and candidate validation has 134 (133 pending,
one already complete); additional observed opens/closes explain the expanded
trace. Input/DEV unwind is 36 modeled ms and candidate interruption 12 modeled
ms with the expanded six-ms transport clock; the heterogeneous admission bound
remains 52 modeled ms. The 192-cell turn matrix and final host/native/release
gates are required before handoff. Hardware timing and heap/power acceptance,
and the exact-source Mac ImageIO gate, remain pending.


The first fresh GPT-5.6 Sol high review (`newt21-step5-r3-review-1`,
`47d5cdc..9ef944e`) accepted the initial exact resource measurements, but found
one medium ordering defect and one low scratch-lifetime defect. The initial
boolean pause result polled for input after failure, allowing later input to
replace terminal settlement. Pause results now carry their reason; failed duty
acknowledgements are checked before observing new input, and terminal settlement
precedes the next poll. Real pause I/O can still latch interruption first.
The retry test restores scoped scratch cleanup. New failure-first controls and
a fresh review of the revised source are required before final acceptance.
The round's single pending flash record is rebound to the revised measured
source, preserving all 38 inherited flash records and all protected history.

The revised ordering matrix passes all 424 healthy-pause cancellation replays.
Four sync/reopen cases observe input/DEV inside producer I/O first and remain
pending; ten suspend/progress cases only queue input/DEV before returning a
failure and settle terminally. Fourteen additional cases queue input/DEV after
the failed pause result has been captured: all settle at three tries with no
later attempt or write. Successful progress work interrupted by input still
retries. All seven independent late-order controls fail on `9ef944e`, and the
seven original `47d5cdc` controls still fail as expected. Scoped
`TemporaryDirectory` cleanup replaces the scratch leak; no diagnostic retention
is enabled by default. No test adds a producer checkpoint inside progress I/O.

The fresh revised-source GPT-5.6 Sol high review
`newt21-step5-r3-review-2` is **CLEAN**, with zero actionable findings in
`47d5cdc..19be6fe`. It accepts record 39 after independently rehashing all four
raw reports and sixteen live core outputs, confirming clean `a1639b16d264`
DEV IDs, every selected stack actual/ceiling and entry set, unchanged SRAM/RTC,
the complete 38-record prefix and all 23 protected updates. It confirms the
first-reason authority, failure-first ordering coverage and scoped scratch
lifetime. The actual reviewer is now recorded in the audit; no owner or physical
acceptance is claimed. Two bounded fresh review rounds found and fixed one
medium ordering issue and one low test-lifetime issue. Post-review native and
final host/release gates follow this record.

All four post-review clean `--health-report` lanes pass on committed source
`6fac474` at the exact round-3 sizes above. Current firmware identities are clean
and bind the refreshed reports and linked guard outputs. Every inherited
selected stack actual is unchanged from `47d5cdc`, as are SRAM/RTC. The raw
record-39 archive remains bound to `a1639b1`; the final builds independently
verify its reviewed ceilings. This receipt commit changes only evidence and
this documentation; firmware source remains identical to the reviewed candidate.
Quick/full Linux and pinned adoption-history gates run on this final commit
before handoff. Mac ImageIO, physical heap minimum, device input/SD latency,
USB sleep/wake, unplugged battery sleep/wake and cold power-on remain pending;
this is not owner or hardware acceptance.


## Step 5 round 4: returned results precede later input

The independent round-3 Lead review identified failure laundering outside the
pause path: producer `step`/source reads polled after a returned failure, and
selection tested cancellation before returned probe/census denial. Round 4
makes the ordering rule explicit: classify a returned result before the next
poll. A failure preserves an interruption already observed during the operation;
otherwise it settles terminally. Merely queued input after the failure cannot
rearm the chapter. A denied admission exhausts it immediately, including failures
before any attempt starts.

`ProducerCancellation::result` owns attempt settlement. The existing experimental
FreeInk processing policy forwards lower chapter-source results to that same
owner without changing object size or the pinned vendor tree. The scoped SD
policy reports failure and observed cancellation at the existing I/O boundary;
raw syscall results retain ownership information and are classified by their
operation before another poll. Normal absence, name collision and bounded EINTR
retries remain distinct from returned operation failure. Cleanup still closes
owned descriptors and removes only owned provisionals. Census retains its first
Stopped/Denied result across mandatory directory close. Admission checks Denied
before polling again, observes release errors, and settles a failed live check
before later input.

Sweep inventory (all checked, including success and cleanup):

| Site | Settlement before the next poll |
| --- | --- |
| Generic producer step and source `readAt` | Result hook before checkpoint; invalid read counts fail |
| Cache existence, size, read, begin/write/flush/end | Absence is a successful probe; actual failures settle through the owner |
| SD seek, transfer, EINTR exhaustion, zero/overcount | Caller classifies before freshness/permission checks |
| SD open/reopen, descriptor/path stat, sync, close | Semantic result precedes polling or cleanup |
| XML feed/layout step and page/anchor refusal | Returned status/refusal settles before XML post-poll or pause |
| Chapter decoded read, EOF and finish | Lower source records failure/cancellation before post-poll |
| Receipt/live checks, final receipt exit | False/status result settles before later producer polling |
| CRC/footer validation and checked read exit | Private-writer checksum failure settles before post-CRC poll/close; invalid derived input is a negative probe |
| Rename, destination/backup checks and promotion | Preserve actual ownership; classify refusal before permission poll |
| Candidate probe and cleanup | Local failure or observed cancellation survives close |
| Admission page release and live check | Returned failure denies before cancellation poll |
| Census stat/open/next/close/unlink | First result survives later polls and mandatory cleanup |
| Selection probe/census outcomes and grant | Denied settles before `stopped`; only successful progress polls again |

The fixed ledger, fresh census for each uncached chapter, three-try ceiling,
wear bound, quiet interval and heap-margin caveats above remain unchanged.
Resource measurements, focused before/after controls and fresh review follow
before handoff. USB/battery/cold-power behavior, physical heap minimum and input/SD
latency remain unverified; host evidence does not establish hardware acceptance.


The complete current retry suite passes. Each of the 21 round-4 cases fails
independently on frozen `f1c25e8`: eleven producer operations, six census
operations, and probe/census/live/release results. The two `31a23c4`, seven
`47d5cdc` and seven `9ef944e` controls also fail at their intended assertions.
The confirmed admission miss uses the actual Ready return, spends zero attempts
when later input arrives, and completes after renewed quiet. Pure-cancellation
coverage remains exhaustive; fault-plus-input positives explicitly observe the
attempt policy before returning the fault, while merely queued faults are
terminal. Real EIO retains its existing source invalidation; the one-shot zero
read can recover. No producer callback is inserted into progress I/O.

The current build sweep has 2,499 points (2,492 pending, seven complete), the
candidate sweep has 108 (107 pending, one complete), and healthy pause replay
covers 176 boundaries with 352 input/DEV cancellations. Maximum modeled producer
unwind remains 36 ms and candidate unwind 12 ms, using six-ms transports. These
are host-model observations, with the earlier 52-ms heterogeneous admission
bound; physical timings remain unmeasured.

The first cost build exposed a selected `validate()` frame of 32 B against its
unchanged 16-B ceiling. Those receipts were withdrawn; no cost or budget was
adopted. The existing immutable-read checked exit now remains out of line and a
failed checked exit returns directly, avoiding an extra live result. The C3
probe measures both `validate()` and `finishImmutableRead()` at 16 B. Full clean
lanes must confirm every protected measurement before resource adoption. No
function, measurement or ceiling is removed; per-function frames do not prove
call-chain peaks or physical stack safety.

The storage suite passes its actual transfer/CRC/transaction/cancellation tests
and all failure-order/cause controls. Plain and producer `commit()` frames are
still exactly 32 B. The historical capturing-closure comparison now also
measures 32 B, so it is recorded as a compiler comparison rather than falsely
claimed to violate the guard. The enlarged compiled census control and the
resource ratchet continue to reject real stack growth; the production commit
ceiling remains unchanged.


The runtime integration retains idle repair of corrupt derived canonical data.
A confirmed invalid cache is a successful negative probe, like confirmed
absence: admission may return Ready and replace it. Actual uncertain I/O is
Denied; a private writer's failed CRC is Terminal before a later poll. These
meanings use the existing Invalid/Unknown lookup and invalid-extent results,
without a new policy mode. A runtime regression exposed an over-broad failure
notification during the first implementation; the original repair oracle and
its removed-CRC negative control remain enforced.


Clean pre-adoption lanes on `43629f1` pass every protected guard and fail only
superseded flash ceilings. Record 40 is the sole round-4 flash update, binding
all four byte-exact raw reports and sixteen bin/ELF/map/partition hashes. Its
39-record prefix and all 23 protected updates are unchanged. No preliminary
measurement was adopted. The fresh independent review below accepts these
exact costs; no owner review is claimed.

| Lane | Exact app bytes | Growth from round 3 | SRAM | RTC |
| --- | ---: | ---: | ---: | ---: |
| default | 2,160,080 | 736 | 133,232 | 300 |
| dev-usb | 2,300,192 | 720 | 134,080 | 304 |
| regional | 2,166,752 | 704 | 133,728 | 300 |
| regional-dev-usb | 2,308,896 | 720 | 134,576 | 304 |

All 349/378/351/385 inherited selected stack actuals and ceilings are unchanged,
including validation 16 B, commit 32 B and DEV open/paint 560/96 B. Validation
and the existing outlined checked exit each measure 16 B. No RAM/RTC,
partition/reserve, exclusion, adapter or ledger change accompanies this fix.
The archives are `artifacts/newt21-step5-r4-raw-code-health-*.json`. Pre-adoption
failures leave old firmware-identity receipts unchanged by design; post-review
passing clean lanes must refresh them before handoff.

The final focused combined command passes all five original `dee4232` controls,
the two/seven/seven round-1/2/3 controls, and all 21 independent round-4 controls.
Actual runtime integration passes repair and removed CRC/temporary/destination/
identity controls, idle/handoff/attachment controls, and the compiled cold
zero-RTC durable-preference startup wiring. Producer I/O and storage suites also
pass. Full Linux, final clean native and pinned-history gates follow review;
physical acceptance and the exact-source Mac ImageIO gate remain pending.


Fresh bounded GPT-5.6 Sol high adversarial/maintainability/resource review
`newt21-step5-r4-review-2` is CLEAN on exact target
`6e1344eca5b52d720200705442ee8f4c8ccd89c6` (`f1c25e8..6e1344e`). It accepts
record 40, all four raw-report hashes, all sixteen live core hashes and the
unchanged complete protected history and resource actuals. Two fresh review
rounds were used: the first withheld final acceptance while evidence was being
finalized; the second reports zero actionable findings and accepts the completed
packet. No reviewer-requested fix or unresolved finding remains. This is
engineering/resource review only. Post-review clean lanes and final host gates
still follow; hardware and owner acceptance remain unestablished.


Post-review clean native lanes PASS on
`3f61facecb5f7b082f2e0795cfc215ec7863bed0` in all four variants, each using
`firmware-build.py --clean --health-report` and its variant flags. Refreshed
firmware identities name that clean commit (`dirty: false`). Exact flash sizes
remain 2,160,080 / 2,300,192 / 2,166,752 / 2,308,896 B; SRAM, RTC and all
349/378/351/385 selected stack actuals are identical to round 3. All sixteen
live core hashes match their new reports. The first 39 flash records, all 23
protected updates and every non-flash resource budget remain byte-equivalent.
This receipt-only handoff adds no firmware changes after the reviewed target.
The final receipt commit is the target for quick/full Linux and pinned-history
gates; their actual outcomes are reported at handoff. Physical and exact-source
Mac ImageIO evidence remain pending.


The first final quick gate exposed a stale DEV telemetry negative control. Its
old mutation inserted a poll after the newly canonical failure callback, where
the failure latch correctly suppresses polling; a reused census fixture also
retained prior inventory. The control now moves the poll before the failure
callback and uses its own empty fixture root. The unchanged real telemetry
oracles and the intended first-stop assertion pass, and the reversed-order
mutation fails that exact assertion. This test-only correction changes no
firmware or measured resource cost; final gates are rerun on its commit.


Fresh delta review `newt21-step5-r4-test-delta-review-3` is CLEAN on
`6e1344e..331ab86`; the telemetry mutant preserves its exact failure oracle and
previous production/resource acceptance remains valid. The completed first
quick invocation also found two stale compatibility anchors: the historical
pause control still extracted the former void admission-release signature,
and the read-budget mutant no longer removed the relocated post-transfer
read-live check. The pause control now uses the checked bool signature; the
read mutant asserts both canonical read-step/read-live sites exist and removes
them together. No production algorithm or assertion changes. Focused pause and
cache-read I/O suites PASS, including every retained negative control and the
intended positive-short-read deadline failure. A final fresh delta review and
quick/full gates follow this test-only commit.


Fresh delta review `newt21-step5-r4-test-delta-review-4` is CLEAN on
`331ab86..b130e99`, preserving prior production/resource acceptance. The next
quick run reached an older receipt-recovery combined-event assertion: it queued
input while changing receipt authority, before cancellation had been observed,
yet required Pending. Round 4 requires the returned stale-authority failure to
settle first. Both canonical-appears and backup-disappears cases now require
Denied, all three tries charged and no work after later quiet. Pure input/DEV
receipt checks still require pending and retry; the 14-by-two heterogeneous
admission matrix and all card/quota/power/storage/session/sleep cases PASS.
Admission remains bounded by six following observed operations (36 modeled ms;
52 ms including triggering operation and Reading tick). No firmware changes
follow the four accepted clean builds. Final gates and bounded delta review
run on the resulting test-only commit.


Fresh delta review `newt21-step5-r4-test-delta-review-5` is CLEAN on
`b130e99..8369e94`, independently confirming the stale-authority-before-poll
expectation and preserved production/resource acceptance. The final original
`dee4232` retry control compiles this shared admission helper but predates the
chapter ledger API. Its frozen wrapper now declares that legacy visit allowance
explicitly; only the unavailable ledger-count assertion is excluded there.
Current and all round-1/2/3/4 binaries retain the three-try assertion, and both
denial/no-later-work checks remain unchanged in the shared helper.


The final combined retry command PASSes all current cases and all 42 expected
historical failures on `d352ab1`; its quick Linux gate PASSes too. Fresh delta
review `newt21-step5-r4-test-delta-review-6` is CLEAN on `8369e94..d352ab1`,
confirming the pre-ledger compatibility guard is limited to `dee4232`. The full
Linux run then exposed one more stale exact-source mutation anchor in
`receipt_transaction_test.py`: the seek guard now settles its returned result
before polling. Both mutation arms now retain that settlement and remove only
the same post-seek authority check as before. All four receipt configurations,
all positive/failure cases and intended negative-control assertions PASS in
the focused run. This changes only the test anchor, with no firmware, resource
or assertion change. The resulting commit is the exact final review and
quick/full Linux plus pinned-history gate target; actual results are reported
at handoff. No hardware or exact-source Mac ImageIO acceptance is implied.

## Derived records beside Pages/ (#179)

The card cache root `/Newt/Cache` holds `Imported/` (receipts), `Pages/` (page
caches only) and `Meta/` (derived book records). `Meta/` holds the
progress estimate (`pe`), semantic table (`sm`) and checkpoint identities
(`c0`–`c2`), with the same names, `.bak` and `.tmp` siblings and old-or-new
promotion as before. `cacheFolder()` sends every non-page name there. The
name is no longer than `Pages`: a semantic record's temporary path nearly
fills FilePort's 256-byte path, so a longer folder would shrink the roots that
fit (the gate's scratch roots already sit at the edge). `Meta/` is outside the 32 MiB page quota and the census; its records are a
few KB per book. `/Newt/State` is untouched. DEV `purge-cache` clears it too.

Before #179 these records shared `Pages/`. Each counted against the census's
128-name window, so about 43 books with three records each denied every new
page (#175 name-cap study). Pages now carry no record names.

**Legacy reads.** A lookup reads the old `Pages/` name only when `Meta/`
has neither the canonical nor its `.bak`. A damaged `Meta/` file (CRC or
extent) stays a miss and never falls back to an older value. The legacy copy is
validated like any record. A record miss therefore opens four names, not two;
the existing two-name absence memo still makes repeats free.

**Adoption.** A validated legacy read is copied, not renamed (FAT has no atomic
cross-directory rename), into `Meta/` through the normal temporary and
promotion writer. That happens at `close()`, when its read lease ends outside
any checked scope (Home loads and the reader's `closeMetadata()` both close).
The copy uses one transient heap block, the record plus 128 B for its name
(about 4.5 KB for the largest, a 4,424-B semantic table), freed before return;
nothing stays held. A cut during adoption leaves the legacy name readable and
`Meta/` old, new or absent, so the shown value does not change. The legacy
name stays until the census reclaims it, so a later save cut that leaves
neither `Meta/` name (canonical or `.bak`) shows that older legacy value
again: complete, never torn, and accepted because the records are derived.
The Home book and any opened book keep their % read and chapter label.

**Census reclaim.** The census ranks page temporaries first, then legacy record
names, then the oldest page. Legacy names go only when room is needed (quota,
the 8-name entry reserve of "Chapters over 128 pages" or the 129th name),
oldest first by mtime, a name hash as the tie-break, and a `.bak` before its
canonical. A legacy record lost before its book is reopened is an
accepted loss: the book shows what a fresh card would. A census deletes at most
128 names, so a large legacy card needs one admission per 128 deletions.

`tests/derived_record_folder_test.py` covers the K = 0–200 sweep with 1/3/5/10
names per book, a 65-book legacy card, cold cuts through adoption, save and
reclaim, and Meta/ precedence. Its `--negative-controls` phase removes each
rule above and requires its own check to fail.

## The census converges on a full Pages/ (#175)

**Why.** On the bench card with a full `Pages/`, each census attempt ran
20–26 s, then input or the deadline cut it. Each attempt deleted about two
names and restarted from zero. The time went to card reads, not polls:

- Every scan sized each name with a `stat` by path. FatFs finds a path by
  walking the folder from its first entry, deleted slots included, so one scan
  of n names cost O(n²) entry reads (about 9 s at 128 names on the bench).
- Each scan deleted one victim, so one deletion cost a full scan. A missing
  `.bak` probe walked the whole folder again.

Deletions were durable, but an attempt that could not finish 128 scans made
almost no progress.

**Design.** Each scan reads the folder twice through `readPageEntries`
(`firmware/page_entry_directory.h`). On the card it reads FatFs entries
directly (`firmware/page_entry_fatfs.h`), and each entry carries its size and
time, so a pass costs one folder read and no path lookups. The ESP VFS
`dirent` has neither field. On the host it is `readdir` plus `lstat`.

- *Bus and cadence:* the census is the reader's only caller, so every read
  runs inside the census's idle bus scope (`pc-census`), with the same cancel
  and yield poll at each entry as before. FatFs's volume lock orders it with
  the VFS calls.
- *Names:* FatFs gives the whole long name, or the short name when the long
  one is lost or does not fit, never a cut long name. That is the name the VFS
  `readdir` gave before. A short name such as `AAAAAA~1.FIB` or `?` is unknown,
  so the census denies before deleting anything.

- *Names pass:* deletes the victims the previous scan chose, counts kinds and
  notes backups. A victim is matched by name hash, size and time, and must
  still be a regular file; a changed file or an unmet victim denies.
- *Sizes pass:* sums the folder and picks up to 8 victims (temporaries, then
  legacy names, then pages, oldest first, then by name hash). It keeps only
  those the survivors need: every temporary, then legacy names and pages
  while the folder is crowded. Over 128 names, pages alone are never evicted.
  A batch holds one rank: the names pass deletes in folder order, so a cut
  never takes a page while a temporary or legacy victim stays. Within a batch
  a cut may keep an older victim and take a newer one of the 8.
- A scan that picks no victim computes the grant as before.

Every safety stays: the pinned page, a canonical waiting for its backup, the
128-deletion bound per census, the 8-name reserve and cancellation at each
entry and around each unlink. A cut keeps every deletion already made, and the
next attempt starts from the smaller folder.

**Cost.** A deleting scan costs two folder reads plus at most 8 unlinks, so
the census frees 8 names per two folder reads instead of one name per O(n²)
scan. 32 temporaries now take 5 scans and 165 host stats, not 33 scans and
593. On the card the stats are free: the host counts them only to bound the
host path.

**Limits.**

- An attempt must cover about three passes before its first deletion. At the
  bench rate of about 1 ms per name, that is about 1 s for 330 names.
- An unlink still finds its name from the first entry, so a folder of
  thousands of names stays slow. Rotating a polluted `Pages/` would fix that
  if a card ever shows it.
- The convergence fixtures prove progress over repeated census runs, not
  completion within one reading visit. They retry straight after every
  refusal, but real admission is stricter: a Denied census ends admission for
  that target (300 legacy names are Denied after the 128-deletion bound), and
  interrupted attempts still count toward the three-try limit. That limit
  predates this change.
- The DEV census row now counts the whole folder, not the first 128 names, so
  the bench can read the folder size.
- This is host evidence. The bench proof follows the merge.

**Tests.** `chapter_cache_producer_storage_test.py` runs the binary's
convergence mode, which seeds three
shapes (300 legacy names, 126 pages, and a mix with temporaries), each
with a lone backup and a pinned page. It cuts attempts at 3,000 to 12,000
cancel checks, about 1.5 to 6 s on the card. Every cut attempt must delete
names or grant, and every shape finishes. A small census under byte pressure
is cut at every step: the cut deletes only what an uncut census would, and the
next census grants while keeping every page the uncut census kept. The
`progress-kept` control turns the unlink into a no-op and fails
`census_progress_kept`. A short-name fixture keeps every file.
`page_entry_fatfs_test.py` drives the card reader against a scripted FatFs:
full long names, the short-name fallback, size/time/type, a close on every
stop, and the card mount only, each with a control that must fail.
