# 089: Instant resume picture (#199)

**Status:** implemented (#199), flash record 96 and protected update 49
(reviewed). Host tests and
controls pass; the bench numbers are under "Measurements". Owner checks still
pending: a battery wake on the main X3, and that the picture and the first
turn look right.

## What it does

When the reader leaves a page, Newt saves that page's exact panel image to the
card. At the next wake or Home → Read, if nothing that decides those pixels
has changed, the panel shows the saved page first, with no "Loading" screen.
The reader is rebuilt behind it. When the reader is ready, its page is
compared with the picture, and the panel is painted again only if they differ.

The picture is a cache only. It never decides the reading position: the
saved progress does. Any doubt takes the ordinary open.

**Invariant:** the picture never shows a page that the ordinary open would
not show before it verifies the book.

## Binding

One slot, `/Newt/Cache/resume.pic`. It is shown only when all of these match:

- the running firmware image (its ELF SHA-256: fonts, layout and renderer);
- the book's content SHA-256 (from the selection, so no book is special);
- the saved anchor (spine and character);
- the reader settings (orientation, margins, refresh and every other view
  setting).

The anchor and settings are the open's own: the saved progress at Home →
Read, or the pending recovery checkpoint and its settings at a wake, a
power-on resume or after an unconfirmed save. The open still runs every
recovery check; a checkpoint that cannot open names no picture and takes the
ordinary path.

The footer is not bound. It is redrawn from today's values (battery, page
label) before the first byte reaches the panel. Nothing else on the page
changes after a save.

## Card format

A 512-byte header followed by the 52,272-byte logical frame. The header holds
a magic and version, the binding, the footer values, the frame CRC-32 and its
own CRC-32. All fields are explicit little-endian. A file is read only if its
size is exact and both CRCs match.

A write goes to `resume.pic.tmp`, syncs it, and then renames it over the slot
(on FAT, it removes the old slot first). A power cut therefore leaves the old
picture, no picture, or the new one, never a torn one. Leaving the same page
again writes nothing.

## When it runs

- **Save:** in the reader exit, after the checkpoint is saved and the stats
  fold. On Sleep, the sleep image is already on the panel; on Back, the
  page (or, while the book check is pending, the Saving card) is still
  shown ([093](093-back-paints-home-first.md)). The page is rendered into the logical frame
  with no reader overlays.
- **Show:** in `openReader`, the header is read and checked first. The press
  becomes `Deferred` and the working frame is held back only on the fast
  path: the book file's size and time still match the record of its last
  verified open. They are read by name, so a wake asks before the reopen:
  Sleep has closed the book, and alpha.8 read them only from an open file,
  so every wake drew "Opening…" first (#222). Otherwise the open hashes or imports the book first, so
  "Opening…" answers before that work starts. Once the source and saved page
  are settled, the pixels are read and verified, the gate below is checked,
  and the picture is shown. The slot is read and closed before the gate,
  because the gate's cache query keeps its page open and the card allows
  three open files. If anything fails there, the held-back working frame answers the
  press. A deferred press still takes any other working frame (a reimport,
  for example), as an accepted one does. A deferred press never waits on
  slow work: if the open still hashes or imports the book (a lost or corrupt
  receipt behind matching hints), the session calls `beforeSlowSourceWork`
  first, and the held-back working frame answers there.
- **Gate:** the picture shows when the book is verified, or when its
  verification is pending and the page cache holds the saved chapter under
  the same digest and settings. That is the ordinary open's own rule: a
  pending book shows a cached chapter before verifying and hashes first
  otherwise. Both ask the same cache file names
  (`ChapterPageCache::cached`/`stored`) through one query,
  `ChapterPageCache::presentIn`, inside the opening's read scope. A page
  file has no byte cap there (NEWT-87): the 128 KiB opening cap bounds only a
  full-CRC record, so a whole-chapter file over 128 KiB is cached for both.
  Before, it was uncached for both: the open hashed and laid out the chapter,
  and the picture was skipped. A test pins the two rules together. In DEV
  builds a skip prints `Reader picture: result=unverified`.
- **Ready:** the first reader paint checks the picture's mark: the panel
  generation after the picture and the shown frame's CRC. Same generation and
  pixels: no panel pass. Otherwise the ordinary pass draws the page at once.
  In DEV builds, a differing page that is still the picture's page (the
  slot's header says which) prints one `Reader picture miss` row; a page
  reached by an early turn is not a miss.

## Early presses

A turn pressed during the open is queued and runs as soon as the open
finishes. It is never dropped.

**Limit:** "Loading page…" cannot be shown *during* the open. Both 52 KB
frames are lent to the open (action storage, staging and the XML arena), so
nothing can be painted until it ends. Once the open ends, the queued turn
takes the ordinary path, which shows "Loading page…" past the slow-action
threshold. No toast was added.

## Memory

No retained RAM, and nothing held while reading. The save reuses the logical
frame after the reader has finished with it, and the show reads into the same
frame. The mark is 8 B of `.bss`, which fits existing padding. The slow-work
hook (`beforeSlowSourceWork`) is one 4 B function pointer in `.bss`; with
alignment, resident SRAM grows 16 B (protected update 49). The pending state is a `PressState` value, not a new flag. RTC is
unchanged. Showing the picture uses its own panel call, not
`displayBinaryFrame`, so the reader's paint frame keeps its inlining and its
96 B stack frame.

## Measurements

Bench X3, DEV USB power. "Before" is 0.1.0-alpha.6 (`ab73fc22`); "after" is
the #199 candidate `a9f20015` (the review's fixes, on main `f5dc28c6`). Times
are from the press. Row 38 is a library book with a long, segment-served
chapter; it is timed only.

| Home → Read | Before: first frame | Before: ready | After: picture | After: ready |
|---|---|---|---|---|
| Ordinary book | "Opening…" | 1.95–1.96 s | 1.01–1.03 s | 1.60 s |
| Row 38, long chapter | "Opening…" | 2.57–2.58 s | 1.28 s | 1.83–1.86 s |

The first candidate (`659316b4`) showed the picture at 0.85 s and 0.98–0.99 s.
The gate now reads the slot, then asks the page cache, before the panel
pass; on row 38 that query finds a segment map. After the picture, the
panel ring shows no second pass when the reader is ready, and no miss row
was printed. No `unverified` row was printed in these five opens; both
chapters were cached. When the slot holds another page, the candidate takes
the ordinary open.

| Save at Back | Cost |
|---|---|
| New page | 225–233 ms, 52,784 B |
| Same page again | 7–9 ms, 0 B |

An earlier run on the pre-rebase candidate measured one new-page save on
row 38 at 427 ms. On Sleep, the save runs after the sleep image is on the
panel; the bench cannot sleep, so it was not timed there.

| Memory | Before | After |
|---|---|---|
| Open heap low, ordinary book | 63,120 B | 63,120 B |
| Open heap low, row 38 | 63,492 B | 63,492 B |
| Loop stack low, turns within a chapter | 6,972 B | 6,988 B |
| Loop stack low, a turn into a new chapter | 6,412 B | 6,412 B |
| Loop stack low, boot (first candidate) | 12,604 B | 12,524 B |

The heap floor is 50,468 B. No existing static stack frame changed. The
overall loop stack low is the chapter-crossing turn, the same before and
after.

An early turn cannot be injected over DEV USB (the bridge holds new commands
while a press is pending), so the held press is covered by the host test and
an owner check.

## Tests

- `tests/resume_picture_test.py`: the codec, header damage, binding, torn,
  truncated and unsynced writes, power cuts around publish, skip-same and the
  mark, all on real files. Ten controls.
- `tests/resume_picture_runtime_test.py`: the real `openReader`, paint and
  exit save over four authored EPUBs. It covers a cold boot per layout in its
  own process, every fallback (missing, corrupt, torn, layout, firmware and
  another book), a live battery, an early turn, a miss and skip-same. It also
  covers the gate (pending and uncached, failed, pending and cached), the
  fast-path hold, and setup's actual restore and boot open for a retained
  wake, a power-on resume with no RTC record, and a bad checkpoint, each in a
  new process, and a lost receipt answered with "Opening…" before its import.
  Thirteen controls.
- `tests/deferred_reader_runtime_test.py`: on a real pending session, the
  picture's gate equals the ordinary open's scoped cache rule for two chapters
  and two layouts, and both find a whole-chapter file over 128 KiB. A lost
  receipt behind matching hints calls the slow-work hook before it reads the
  source; with its receipt, nothing. Three controls (no cache rule, a gate
  that keeps the old 128 KiB cap, no hook call).
- `tests/chapter_cache_producer_io_test.py`: real produced caches of plain,
  dense and styled chapters, under 128 KiB, over it and segmented, in both
  orientations. A pending-source cold wake deep in the chapter passes the
  picture's gate before any book hash, then opens with no layout while the
  source stays pending; so does a cold verified reopen. Both show the ordinary
  open's page, and a damaged block parses to it. One control (the cap
  restored) fails the over-cap cases and the #192 segment reopen.
- `tests/resume_picture_wake_test.py`: the real sequence on the deferred
  runtime fixture: a Library open, turns, an idle save of the same page or
  none, Sleep's exit and sleep entry, then a cold power-on with all RTC and
  RAM state lost and a retained wake, through setup's restore and boot open,
  in three layouts. Each wake shows the picture, equal to the ready page,
  with no working frame. Seams: the card, the panel, and the producer's
  output (the saved chapter's page cache, seeded). Two controls: a card that
  gives hints only while the book is open (alpha.8) draws "Opening…" at the
  cold and at the retained wake.
- `tests/sd_library_lifecycle_test.py`: the real `SdLibrary` and selection
  match a closed, unchanged book's hints and refuse a changed one. One
  control (alpha.8's open-file guard).
