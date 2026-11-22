# Books in place (NEWT-76, NEWT-80)

The Library lists every EPUB on the card, in whatever folder another firmware
or its owner put it. `/Newt/Books` keeps working exactly as before and is
optional. Since NEWT-80 it lists every book, not the first 128: the list
lives on the card (see The list), and RAM holds only one shelf.

## Names

A book's library name is one string of at most 95 bytes, the same field every
record already stores (`kLibraryFilenameBytes`):

- a book directly in `/Newt/Books` is its bare filename (`Dune.epub`), exactly
  as before;
- any other book is its card name: its card path with ':' for each '/'
  (`:Books:Dune.epub`, `:Newt:Books:Series:One.epub`). FAT names cannot hold
  ':', so the walk counts any name or folder with one as a name it cannot list.

No name holds a '/', so `validLibraryBookName` is unchanged and every older
Newt accepts every name this build stores (see Rollback). A card name never
names `/Newt` except a folder below `/Newt/Books`, and has no empty or dot
folder, so one book has exactly one name; the catalog lists only such names.
`libraryBookPath` turns a name back into its file, and `libraryNameFile` gives
the flat file of a name's own data: the bare name itself, or for a card name
`.<FNV-1a 64 hex>.epub`, which is hidden, so no bare name can equal it.

**Nothing on the card or in RTC changes format.** The selected-book record
(`newt_selected`, a 96-byte name field), the RTC resume record
(`SdResumeRecord`, `filename[96]`), the reader cache route key and receipts
(SHA-256 of the name) and covers (`/Newt/Covers/<name>.nlc`) all key on the
name. For a book in `/Newt/Books` the name is the same bytes as before, so
every existing record, receipt, cache and cover still finds it
(`tests/library_directory_test.cpp`, case `identity`). Progress, statistics
and book days key on the book's SHA-256 digest, not its name.

Moving a book between folders gives it a new name:

- progress, statistics and finished state follow it (digest);
- its chapter caches, receipt and cover are made again at the new name, the
  first time it is opened or shown; chapter caches stay within the existing
  cache quota, and the old cover file stays on the card unused;
- Home's Read of the old selection says "Saved SD book unavailable or
  changed. Position kept." until the book is opened from the Library again,
  which restores its position by digest.

## Rollback

Whatever this build stores, an older Newt loads and saves beside it. Its
validator accepts a card name, so the selection slots and the RTC record stay
valid and its saves keep working. It looks for a card name inside
`/Newt/Books`, which FAT cannot hold, so Home's Read of that selection says
the book is unavailable; the book's progress, keyed by digest, is kept, and
any `/Newt/Books` book opens and saves as before. Losing an outside book's
selection after a rollback is accepted; losing saves is not.

`tests/library_rollback_test.py` proves it in three processes, so nothing
survives in RAM between them: this build lists a card and stores a book
outside `/Newt/Books` in both selection slots and RTC; alpha.13's store and
validator (history fixtures) boot from those records, load them and save a
`/Newt/Books` selection; this build loads that and saves again. Its negative
control restores '/' card names, the reviewed first design, and alpha.13
then fails to load (and so to save), as the review reproduced.

## The card walk

`scanLibraryCard` (firmware/library_directory.cpp) walks the card depth
first, reading each folder once:

- `/Newt/Books` first, so its books are always listed and its copies are the
  ones kept, then the rest of the card;
- skipped: names starting with '.', folders the card marks hidden or system
  (FAT attributes, device only: `System Volume Information`, `.Trashes`,
  `.crosspoint`, `.papyrix` and similar data folders), and Newt's own folders
  other than `Books`;
- on the device the FatFs directory entry gives each file's size, so no book
  is stat'ed or opened (page_entry_fatfs.h explains why a stat by path is
  slow); the host build uses POSIX `readdir`/`stat`.

Bounds, each counted when reached:

| Limit | Bound | What the Library says |
| --- | --- | --- |
| Entries read | 32,768 per scan | The card has too many files to list all. |
| Books listed | every book, while the card holds the list; else 128 (`/Newt/Books` first, then Library order) | Not every book is shown; the card may be full. |
| Name | 95 bytes, valid UTF-8, no control characters, no ':' | Some books have names Newt can't show. |
| Folder depth | 6 below the root | Some folders are too deep or unreadable. |
| Folder path | no name could fit below it | (same) |
| Unreadable folder | open or read fails | (same) |

The first that applies is shown as one line under the shelf for the whole
visit; the focus-only repaint keeps it. Only a failure to read the card's
root makes the Library unavailable ("The card could not be read.").

Library order is by filename, case-insensitive, then by the whole name, so a
book sorts the same wherever it lives. Two different books with the same
filename in different folders both show, with the same title.

## Identical copies

A file is hidden as a copy only on proof that its content is identical to a
listed book's. The proof is the full SHA-256 Newt already keeps per book:
each verified open records the file's size, modified time and full digest
beside its reader cache (`ReaderSourceHints`, the per-book cache identity).
Two books of equal size are one when both have such a record, each file
still has the recorded size and time, and the two digests are equal
(`verifiedSourceDigest`, `SdLibrary::sameBook`). Anything else (a book never opened in Newt, changed
since its last open, or a record that cannot be read) is not proven, and
both books show.

The copy kept is the one in `/Newt/Books`, else the first in Library order. A
copy is recognised as it is found. Every book is compared with the scan's
table: the first 128 books found, `/Newt/Books` first. Two books both found
after the table are not compared with each other, so both show: a copy is
never hidden without proof, and the table bounds the work.

A copy shows until both files have been opened in Newt once. A full compare
or SHA-256 during the scan was rejected: hashing takes about 6 s per book on
the device (docs/044: source hash 5,985 ms), and a full compare reads both
files, on every Library visit. The ZIP-tail check of the first design was
dropped: a different or damaged book can share its size and ZIP tail (the
review built both cases).

**Residual risk.** The proof trusts a file's size and modified time, as the
reader's deferred verification already does: a file rewritten with the same
size and time keeps its old digest until it is next opened, which verifies
it again.

**Cost.** Size comes free with the directory entry; only equal-size pairs
are proven. A pair costs two name hashes, two stats and two 72-byte record
reads, with no new record type. One walk asks at most 128 proofs, failed
ones included; past that, both books stay listed. A card with no equal-size
pair does no extra work.

## The list

`scanLibraryCard` walks the card into a RAM table of 128 names (12,804 B with
their sizes), the same size as the catalog before NEWT-80. Past the table, the
walk hands each book to the list instead:

1. **Unchanged card.** One walk. A fingerprint (FNV-1a 64) covers the names
   past the table in walk order, then the sorted table, then the count. When
   it, the count, the outside count and the file size match the list's header,
   nothing is written.
2. **Changed card, at most 128 books.** One walk; the sorted table is the
   list.
3. **Changed card, more books.** A second walk writes the books past the
   table to a scratch file. The build sorts it in runs of 128 (using the
   table), then merges up to 128 runs a pass into the list. 32,768 entries
   bound the books, so at most two passes.
4. **The card cannot hold the list** (no folder, a failed write or read, a
   full card). The build removes its files, and a last walk fills the table
   as before NEWT-80: `/Newt/Books` first, then Library order, and the
   notice "Not every book is shown; the card may be full." With no more
   books than the table, every book shows and nothing is said.

The list is `/Newt/Cache/Library/books.nlb`: a 32-byte header (magic
`NEWTLST1`, count, outside count, fingerprint, check), then one 100-byte
record per book in Library order (the name, zero padded, and its check). The
header is written last, so an unfinished list reads as absent. A missing,
damaged, short, foreign or older list is built again on the next scan. A
record whose check fails removes the list, so the next visit builds it
again. A shelf is kept in RAM only once all its records read back; a shelf
that cannot be read moves the visit to the RAM table (with its notice past
128 books), as when the card cannot hold the list, so the Library never shows
a name it could not read. The scratch files `books.s0` and `books.s1` are removed after each
build, and on a visit that keeps the list, so a build cut short by power loss
leaves nothing behind. The list is a cache: deleting it loses nothing.

Every Library visit walks the card. A long walk calls a progress hook every
64 entries or records: the firmware yields every 50 ms, so the watchdog never
trips, and after 500 ms it paints the Library's loading page ("Looking for
books on your card."). Home never scans.

## Measurements

Host, synthetic cards (Linux, SSD and page cache, `-O2`, `scanLibraryCard`
alone, best of three for an unchanged card). These show the walk's and the
build's work, not device time:

| Card | Books | Entries read | Folders | First scan | Runs | Unchanged | A shelf |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000 books: a quarter in `/Newt/Books`, the rest in 679 author/series folders, plus notes | 1,000 | 3,570 | 690 | 32.8 ms | 8 | 10.2 ms | 0.01 ms |
| 5,000 books, the same shape | 5,000 | 9,837 | 779 | 115.5 ms | 40 | 24.3 ms | 0.01 ms |
| Deep, wide: 20 x 6 x 5 folders, books down to six deep, covers beside them | 9,000 | 29,226 | 6,142 | 284.2 ms | 71 | 85.0 ms | 0.01 ms |

A first scan walks twice; an unchanged card walks once and writes nothing.
The device walk is bounded by entries read: FatFs reads a 512-byte sector per
16 short directory entries (fewer with long names). Bench device timings are
in the PR (the `LIBRARY` DEV row: counts only, never a name).

Home does not scan the card. Opening a selected book resolves its name with
`libraryBookPath`, which for a `/Newt/Books` book builds the same path as
before.

## Memory

- During a scan: the table (12,804 B), the walk (open folders and the path),
  and during a build two small objects; before NEWT-80 the catalog itself held
  12,300 B and the walk held 512 B of sizes. On the bench X3 (65 books,
  USB power, build c9275cc2), the lowest free heap through a build and a
  kept-list scan was 89,204 B, against the 50,468 B floor; before NEWT-80,
  76,616 B. The loop task kept 12,572 B of its stack free through the
  Library (PR #269).
- While the Library is open: the catalog (32 B on the device) and one shelf
  (4 names and the list's path, 580 B), or the table when the card cannot
  hold the list or give a shelf (12,836 B in all, 536 B more than before
  NEWT-80).
- Flash: +8,912 B (3,312,032 -> 3,320,944 B), one measured record. RAM,
  the lent Wi-Fi range and RTC are unchanged; no selected stack frame changes.
- Closed: nothing. Leaving the Library (Home, Sleep, the sample) releases the
  catalog; before NEWT-80 it stayed allocated until a book opened.

## Tests

`tests/library_directory_test.cpp` walks real folders: names and identity,
books anywhere, `/Newt/Books`-only cards, copies (with a whole-file proof),
the proof budget (more than 128 eligible pairs, unreadable ones included),
every limit and read failures; and the list: 1 to 1,000 books and a
two-pass merge of 16,513, an unchanged card writing nothing, added, removed,
moved and renamed books (one past the table), damaged, short, empty, foreign
and older lists, a damaged record, a shelf that fails to open or read (the
visit moves to RAM), a build's leftover scratch, and a card failing each
write and read in turn. `tests/library_card_test.py`
breaks one rule at a time in production code and expects the named case to
fail. `tests/reader_source_hints_test.py` covers the firmware's proof
(`verifiedSourceDigest`): equal digests with unchanged files, a changed file,
different digests, a book never verified and a failed read, with negative
controls. `tests/library_rollback_test.py` covers rollback.
