# Canonical Home cover presentation adapter

> Publication status: isolated integration of draft PR #19 on the port branch, wired into the firmware Home paint. This document records host evidence and native health builds; its flash record and frame guards are in `docs/CODE_HEALTH.md`, "Home cover cost". Device results for the missing-cover and present-cover paths, from separate device runs, are under Performance. This is not an installed image.

The painter was first an isolated source-only slice, based on delivered footer
checkpoint `fbd8bb90248ed4ab482647a0231380b5b269a7fd` on the original draft
branch `feat/canonical-home-cover`, outside this history. That slice supplied
the shared canonical painter and its bounded input contract; integrated commit
`fbb08ca` names it and its draft in `Source-Commit`/`Source-Draft` trailers.
This integration also wires it into firmware Home and
implements the refresh contract below. Automatic cover generation stays
outside: a producer only publishes NLC2 files and calls one hook.

## Minimal runtime interface

`PreparedHomeCover` is one caller-owned fixed snapshot: 3384 raster bytes plus
one readiness byte, exactly 3385 bytes. It carries no filename, catalog, source
handle, identity pointer or heap allocation. `prepareHomeCover(source, filename,
snapshot)` uses the existing `LibraryCoverSource`: `begin`, then `readAll` for
all 188 rows of 18 bytes, then `end` before returning. A failed begin, read or
CRC check clears all snapshot bytes and leaves readiness false. Null/empty
filenames do not start a read and still close the supplied source.

`readAll` is a virtual with a default that calls `readRow` 188 times, so every
source keeps its own validation. `PreparedLibraryCovers` overrides it with one
unbuffered `read` of the 3,384-byte payload and the same CRC32 check that its
last `readRow` performs. It is valid only straight after `begin`; anything else
ends the stream. The firmware read is one open of the cover file, the header
read inside `begin`, and one payload read. Before, it was the open, the header
and 188 row reads. Home stats the book itself only until it has learned the
book's size and mtime (see What Home remembers of the book).

It uses the same validated cached-cover adapter as Library,
`PreparedLibraryCovers`; the presentation helper adds no cache format or EPUB
decoder. The filename comes from the selection's owned identity, never from a
catalog pointer retained through reader admission.

Every Home render is the one `renderFrostedHome(..., style, const
PreparedHomeCover* cover = nullptr)`. A non-null snapshot is drawn last, over
the canonical slot, identically in every plane. A null snapshot, or any Home
card flag, gives exactly today's Home pixels. Firmware passes the session's
held snapshot to every full plane. The generic `render()`, the host dispatcher
(`tools/host.cpp`), the approved previews and Sleep pass none, so they render
today's Home. Drawing performs no storage I/O, so an I/O failure or cache
replacement cannot mix valid artwork in one plane with a placeholder in the
other.

## Canonical geometry and failure treatment

The painter takes its slot from the immutable `kHomeImages[0]` asset metadata:
x332, y87, 148x222. It clears that complete rectangle to canonical white paper,
centers the unscaled 144x188 thumbnail at x334,y104, and preserves every pixel
outside the slot. No crop, stretch, font, wordmark, footer or frame changes.
The snapshot's MSB-first 1=black pixels become scalar black in both absolute
selectors and ink in monochrome; padding is scalar white. An unready snapshot
restores the exact existing Inter `No cover` treatment, including over the
fictional fixture art. The fixture remains unchanged without a snapshot.

The slot is written in whole frame bytes: paper, then the art's MSB-first bits
shifted to their column, masked at the slot's two edge bytes. The frame is
MSB-first too, and selectors set paper where monochrome sets ink, so art ink
flips the plane's paper byte. A static assertion keeps the art inside the
slot. With a snapshot, the fixture stamp and the placeholder are no longer drawn
underneath only to be cleared. The pixels are unchanged; the tests compare every
plane with the reference renders and Library's own art.

## Cover provider contract

A prepared cover is main's NLC2 file `/Newt/Covers/<book filename>.nlc`: a
28-byte header (magic, 144x188, source size, FAT-rounded source mtime, payload
CRC32), then the 3,384-byte raster. It is decoration only: never content
identity and never permission to read a book. Home validates it through the
same `PreparedLibraryCovers` as Library: name, exact size, header, size and
mtime binding, and the payload CRC. Library binds the header to a stat of the
book on every read; Home binds it to the size and mtime it learned of the book
(see What Home remembers of the book). Any failure is the placeholder.

The cover producer (#20, Decision 059) also writes no-art records: a bare
28-byte header whose last field is its route, `kCoverNoArtRoute`
(`include/newt/library_cover.h`), instead of a payload CRC. Library rejects one
by its size, unread. Home shows the placeholder too, and remembers that the
book has no cover when the record is bound to the learned size and mtime and
names the current route, exactly like a missing cover file. Any other bare
header (a torn cover, which names its CRC, another route or another version's
record) is rejected and not remembered.

A producer's whole contract with Home is four functions in `newt::firmware`:

- `noteHomeCoverChanged()` (`firmware/home_cover_signal.h`), called once a
  cover is published completely and atomically. It increments
  `homeCoverGeneration`, a relaxed `std::atomic<uint32_t>`. It never names a
  book and wraps only after 2^32 changes. It may run on any task. It performs
  no allocation, storage or display access, and it does not schedule a paint.
  Home's next ordinary paint, never the timer's status-only pass, is then a
  complete composition that reads the cover again. A spurious call costs one
  full Home and one read. The call is required, not a latency nicety: Home
  remembers a cover file the card said does not exist, and a no-art record, so
  without the call a newly published cover stays the placeholder across
  sessions until the book key changes (see What Home remembers of the book),
  and a held cover stays until the session ends. That is never wrong content.
  The card runtime calls it too, from `LibraryReaderSelection`: when
  `cardAbsent()` finds no card, and when Read (`ensureSource()`) finds the
  selection's held file left stale by a swapped card. Every call also makes
  Home forget what it learned of its book's files.
- `homeCoverEligible(selection, data)` (`firmware/home_cover.h`), Home's own
  test of whether a selection shows a cover at all (see Runtime lifecycle).
  A producer targeting Home's book uses it so that it never prepares a cover
  Home would not read.
- `homeCoverIdentity(selection, identity)`, the selection's own validated
  `SdResumeRecord` copy. Its filename names the cover file, exactly as Home's
  read names it. False means there is no file to target.
- `releaseHomeCover()`, which frees the session's held art to reclaim heap
  (see Runtime lifecycle and heap).

**The bus rule.** The display and the card share one SPI bus, and every SD
operation needs exclusive use of that idle bus (`sd_shared_bus.h`). Home reads
the card inside its paint on the UI task, between the same paint's display
transfers. A producer's card operations therefore run only on the UI task
between paints, hold no card file handle across a paint, and each has
exclusive use of the bus.

## Home session and refresh contract (implemented)

The cover never enters `HomeRefreshPolicy`'s key: no pointer, filename or
title hash. `HomeCoverRuntime` keeps a session instead. Its key is `current`
(a session is open), eligibility, the selected book's digest-derived
`bookId()` (0 when ineligible) and the cover generation it last accepted. It
also holds the session's art.

First in every Home paint, `observeHomeCover()` loads the generation before any
read and takes eligibility and the book. If no session is open or any part of
the key differs, it releases the held art, records the new key and calls
`policy.invalidate()`. An invalidated key is a whole Home: never a focus-only
or battery-only update, even when title, progress, focus and battery match.
An unchanged key keeps both the art and the refresh key. A publication during
the paint's read is seen by the next paint, which repaints once more.
Firmware has exactly one Home key, which this runtime invalidates.

Each full composition calls `readHomeCoverOnce()` before its first full-plane
display operation: in the Overlay build right after the 52,272-byte mask
allocation succeeds, otherwise before the Direct or monochrome base. The
Overlay build's Direct fallback calls it again before its own base. Only the
session's first call reads. Every later call, in the same paint or a later
one, and even after a failure, changes nothing. `renderHomePlane()` draws
`heldHomeCover()` and never reads. Therefore:

- A focus move with an unchanged key never reads the card. Regionally it stays
  the binary island; the default Direct build composes from the held art
  without allocating.
- A forced full Home (the cleanup refresh) recomposes from the held art.
- The regional Direct battery pass draws the held art. The art changes only
  at a full composition, so it is what the panel shows.
- A key change, including a release from outside a paint, invalidates the
  Home key. The timer's status-only pass then neither paints nor reads, and
  the next ordinary paint is a whole Home.

**Failures and the bounded retry.** Late validation, I/O and allocation
failures paint the placeholder in every plane and hold nothing. The session
does not read again: focus, battery and forced paints keep the placeholder.
The only retries are key changes, each one read: a pulled card's return, a
publication, another eligible book, or a new session after leaving Home.
A pulled card sets the Home card flags, ends eligibility and is a whole Home
without a read. When the card returns, Home reads once more. A failing or
flaky card therefore costs at most one read per session or per key change,
never one per paint or timer tick.

**Leaving Home** ends the session. Every non-Home paint, including book
opening, calls `leaveHome()`, which invalidates the key and releases the art.
A Home action that leaves Home (`dispatchLeavingHome`) releases the art before
the destination runs, because Read opens the reader on this heap before its
first paint. Moving focus keeps the art. Display reset and sleep invalidate as
before.

**Same-name limitation.** The NLC2 binding is the book's name, size and
FAT-rounded mtime, not its content. A different book saved under the same name
with the same size and 2-second mtime validates the old cover until its cache
is regenerated. That is wrong decoration, never wrong content. Home shows the
bytes it read for the whole session, so a cache rewritten under the same name
without a call to the hook appears only in a later session. A cache written
where the card said there was none appears only after the hook or another
book-key change, which is why producers must call it. The key's
`bookId()` comes from the book's digest, not its filename, so the same book
under a new filename does not change the key. It does not need to: the
selection changes only away from Home (in Books, or at boot or wake), those
paints end the session, and the next session reads the new name. Its other
identity record forgets the remembered facts, unless the two records'
checksums collide (see the limits below).

Home compares a cover's header with the size and mtime it learned, not with
the book on the card now. The runtime sees a swap when it finds no card (at
Read, Books, a failed read of the card's state, or wake), or when Read finds
the selection's held file stale, and each forgets the facts. Only a swap it
cannot see keeps them: no card-missing answer, no wake, and no held file for
Read to find stale. For example, a swap while Home is shown, or a swap seen
only through Books, which does not read the held file: that read would add a
card access to every Books entry after reading, for decoration only. Then a
changed book of the same name can show a cover bound to the learned book, and
a cover bound to the new book is rejected (the placeholder), until the book
key changes. Likewise, if FATFS's disk-status check missed a pulled card while
the covers folder's directory sector was still cached, the open could report
no file, and Home would remember no cover until the book key changes.

The facts' record is its 32-bit checksum: `prepareSelectedHomeCover()` keeps
them while `facts.record` equals `workspace.identity.checksum`
(`firmware/home_cover.h:91`). The same book saved under two filenames has one
`bookId()`, so if the two records' checksums collide, the second file inherits
the first's facts. A remembered "no cover", or the first file's size and
mtime, then shows the placeholder for the second file until the book key
changes (a publication or a card seen missing) or the device restarts, which
clears the facts. Comparing the whole record would keep its digest and
filename in the facts (more static RAM), or would signal a change when the
selection accepts another record (another line past `library_reader.h`'s
line exception), so this stays a documented limit.

All of these are decoration only, never wrong content.

## Runtime lifecycle and heap

Eligibility comes from the firmware selection alone. The selection must be
active (an SD book, not the sample), `selectionVerified()` (no save failure, no
unavailable or unconfirmed selection), and on a present card (no Home card
bits after `noteCard`). No card, a chosen sample, an unconfirmed selection or
no selection never touches the card. Each of these paints today's exact Home.

`paintHome` observes the selection first and has a single exit that returns the
borrowed selection (`returnHomeCoverSelection()`); no destructor joins the
selected `paint` frame. The art is
one `new(std::nothrow)` workspace of 3,528 bytes (a static assertion): the
3,385-byte `PreparedHomeCover` and the selection's own 140-byte
`SdResumeRecord` copy from `retain`. `validSdResumeRecord` checks it, and its
own NUL-terminated filename then names the file. A damaged record never reads.
The workspace is kept only while it holds ready art. It is deleted on failure,
on a key change and on leaving Home. In the Overlay build it is allocated after
the mask, so a tight heap fails the cover (the placeholder) before the Overlay.

Holding the art for the session keeps 3,528 bytes, plus allocator overhead,
on the heap while Home is shown. The #20 producer reported about 130 KB free on
Home and needs about 118 KB, which leaves about 126.5 KB with the art held
(figures from #20, not measured here). If the producer needs the art's memory,
it calls `releaseHomeCover()` on the UI task, never during a Home paint. The
`current` flag then makes Home's next ordinary paint a whole Home with one
read, never a battery pass drawn without the art the panel shows.

Card access goes through the card the runtime already mounted. There is no
new mount and no write. One descriptor opens the cover file, reads the header
and the payload unbuffered, and is closed before `prepareHomeCover` returns.
The snapshot is on the heap, never on the stack.

**What Home remembers of the book.** On the card each path is a FAT walk: a
stat of `Newt/Books/<name>` walks the books folder, and the open of
`Newt/Covers/<name>.nlc` walks the covers folder. `fstat` on the selection's
open descriptor gives the size but no mtime (ESP-IDF's FAT `fstat` leaves
`st_mtime` zero), so the mtime that binds a cover needs a stat of the book.
`HomeCoverRuntime.facts` (`CoverBookFacts`, `firmware/library_covers.h`) is what
Home keeps instead, across sessions: the identity record it belongs to, the
book's size and halved mtime once learned, and whether the book has no cover:
the card answered that the cover file does not exist (`ENOENT`), or the file is
the producer's no-art record for this version of the book.
`PreparedLibraryCovers::begin` fills them; the runtime skips the read while
"no cover" is remembered and resets them whenever the card or the book may
have changed. Home's read opens the cover file first. If the card says it does
not exist, Home remembers that and never stats the book; the next sessions
make no card access at all. Any other failed open (a card out or failing, no
descriptor or memory) is not remembered, and the next session tries again. If
the file opens at the exact art size or as a bare 28-byte header and the facts
are not yet learned, Home stats the book once and keeps its size and mtime;
later sessions compare headers with them and do not stat again. A bare header
that matches them and names the producer's current route is a no-art record:
Home remembers "no cover" as for a missing file, so the next sessions make no
card access. A bare header that does not match, names a CRC or another route,
or whose read fails is never remembered: the next session opens it again.

The facts last as long as the **book key**: the session key without
`current` (eligibility, `bookId()` and the generation), plus the identity
record the read used. `observeHomeCover()` forgets them when eligibility (a
pulled card, the sample, an unconfirmed selection), the book or the
generation (a publication, a card seen missing, a stale held file after a
swap) changes. `prepareSelectedHomeCover()`, which alone holds the record,
forgets them on another record for the same book, compared by the record's
32-bit checksum alone. A new session alone keeps them.

**Sleep is unchanged.** Sleep over Home frosts the existing Home, which has no
cover snapshot, so its backdrop keeps today's `No cover` placeholder. Sleep
never reads the card for a cover.

The binary focus island (native x432, y464, 240x16) is logical x48 to x63,
y432 to y671. That is disjoint from the cover slot at x332 to x479, y87 to y308.
The island writes only its region, so shown art is kept. The battery region
exists only in a regional Direct build (`NEWT_REGIONAL_HOME` with
`NEWT_HOME_OVERLAY=0`). That build is not one of the four measured variants.

## Resources

`HomeRefreshPolicy`'s unused clock cache shrinks from 128 to 64 bytes. The
firmware never sets a clock, and a longer clock always repaints, which a test
proves. The policy falls from 404 to 340 bytes, which pays for the 4-byte
generation and the 48-byte `HomeCoverRuntime` (with its 16-byte book facts).
The measured flash record, static RAM (16 bytes below ceilings that are kept
until the stacked layers are measured together), the frame guards and the
deepest cover stack chain are in `docs/CODE_HEALTH.md`, "Home cover cost".

## Performance

A Home session with a cover costs one open of the cover file (a walk through
the covers folder), one header read and one payload read. The book's stat (a
walk through the books folder) happens once per book key, only after its cover
file opened. A book with no cover file costs one failed open once per book key
and then no card access. A book with the producer's no-art record costs one
open, one stat of the book (if not yet learned) and one 28-byte header read
once per book key, and then no card access. Neither no-art figure is measured
on a device. Focus, battery, forced and fallback paints from every
route make zero reads. On the host (`-O2`), a per-pixel slot painter cost
about 57 µs per plane over plain Home; the byte painter, which skips the hidden
stamp and placeholder, costs nothing measurable. Neither figure is device
timing.

Device facts come from separate runs on the owner's card with regional and
default DEV USB builds; they are not host evidence. The remembered-facts build
and the present-cover build below are earlier measured builds of this series,
not commits of this branch: the fixes made after each were folded into the
commits beneath them before the final record.
Compare only within one measurement window: the card's write latency rose
after the cover producer first ran, and base Reader to Home on it went from
about 1,236 ms to about 1,287 ms.

**Missing cover** (a large books folder, no cover file for the selected book).
An earlier regional DEV USB build of this series with uncommitted timing
instrumentation stat'ed the book in every session. It measured that stat at
29.3 to 30.0 ms per Home entry, and the failed open of `Newt/Covers/<name>.nlc`
at 3.1 ms. The remembered-facts build overlaps base firmware on every route:

| Build | Route | Remembered-facts build | Base |
| --- | --- | --- | --- |
| Regional DEV USB | Reader to Home | 1,234.6 to 1,237.8 ms | `0184bbb`: 1,233.6 to 1,241.7 ms |
| Regional DEV USB | Library to Home | 976.7 to 981.8 ms | `0184bbb`: 976 to 983 ms |
| Default DEV USB | Reader to Home | 1,535.3 to 1,542.2 ms | `f008308b64a3`: 1,532.7 to 1,538.5 ms (same session) |
| Default DEV USB | Library to Home | 1,276.8 to 1,277.0 ms | `f008308b64a3`: 1,276.8 ms |

The regional focus island measured 541 to 587 ms, and book opening times were
unchanged.

**Present cover** (the selected book's cover on the owner's card, prepared by
a build of #20's cover producer with the owner's approval). The regional DEV
USB present-cover build against base `0184bbb` in the same window:

| Route | Present-cover build | Base `0184bbb` |
| --- | --- | --- |
| Reader to Home | 1,283.9 to 1,296.4 ms, paint 949 to 950 ms | 1,282.8 to 1,292.1 ms, paint 942 to 943 ms |
| Library to Home | 983.3 to 990.7 ms, paint 949 ms | 976.9 to 978.0 ms, paint 942 ms |
| Warm open | 2,138.8 to 2,146.8 ms | 2,138.6 to 2,146.2 ms |
| Restart open | 2,473.3 to 2,473.5 ms | 2,476.9 to 2,477.8 ms |
| Library open, paint | 918 to 919 ms | 913 to 914 ms |

Home paint, which includes reading and painting the cover, measured about 7 ms
above base. The remembered-facts build, measured earlier on the same card with
the cover present: Reader to Home 1,286.7 to 1,291.0 ms, Library to Home 983.7
to 983.9 ms, Library open paint 911 to 913 ms. The present-cover build's
Library open paint was 4 to 6 ms above base in the same window; the cause was
not isolated. Its Books entry read one byte of the selection's held book to
look for a swapped card, but that read runs in `openLibrary()`, before
`paint()` starts the timed interval (`refreshMs`), so the paint timing does not
include it. The final source removes the read on its merits: it added a card
read to every Books entry after reading, for decoration only. The
remembered-facts build never had it. The final source's Library open is not
measured.

The final source differs from the remembered-facts build in two ways: it
remembers only an `ENOENT` open, and the missing-card change moved from storage
to the selection, which reads nothing more. That none of this adds card access
to a Home entry on the missing-cover path is an inference, not a measurement:
the pinned ESP-IDF FAT layer maps a missing file or path (`FR_NO_FILE`,
`FR_NO_PATH`) to `ENOENT`, so a missing cover file is still remembered after
its first failed open.

## Evidence and limits

`tools/home-cover-preview.py` is review evidence outside the gate. It compiles
the actual shared painters with original authored illustrations and writes
untracked previews of Home beside Library (valid, missing and late-failing
art) in `.build/home-cover-presentation/`. No private book data. The gate
syntax-checks it against the current shared sources.

`tests/frosted_home_test.cpp` covers the painter, and
`tests/home_cover_runtime_test.cpp` covers the runtime through `paintHome`.
Both are linked into one binary for the Direct, Overlay and regional Direct
builds, with the display fake in `tests/fixtures/home_paint_fake.h`. The runtime
test starts every scenario in a fresh Home session. It proves one read per
session after the mask, the fallbacks and failures, publication, an external
release and the exits. It also follows five routes into Home (boot, Reader,
Library, wake and the battery tick), each with a focus move that must not read.
Its fake models the remembered book facts in every build: a missing cover file
is opened once per book key, a found cover's book is stat'ed once per book
key, and a card pulled and back, another book or record and a generation
change each forget them.

`tests/home_cover_test.cpp` drives the real `PreparedLibraryCovers` over
heterogeneous authored caches and every failure kind in a project-local
temporary card. It also checks Home/Library pixel identity, at the art origins
the painters themselves produce. It counts the adapter's stats of the book:
none for a missing cover file and none in later sessions, one after a cover
opens, again after a card is pulled and back, and a stale header is rejected
against the learned size and mtime. One table states each fault's first and
later session; an open failing with `EIO` is not remembered and the next
session shows the cover. The table includes the producer's no-art record,
remembered after one open, stat and header read, and bare headers that are not
one (a torn cover, another route, another version's record at another size or
at the same size with another mtime, a damaged magic, a failed header read),
which are opened again in every session. A further
scenario selects a coverless book with a no-art record: later sessions make no
card access, another selected book with art shows, and art published over the
record shows after the hook. `tests/library_cover_test.cpp` checks the same
remembered facts on the adapter alone, that Library rejects a no-art record
unread, and that Library still stats on every read. The lifecycle test checks
that storage never changes the generation, that a card seen missing does, and
that Books neither reads a held file nor changes it, live or stale; the
reliability test checks Read's stale handle.
`tests/home_cover_test.cpp` runs only in the non-regional Direct host build.
Overlay's file-backed path is covered through the authored fixture source in
the runtime test. The `home` golden and every reviewed frame are unchanged.

The runtime opening probe (`tests/runtime_opening_recovery_test.py`) runs
`main.cpp`'s own Home dispatch, reader entry and paint releases: Home's Read
ends the cover session before the actual `openReader`, and another screen's
paint and a cold opening paint each end a session they find. After
`leaveHome`, no identifier in `paint()` (comments and literals are not code)
may name a Home paint, Home's cover or its state; structural controls add
`paintFrostedHome`, `paintOverlayHome`, `readHomeCoverOnce` and a
`homeCoverRuntime` write there and require rejection, and require line and
block comments and a string naming `paintHome`, and a reworded comment on the
release line, to pass. Its
negative control replaces `dispatchLeavingHome` with `newt::dispatch` and must
fail.

Host tests and native builds are separate facts, and neither is hardware
evidence. The device timings under Performance come from separate runs on the
owner's card; this document records no panel image of the art. No limits,
exclusions or adapters are relaxed.

The host evidence used no hardware, USB, SD card, private book, publication,
push or merge.
