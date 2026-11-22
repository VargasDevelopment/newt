# Automatic visible-shelf cover port

> Port status: ported from draft PR #20 (sources `4f24832`, `c2df314`; drafts
> `6f6e25d`, `f11e071`) and stacked on #19's Home cover. Phase 1
> delivered host-tested producer components. Phase 2 links the producer into
> all four firmware variants as an idle tick at the end of `loop()` on Home and
> in the Library. The evidence here is host tests, the native C3 component
> compile and four clean firmware builds with the resource gate. The owner has
> since run a phase-2 image on an X3 ("Device evidence on the phase-2 image"). That run led to
> the read path, restart and write changes below. Images of this port then ran
> on the same X3 ("Device evidence on this port"): an earlier image of this
> series timed those changes; `8f7e281` was timed for press latency, presses
> during production and base timings after generation; and the current
> firmware, `ce4360e`, for press latency and presses during production beside
> base in one session. A persistent +40 ms step in base reader exits followed the first
> producer session. A later device A/B on the owner's card found that a
> further producer run did not slow base reader exits or opens. The step is
> not attributed to the producer, and its cause is unknown ("Device evidence
> on the phase-2 image"). Every other device statement below is a gate still to be
> passed, not a result.

## What changes outside the producer

* `firmware/main.cpp` stays at its 681-line ceiling. The integration uses the
  existing include, development sleep and closing delay lines; the press hook
  shares the existing queue-loop line:
  * The include of `library_cover_producer.h` replaces `library_covers.h`.
  * The development-build sleep line gains braces. It now also returns after
    the 10 ms delay while the USB bridge has work.
  * The closing `delay(10);` becomes the hook.
  * Each dequeued press notes input on that loop line before the DEV/non-DEV branches.
* `firmware/home_cover.h`: the producer reuses Home's existing
  `homeCoverEligible` and `homeCoverIdentity`, so Home and the producer apply
  the same checks.
* `firmware/sd_card_paths.h` (new) defines the card namespace once:
  `/newt-sd`, `Newt/Books` and `Newt/Covers`. The library scan, the prepared
  covers and the producer all use it.
* `include/newt/library_ui.h` adds `kLibraryShelfBooks` and
  `libraryShelfFirst`. `src/library_ui.cpp` uses them for the same layout, and
  so do the shelf targets.
* `include/newt/library_cover.h` adds the shared little-endian field reader and
  documents the no-art record.

The following are unchanged:

* Frosted rendering, navigation, and the button and hardware mapping.
* The catalog and reader lifecycle.
* Build flags and SDK routing.
* `firmware/library_reader.h` (537 lines).

## Decoder route: unpatched pinned SDK, baseline JPEG only

The producer links only the official pinned FreeInk SDK
(`cd6f5b831c8fad899723b6d406586f00c575f021`). It decodes baseline (SOF0) JPEG.
Everything else gets the existing placeholder: PNG, GIF, SVG, progressive and
every other JPEG frame type, and oversize or ambiguous inputs. Firmware gains
no PNG support. Evidence, with paths relative to
`vendor/freeink-sdk/libs/book/FreeInkBook/`:

* **Dispatch:** `src/render/ImageRenderer.cpp:713-741`. `ImageRenderer::render`
  rereads the source with `probeImage` (722). It sends PNG to `renderPng`
  (725-727) and SOF2 to `renderJpegProgressive` (729-732). Linking it links
  pngle and the progressive decoder even when neither can run.
* **PNG cannot be bounded:**
  * `renderPng` (from 127) calls `pngle_new` (148), which calls `PNGLE_CALLOC`
    (`third_party/pngle/pngle.c:197-199`). That is defined as libc `calloc`
    (`pngle.c:44`).
  * The result is 41,264 bytes of native state outside any caller arena, so
    `CoverMemory` cannot charge or deny it.
  * Valid PNGs also trigger UBSan at `pngle.c:144`.
  * The SDK returns `Ok` with zero rows for a PNG truncated inside IDAT. It also
    returns `Ok` for a PNG with no IEND (doc 058).
* **Baseline JPEG uses only the arena:** `renderJpeg` (239-281) takes:
  * a 1 KiB skip buffer (242)
  * an 8 KiB TJpgDec pool (243)
  * a 16-row band of the scaled width (273, `kBandRows` 191)
  * 7 bytes per output column for the row scaler (`initScaler` 78-87)

  `JDEC` lives on the stack. `src/vendor/tjpgd_impl.c` states that TJpgDec
  makes no allocation, and its input buffer comes from the pool
  (`third_party/tjpgd/tjpgd.c:983`).
* **TJpgDec accepts less than the probe:**
  * It rejects segment lengths of 2 or less (`tjpgd.c:998`).
  * For SOF0 it accepts 1 or 3 components (1010), luma sampling 0x11, 0x21 or
    0x22 (1016), chroma 0x11 (1021) and table ids up to 3 (1024).
  * It rejects segments over `JD_SZBUF` 512 (1004).
  * It rejects SOF1 through SOF15 (1089-1102) and skips unknown segments
    (1104).

**Route.** `firmware/cover_jpeg.h` is an MIT-attributed adaptation of
`renderJpeg` with its `RowScaler`, `jpegInput` and `jpegOutput`
(`ImageRenderer.cpp`, pinned `cd6f5b8`). LICENSE and NOTICE stay in
`vendor/freeink-sdk`. The adaptation keeps the same arena layout, TJpgDec scale
choice, box filter and statuses for a stored entry, and drops the dispatch.
`coverJpegScale` and `coverBaselineJpegArenaBytes` size the arena, and
`coverPlacement` (`cover_decode.h`) centers the image without upscaling. The
decoder and the parity test share these.

**The dispatch is unreachable.** The producer never calls
`ImageRenderer::render`, `probeImage`, pngle or the progressive decoder.
`tests/cover_job_native_sizes.py` links the producer and idle entry points with
`--gc-sections`. It fails if the reachable image contains `pngle_`,
`probeImage`, `ImageRenderer::render`, `renderJpegProgressive`, `renderPng` or
`renderJpeg(`.

In the firmware maps:

* pngle, `ImageRenderer` and the progressive decoder have no linked text.
* `probeImage` is linked, as on #19, for EPUB admission
  (`src/epub_admission.cpp`). The producer does not use it.
* TJpgDec's `jd_prepare` and `jd_decomp` are new.

**Parity.** `tests/cover_jpeg_parity_test.cpp` holds the adaptation to the SDK.
It covers every authored cover member that the SDK probe classifies as baseline
(55 inputs; 5 non-baseline members skipped). For each, it compares
`ImageRenderer::render` with `coverRenderBaselineJpeg`:

* at five placements
* after an early stop
* truncated at three points
* with a read failing at half and at the last byte

Status and every gray row must be identical: 487 comparisons and 49,767 rows,
88 of them ending in an injected read failure. The adapted run gets exactly
`coverBaselineJpegArenaBytes`.

The test also checks the other direction. Every input or truncation that
`coverBaselineJpeg` admits (140 frames) must be one the probe calls baseline,
at the same size. Only this test names the SDK dispatch.

**Admission.** `coverBaselineJpeg` (`firmware/cover_decode.h`) admits only
inputs that both parsers read the same way:

* SOI, then at most 16 segments that each carry a length greater than 2,
  within the first 64 KiB
* then SOF0 with 8-bit samples, 1 or 3 components, TJpgDec sampling, table ids
  up to 3, and 1 to 2048 by 1 to 3072 pixels
* a file that ends in EOI

It classifies the rest as follows:

* **Unsupported:** other signatures, other frame markers, fill bytes, TEM/RSTn
  before the frame, empty segments, too many or too distant segments, and
  oversize images.
* **Invalid:** malformed structure.
* **Incomplete:** truncation or trailing bytes after EOI.

`CoverCheckedImage` serves the decoder strictly in sequence and compares every
pinned byte on every read: SOI, each segment header and the whole SOF0 segment.
A mismatch, or a read that is not the next in sequence, fails the read, and
every read after it fails too. The decode then reports `CoverDecodeDrift`.
That is a fault, not a verdict about the book.

Tamper tests rewrite SOF0 to SOF2, place a PNG header at SOI, flip the frame
size and alter a scan byte, all without changing size or mtime. In the job
suites, pngle is still replaced by a stub that aborts when called.

Decoder scratch is sized from the validated frame: 1024 + 8192 +
16 × (width >> scale) + 7 × min(placement width, width >> scale) + 128 bytes of
alignment slack. Failure to obtain it returns `Declined`.

## The draft's SDK patch (kept, not linked)

`patches/freeink-cover-decoder.{patch,json}` and
`tools/apply-cover-decoder-patch.py` modify a separate verified copy of the SDK
in `.build/freeink-cover-sdk`. They add:

* per-instance pngle allocator callbacks
* defined unsigned shifts
* preserved partial feeds
* strict zlib, raster, IEND and trailing-byte completion checks

The patch suites in the host gate keep this evidence current. The patched copy
is never routed into firmware. Adopting PNG covers needs an owner decision on
SDK patch scope.

## Ownership and production path

`AutomaticLibraryCovers` is a 96-byte job (RV32), including the 32-byte
`CoverResume` ("Cancellation and restarts"). It is built for one tick and
destroyed in it. The caller passes the visible names for one call, selected
first, together with the slots it has already settled. The job copies nothing
and keeps no catalog pointer or raster. One `service()` call settles any slots
whose cover is current, then performs at most one generation transaction. It
returns a `CoverServiceOutcome`:

* the result
* the newly settled slots
* the slot attempted
* whether the verdict was persisted as a no-art record

A call shape it cannot serve, or unconfigured roots, settles every slot. A
reentrant call returns `Busy`.

ZIP discovery is bounded:

* archives up to 64 MiB, 1,024 entries and a 1 MiB central directory
* members up to 4 MiB
* names up to 511 bytes
* a 512-byte overlapping EOCD tail search that avoids the SDK helper's 4 KiB
  stack array

Container and OPF metadata are staged to the card and parsed from there with a
256 KiB cap, so ZIP inflation never coexists with Expat allocations. Stored
images decode through a bounded offset view of the open book, read through a
4 KiB read-ahead ("Read path").

Deflated images are staged to a temporary file. Exact size and CRC are verified
before the inflater is released and before the raster is allocated. The staged
file is removed before publication, so `Produced` leaves no temporary file.

Results are explicit producer states: `Produced`, `Idle`, `Cancelled`,
`Declined`, `Missing`, `Unsupported`, `Incomplete`, `Invalid` and `IoError`.
None is inferred from the SDK's generic success code. Output is one
3,384-byte, 144 by 188 one-bit raster, centered without upscaling.

## No-art records

**Format.** A content verdict is persisted as a no-art record. The verdicts are
`Missing`, `Unsupported`, `Incomplete` and `Invalid`.

A no-art record is a bare 28-byte NLC2 header with no raster. It holds the
usual magic, geometry, source size and FAT-rounded mtime. Its last field holds
the route constant `kCoverNoArtRoute` instead of a payload CRC. The constant is
`0x34424a43`, `"CJB4"`: baseline JPEG with shared ZIP flags. It is defined with the NLC2
format in `include/newt/library_cover.h`, because Home reads it too.
The Rust port writes `0x35424a43`, `"CJB5"` (`newt_core::cover::NO_ART`): #316
widened its JPEG decoder to TJpgDec's tolerance, so it generates each CJB4
negative once more.

The record goes through the same verified `commit()` as art. It is torn-safe:
a short file fails the exact size, and art fails its payload CRC.

`cached()` accepts either valid art or a current no-art record bound to the
book's size and mtime. Current CJB4 records need no book read. CJB3 negatives
use the selective, read-only migration below; art remains valid across routes.

A book is read again in three cases:

* its size or mtime changes
* its record names another route
* the record is damaged

Art replaces a stale record through the ordinary replacement path. Library's
`PreparedLibraryCovers` accepts only the exact 3,412-byte art file, so a no-art
record shows the existing placeholder, unread. Home shows the placeholder too,
and remembers a current no-art record bound to its book like a missing cover
file: later Home entries make no card access until the book key changes, and a
published cover's `noteHomeCoverChanged()` makes Home read again (Decision
057). A torn cover's bare header names its CRC rather than the route, so Home
never remembers one.

**Bump rule.** Change `kCoverNoArtRoute` whenever anything that decides a
verdict changes:

* cover discovery or metadata limits
* JPEG validation or decoder acceptance
* size limits
* the TJpgDec pool or the inflater arena
* the pinned SDK

A record naming another route then reads its book again, except for the
explicit CJB3-to-CJB4 flag migration below.

`tests/cover_job_test.py` enforces this with a tripwire. It hashes the route
sources (`cover_discovery.h`, `cover_io.h`, `cover_decode.h`, `cover_jpeg.h`,
`cover_job.h`, `cover_member.h`, `cover_memory.h`, `cover_resume.h`,
`cover_cache_migration.h`, `cover_cache_probe.h`, `include/newt/zip_record_rules.h` and
`include/newt/library_cover.h`, which holds the format, the route and the
record CRC) together with the SDK pin, and reads the route from the format
header. It fails unless `ROUTE_PIN` names both the current route and that
digest, so any edit to those files needs a deliberate re-pin and a decision on
the bump. The tripwire joins spliced lines, reads comments and literals whole
and accepts digraphs, and collects every header name each route source
includes, in every conditional branch. It is not the preprocessor, so it
refuses what it cannot read the same way. An include must name its header
literally; one through a macro fails, because its target could depend on a
configuration the tripwire does not model. So does an angle name holding a
comment, and any test of whether a header exists (`__has_include`), whose
answer could change a verdict with no include to classify. Every project
header a name can reach, from the source's directory, the project or the host
SDK include directories, must be a route source or be listed in `NOT_VERDICT`
with its reason; today that is only `library_covers.h`, the Library's reader of
prepared covers. A name that climbs out of its include directory (`..`) must
resolve beside its source. `tests/cover_route_tripwire_test.py` holds the
lexer to each spelling found in review. Moving the
constant into the format header for Home changed `cover_job.h` and no verdict,
so the digest was re-pinned under `CJB2`. So did adding the format header and
`cover_memory.h` to the digest, which changed no code. This port moved from
`CJB1` to `CJB2` because the fault rules below changed which outcomes are
verdicts. `CJB3` (issue #41) gives every member Reader admission's ZIP rules.
Before, an anomaly in any member, such as one marked version 4.5, declined
the cover of a book the Reader opens, and the negative was cached. The new
route makes those books read again.

Reader admission and cover discovery now use **one per-bit classification** in
`include/newt/zip_record_rules.h`, for every central record and required member.
The former `0x080e` read-member mask is removed. Equal stored sizes remain
required when reading; `ZipEntryReader::open` independently requires them.

The meanings below derive from [PKWARE APPNOTE 6.3.10, section 4.4.4](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT).
SDK traces are relative to `vendor/freeink-sdk/libs/book/FreeInkBook/` at
pinned `cd6f5b8`, in `src/epub/ZipCatalog.cpp`. "Before" refers to CJB3's
required-member read policy (all other envelope/size/CRC checks still apply).

| Bit | APPNOTE meaning | SDK trace (line) | Reader before → after | Cover before → after |
| --- | --- | --- | --- | --- |
| 0 | Encryption | No decrypt step, 237–261 | Encrypted → Encrypted | Refuse → Refuse |
| 1–2 | Deflate compression options; undefined for stored | Raw inflater, 253–261, 296–310 | Accept → Accept | Accept → Accept |
| 3 | Data descriptor | Central sizes 169–190; reads bounded by them 272–310 | Accept → Accept | Accept → Accept |
| 4 | Reserved for enhanced deflating, method 8 | Method 8 still raw DEFLATE, 253–261, 309 | Accept → Accept | Refuse → Accept |
| 5 | Compressed patched data | No patch application/base source, 249–269 | Accept → Unsupported | Refuse → Refuse |
| 6 | Strong encryption | No decrypt step, 237–261 | Encrypted → Encrypted | Refuse → Refuse |
| 7–10 | Unused | Flags not retained, 169–190; byte reads 249–310 | Accept → Accept | Refuse → Accept |
| 11 | UTF-8 names/comments | Byte-exact lookup 198–206; payload independent 265–269 | Accept → Accept | Accept → Accept |
| 12 | Reserved enhanced compression | Method alone selects inflater, 249–261 | Accept → Accept | Refuse → Accept |
| 13 | Encrypted directory / masked local fields | No directory decrypt 164–190 or local unmask 237–247 | Accept → Encrypted | Refuse → Refuse |
| 14 | Reserved alternate streams | No alternate-stream interpretation, 184–190, 265–269 | Accept → Accept | Refuse → Accept |
| 15 | Reserved | Flags not retained, 169–190 | Accept → Accept | Refuse → Accept |

Options/reserved bits do not select another data path for supported methods
0/8, including reserved bit 4 on stored members. Accepting such bits promises
only that an otherwise valid stored or raw-deflate payload remains readable;
unsupported compression methods are still rejected. Patched data requires a
base and patch engine which this reader has neither. Masked headers require
unmasking/decryption that the SDK does not implement. These two explicit
Reader acceptance reductions prevent pretending those formats are readable;
no decoder scope or dependencies change. Local/central flags must still match
exactly: mismatches are Malformed in discovery (Unsupported in admission's
existing public status mapping). Cover staging checks the central CRC in
`firmware/cover_member.h`; the SDK does not retain CRC or consume descriptors.
Descriptor CRC is deliberately non-authoritative; wrong central CRC still
fails production cover validation.

**Selective CJB3 migration.** Old records have no refusal reason, so a global
route invalidation cannot distinguish flag refusals from unsupported decoders.
CJB4 writes all new negatives with the new route. A bound CJB3 negative stays
settled when a bounded central-directory walk finds no newly admitted flag.
It opens the book read-only, allocates nothing, parses no XML, and writes no
scratch or durable record. A newly admitted flag on any member invalidates the
old negative once, only after the complete central walk and source binding
succeed; normal production then publishes art or a CJB4 negative. The migration
returns three outcomes: qualifying flags, unchanged, or inconclusive. Archive
count/directory, member name/method/size, and expanded-total limits that still
refuse the archive leave its negative settled, even if another member has new
flags. The structural member walk is shared with production discovery: malformed
central/local names, headers and data extents also leave the record alone.
Read/open/metadata/close errors and stops are inconclusive: keep the CJB3
negative and settle its bit without generation or writes. Retaining a record
is independent of the work result: I/O faults report `IoError`; a stop reports
`Cancelled` (input) or `Declined` (heap reserve). Those outcomes spend the same
visit allowance as the corresponding non-migration failure. We deliberately
retain the negative rather than schedule deferred regeneration. Future normal cache checks may repeat the
same bounded read-only scan (the overlapping EOCD search reads under 70 KiB,
plus at most 1,024 records: 46-byte central and 30-byte local headers and
two copies of each name, at most 511 bytes each), with no growing retry budget,
persistent retry state or SD writes.
Thus even a still-unsupported decoder settles without a retry loop or repeated
writes on later boots. Unchanged PNG/progressive/missing negatives retain their
CJB3 bytes. Because the old reason was lost, a flag on an unused resource can
conservatively cause one extra retry. This is deliberately documented rather
than claiming exact cause reconstruction. Old CJB1/CJB2 records remain stale,
preserving CJB3's original fix for unrelated-member ZIP anomalies. Home
recognizes both CJB3 and CJB4 bare records as placeholders.

**Cache-probe outcome contract.** `cover_cache_probe.h` owns one typed result
for every cache probe. The selected `bool cached(const char*)` signature and
explicit frame boundary stay intact; `noipa` also prevents GCC from replacing
the symbol with a clone after its boolean return becomes unused. It performs
one probe and retains its
typed outcome in the job for the active `service()` call; its boolean only
reports `Current`. `service()` consumes the stored typed outcome directly, so
an I/O failure or retained stop cannot become a cache miss. The active-call
lease prevents callback reentry from replacing that outcome. Only an explicit
`Generate` result authorizes an attempt; an added or unknown outcome cannot
implicitly fall through into generation.
Storage faults are latched through descriptor cleanup, then sticky stops take
precedence before any result reaches the scheduler. All pre-attempt results
keep `slot=4`: no generation ran and no source version was bound. SF1 therefore
preserves any other book's resume bytes, interruption count and slot. Retaining
a validated record and reporting an interrupted probe are separate facts.

| Probe event | Generation / settled bit | Service result | Scheduler work charge |
| --- | --- | --- | --- |
| Qualifying newly accepted flags, complete bound scan | Normal attempt; ordinary attempt rules | Ordinary attempt result | Ordinary generation/per-book interruption rules |
| Unchanged legacy negative (including unchanged limits or malformed structure), current CJB4 negative, valid art | None; retain settled bit | `Idle` if no later work | None, as for any completed cache hit |
| Inconclusive migration I/O, including open/read/metadata/close/binding failure | None; retain CJB3 and settled bit | `IoError` | Exactly one visit failure, as for normal I/O failure |
| Inconclusive migration input/timeout stop | None; retain CJB3 and settled bit | `Cancelled` | Exactly one unbound visit failure, even at the last slot |
| Heap reserve lost during a retained probe | None; retain settled bit | `Declined` (input takes precedence if also pending) | Exactly one visit failure, as for normal heap decline |
| Stop after a fully validated unchanged/current cache hit | None; retain settled bit | `Cancelled` or `Declined` | Exactly one visit failure; a cache hit cannot mask a stop |
| Stop before cache validity is known, or after flags conclusively make the legacy record stale | None; keep slot unchecked | `Cancelled` or `Declined` | Exactly one unbound visit failure, as before a normal attempt binds |
| Missing cache (`ENOENT`), stale route/binding, verified corrupt format/CRC | Normal attempt; ordinary attempt rules | Ordinary attempt result | Ordinary generation rules |
| Other cache-probe storage fault (including cache header/payload read or close) | None; settle that slot for this view | `IoError` | Exactly one visit failure; a fault is never an implicit cache miss |
| Input already pending before the idle tick admits a job | No job or probe; no new settled bit | `Cancelled` | None: no admitted work, matching the ordinary path |

A qualifying scan clears the retained-negative state before stop normalization:
if input first arrives on its final successful close, the now-stale record stays
unchecked for a normal paced restart. An inconclusive scan instead keeps its
negative settled. The explicit `Stopped` versus `RetainedStop` outcomes preserve
that distinction. The service checks stops before continuing past any cache hit,
including the final unchecked slot. Cache-probe I/O now fails visibly rather
than silently regenerating after a transient read/open/close error; only an
absent or conclusively invalid cache grants generation. The exhaustive fault
suite asserts `IoError`, zero Session allocation and zero writes for those
pre-attempt faults, preserving normal stale/corrupt-cache regeneration tests.

Visit failures survive shelf changes. At four, further admitted work waits for
15 seconds of quiet; a fifth failure rests the producer until the screen
changes. Settled bits avoid a repeated scan within the current view, and the
visit budget bounds interrupted scans across changing shelves. The idle harness
alternates two shelves of five authored books, interrupts each migration after
12 reads, and requires five charged stops / 60 reads followed by zero work
across further shelf changes. It also checks I/O parity, heap decline, SF1 and
all Library slots, late art/current-negative stops, and qualifying retry-once
publication. Restoring the previous boolean cache decision must fail this
scheduler-level regression; helper-only or direct-service tests are insufficient.

The authored matrix covers every bit individually and mixed flags (including
`0x0816` on container, OPF and image), stored and deflated members, and benign
local/central mismatches. Reader/lookup parity checks the whole matrix. An
old-mask mutation control must fail the authored bit-4 success assertion.
Legacy-negative regressions check retry-to-art and retry-to-CJB4, zero-write
unrelated negatives, cancellation and storage failures. These are host facts;
never-opened/opened device lifecycle remains awaiting_device in NEWT-9.

Version-needed, member disk start and the local CRC and sizes are no longer
checked anywhere. Data is located by the central record, art is published
only after the central CRC matches, and ZIP64 sentinels fail the size,
extent and offset limits. Early negative image verdicts are unchanged: as
before, they do not verify the member CRC. `tests/cover_job_test.py` runs Reader
admission over the same already-built anomaly books as the cover matrix.

**Verdict or fault.** `IoError`, `Declined` and `Cancelled` are never
persisted. A verdict is recorded only when `Session::unfaulted()` holds.

* **Card operations:** every card operation in the session goes through
  `CoverFaultLatch`. Any failed open, read, write, sync, close, rename, unlink,
  mkdir, stat or fstat latches a fault. So does any zero or overlong write. A
  positive partial write is not a fault: `coverWrite` retries the rest. The
  only exception is `ENOENT` on stat, open or unlink, which is an answer.
* **Source:** no empty read, size change or rebinding.
* **Decoder:** no `CoverDecodeDrift`.
* **Memory:** no refused allocation.
* **Lifecycle:** no publication step and no stop. A poll and a rebinding check
  also run just before the record's commit.

**Staging.** A staged member copy is judged only when it completed. A size or
CRC mismatch on a complete copy is `Invalid`. A failed write or a stop says
nothing about the member. An inflater that cannot get memory is `Declined`. A
card error opening the member is `IoError`.

A failed card operation or a stop therefore never hides a real cover. The
staged-member sweep (below) fails if this regresses. A read that succeeds with
wrong bytes is outside this model ("Risks and deferred work").

## Idle scheduling

**Hook.** `loop()` ends with
`if (newt::firmware::serviceIdleCovers(app.screen,sdSelection,libraryUi,home,pressQueue)) paint();`
in place of `delay(10);`.

`serviceIdleCovers` (`firmware/library_cover_producer.h`) is `noinline`. Its
locals stay out of `loop()`'s own frame, which has an 80-byte stack ceiling.
It ends with the same 10 ms delay except after `Cancelled`, so pending input
returns directly to `loop()`. It returns true only when art was
published, so the screen repaints once.

In development builds, the existing sleep line returns after that delay while
`usbBridge->backgroundWorkReady()` is false. This is the gate the 30-second
progress save already uses. Bridge automation, synthetic input and timed
reader actions therefore never share a tick with generation, and a
post-generation repaint never interleaves with a bridge render.

**Targets** (`firmware/cover_idle.h`, portable and host-tested):

* **Library:** the visible shelf exactly as `src/library_ui.cpp` lays it out
  (`libraryShelfFirst`, up to `kLibraryShelfBooks` names), selected book
  first. There are none while loading, without a card, or without an `Ok`
  catalog.
* **Home:** the one selected book, only under Home's own cover eligibility.
  `homeCoverEligible` requires an active, verified selection and no Home card
  flag. `homeCoverIdentity` requires a retained identity that is a valid SD
  resume record. While `heldHomeCover()` is non-null there is no target: the
  session already validated that art through the same prepared cache (doc 057).
  A missing cover or a no-art record holds no art, so its book still gets a
  target. Sample books and cardless Home have no target.
* **Every other screen**, including the reader and Sleeping, has no target. It
  only ends the visit.

**Settle period.** Before any heap probe, allocation or card operation, a tick
waits for `kCoverSettleMs` (2,000 ms) without input, a screen change or a view
change, including the first tick after boot. The existing four-byte clock is
`quietFrom`: those changes, an interruption, pending input at tick entry and
every press consumed by `loop()` restart it. The inline press hook precedes
all input branches, including development automation early returns. Unsigned
subtraction keeps the gate safe across clock wrap.

At four visit failures (`kCoverFailureBudget`) the same comparison waits for
`kCoverQuietMs` (15,000 ms) instead, and a fifth failure rests the producer
until the screen changes ("Bounded retries"). No state is added.

Device runs on the regional DEV USB image of dd1bfcd measured
bridge request-to-consumed times of 100.0–102.2 ms instead of 11–13 ms in
10 of 15 warm Home reader opens and 5 of 12 Library opens, and 17.9–25.7 ms
in 7 of 12 Library-to-Home presses. Device open totals stayed at 2,138–2,148 ms;
an earlier image of this series showed the same signature less often
(2 of 13 warm opens, 1 of 3 Library opens).
Base and #19 without the producer did not show the delay; a 1.5 s silent
pause before each press removed it. The immediate entry tick was doing
cache checks: each book binding made three unpolled FAT path walks, about
30 ms each on that card, followed by the wrapper delay. The settle gate
keeps that work out of immediate navigation; path polling bounds a later
stop ("Read path"). In the navigation timing runs, every press after a Home
or Library render was consumed in 10.8–12.6 ms on the image of `8f7e281` and
in 10.3–12.6 ms on that of `ce4360e` ("Device evidence on this port").

**One tick** handles at most one book, synchronously on the UI task. Input,
reader open, resume, sample selection, sleep, wake and card handling therefore
cannot interleave with it.

Every descriptor is released before the tick returns, and so is every
allocation but one. The job itself is one heap block allocated after
admission and freed at the end of the tick. The exception is the 32-byte
resume block of an interrupted book ("Cancellation and restarts"). It lives
until a tick finds the view settled, changed or left. A reader opened by the
press that interrupted a book therefore opens with that one block live, and
with no producer file open. The block is 32 bytes; about 48 with the
allocator's overhead is an estimate, which the private heap probe's `resblk`
field measures on a device. In development builds `loop()` returns before the
tick while bridge automation holds background work (`firmware/main.cpp`), so
the block can also outlive a screen change until the first tick after the
automation ends. The job yields 1 ms per 5 ms of work, the reader's cadence.

**Cancellation.** Generation stops at the next poll, and polls are sticky
within a call, when any of these occurs:

* a press is queued (`uxQueueMessagesWaiting`; the power button arrives
  through the same queue)
* in development builds, an unread host byte is available
* free heap falls below the 64 KiB reserve (reported as `Declined`)

A press that is already queued stops the tick before any allocation.

A cancelled tick with no bound attempt spends one visit failure: a stop during
a cache check (slot 4) or before an attempt binds its source
(`resume.book==0`), also when the tick validated another slot's cache first.
Those slots stay settled. Without that charge, alternating shelves would renew
the exemption indefinitely, since a view change clears the memo. The retained
record, its key and its interruption count stay unchanged (SF1); the tick that
finds the budget spent frees the block. Pending input at tick entry creates no
job or card access and spends nothing.

An interrupted (`Cancelled`) bound book stays unsettled and costs no failure. Its
stale temporaries are removed and it is tried again, resuming what the
interrupted attempt finished. A book's first two restarts in a view come on
the first idle tick after the settle period (15 s quiet at four failures);
later ones wait for a quiet interval, and its seventh interruption sets it aside for the view
("Cancellation and restarts"). Pending input at tick entry costs no card access.

**Home signal.** On `Produced` the tick calls `noteHomeCoverChanged()`
(`firmware/home_cover_signal.h`; contract in doc 057). It does so once, and
only after the complete atomic publication of art. A no-art record, a decline,
a failure and a cancellation never call it.

**Card ownership.** Library `browse()` closes the reader and source before
`scan()`, and Home targets exist only while no book is open. The producer is
therefore the only SD user while it runs. It holds at most two descriptors at
once (host-measured), under the mount's `maxFiles=3`
(`firmware/sd_library.cpp`). The producer never writes progress, selection or
resume records.

If a descriptor is unavailable, the producer fails each book once per view
with `IoError`. It persists nothing and counts each failure against the budget
(host test `descriptors`). For example, this happens if Home's selection still
holds its source.

## Cancellation and restarts

**Why.** On the phase-2 image one book took 95 s to generate. Under the development
bridge's idle polls every 40 s, the same book was cancelled at 40 s four times
in a row, and each restart repeated every discovery and staging write ("Device
evidence on the phase-2 image"). An attempt started from nothing, and an interrupted
book restarted at the next tick. Making attempts faster ("Read path") shrinks
the window; this section bounds what an interruption costs.

**What a restart skips.** `Session` records its progress in `CoverResume`
(32 bytes RV32, `firmware/cover_job.h`):

| Step | Recorded when | A restart of the same version skips |
| --- | --- | --- |
| `Empty` | the version key is known, before the first CRC pass | nothing; the CRC pass starts over |
| `Read` | the first whole-source CRC pass completed | that pass; its CRC is reused |
| `Located` | the container, OPF and ZIP lookups found the member without a fault | also the three ZIP directory walks and the container and OPF staging and parsing |
| `Decided` | a verdict was reached without a fault, but its record was not committed | everything up to recording the verdict |

A `Located` restart rebuilds the member from the record: local header offset,
data offset, sizes, method and CRC. Staging the member, decoding and
publication do not resume. They hold the heap peak or the raster, and keeping
either would cost tens of kilobytes.

**Key.** `coverResumeKey` is FNV-1a over the filename and its terminator, then
the 64-bit size and 64-bit mtime from the book's path `stat`. That is the
generic identity the no-art record binds. No title, digest or particular book
is special. A record under another key is discarded.

**A hint, never authority.** Art and no-art records alike publish only through
`commit()`. It CRCs the whole source again and compares the result with the
remembered CRC. It also rechecks the source binding and, on FAT, reopens the
pathname and streams its CRC. A staged member is still checked against its
size and CRC, and the decoder checks the header bytes it validated. A book
changed in place with the same name, size and mtime therefore publishes
nothing, and its record is discarded. So is a book whose key collides with
another's. A stale or colliding record costs at most one wasted attempt. The
host tests change a book's content under an unchanged `stat` while its record
waits.

**Where it lives.** The job holds the record during a call. An attempt that
ends other than `Cancelled` clears it; a call that attempts no book leaves it
as it was. After `Cancelled` with a version key, the idle tick copies it into
one heap block (`CoverIdleState::resume`), including an `Empty` record stopped
before the CRC completes. The block is allocated when that keyed record needs
one, after the job has been freed. An attempt knows its version key only once
it has bound its source. Until then the record still names the version an
earlier attempt bound, so the job clears the key: a stop while stale
temporaries are removed or the book is opened is charged to no book. Like a
stop before any book, it restarts the quiet clock and spends a visit failure,
also when a new slot settled; the interrupted book keeps its count and block. If
allocation is refused, the slot is set aside for the view without spending a
failure, and its count is cleared: an unknown version must not inherit
interruptions. A new view or screen tries it again. The block is freed after
any attempt that ends other than `Cancelled`, when another book's interruption
(or another version's) replaces it, when the book is set aside, above the failure
budget, when the view is settled, changes or ends, when the screen changes,
and by deep sleep, which restarts the firmware. While a resumed attempt runs, the record exists twice: in the block
and in the job. The start threshold charges both
(`AutomaticLibraryCovers::reservationBytes()`).

Static RAM was the alternative. It would add 28 to 32 bytes of linked SRAM,
and no byte remains below the unchanged ceiling in any variant. It would
also take those bytes from every screen's heap, the Reader's included. The
heap block exists only while an interrupted book waits.

**Back-off.** `CoverIdleState` follows one interrupted book: its slot bit
(`book`) and its interruptions in the view (`interruptions`, four bits each),
and the millisecond clock when quiet last began (`quietFrom`):

* The book's first two restarts run at the first idle tick after the 2 s
  settle period, or 15 s quiet while the visit has four failures.
* Each later restart measures from the same quiet instant: 15 s before the third,
  then 30, 60 and at most 120 s.
* Only that book waits. While it or a settled book is selected, a tick inside
  the interval returns at once, with no heap or card access. When another book
  that needs work is selected, it starts after settling (15 s quiet at four
  failures), and the call passes over the waiting book.
* The count and the record belong to the book. An interruption of another
  slot, or of another version of the same name (another resume key), starts a
  new count and record. A stop before an attempt binds its source counts for
  no book. An attempt that ends any other way resets both, so the
  next interruption is a first. Books alternate only when the selection
  changes, since a tick tries the selected book first.
* The seventh interruption of the book in one view (`kCoverInterruptionLimit`)
  sets it aside: its slot is settled for the view without spending a failure,
  and its block is freed. Its record advances at most three times (`Read`,
  `Located`, `Decided`), so at least four of those interruptions made no new
  progress. A new view or screen tries it again from the start, uninterrupted
  if the input has stopped.
* A press already queued when the interval ends starts it again.
* A new view or screen forgets the count and frees the block.

Input itself never waits. The back-off limits the rate of repeated card work,
not its total; the interruption limit bounds the total per book and view. The
clock is 32-bit `millis()`, and the interval is computed with unsigned
subtraction, so a wrap is harmless (tested).

**Host evidence** (`tests/cover_resume_test.cpp`, `PASS idle resume`, `PASS
idle resume sweep`, `PASS idle resumed faults`, `PASS idle resume edges`,
`PASS idle early versions`, `PASS idle interruption slots`, `PASS idle back-off`
and `PASS idle per-book back-off`). One
full attempt is compared with an attempt resumed after an interruption at the
pending file's open, on the host filesystem and on modeled FAT. Counts are for
the attempt; the resumed attempt includes its own whole-source verification.
"Fsyncs" counts explicit `fsync` calls. "Volume syncs" counts the FatFs volume
syncs the harness models from the pinned library ("Read path", Writes). The
host filesystem rows follow. The modeled FAT rows have the same writes, syncs
and unlinks, and more reads for the pathname reopen.

| Book (host) | Attempt | Stage-file opens | Writes | Bytes written | Fsyncs | Volume syncs | Unlinks | Book reads | Book bytes |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| stored | full | 4 | 7 | 4,137 | 1 | 6 | 2 | 462 | 524,946 |
| stored | resumed | 0 | 5 | 3,412 | 1 | 2 | 0 | 221 | 346,881 |
| deflated | full | 6 | 176 | 176,871 | 1 | 8 | 3 | 406 | 412,531 |
| deflated | resumed | 2 | 174 | 176,146 | 1 | 4 | 1 | 202 | 272,234 |
| no cover | full | 4 | 3 | 663 | 1 | 6 | 2 | 44 | 4,983 |
| no cover | resumed | 0 | 1 | 28 | 1 | 2 | 0 | 2 | 1,188 |
| truncated scan | full | 4 | 3 | 753 | 1 | 6 | 2 | 122 | 49,582 |
| truncated scan | resumed | 0 | 1 | 28 | 1 | 2 | 0 | 23 | 22,802 |

A resumed deflated cover still stages and writes its image again: staging
holds the inflater, the heap peak, and is not resumed.

The same test changes a book's bytes under an unchanged `stat` while its
record waits, once at `Located` (stored art) and once at `Decided` (a
coverless book's remembered `Missing`). The resumed attempt is refused at
commit, persists nothing, frees the block and spends one failure; once
restored, the next visit reads the book again and records it.

It also refuses the resume block's own allocation at an interruption. The tick
still returns `Cancelled` with no block and no leaked memory, sets the slot
aside with no failure or retained count, and persists nothing. Settled ticks
do nothing; the next visit produces the cover from the start, staging the
container and OPF again.

The early-version test stops stored art, deflated art and a coverless book
during the initial CRC six times, then changes size or mtime in the same view.
The next interruption starts at one with the new key. A keyed `Empty` restart
still does the full CRC and discovery. Refusing the first block or a changed
version's replacement block sets the slot aside; changing the version while
set aside does no work, and a new view starts at one. The slot test reaches
the seventh interruption in all four Library slots and Home's slot zero,
checking the exact settled bit, unchanged failure count, freed block and idle
ticks while the neighbours finish.

The sweep interrupts three books at every poll: a deflated art book (1,531
polls), a decoder verdict (truncated scan, 298) and a metadata verdict (no
cover, 141). Each restart is stopped again after half as many polls as the
first attempt left, at least one. A resumed attempt skips work, so a restart
may finish first; the test requires at least one restart per book to be
interrupted, and in the recorded run every one was. The final attempt must end
as an uninterrupted one would, with exact art or the no-art record. `Read`,
`Located` and `Decided` are all reached where they apply, and a restart never
moves a record back.

The resumed-fault sweep interrupts a stored and a deflated art book at the
pending file, then fails one call of the resumed attempt, in its own run.
Each write, sync, rename, close, unlink, directory creation, open and
allocation call fails in turn; reads, stats and handle stats fail at every
fifth call and at the last. That is 528 single-call failures. None writes a
no-art record; 7 still produce exact art, and every other one produces exact
art at the next visit.

The edge test covers the record itself. A failed close of a staged discovery
file latches a fault: discovery still finds the member, but the record stays
at `Read`, and the restart stages the container and OPF again. A second
interruption after `Located` rewrites the same block. A restart refused by the heap leaves the
block until the next tick, which finds the view settled and frees it. A heap
that charges the full 128-byte allowance per block still admits a resumed
deflated attempt at exactly the start threshold, with the planned peak
reached and the reserve intact.

The back-off test runs two restarts after settling and then the four intervals with no
card access inside them, across a clock wrap; the seventh interruption sets
the book aside with no block, count or failure, while the shelf's other three
books run. It then checks that a new view and a new screen each forget the
count and free the block, and that a later quiet visit produces the book.

The per-book test interrupts one book three times and selects another, which
starts after settling and begins its own count and record. A produced book resets the
count; a changed mtime under the same name starts a new count; and a selected
book that is already cached, ahead of the waiting one, is passed over together
with it, so the call produces the next book. Every tick still asserts a clean
heap apart from the resume block, and no open descriptor.

These are host results. On the device, an earlier image of this series
resumed an interrupted book from its kept record ("Device evidence on this
port"). No device has timed the back-off intervals.

## Read path

**Path walks and stops.** Active work polls before every pathname `stat`,
`open`, `unlink`, `mkdir` and `rename`: each of the book binding's three
walks, `stillBound`, directory creation, each temporary's stat and removal,
and pending-file creation and final replacement. Input arriving during one
walk prevents the next active walk; descriptor metadata checks do not walk
the directory. The measured three-walk input delay above motivates this bound.

The remaining exception is removal of a temporary this attempt created: a
stop while a staging copy or the pending file exists pays its stat and unlink
during unwind. `stage()` removes a copy it did not complete, as before; the
session destructor removes only a complete staged copy or a pending file this
attempt still holds. A copy whose removal failed is left to the next attempt's
polled start removal, which also recovers temporaries a crash left. Stops
during the source CRC, locate, or before staging creates its copy cause no
cleanup walk, and `Produced` leaves no temporary and causes no destructor walk.
Descriptor close always runs. Publication still polls after FAT's final-file
unlink and before rename: a stop in that deliberate missing-cache interval
leaves the placeholder, and a completed rename remains `Produced`. No poll is
added after rename that could undo that outcome.

`PASS path polls` and `PASS creation/rename path polls` cover each of these on
stored, deflated and coverless books, POSIX and modeled FAT, with missing and
stale caches. Input queued in the missing-directory stat issues no `mkdir`.
Input queued in the walk before pending creation, the first final-file stat or
rename stops before it, paying only the owned pending file's stat and unlink.
Input during the final-file recheck prevents FAT's old-cache unlink, and input
during rename leaves `Produced` or the recorded no-art verdict. Removing any
one of the 17 producer path polls fails a host test.

**Current-cache probe.** `cached()` follows Library's painter: one book stat
for size and FAT-rounded mtime, then one cover open whose size comes from
`statHandle`. Its magic, geometry, source size/mtime, exact bare-or-full size,
payload CRC and no-art route checks are unchanged. It opens no book and
does no `stillBound` pair: two pathname walks instead of about eight. It
polls between operations and allocates nothing.

The probe only decides whether this view needs an attempt. An attempt still
binds its book with the full stat/open/stat checks, and publication still
revalidates that binding and content. A book or cover changed between the
stat and read can at worst be judged current until the view changes, the
same window Library's painter accepts; the probe establishes no fresh EPUB
identity. The no-art route stays CJB2 because no content verdict changes;
the source digest is deliberately re-pinned for these scheduling/I/O changes.

**What the pinned VFS does.** The pinned ESP-IDF 5.5 FAT VFS implements
`pread` as lock, `f_lseek(offset)`, `f_read`, `f_lseek(previous position)`,
unlock. The pinned C3 configuration has `FF_USE_FASTSEEK` off, a per-file
sector buffer and 512-byte sectors. Without fast seek, `f_lseek` to a position
behind the current cluster walks the chain from the file's first cluster.

Phase 2 read with `pread` and never moved its descriptor, so every request
walked from the first cluster. A whole-source pass over n clusters cost about
n²/2 chain steps. The volume's FAT window holds one 512-byte sector (128 FAT32
entries), so a walk also reloads FAT sectors whenever the chain spans more
than one. The native suite checks that the linked library has these shapes
(`vfs_fat_lseek` calls `f_lseek`, `vfs_fat_read` calls `f_read`).

**Now.** `CoverFiles::readAt` (`firmware/cover_io.h`) seeks, then reads.
A sequential pass leaves the descriptor at the next byte, so the next seek
walks nothing. One request moves at most 4 KiB (`kCoverReadBytes`), up from
1 KiB.

TJpgDec asks for 512 bytes at a time. The decoder now reads through
`CoverReadAhead`, a 4 KiB window refilled by one read. The window and its
object sit at the front of the decoder's own scratch block, which grows by
4,128 bytes (RV32). That is inside the generation and below the planned peak.
In the matrix, the largest stored-member generation that uses the read-ahead
(`m-wide-boundary`, 2,048×16) accounts 25,892 host bytes. `m-wide-one-row`
(1,800×1), which decodes without it, accounts 46,452. The planned peak is
48,464 host bytes. The fit check below keeps the read-ahead inside the plan by
construction, not only for these inputs.

The read-ahead is used only while the decode phase's blocks (layout, raster,
read-ahead and decoder arena) fit the inflater charge they replace in the
planned peak. TJpgDec's band is 16 rows of the scaled width, so only very wide
frames one pixel high reach that limit: with the read-ahead up to a
width of 1,669 RV32 (1,666 host), and without it up to 1,927 (1,925 host).
The check is `coverReadAheadBytes` (`firmware/cover_decode.h`); the matrix
pins the host boundary at 1,666 and 1,667 and the inclusive fit at the exact
arena size (`PASS read-ahead boundary`).
Wider one-row frames, up to the 2,048 limit, already exceeded the planned peak
at phase 2 by up to about 1.9 KiB (1,956 bytes host at 2,048×1). That is
unchanged. It is safe, because every allocation rechecks the reserve, but such
a book can be `Declined` on a heap just above the start threshold and spend the
failure budget. The authored `m-wide-one-row` (1,800×1) decodes without the
read-ahead and within the plan; the matrix fails if it ever exceeds it.

The whole-source CRC passes still read 1 KiB at a time. Their chunk is the
1,024-byte stack buffer of `sourceCrc`, a guarded 1,072-byte frame. Moving it
to a 4 KiB heap chunk would cut card commands per pass fourfold (2,923 to 732
for a 3 MB book in the model) without changing chain steps. That is deferred
to the restack.

**Host profile** (`tests/cover_read_profile_test.cpp`). The books are authored
by `build_profile` in `tests/cover_job_fixtures.py`, and none is a library
book:

* a 2.99 MB stored baseline JPEG
* a 1.42 MB deflated baseline JPEG
* a 6.39 MB archive with 251 members and a 36 KB cover
* a 33 KB ordinary book

The suite counts every request that reaches the card and every TJpgDec input
request. It replays each card request against a model of the inspected FatFs
rules: contiguous clusters, the seek and read rules above, and 4 KiB and
32 KiB clusters. The model counts:

* chain steps (links followed)
* FAT sector loads
* card commands (one per contiguous sector run or buffered sector)

These are model counts, not card timing.

Whole producer, 4 KiB clusters, phase 2 → this change:

| Book | Book reads | Chain steps | FAT sector loads | Card commands |
| --- | ---: | ---: | ---: | ---: |
| 2.99 MB stored JPEG | 14,699 → 9,573 | 5,356,010 → 13,870 | 47,042 → 114 | 14,666 → 11,016 |
| 1.42 MB deflated JPEG | 5,630 → 4,936 | 1,458,441 → 6,940 | 15,583 → 61 | 9,780 → 6,305 |
| 251 members | 21,772 → 21,760 | 19,248,308 → 1,185,700 | 161,213 → 9,893 | 20,549 → 20,471 |
| 33 KB ordinary | 252 → 183 | 896 → 160 | 1 → 1 | 211 → 172 |

With 32 KiB clusters, the stored book falls from 663,117 to 1,729 steps, and
the 251-member book from 2,396,204 to 147,720 steps and from 19,054 to 1,522
FAT loads. The staged-copy reads of the deflated book fall from 2,827 to 372.
The 2.99 MB stored image's 5,857 TJpgDec requests now reach the card as 730
reads of 4 KiB, where they were 5,846 reads of 512 bytes.

One whole-source pass of the 2.99 MB book, 4 KiB clusters:

| Request | Chain steps | FAT loads | Card commands |
| --- | ---: | ---: | ---: |
| parked 1 KiB `pread` (phase 2) | 1,065,800 | 9,364 | 2,923 |
| seek and read, 1 KiB (now) | 730 | 6 | 2,923 |
| seek and read, 4 KiB (deferred) | 730 | 6 | 732 |

The suite asserts that no request uses a parked descriptor, and that a
sequential pass walks each cluster once. It also bounds requests, chain steps
and decoder reads per book, and pins one explicit fsync and one rename per
cover.

**What remains: ZIP lookups.** `locateCoverMember` validates every central
record against its local header. Each member costs four small reads: the
46-byte central record, its name, the 30-byte local header and a name compare
in 64-byte chunks. The reads alternate between the central directory at the
end of the file and local headers before it. Each jump back to a local header
walks from the first cluster, and each jump on to the next central record
walks forward to it.

For the 251-member book, one lookup costs 394,185 chain steps and 3,289 FAT
loads with 4 KiB clusters. Generation runs three lookups (container, OPF,
image), which is 1,182,555 of the remaining 1,185,700 steps. A resumed attempt
skips them. Two changes would remove most of the rest; neither is implemented,
and both touch discovery, a route source:

* validate the archive once per attempt and keep the three members it needs
* read central records in batches and visit their local headers in ascending
  order

**Writes.** In phase 2, each staged copy (container, OPF, deflated member)
was fsynced before close: 3 or 4 explicit fsync calls per cover. The device
counters show 4 for the 95-s book and 3 for a coverless one. `stage()` no
longer calls `fsync`; only the pending file is fsynced, before close,
read-back and rename. That is one explicit fsync per cover, art or no-art.

This saves no card work on the pinned FatFs. `f_close` calls `f_sync`, which
for a file created or written since its last sync writes back its buffer and
directory entry and syncs the volume (`sync_fs`: the dirty FAT window, FSInfo
on FAT32, then `CTRL_SYNC`). `f_unlink` and `f_rename` sync the volume too. The
native suite checks these calls in the linked library. The host harness
counts them as volume syncs: per uninterrupted cover, two per staged copy (its
close and its removal), one for the pending file and one for the rename. That
is 6 for a cover without a deflated image and 8 with one, the same as at
phase 2. Bytes written and allocations per uninterrupted cover are also
unchanged: the read-ahead lives in the decoder's existing block, and the
resume block is allocated only when a book is interrupted. What falls is the
cost of a restart: a resumed attempt stages no container or OPF ("Cancellation
and restarts"). Host tests pin the explicit fsyncs, volume syncs, unlinks and
renames per cover (`cover_job_test.cpp`, the read profile and the resume
test).

The staged copies still need no fsync of their own. Each is read back within
the same transaction and removed before publication. After a power loss it is
a stale temporary that the next attempt removes unread.

Three changes would cut card writes further; none is implemented:

* keep a complete staged image across an interruption, so a restart does not
  inflate and write it again (the decoder already checks it against the member
  CRC)
* reuse one stage file for the container, OPF and image, removing it once
* parse a stored container or OPF in place instead of staging it

## Heap admission

Device heap figures are owner-measured on X3 regional dev-usb firmware
`f008308`, with the owner's library, read through the DIAG bridge. They are
used here as host and native **fixtures**. This port did not measure them
again, and a different library or firmware can differ.

| Screen (fixture) | Free | Largest block |
| --- | ---: | ---: |
| Home, fresh boot | 133,140 | 114,676 |
| Home, after a reader session | 130,636 | 114,676 |
| Library | 119,800 | 102,388 |
| Reader | 65,624 | 53,236 |

`CoverMemory` charges each allocation as header + bytes + 128. It admits an
allocation only when all of these hold:

* free ≥ charge + 65,536 (the reader reserve)
* largest block ≥ charge
* generation total ≤ 64 KiB

The C3 adapter (`cover_native_heap.h`) rechecks the real heap before and after
each allocation. These rules are unchanged from phase 1.

**Start admission.** The idle tick adds a check before the job is allocated.
The planned peak is the structural maximum: while a deflated member is staged,
three things are live together.

```
kCoverPlannedPeakBytes = (sizeof(AutomaticLibraryCovers) + 128)
                       + (sizeof(CoverResume) + 128)
                       + chargeFor(sizeof(Session))
                       + chargeFor(44 KiB inflater)
RV32: (96 + 128) + (32 + 128) + (16 + 2,520 + 128) + (16 + 45,056 + 128)
    = 48,248
kCoverStartFreeBytes  = 65,536 reserve + planned peak + 1,024 slack
```

| | RV32 | Host |
| --- | ---: | ---: |
| Planned peak | 48,248 | 48,464 |
| Start: free ≥ | 114,808 | 115,024 |
| Start: largest block ≥ | 48,248 | 48,464 |

The `CoverResume` term is the caller's copy of an interrupted book's record,
which may be live during a resumed attempt. Container and OPF parsing and the
decoder, with its 4 KiB read-ahead, stay below the planned peak, apart from
the one-row frames over about 1,900 pixels wide ("Read path"). On host, the
largest accounted peak over all 62 matrix fixtures equals it (48,464, deflated
staging). `tests/cover_job_test.py` fails if any measured peak ever
exceeds the host planned peak.

**Margins.** Against the fixtures (RV32):

| Screen (fixture) | Free margin | Largest-block margin |
| --- | ---: | ---: |
| Home, fresh boot | +18,332 | 66,428 |
| Home, after a reader session | +15,828 | 66,428 |
| Library | +4,992 | 54,140 |
| Reader | −49,184 | 4,988 |

Home and the Library are admitted. The Reader is not, by screen and by heap.

**How the peak came down.** The planned peak fell in two steps:

| Stage | Peak | Start needs | Library margin |
| --- | ---: | ---: | ---: |
| Phase 1: raster allocated while inflating | 52,384 (host) | 118,944 free | 856 |
| Staging the member before the raster | 48,848 host, 48,632 RV32 | 115,192 | 4,608 |
| One-tick API | 48,264 host, 48,048 RV32 | 114,608 | 5,192 |
| Restart record | 48,464 host, 48,248 RV32 | 114,808 | 4,992 |

The one-tick API moved the names and memo to the caller. That shrank the job
from 656 to 64 bytes. The fault latch added 8 bytes to `Session`. The restart
record adds 32 bytes to the job, a reference to `Session` and the charged copy
above.

On the device, the phase-2 image's lowest free heap during the first Library
generation was 65,916 bytes, 380 above the reserve ("Device evidence on
the phase-2 image"). This change adds 32 bytes to the job and, while a book is
interrupted, the 32-byte resume block (about 48 with allocator overhead, an
estimate not yet measured on a device). Every allocation still rechecks the
reserve, so a shortfall is a decline, never an overdraft.

**Declines.** A refused start costs one heap probe and no file access. It
settles the view, so the producer does nothing more until the shelf, the Home
book or the screen changes. Repeated declines therefore cannot storm.

A larger library with less free Library heap degrades to no generation in the
Library. Home has 12 to 15 KiB more free in the fixtures, so it still generates
the selected book's cover. A start just at the threshold still keeps the
64 KiB reserve, because every allocation is checked again.

## Bounded retries (negative memo)

Two mechanisms bound card work. Both use only generic per-book identity:
filename, source size and FAT mtime. No title, file or digest is treated
specially.

* **Persistent: the no-art record above.** A book with no usable cover costs
  one generation. After that it costs one header check per visit and never a
  book read.
* **Per visit: `CoverIdleState`,** 16 bytes of static RAM (RV32). It holds:
  * `view`: an FNV-1a hash over a screen seed and the NUL-terminated target
    names
  * `settled`: the memo of settled slots
  * `failures`: a failure count
  * `screen`: the last screen
  * `resume`, `quietFrom`, `interruptions` and `book`: the resume block,
    the clock when quiet began, and the interrupted book's count and
    slot bit ("Cancellation and restarts")

**How the per-visit memo works.**

* A new view clears `settled`.
* A new screen clears everything.
* A slot settles once it is attempted in the view, whatever the result, unless
  the attempt was interrupted. An interrupted book settles at its seventh
  interruption in the view.
* A settled view, including a move within the same shelf, costs nothing per
  tick.
* Returning to a shelf rechecks headers only.

**Failure budget.** These outcomes are failures of the screen visit:

* `Declined`
* `IoError`
* a verdict whose record could not be written
* a cancelled tick with no bound attempt, also when it settled another slot's
  cache first (pending input at tick entry creates no job and spends nothing)

The first four failures (`kCoverFailureBudget`) follow the 2 s settle gate. A
failure does not restart the quiet clock, so failures can follow on
consecutive ticks; an unbound stop is input, so it does. At four failures a
tick waits for 15 s without input, a screen or a view change, and the retained
block, key and count stay. Success does not reset the count: once quiet,
productive ticks continue until input. A fifth failure rests the producer
until the screen changes: the next tick frees the block and returns `Idle`
with no probe, allocation or card access, however long the quiet. A view
change clears the memo and restarts the quiet clock but keeps the count.

The reason for the paced fifth attempt: four stops can come from ordinary
browsing, each press landing just after the settle period, before the book is
bound. Resting at once would end generation for the visit (host test `PASS
idle browsing budget`). A fifth failure still ends it.

Failing or unbound-stopped work is finite per screen visit: at most five
charged ticks, plus productive ones (each produced or recorded book settles).
A refused start admission is not charged: it settles the view after one heap
probe, with no allocation or card access, so only a view change, which needs
input, repeats it. Bound
interruptions keep their per-book limit per view, and a view change renews
that separate allowance; each renewal needs input and a settle period.

Deep sleep restarts the firmware, which resets the memo.

A 32-bit view-hash collision between two consecutive views of one screen could
at worst leave the second view's unsettled books untried until the screen
changes. Nothing is persisted by it.

**Correction to phase 1.** The phase-1 plan said static state would be limited
to one pointer to a heap block holding the job. The implementation has no such
pointer. Static state is the 16-byte `CoverIdleState`. The job is allocated per
tick inside the admitted heap and charged to the generation, and the
`CoverFiles` adapter lives on the stack. The one pointer it holds is to the
resume block of an interrupted book.

## Memory and identity boundaries

**Host accounted bytes** (headers and allowances included):

* stored member: 24,223
* deflated member: 48,464
* every one of the 62 matrix fixtures: at most 48,464

No bytes and no descriptors survive a completed call. An interrupted call
leaves only its 32-byte record, which the idle tick moves to the resume block.
Filesystem allocations, libc and the task stack are outside these figures.

Pinned FAT `fstat` supplies only size and mode. Source timestamps therefore
come from path `stat`, checked around open and again at publication. Size is
checked on every read, and an empty or failed read is a fault.

Generation CRCs the entire source before discovery, unless a restart reuses
that CRC, and again after the pending file is synced, closed and read back. On
FAT it also reopens the current pathname and streams its CRC. That rejects a
same-size, same-time replacement while the original descriptor remains
readable.

NLC2 reuse keeps its existing identity policy: source size, FAT-rounded mtime,
and the payload CRC (or the route constant for no-art). A substituted book
with equal size and mtime can keep an old decorative cover or placeholder. No
cache result authorizes reading.

## Publication and recovery

Publication, for art and no-art records alike:

1. Exclusive-create the pending file with mode 0600.
2. Write the header and raster (header only for no-art).
3. Sync, close, read back and compare.
4. Recheck the source CRC and binding, then cancellation.
5. Remove only a recognized regular NLC2 destination. Pinned FAT rename does
   not replace.
6. Rename.

Stale stage and pending temporaries are removed at the start of the next
attempt. A card pulled or a press during generation therefore leaves at most a
temporary that the next attempt recovers. Arbitrary files, hardlinks and
directories in the covers directory are preserved.

On host, created files are set to exactly 0600 and the covers directory to
0700, whatever the process umask. This is tested under six masks. FAT ignores
mode bits.

A cancellation or power loss inside the deliberate replacement gap can leave no
cache. The placeholder shows, and a later attempt regenerates the file.
Physical rename and directory durability are device gates.

## Host evidence (tools/check.py)

`tests/cover_job_test.py` first checks the SDK checkout against its pin and the
route tripwire. It then builds the real producer headers with ASan/UBSan
against the pinned SDK and runs six suites. Its build directory, like those of
the producer, discovery, decoder and native size probes, must hold only real
project-local directories and unaliased files, so a symlink or hard link cannot
redirect a fixed-name output. The whole directory is checked before compiling,
because `-fstack-usage` writes a sidecar the script never names, and a test
holds every stack-measuring evidence script to that. It requires every expected
`PASS` line, and checks that the idle suite's planned peak covers the matrix
maximum. It checks the SDK pin again before writing its report. The
patched-decoder tests likewise check that their SDK copy holds no links and
still has the digest it had before compiling. Neither check sees a change made
and undone between the two.

Every cover evidence script runs its compilers, `git` (the SDK pin check's
too) and `tar` with one explicit environment, so inherited variables such as
`DEPENDENCIES_OUTPUT`, `GIT_DIR` or `TAR_OPTIONS` cannot change what they read
or write. Each report is bound and written by one helper,
`tools/evidence_report.py`. Each compile behind a report digests its inputs on
both sides. First the preprocessor lists them and the script digests every file
named, including transitive project and SDK headers. The compile then writes
its own dependency rule, which must name the same files with the same bytes.
Every other file a compile names must be an output of that report, unchanged
on both sides of the step, such as a link's object. Steps that only transform
outputs, `ld -r` links and the `objcopy` extracts the native probes measure,
may read nothing else. The helper keeps these digests and each step's output
bytes, `-fstack-usage` sidecars included. Stack reports list only the sidecars
it recorded. It also keeps the project Python running the report, as loaded
when it starts, and the executable each step names. It does not cover programs
that executable runs in turn, such as GCC's `cc1`. Just before writing, it
checks everything again and adds it to the report, so a change that lasts past
a step is refused. The same limit applies: a change made and undone between the
checks goes unseen. Outputs are digested just after their step, so one replaced
in that instant counts as produced. `tests/evidence_report_test.py` holds the
helper to this with a real compiler, and a structural test makes every cover
evidence script use it.

**`cover_job_test.cpp`** covers:

* stored, deflated, legacy and mislabeled parity, and short reads and writes
  down to one byte
* the one-tick API: selected-first order, refused names, invalid call shapes,
  unconfigured or overlong roots, and reentry returning `Busy`
* no-art records:
  * binding
  * zero book reads when settled
  * mtime and route invalidation
  * art replacing a record
  * failed record writes persisting nothing
* mutation, cache and cancellation, and partial-write recovery
* modeled FAT metadata, non-replacing rename and same-size/time pathname swaps
* I/O faults
* path stops at binding, active cleanup, directory creation and publication
  (no `mkdir` or rename after a stop; a rename in progress completes)
* **staged member faults** on a multi-chunk deflated book:
  * each write, sync, close, unlink and open call fails in turn, once, in its
    own run
  * reads, stats and handle stats fail at every fifth call and at the last one
  * a full card is modeled at 95 points

  That is 491 single-call failures, 125 of them after a content verdict was
  reached. None writes a no-art record, and each full-card point ends in
  `IoError` or `Produced`. The count was 725 (167) at phase 2. Every
  operation still fails once; there are fewer operations, because staged
  copies are no longer synced and decoder reads arrive 4 KiB at a time.
* explicit fsyncs, modeled volume syncs, unlinks and renames per cover, on
  host and modeled FAT

**`cover_job_matrix_test.cpp`** runs over 62 authored fixtures
(`tests/cover_job_fixtures.py`):

* 15 produced variants, including a 1,800×1 frame decoded without the
  read-ahead (`m-wide-one-row`: interrupted at each of its 279 polls, and 472
  single-call failures, 6 of them tolerated with exact art)
* declined inputs: PNG, GIF, SVG, progressive and other SOFs, oversize, bad
  sampling or precision
* malformed ZIP, container or OPF; CRC errors; a missing cover; multiple
  candidates
* cancellation, ineligibility and heap-reserve loss at every poll point
* exhaustive single-call failures of every file operation and allocation, with
  and without a stale cache: 427 to 478 failures per run (482 to 548 at
  phase 2, for the same reason), each asserting that no no-art record is
  written for a fault.
* tamper and umask cases

**`cover_idle_test.cpp`** and **`cover_resume_test.cpp`** share one rig
(`tests/cover_idle_harness.h`) and run over heterogeneous books: no cover, PNG,
gray and deflated baseline JPEG, progressive, oversize, corrupt CRC, and a
truncated scan. The idle suite's tests:

* **targets:** shelf and Home targets
* **heap fixtures:** the four fixtures above. Home and the Library produce with
  the reserve intact, and the Reader declines without I/O.
* **degradation:** one byte below each threshold
* **memo:**
  * each book is attempted once
  * revisits read no book
  * an mtime change redoes only that book
* **budget:** four failures after settling, a fifth after 15 s quiet, then
  rest across shelves; a new screen resets the count
* **browsing budget:** four unbound stops 2,050 ms apart on Home and in every
  Library slot; no probe, allocation or card access through 14,999 ms of
  quiet, `Produced` at 15,000 ms, and production continuing while quiet. A
  fifth unbound stop or `IoError` rests through 120 s; a view change keeps the
  count and a screen change resets it; across clock wrap
* **paced verdicts:** after four unbound stops, the 15 s attempt's recorded
  no-art keeps the count at four and the shelf produces; a no-art record that
  fails to publish spends the fifth and rests
* **cancellation:** before start and at every one of 268 polls (276 at
  phase 2), with no persistence or signal and a later retry
* **descriptors:** two free produce; one free fails each book once per view and
  persists nothing
* **Home:** only Home's one book, the hook's single signal, and stale temporary
  recovery

The interrupted-book suite's tests are resume, resume sweep, resumed faults,
resume edges, early versions, unbound budget, shelf alternation,
interruption slots, back-off and per-book back-off ("Cancellation and
restarts" above). `PASS idle unbound budget` keeps SF1's record, key and count
through the 15 s wait, with and without a retained block, at the cache check
and at cleanup, on Home and in every Library slot; the tick after the fifth
stop frees the block without work. `PASS idle shelf alternation` alternates
two shelves, each a current cache ahead of an uncached book: five charged
stops cost exactly 10 stats and 5 opens and no book read, and 32 further view
changes, each after 120 s of quiet, cost nothing.

Every tick asserts four things:

* a clean heap
* no open descriptor afterwards
* at most two concurrent descriptors
* the Home signal rose exactly on `Produced`

**`cover_jpeg_parity_test.cpp`** checks the decoder parity above.

**`cover_read_profile_test.cpp`** profiles card reads on large authored books
("Read path").

`tests/cover_discovery_test.py`, `tests/cover_decode_budget_probe.py` and the
patch suites are unchanged. Host timing is not SD latency or battery evidence.

## Native C3 component evidence (compile-only)

`tests/cover_job_native_sizes.py --compiler <riscv32-esp-elf-gcc> --packages
<platformio packages>` compiles the real headers and pinned SDK sources with
the approved toolchain into `.build/cover-job-native`. It is not part of the
host gate.

It reports the RV32 idle constants and the start margins against the fixtures.
It fails in any of these cases:

* the pinned FatFs enables fast seek, a shared sector buffer or immediate
  fsync; its `f_close` stops calling `f_sync`; its `f_sync`, `f_unlink`,
  `f_rename` and `f_mkdir` stop calling `sync_fs`; or `vfs_fat_write` starts
  calling `f_sync` ("Read path")
* the fixtures stop admitting Home and the Library
* the fixtures start admitting the Reader
* the reachable image contains any of the dispatch symbols above
* the selected `cached(const char*)` production symbol is absent or replaced
  by an IPA signature clone

**Object sizes:**

| Object | Bytes |
| --- | ---: |
| `AutomaticLibraryCovers` | 96 |
| `Session` | 2,520 |
| `CoverResume` | 32 |
| `CoverFaultLatch` | 12 |
| `CoverServiceOutcome` | 4 |
| `CoverIdleState` | 16 |
| `CoverIdleTargets` | 28 |

**Conservative selected call chains** from `coverIdleStep` (the settled-book
row is remeasured for the flag-migration probe; other rows retain the earlier
read-path evidence):

| Chain | Bytes |
| --- | ---: |
| JPEG entropy read (through the read-ahead) | 1,440 |
| JPEG row output | 1,328 |
| Container/OPF XML callback (deepest) | 5,936 |
| ZIP directory with source validation | 4,048 |
| Settled-book header check | 1,568 |
| FAT pathname-reopen CRC | 2,128 |
| FAT pathname-reopen open | 1,216 |

In the earlier read-path evidence, the entropy read is 80 bytes deeper than
at phase 2, as it passes through the read-ahead's 64-byte `readAt`; every
other chain was 16 bytes deeper. The current settled-book sum includes idle
128, `service` 96, `cached` 480, the outlined `coverProbeCache` lambda 560 and
`CoverFileSource::open` 304. Optional outlined probe frames are included by
the component test rather than disappearing from its chain accounting.

These sums exclude the `loop()` frame, FAT/VFS (which has long-filename buffers
on the stack), SPI and interrupt frames.

**Text** kept after `--gc-sections` from the producer entry points is 110,883
bytes (109,159 at phase 2). Most of it is Expat and miniz, which firmware already links.

The four-variant firmware images, their flash, RAM and stack figures and the
reviewed records are in `docs/CODE_HEALTH.md`.

## Firmware builds (compile evidence only)

Four clean `--health-report` builds of `ce4360e` measure the following. The
deltas are against #19's ceilings (`4730a0d`), which four clean builds of
that commit reproduce exactly.

| Variant | Flash | Δ flash | SRAM | Δ SRAM | SRAM ceiling |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 1,470,928 | +29,712 | 133,264 | +16 | 133,264 |
| dev-usb | 1,490,768 | +29,808 | 134,080 | +16 | 134,080 |
| regional | 1,475,152 | +29,712 | 133,776 | +16 | 133,776 |
| regional-dev-usb | 1,494,992 | +29,808 | 134,592 | +16 | 134,592 |

Clean builds of each stage on the same base split the flash growth:

| Stage | default | dev-usb | regional | regional-dev-usb |
| --- | ---: | ---: | ---: | ---: |
| Phase 2 (to `0b9c0e6`) | 27,408 | 27,488 | 27,408 | 27,472 |
| Read path, restarts and writes (to `833d370`) | 1,552 | 1,568 | 1,552 | 1,584 |
| Review fixes (to `96c3634`) | 144 | 144 | 144 | 128 |
| Per-version interruption key (`13ff713`) | 16 | 16 | 16 | 16 |
| Unbound stops charge no book (`34a1e3d`) | -32 | -32 | -32 | -16 |
| Inline Home identity check (`cb37b3b`) | 0 | 0 | 0 | 0 |
| Home no-art memory (`dd1bfcd`) | 64 | 64 | 48 | 48 |
| Path polls and cache probe (`2186b90`) | 496 | 480 | 496 | 496 |
| Settle period (`8f7e281`) | 96 | 96 | 96 | 80 |
| Every unbound stop charged (`ce4360e`) | -32 | -16 | -16 | 0 |

The suite split (`0b4f2af`), before the per-version key, changes tests only,
and the stale-mtime no-art tests folded into `dd1bfcd` add no firmware.

The SRAM growth is `CoverIdleState`: phase 2 adds 16 bytes in the DEV USB
variants, and the read path's larger state 16 in the others. It takes the 16
bytes #19 left below each ceiling; the ceilings are unchanged and now equal
the measured values. One reviewed flash audit entry records the growth.

**Stack.**

* `loop()` measures 80 / 96 / 80 / 96 and `paint(bool, bool)` 80 / 96 / 80 /
  96, both at their ceilings.
* `serviceIdleCovers` is 336 at every stage. The read path keeps
  `Session::attempt` at 256, `coverDecode` at 208 and `sourceCrc` at 1,072.
  The settle gate first took it to 352; `coverIdleStep` now borrows the heap
  rather than holding a second 24-byte copy, which keeps it at 336.
* `cached` measured 960 here (976 first proposed): it binds no book source.
  The current flag-migration probe preserves its selected boolean signature
  and mangled name at 480, under the unchanged 960 ceiling. Its outlined
  `coverProbeCache` lambda measures 560; `service` remains 96. The new frame
  needs an exact guard in the subsequent reviewed resource record.
* `stage` stays 1,200 because it keeps its signature. Recording temporary
  ownership through a reference parameter measured 1,216, one more saved
  register, so the session sets the bit when `stage` returns `Ready`.
* The poll before `mkdir` outlines `Session::ensureDirectory` (112) while
  `Session::attempt` stays 256.
* `coverResumeKey` and `noteInterruption` are out of line. Inlined, they grew
  `Session::attempt` past 256 and pushed `coverIdleStep` out of
  `serviceIdleCovers` as a new 128-byte frame. Neither frame is live under the
  deep chains: the key returns before discovery starts, and
  `noteInterruption`, which holds a copy of the record, runs after the job's
  calls have unwound. `coverIdleStep` is `always_inline`: once the per-book
  checks grew it, the compiler outlined it as a separate 128-byte frame under
  the job's call chains.
* `homeCoverIdentity` is `always_inline`. Outlined, with the producer as its
  second caller, it took `prepareSelectedHomeCover` to 64 bytes, over its
  48-byte ceiling.
* The protected audit guards 80 nonzero frames at their measured sizes,
  identical in every variant: the entry `serviceIdleCovers` 336, the largest
  `parseCoverOpf` 1,632, the read path's `CoverReadAhead::readAt` 64,
  `noteInterruption` 64, `coverResumeKey` 48, `Session::locate` 32 and
  `forget` 16, the seek-and-read `CoverFiles::readAt` 32,
  `LibraryReaderSelection::retain` 16 and `Session::ensureDirectory` 112. It tightens
  `PreparedLibraryCovers::begin` from 384 to 368, which now calls the shared
  `libraryCoverLittle` (32) out of line.

**Linked text.** In every map, pngle, `ImageRenderer` and the progressive
decoder have no linked text, and TJpgDec's `jd_prepare` and `jd_decomp` are
new.

**Build results.** Each build stops only on the old flash ceilings, which
the record commit updates. The record and its reasons are in
`docs/CODE_HEALTH.md` ("Automatic cover producer cost"). These builds say
nothing about device stack high-water, heap, latency or behaviour. The device
evidence follows; stack high-water is still unmeasured.

## Device evidence on the phase-2 image

The owner ran the phase-2 image on an X3: the regional dev-usb variant, with a
private probe build, the owner's library of 62 books and the DIAG bridge. The
phase-2 image is phase 2 as first recorded, built on an earlier state of #19
before this series was restacked. These are
device facts about the phase-2 image, and, in the last part, about base firmware
timed before and after the phase-2 image ran. They are not about the read path,
restart and write changes above, and this port did not reproduce them.

**Per-cover cost.** An uninterrupted visible shelf of four books, in a silent
300 s window:

| Result | Time | Written | Writes | Syncs | Unlinks | Renames | Opens |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `Produced` | 95,273 ms | 69,533 B | 71 | 4 | 3 | 1 | 11 |
| `Produced` | 6,676 ms | 126,858 B | | | | | |
| `Produced` | 10,187 ms | 110,773 B | | | | | |
| `Missing` | 2,553 ms | 10,008 B | 12 | 3 | | | |

Blank cells were not reported.

**Cancellation churn.** Under the development bridge's idle polls every 40 s,
which cancel through unread host bytes, the 95-s book was cancelled at 40,024
to 40,026 ms four times in a row. The counters were identical each time:
69,533 B written in 71 writes, 4 syncs, 4 unlinks and 10 opens. Each restart
redid all discovery and staging writes from zero.

The cancelled attempts wrote as much as the completed one. They differ only by
one more unlink and one fewer open. The reading here, an inference rather than
a measurement, is that each was interrupted inside `commit()`: after the
pending file was written and synced, and before the pathname reopen. That is
where the whole-source verification passes run. The read path analysis above
predicts quadratic cost in exactly those passes. This agrees with the
diagnosis but does not prove it.

**Responsiveness.**

* The next bridge reply came about 20 ms after a cancel.
* A press during generation measured 862.8 to 871.7 ms, against 866.6 to
  876.8 ms on a settled shelf.

**Heap and stack.**

* The lowest free heap during the first Library generation was 65,916 bytes,
  380 above the 65,536 reserve.
* Library free heap dropped once by 216 bytes (120,360 to 120,144) at the
  first generation and then stayed flat.
* The reader stack's lowest free after generations was 9,984 of 16,384 bytes.

**Behaviour.**

* Revisiting a settled shelf caused no repaint.
* Home after boot had no target.
* The reader opened normally after a generation.

**A step in base reader timings, not attributed to the producer.** Device
facts, on the owner's card:

* After the first producer session, base firmware without the producer was
  slower. Reader to Home took 40 ms longer with an unchanged paint. A warm
  open took 22 to 25 ms longer, and an open after restart 25 ms longer. The
  "Reader route elapsed" figure for warm opens went from 1,362 to 1,369 ms to
  1,382 to 1,390 ms.
* About 4.5 hours later the step was still present on base firmware. Reader to
  Home took 1,282.9 to 1,290.9 ms, against 1,233.6 to 1,241.7 ms before the
  producer had run. A warm open took 25 to 30 ms longer. Library paths were
  unchanged.
* The owner then ran an A/B with an instrumented base build,
  `f008308b64a3-regional-experiment-dirty` (`f008308` plus a private,
  uncommitted probe), before and after letting the phase-2 image generate four more
  covers on an untouched shelf (300 s quiet). Reader exit, open and
  per-sector latencies were identical. The `saveEstimate` stage took 227.7 ms
  before and 226.6 ms after; exits took 1,293.7 to 1,300.6 ms before and
  1,293.7 to 1,296.6 ms after.

One A/B on the owner's card showed that a further producer run did not slow
base reader exits or opens. The earlier persistent +40 ms step that followed
the first producer session is not attributed to the producer. No census from
before that step exists, so its cause is unknown. It is consistent with base's
own path walks: each exit stats the estimate file 14 times, and each stat
reads every directory sector up to the entry's position in the shared Pages
directory. Base's temp+rename publish moves that position.

No producer mitigation follows from this. The fsync and read-path reductions
above stand on their own host evidence ("Read path"); they do not reduce what
an uninterrupted cover writes or syncs on the card.

## Device evidence on this port

Regional DEV USB images of this port ran on the same X3 and card (62 books)
through the DIAG bridge. A private host script drove them, and the
implementing agent measured them; the owner did not. Press figures are bridge
timestamps: request to the terminal record, and request to the record that
`loop()` consumed the press. The bridge sometimes stops a run ("Protocol
response timed out"), with base firmware too; those runs were repeated after
a USB reset, and their completed presses are included. Base is
`0184bbb0da20-regional-experiment`, the firmware of `f008308`.

**An earlier image of this series**, before the restack onto #19's final
state, had the read path, restart and write changes and a private probe of
attempts and heap. The probe's static log lowered free heap by about 4.6 KB.

* An untouched shelf, 300 s quiet: `Produced` in 13,552 ms (5.0 MB read) and
  5,118 ms (1.5 MB). A declined cover took 28,704 ms: its whole 13.2 MB
  source was read before its no-art record.
* Under the bridge's 40 s idle polls: `Produced` in 16,646 ms (6.9 MB).
  Another book was cancelled after 11,931 ms with its record kept, resumed
  from it, was cancelled again after 1,481 ms, and resumed to `Produced` in
  8,674 ms.
* A third shelf, uninterrupted, with per-allocation heap probes: `Produced`
  in 5,734 ms and 15,451 ms (5.8 MB), `Missing` in 980 ms. Free heap fell
  from 115,456 to 67,704 bytes, 2,168 above the reserve. The producer's own
  peak was 47,752 bytes against the planned 48,248 RV32, and it equalled the
  fall, so nothing else allocated meanwhile.

**The press delay and its fix.** Base and `dd1bfcd` ran in one session; the
image of `8f7e281` and base again ran in a later one, after `dd1bfcd` and
`8f7e281` had generated covers. Request to terminal, in ms:

| Press | Base | `dd1bfcd` | `8f7e281` | Base, after |
| --- | --- | --- | --- | --- |
| Warm open from Home | 2,142.2–2,146.1 | 2,177.8–2,185.6; 2,264.5–2,287.5 in 10 of 15 | 2,171.7–2,177.5 | 2,141.7–2,146.0 |
| Open after restart | 2,475.8–2,478.3 | 2,511.1–2,525.7 | 2,504.2–2,518.2 | 2,474.0–2,480.4 |
| Reader to Home | 1,282.7–1,291.9 | 1,292.8–1,300.0 | 1,292.3–1,301.2 | 1,282.7–1,288.2 |
| Library open | 1,009.6–1,010.7 | 1,010.5–1,016.9; 1,098.3–1,104.1 in 5 of 12 | 1,008.0–1,009.2 | 1,009.7–1,011.2 |
| Library to Home | 976.0–977.2 | 986.0–1,002.8 | 983.3–984.1 | 976.5–978.2 |
| Request to consumed | 10.9–12.3 | 10.8–102.2 | 10.8–12.6 | 10.5–12.2 |
| Presses | 34 | 64 | 39 | 52 |

* `dd1bfcd`: the slow opens are the delayed presses, consumed after
  100.0–102.2 ms. Library to Home was consumed after 17.9–25.7 ms in 7 of 12
  presses, and one open after restart after 70.0 ms.
* Outside the ranges: one base Reader to Home of 1,460.7 ms, not
  investigated, and two on `8f7e281`. In one (2,797.7 ms) the bridge held the
  device's output, and the device's own total was 1,261 ms. The other
  (1,401.1 ms) included a 174.8 ms progress write.
* The remaining excess over base, about 30 ms on opens and 6–10 ms on the
  paths to Home, does not come from the producer. The image of #19 alone
  (`4730a0d`) measured warm opens of 2,180.6–2,182.7 ms and Reader to Home
  1,295.7–1,300.7 ms, in a session beside an earlier image of this series
  (2,177.7–2,185.8 and 1,292.6–1,298.7 ms, apart from that image's two
  delayed presses and three outlying exits).
* With 2.02–2.25 s of silence after each reply, so that each press arrives
  just after the settle period, `8f7e281` consumed 30 presses in 3.0–12.2 ms.

**Presses during production (`8f7e281`).** These ran on shelves that no image
had generated.

* A 180 s quiet visit produced two covers. The 66 presses around it were
  consumed in 10.9–16.5 ms, except one artifact (below).
* Presses 2.3–5.2 s after the previous reply land after the settle period,
  while the producer works. Three runs on two shelves had 277 presses, 40 of
  them so timed. Request to terminal minus the device's own refresh time was
  35.8–36.0 ms at the median of each run. For the timed presses it was at most
  73.2 ms, and they were consumed in 3.1–49.5 ms. The 37 ms excess is
  consistent with one card operation in flight at the stop, which the path
  polls allow ("Read path"). Which phase each press interrupted was not
  recorded. No cover finished during those presses.
* Four presses had consumed records 856.8–884.6 ms after the request.
  Each took 927.1–950.0 ms to the terminal with a device refresh of
  887–915 ms, the same 35.0–40.6 ms overhead as its neighbours. The device
  consumed them on time; the bridge delivered the consumed record late,
  together with the terminal record.

**Base timings after generation.** The base timings after the `dd1bfcd` and
`8f7e281` sessions overlap the earlier ones on every path (table above). These are bridge timings only; no per-stage timing or card census was
taken.

**The current firmware (`ce4360e`).** Its firmware differs from `8f7e281`
only in the failure budget ("Bounded retries"). Its image and then base ran
in one later session. Request to terminal, in ms:

| Press | `ce4360e` | Base, after |
| --- | --- | --- |
| Warm open from Home | 2,178.7–2,185.3 | 2,140.4–2,146.4 |
| Open after restart | 2,511.0–2,532.6 | 2,476.2–2,478.6 |
| Reader to Home | 1,291.2–1,298.9 | 1,282.2–1,290.6 |
| Library open | 1,008.4–1,014.3 | 1,004.5–1,012.0 |
| Library to Home | 983.0–989.9 | 976.6–982.5 |
| Request to consumed | 10.3–12.6 | 10.8–12.3 |
| Presses | 62 | 56 |

* Five Reader to Home presses on `ce4360e` took 2,807.7–2,811.6 ms. Each was
  consumed in 11.5–12.0 ms, the device's own totals were 1,261.3–1,263.3 ms
  (4 of 5 logged), and the reply arrived only after the driver's bridge kick.
  They are left out of the table. First opens that followed no restart (2,205.6–2,206.5;
  base 2,143.6–2,170.1) are left out of the restart row.
* Warm opens were 2,171.7–2,177.5 ms on `8f7e281` in its own session. The
  open path's firmware is the same in both images; the difference between
  the sessions was not investigated.
* Base after `ce4360e` overlaps the earlier base timings on every path.
* Presses during production: three runs on shelves that no image had
  generated had 260 presses. In two runs, 16 presses came 2.4–5.2 s after the
  previous reply; the third lost a press to the bridge before its timed ones.
  Request to terminal minus the device's refresh time was 35.8–35.9 ms at the
  median of each run. Fifteen timed presses were consumed in 3.1–46.0 ms, at
  most 69.9 ms over the refresh time.
* The sixteenth was consumed 841.1 ms after the request. Meanwhile the
  Library repainted for a newly produced cover, a 693 ms refresh that no press
  requested, and the press was consumed 50 ms after that refresh ended. The
  log does not show whether the press arrived during the cover's publication
  or its repaint; a press that arrives during publication is handled after
  the repaint.

## Device acceptance (remaining)

Each check names what to observe. None is established by the host suites.
The phase-2 image gave partial device evidence for checks 2, 3, 6 and 8 (above).
An earlier image of this series gave partial evidence for checks 6 and 11,
and `8f7e281` and `ce4360e` for checks 3 and 12 ("Device evidence on this
port"). Every
other check, and every part of these four not reported there, still needs to
be run on this change.

1. **Library, mixed shelf.** Put books with a baseline JPEG cover, a PNG cover,
   a progressive cover and no cover on one shelf, and open the Library.
   * Expected: the baseline covers appear one repaint per book, selected book
     first. The others keep the placeholder. The covers' appearance has not
     been checked on a device.
   * Expected in `Newt/Covers`: a 3,412-byte `.nlc` for each art cover, a
     28-byte `.nlc` for each other book, and no `.tmp` file. No file listing
     has been taken on a device.
2. **Revisits.** Leave and re-enter the Library, and move within the shelf.
   * Expected: no repaint from the producer (seen on the phase-2 image).
   * Expected: the `.nlc` files keep their modification times.
3. **A press during generation.** Use a book with a large deflated cover.
   * Expected: the press is handled as promptly as on an idle shelf (seen on
     the phase-2 image, and on `8f7e281` and `ce4360e` within one card
     operation, except one press on `ce4360e` that waited 841.1 ms while a
     newly produced cover was published and repainted).
   * Expected: the interrupted book's cover appears after the next idle tick,
     or after the quiet interval once two restarts have passed. A book
     interrupted seven times in one view keeps its placeholder until the view
     or screen changes.
   * Expected: no `.tmp` or partial `.nlc` remains.
4. **Card pulled during generation.**
   * Expected: no crash or hang; at most four failed attempts after the 2 s
     settle period, a fifth only after 15 s without input, then rest until
     the screen changes.
   * Expected: after reinsertion and a rescan, generation continues while the
     visit's budget lasts: at four failures after 15 s without input, a
     screen or a view change; after a fifth only once the screen changes
     (host tests `PASS idle failure budget` and `PASS idle browsing budget`).
5. **Home after a reader session.**
   * Expected: the selected book's cover appears once and Home repaints once.
   * Expected: sample books and cardless Home never generate.
6. **Heap.** Watch DIAG heap during generation.
   * Expected: free heap stays at or above 65,536 bytes (lowest 65,916 on
     the phase-2 image; the job is now 32 bytes larger, and an interrupted book adds
     the 32-byte resume block, about 48 with overhead by estimate).
   * To record: the resume block's charge and address and the largest
     generation allocation's address (the private probe's `resblk`, `resat`
     and `bigat`).
   * Expected: the Reader opens normally right after a generation, including
     an open that interrupted one, and no generation runs while reading.
7. **Lifecycle.** Exercise each of these during and after generation, and
   expect normal behaviour with no stray temporaries:
   * sleep and wake
   * cardless boot and wake
   * sample selection and wake
   * saving newer progress after reinsertion
8. **Development builds.**
   * Expected: no generation while bridge automation is active.
   * Expected: the `Reader stack` high-water after the deepest generation
     (deflated cover, large OPF) stays inside the 16 KiB loop task (9,984
     bytes free after generations on the phase-2 image).
9. **Descriptors.** On Home with a selected SD book, after closing the reader:
   * Expected: the cover generates.
   * If it does not: four `IoError` attempts after settling, a fifth after 15 s
     without input, then rest until the screen changes.
10. **Per-cover cost.** Repeat the four-book shelf of "Device evidence on
    the phase-2 image" with the same counters.
    * Expected: one explicit fsync per cover. Volume syncs are not expected
      to fall ("Read path", Writes).
    * To record: time, bytes written, writes, unlinks and opens per cover.
11. **Restarts.** Repeat the 40-s idle polls on the slowest book.
    * Expected: resumed attempts write no container or OPF staging, and
      restarts after the second wait 15, 30, 60, then 120 s. The seventh
      interruption of the book in one view sets it aside until the view or
      screen changes.
12. **Base timings after generation.** Repeat the A/B of "Device evidence on
    the phase-2 image" with this change as the producer: time base reader exits and
    opens with an instrumented base build before and after this change
    generates a shelf.
    * To record: the exact build names of both firmwares, the exit, open and
      per-stage timings before and after, and whether any card state differs
      (for example directory positions in the census).
    * Bridge timings of base after `8f7e281` and `ce4360e` generated overlap
      the earlier ranges ("Device evidence on this port"). The per-stage timings and the census are still
      to be recorded.

## Risks and deferred work

* **Stack:** the deepest selected chain is 5,936 bytes on the 16 KiB loop
  task, before FAT long-filename buffers and driver frames. The phase-2 image left
  9,984 bytes free on the device; this change needs its own measurement.
* **Heap:** the Library margin is 4,992 bytes against the owner's fixture
  figures. The phase-2 image's lowest free heap on the device was 380 bytes above the
  reserve. A larger catalog leaves the Library declining, which is safe but
  shows no new covers there. Home still generates.
* **Descriptors:** with `maxFiles=3`, a source held on Home would leave too few
  descriptors. Host tests bound this to a few failed attempts per visit with
  nothing persisted. Device check 9 settles it.
* **Budget and browsing:** ordinary browsing can spend the four failures
  ("Bounded retries"). Generation then needs 15 s without input, and a fifth
  failure ends it until the screen changes, even after a long quiet or a view
  change.
* **Resume block lifetime:** an interrupted book's 32-byte block (about 48
  with overhead, an estimate) can be live while the press that interrupted it
  opens the reader, until the next tick. In development builds it can outlive
  a screen change while bridge automation holds background work, until the
  first tick after it ends. Forgetting it there would discard the progress
  that the bridge's own idle polls interrupt, so this is documented, not
  changed. Device check 6 covers it.
* **Resume block placement:** the block is allocated after the job's block is
  freed, so it can reuse the job's place. The pinned allocator (TLSF) picks a
  free block by size class and takes the request from its front, so the block
  shortens a free region by its charge rather than splitting it. It can still
  separate free space that would otherwise merge when a neighbour is freed.
  This is reasoning about the allocator, not device evidence; device check 6
  records the addresses.
* **Stale temporaries of a cached book:** an attempt removes its book's stale
  `.cover-stage.tmp` and `.nlc.tmp` first, but a book whose cover is already
  current is never attempted. A temporary left beside a published cache
  therefore stays: for example a deflated book's staged image, which is
  removed only after its no-art record is published, when that removal fails
  or power is lost first.
  This predates the port (phase 2 behaves the same) and is out of scope;
  removing it would cost two stats per settled book per view.
* **Latency and battery:**
  * On host, each generation reads the source about three times: 529,083 bytes
    for a 174,112-byte stored book. FAT adds one more pass.
  * A deflated image stages up to 4 MiB to the card.
  * On the phase-2 image one book took 95 s. The host model is consistent with most
    of that being parked `pread` chain walks, which this change removes, but
    it does not establish the device cause. That book has not been timed
    again. On an earlier image of this series, covers took 5.1–16.6 s, and
    a declined cover 28.7 s, because its whole 13.2 MB source was read
    ("Device evidence on this port").
  * Many-member archives still pay for ZIP lookups that jump between the
    central directory and local headers ("Read path"). The whole-source passes
    still read 1 KiB at a time.
  * A 64 MiB archive would take many seconds per pass on SPI SD. This is
    cancellable but not yet measured.
* **Base timing step:** a persistent +40 ms step in base Reader to Home
  followed the first producer session on the owner's card. Its cause is
  unknown and it is not attributed to the producer: one device A/B found that
  a further run of the phase-2 image did not slow base reader exits or opens ("Device
  evidence on the phase-2 image"). Bridge timings of base after `8f7e281` and
  `ce4360e` generated overlap the earlier ranges; the instrumented A/B of device check 12 remains.
* **Declined covers:** these leave the placeholder and a no-art record:
  * trailing bytes after EOI
  * TJpgDec restrictions, for example 4:1:1 or 12-bit
  * table segments over TJpgDec's 512-byte buffer
* **Route changes:** a later decoder change must bump `kCoverNoArtRoute`, which
  makes every recorded book generate once more. The tripwire forces the
  decision but cannot make it.
* **Reads that succeed with wrong bytes:** the producer trusts a read that
  reports success. A successful decode is checked against the member CRC before
  its art is kept. Staging checks each copy it completes, and a staged copy's
  later mismatch is the card's. A negative outcome from the ZIP directory, the
  metadata, JPEG validation or the decoder can follow such a read, and is
  recorded like any verdict. A stored member's CRC mismatch is likewise taken
  as the book's. The record stays until the book's size or mtime changes or the
  route is bumped, so a valid cover can keep its placeholder. Checking the CRC
  after negative outcomes alone would not settle it: a stored member's mismatch
  stays ambiguous, and a re-read can be served from FatFs's per-file sector
  buffer. One remedy reproduces verdicts from independent reads, through
  producer frames that are at their stack ceilings. Another keeps suspect
  negatives unrecorded, at the cost of generating them again. Both are left for
  a separate decision.
* **View identity:** the memo knows a view by a 32-bit FNV-1a hash of its kind
  and names. Two different shelves with the same hash share its settled bits,
  so the second shelf's covers wait until the view or the screen changes. Names
  can be chosen to collide. For other names the chance is about 2^-32 per view
  change if the hash spreads them evenly, which FNV-1a does not guarantee. An
  exact identity needs the names or a catalog version, and static RAM is at its
  ceiling.
* **Owner decisions:** the PNG patch, and every device gate above.

## Commands

```
umask 022
python3 -B tools/check.py --profile linux      # includes all cover host suites
python3 -B tests/cover_job_native_sizes.py --compiler <gcc> --packages <dir>
python3 -B tools/firmware-build.py --clean --health-report   # the one firmware build
```

Fixture renderings and reports go to the ignored `.build/cover-job-tests`.
Nothing here publishes, flashes, mounts a card or establishes device
acceptance.


## Structural early-rejection correction (2026-10-11)

The interim f73 combined image reported an Unsupported cover at 53,179 ms,
23,893 reads / 24,460,196 wrapped bytes before decoding. Its early speculative
walk returned fixed archive/XML Unsupported without marking that result as a
proved early verdict, so the producer still performed whole-source validation.

The correction promotes only the existing fixed ZIP/XML admission rejections
to the early route for archives at least 8 MiB. Parser inputs are staged and
CRC-checked before content policies can decide; declared archive/member and
XML size limits themselves require no payload scan. Before renaming the durable
28-byte no-art record, the producer reopens the current pathname with the
existing stat/open/stat binding and requires another definitive Unsupported
proof. Invalid CRC, malformed data, missing artwork, I/O, allocation failures
and cancellation retain their existing paths. The supported JPEG path, limits,
SDK and stored record format are unchanged. CJB2 is retained: stable-source
content admissibility/verdicts do not change; only proving the existing negative
result and refusing an ambiguous changed-path publication change. Its route
source digest is deliberately refreshed, preserving valid existing negatives.

Authored heterogeneous host tests add an oversized selected member, the exact
4 MiB boundary, corrupt OPF CRC fallback, cancellation before the record and a
same-size/same-mtime replacement by admissible artwork. These supplement the
existing unsupported-signature, I/O, supported JPEG and resume coverage. Final
Linux/resource and hardware evidence belongs to Decision 060; this record does
not claim optical acceptance, a formal latency baseline or final gate success.
