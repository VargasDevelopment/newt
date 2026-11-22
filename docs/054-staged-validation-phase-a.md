# Staged book validation, phase A

Status: implementation of the approved Phase A of the staged-validation
proposal (`docs/052-staged-book-validation.md`, on branch
`proposal/staged-validation`, not in this tree). Phase B (deferred identity)
stays a proposal. The full-file SHA-256 identity, the per-4 KiB-block receipt
and block verification are unchanged. Host evidence is in "Test evidence";
one-device X3 evidence is in "Build and device evidence".

## What changed

A cold open used to inflate every ZIP member, including images, CSS and fonts
the reader never uses, before the first page. On the X3 that was 30–55% of a
cold open, and books with many members ran past the 20 s open budget.

Admission is now split:

- **Stage 1** (blocking, before the first page) keeps every structural check
  and the declared-size limits. It inflates only the members the reader needs
  to open the book: `encryption.xml`, `container.xml`, the OPF, and the nav or
  NCX. Each is checked before it is parsed.
- **Stage 2** (at use) inflates every other chapter only when it is first
  needed, through the existing capped, exact-length and block-verified path.
- **A chapter that fails at use** gets its own error page. The book stays
  open, the place is kept, and Next or Previous moves past it.

Images, CSS and fonts are never inflated. The reader does not use them; layout
runs with a null ZIP catalog.

## Stage 1: before the first page

`admitEpub` (`src/epub_admission.cpp`) does the following, in order:

1. **EOCD, central directory and every local header.** These checks are
   unchanged:
   - EOCD placement, no ZIP64, single disk, consistent counts;
   - entry and directory limits;
   - central and local header agreement: signature, flags, method, name
     bytes;
   - encryption flags; stored and deflate only;
   - data must not overlap the directory;
   - per-member (4 MiB) and cumulative (64 MiB) **declared** size limits.

   No member payload is read in this step.
2. **Unique names.** This is a directory-only check, run before any lookup.
3. **Required members**, each expanded before its first parse:
   `encryption.xml` (with its policy), `container.xml`, the OPF and any nav or
   NCX member. "Expanded" means the following checks:
   - requests never go past the declared size plus one probe byte;
   - output beyond the declared size fails;
   - a short or corrupt stream fails;
   - compressed input must be within the bound described below.

   A malformed nav still falls back to generic labels, as before. A damaged
   nav **stream** still rejects the book, because every selection parses the
   nav.
4. **Every spine item** must resolve to a directory member and pass the
   selection's href policy: bounded UTF-8 and a safe path. This is a lookup,
   not an inflate.

The chapter at the resume position (chapter 0 for a new book) is then
inflated and laid out as part of the same open, as before. If it fails, the
open fails.

The receipt is still sealed only after strict admission (`finishImport`),
which is now Stage 1. The strict fallbacks (`strictOpen`, `finishPreparedOpen`
and the receipt-unavailable path) and Home overview preparation all run
Stage 1. None of them repeats an all-member pass.

## Stage 2: at use

Every path that inflates a member that Stage 1 did not admit goes through
`ChapterSource`. That covers:

- layout (`PhasedReader::layout`);
- the semantic index (`buildSemanticIndex`);
- the page-cache writer (`ChapterPageCache::streamChapter`);
- warm and strict sessions alike.

I checked each path. The use-time checks:

- **Output cap.** The parser sees EOF at the declared size and can never
  receive more.
- **Exact length.** `finish()` requires both the declared size and inflater
  EOF.
- **Block verification.** On a receipt source, every consumed byte belongs to
  a block hashed against the receipt. Without a receipt, the strict path keeps
  its full-SHA and live-source guards.
- **Input bound (new).** `inflateInputBounded` limits a deflated member to
  `declared + declared/8 + 64 KiB` of compressed input. zlib's worst case is
  far below that. `ChapterSource::open` fails with `Unsupported` before reading
  anything else, and Stage 1 applies the same bound to required members. The
  SDK reader never reads past the compressed size, which Stage 1 keeps inside
  the data region.
- **No loops.** Each SDK inflate step consumes or produces bytes, or fails.
  The host tests show that damaged, unterminated, over-long and short streams
  all end in a bounded failure.

## Failure handling at use

A chapter failure is treated as **chapter-local** when the status is `IoError`,
`Truncated`, `ParseError` or `Unsupported`, the book still has its last good
page, and a no-payload probe of the source passes. The probe covers the
receipt's validity, the live file and cancellation.

In that case (`SampleReader`, `src/sample_reader.cpp`):

- **The reader stays "not opened", as for any failure today.** No progress is
  tracked and the last good page and checkpoint stay authoritative. Nothing is
  ever saved inside the failed chapter.
- **The page names the problem.** The renderer draws a chapter page in place of
  the generic "This book could not open" text, for example: "This chapter
  can't be read (damaged data). Your place is kept. Press Next or Previous for
  another chapter, or Back for Home." The other variants are incomplete data,
  invalid text, malformed content, and too large or unsupported.
- **Navigation continues.** Next and Previous move to the adjacent chapter,
  and on success the place moves to that page. Back returns Home and Sleep
  works as before. A jump from the contents list to a failed chapter shows the
  same page.
- **Failures are remembered** for the open session (up to 8 chapters). A
  revisit shows the page without reading or inflating anything; reopening the
  book clears the list.
- **The book is not marked bad**, and a pure inflate or parse failure does not
  invalidate the receipt. A block-hash mismatch still fails `ImportedRevision`
  and invalidates it as before. Its latched source probe then makes the
  failure book-level, as do cancellation, source loss and out-of-memory.

`reader_chrome.cpp` and every progress or percentage path are untouched. The
chapter page is drawn over the unavailable chrome by `renderChapterFailure`,
which `renderReaderUi` and `renderSampleScene` call. The footer shows `--`, as
it does for any unavailable page.

## What moved from open time to reading time

| Failure | Before | Now |
| --- | --- | --- |
| Corrupt deflate in a non-resume chapter | Book rejected at open (`IoError`) | That chapter's error page |
| Chapter inflating longer or shorter than declared | Book rejected | That chapter's error page |
| Chapter compressed input beyond the new bound | Accepted | That chapter's error page (`Unsupported`) |
| Corrupt image, CSS or font stream | Book rejected | **Not checked while unused** |
| Malformed XHTML in a later chapter | Fails at use (book unavailable) | That chapter's error page |
| Corrupt resume chapter | Rejected at open | Rejected at open (first page); receipt already sealed |
| Member declaring > 4 MiB, cumulative > 64 MiB | Rejected | **Unchanged** |
| EOCD, directory, local headers, duplicates, overlap, encryption, zip64 | Rejected | **Unchanged** |
| Damaged container, OPF or nav stream | Rejected | **Unchanged** |
| Spine item missing from the directory, or unsafe spine href | Failed at use | **Now rejected at open** |
| File changed after import | Detected at use by block hash | Unchanged |

## Tradeoffs, explicitly

1. **A book with a damaged later chapter now opens.** You find the damage when
   you reach that chapter. No damaged byte is displayed: everything shown is
   capped, exact-length checked and, with a receipt, block-verified.
2. **Corruption in unused images, fonts or CSS is no longer detected.** The
   receipt names the revision imported by the full hash. Unconsumed blocks
   could already change undetected until consumed (`imported_revision.h`), and
   unused members are now never consumed.
3. **A book with a damaged resume chapter gets a sealed receipt** before the
   first page fails. The next open is warm and fails the same way, without a
   rehash.
4. **No time budget for chapter crossings.** Adding one needs `main.cpp`, which
   this change does not touch. The new input bound limits the worst case for
   one member to about 4.5 MiB read plus 4 MiB inflated. Before, such a member
   was rejected inside the 20 s open budget.
5. **Without a receipt, a transient SD read error during a crossing** looks
   like a damaged chapter. `FileBookSource` does not latch errors, so the
   chapter is shown as failed until the book is reopened. With a receipt,
   read errors still latch, and the failure stays book-level as before.
6. **Stricter at open:** a missing spine item or an unsafe spine href now
   rejects the book at open instead of failing that chapter at use.
7. **Heap:** the per-open reader allocation grows by 80 bytes, for the failure
   list. Static RAM and `sizeof(SampleReader)` are unchanged.
8. **Whole-book percent for a book with a damaged chapter stays unknown.** The
   exact percent needs a stored semantic index. Building one
   (`buildSemanticIndex`) reads every chapter and would fail at the damaged
   one. Since `17bd3e8`, `loop()` never calls `prepareHomeOverview`, so no
   index is built for any book. Only a table already stored on the card gives
   an exact percent; otherwise Home shows "Progress unavailable", damaged or
   not. Before Phase A, such a book did not open at all.
9. **The 4 MiB member limit still covers members that are never inflated.**
   Stage 1 checks every central-directory entry's declared size, so a book
   with one large image is refused even though Phase A would never inflate
   that image. One book on the owner's card is refused for this reason (see
   "Books that still do not open"). Relaxing the limit for unused members is
   left for a separate, reviewed change.

## Test evidence (host only)

Host runs (`tools/check-phased-reader.py` and the suites listed below) cover:

- **Stage 1 outcomes** (`tests/epub_admission_test.cpp`, with fixtures built by
  `staged_fixtures()` in the checker):
  - A damaged later chapter, a damaged resume chapter, a damaged image and
    font, an over-long chapter and a chapter over the input bound all pass
    admission. Each fails later at use.
  - An image declaring more than 4 MiB is rejected (`Unsupported`).
  - Rejected at open: damaged OPF or nav (`IoError`), a container over the
    input bound, duplicate names, two directory records over one local
    header, data overlapping the directory, a truncated EOCD (`NotZip`), a
    truncated directory (`Truncated`) and an unsafe spine href.
- **Byte ranges.**
  - Recorded admission reads touch no byte of any unused member outside the
    fixed 4 KiB EOCD tail scan: 12,021 bytes read from a 153,216-byte book,
    with 151,289 bytes of payload untouched.
  - At use, chapter input stays inside its member, at or below the compressed
    size. The output delivered to the parser never exceeds the declared size.
  - Over-long, short, corrupt and unterminated streams end in bounded
    failures.
- **Reader behavior** (`tests/staged_reader_test.cpp`, actual `SampleReader`,
  UI and renderer):
  - A damaged later chapter shows its page with the exact rendered text. The
    place and checkpoint are kept.
  - Next reads the following chapter, and Previous returns to the exact last
    page and pixels.
  - A revisit or contents jump performs zero source reads and zero layouts.
  - Back works. A lost source is still a book-level failure.
  - A damaged first chapter fails the open with no progress or frame change,
    while another resume point opens.
  - Damaged unused members produce pixel-identical pages for the whole book.
- **Receipt path** (`tests/reader_revision_staged_checks.h`):
  - One import pass, then Stage 1.
  - The damaged chapter fails without publishing a page or moving the place.
  - The receipt stays valid: the warm reopen has no admission and no
    whole-file hash, and shows the same pixels.
- **Updated expectations** that pinned the all-member policy:
  - `reader_revision_session_test.cpp` (wording);
  - `imported_revision_test.cpp`: no image is inflated. The consumed-block
    mutation now targets the directory, and the unused-block mutation is
    deferred until first consumption;
  - `reader_open_phase_proof_*`;
  - `phased_reader_runtime_test.cpp`: missing spine;
  - `failed_chapter_sleep_test.py`: kind 3 is now a damaged deflate stream,
    and chapter failures are named;
  - `epub_admission_test.cpp`;
  - `chapter_transition_test.cpp` and `imported_revision_boot_test.cpp`: a
    missing chapter is rejected at open, before any page;
  - `zip_directory_window_test.cpp`: the authored ZIP carries a minimal
    stored package, and the uncached control uses a 4,095-byte arena (one byte
    short of the window) so the package parse fits;
  - `tools/reader-admission-profile.py`: only container.xml, the OPF and the
    nav are expanded. Beyond its local header, another member's compressed data
    is read only within FreeInk's last-4 KiB end-of-central-directory scan, and
    a damaged unused stream is never inflated.

Measured byte counts on the same deterministic fixtures, base `567e737`
against this change:

| Fixture | Before | After |
| --- | --- | --- |
| Strict reopen, 3,147,948 B (2 MiB + 1 MiB images): admission source bytes | 3,157,083 | 10,684 |
| Same: members expanded, expanded bytes | 10, 3,169,960 | 2, 726 |
| Receipt import, 1,067,601 B (1 MiB unused image): payload bytes | 2,209,847 | 1,146,342 |
| Native cold open, 1,050,889 B: Stage 1 bytes after the import pass | — | 107,314 |

The last row reads more than the strict case because, with no lent action
storage, each small receipt read authenticates a whole 4 KiB block (about 26
blocks). Host timing was not measured; it says nothing about the X3.

## Build and device evidence

**Builds.** Four clean `--health-report` builds; see "Round 2 reader and Home
cost" in `docs/CODE_HEALTH.md`.

- The bodies of `admitEpub`, `PhasedReader::open`, `select` and `layout` are
  unchanged. `CanonicalBook::open` gained an optional admission pointer,
  defaulted to null at every existing call site. The inlined `copyUtf8` now
  delegates to `boundedUtf8`, with the same logic.
- The selected `verifyStreams` key (`constprop`, by parameters) still resolves
  and falls from 448 to 64 bytes. Its body moved out of line:
  - `inspect` (400 bytes);
  - `RequiredMembers::admit` (96 bytes);
  - `expandMember` (160 bytes).

  All three are guarded at their measured sizes.

**Cold opens (owner's X3, regional dev-usb, 2026-10-05).** Each book was
opened from the Library after a USB-reset boot and the dev
`PRESS purge-cache` (commit `ea019be`). One sample each. Times are the device's
action total. "Before" is `567e737` plus the dev purge only (`c1c184c`, tag
`evidence/cold-before-c1c184c`, tree-identical to `ea019be`). "After" is the
round-2 stack at `54cf1bc` (tag `evidence/round2-54cf1bc`), which adds the
opening screen's fast pass to each open.

| Book | Before | After | Members expanded, before → after |
| --- | --- | --- | --- |
| 0 | 10.6 s | 6.2 s | 96 → 3 |
| 6 | cancelled at the 20 s budget | 18.8 s | 3 |
| 12 | 5.2 s | 4.2 s | 59 → 3 |
| 18 | no result (dev-USB protocol timeout) | 3.0 s | 3 |
| 24 | 21.0 s | 10.9 s | 213 → 3 |
| 36 | 17.7 s | 8.9 s | 128 → 3 |
| 42 | 20.6 s | 10.4 s | 160 → 3 |
| 48 | 3.5 s | 3.2 s | 80 → 3 |
| 54 | cancelled at the 20 s budget | 12.1 s | 3 |
| 60 | 9.1 s | 5.9 s | 100 → 3 |

Every book opened after Phase A. The three members are the container, the OPF
and the nav or NCX; images, CSS and fonts were never inflated. Book 6 is still
close to the budget. The open budget now excludes the opening screen
(`1d5e298`).

**Final image.** This is the `1d5e298` build whose hashes the flash record
lists. Each book ran in one session after a USB-reset boot and a purge:

- **Cold opens:** 11.1 s for book 24 and 18.8 s for book 6. The opening
  screen is now outside the budget.
- **Reader-task stack (16,384 B):** the minimum free was 7,084 B (book 24) and
  7,068 B (book 6) after the cold open. The lowest over book 24's whole
  session was 7,052 B. That session included 10 turns, three warm reopens and
  the Library.
- **Warm reopens:** 1.23 s for book 6 and 1.74 s for book 24, three each.
  Book 24 reopened at spine 9, so its time is not comparable with the 1.35 s
  that `54cf1bc` measured at spine 7.
- **Turns:** 629–1,009 ms (book 6) and 656–1,161 ms (book 24), ten each.

The raw data stays outside the tree (numbers only, no titles or text).

Book numbers above are Library positions before the damaged-chapter fixture
below was added. The fixture sorts at position 25, so every later book moved
down by one: book 54 above is book 55 below.

**Damaged chapter (owner's X3, final image, 2026-10-05).** The owner supplied
a synthetic two-chapter EPUB (3,931 bytes, SHA-256 `c5538199…`): chapter 1 is
valid and chapter 2 is deliberately damaged. After a verified card backup, it
was copied under its own name; no book was overwritten.

- **First import** from the Library: 2.3 s. Chapter 1 has five pages.
- **Crossing** from page 5 into chapter 2: 762 ms to the chapter's error page.
  It reads "This chapter can't be read (damaged data). Your place is kept.
  Press Next or Previous for another chapter, or Back for Home." The footer
  shows `Section --` and `--/--`. The reader reports not opened, and the
  place stays at spine 0, character 2,203.
- **Next on the error page:** no visual change, answered in 32 ms. Chapter 2
  is the last chapter, so this fixture cannot show Next reaching a later good
  chapter.
- **Previous:** 690 ms back to chapter 1 page 5, pixel-identical to the frame
  before the crossing.
- **Back from the error page, then Read:** the same page, pixel-identical,
  both in the same boot and after a USB-reset boot.
- **Sleep from the error page, then a physical wake** (owner's hands, same
  evening). The error page before sleep was pixel-identical to the first
  capture. The owner woke the X3 with the power button about 10 s later and
  saw a normal page of text, not the error page. After the reconnect's
  USB-reset boot, Read reopened chapter 1 page 5, pixel-identical to the last
  good page. The owner took no photo, so which page the wake itself showed
  is the owner's report. Wake time was not measured.
- **Not covered:** a damaged chapter 1 (the damaged-resume refusal), a real
  power cut and repeated samples.

**Final-image timing (2026-10-05).** One session per book. Neither book had a
receipt since the round-2 purge, so nothing was purged. Times are the device's
action total, with one first import and three of each reopen per book:

| Book | Size | First import | Cold reopen | Warm reopen | Turns |
| --- | --- | --- | --- | --- | --- |
| 7 | 482 KB | 3.0 s | 1.29 s | 0.97 s | 602–965 ms |
| 55 | 7.9 MB | 11.9 s | 1.65 s | 1.30 s | 633–987 ms |

A cold reopen is Read after a USB-reset boot, with the receipt present. A warm
reopen is Back, then Read, in the same boot. Each book had eight turns,
including chapter crossings.

**Books that still do not open.** Two Library books fail, both for reasons
that predate Phase A:

- **Book 30 (31 MB).** The first import hashes the whole file before admission
  (`ImportedRevision::beginImport`), at about 0.94 MB/s. The 20 s budget
  cancelled it after 18.8 MB. A failed or cancelled import keeps no partial
  progress, so a book above about 18 MB cannot be imported on this device.
  Main has the same budget and the same whole-file import. That comes from
  reading main's code; main was not run on the device.
- **Book 31 (15.7 MB).** After the 17 s import, Stage 1 returned
  `Unsupported`. Two PNG members expand to more than `kEpubMaxMemberBytes`
  (4 MiB). Phase A never inflates images, but the central-directory pass
  still applies the per-member limit to every member. A build from before
  Phase A (2026-10-04) failed the same way, with status 6 after about 17 s.
  A scan of the card backup found no other of the owner's 61 books over
  these limits.
