# #122: reading statistics graphs

Two graphs on two screens, one drawing path. Phase 1 (PR #230) drew them from
invented fixtures; phase 2 wires both screens and adds the per-book record
(section 5).

Code: `include/newt/stats_graphs.h`, `src/stats_graphs.cpp` (drawing),
`include/newt/stats_graph_data.h`, `src/stats_graph_data.cpp` (library
data), `src/book_days.cpp` (the per-book record), `firmware/reading_stats_*.h`
(store, fold, screens), `tools/stats_graphs_fixtures.h`,
`tools/stats-graphs-preview.py`, `tests/stats_graphs_test.cpp`,
`tests/book_days_test.cpp`.

## 1. Owner choices (2026-11-03)

- Two graphs, on both screens:
  1. **Days read:** which days, and how long each day.
  2. **Time of day:** the total time split into the sleep screen's four
     periods (`statsBucket`: Morning 5–12, Afternoon 12–17, Evening 17–21,
     Night 21–5). No new boundaries.
- **Stats screen** (Home → Settings → Reading statistics): all books together.
- **Reader menu → Reading statistics:** the open book only.
- One drawing path for both screens; only the data source changes.
- Phase 2 (2026-11-03): three views on both screens (Summary → Days read →
  Time of day); keep every reading day of every book, exact; the panel copy
  of section 4.

## 2. Inventory

### Library: no new record needed (confirmed)

| Data | Where | Feeds |
| --- | --- | --- |
| Seconds per day, 31 × u16 per month | `NWTS` (current month), `NWTM` (each closed month) | Days read |
| Seconds per period, 4 × u32 per month | same | Time of day |
| Pages per month, dated month range | same, `NWTS` | (other graphs) |
| Totals: seconds, pages, sessions, finished | `NWTS` | the summary, Unknown time |
| CrossInk: totals, a 730-day "read that day" bitmap | `newt_stats_ci` (decoded today) | Days read (no time per day) |
| CrossInk: period seconds [4], weekday seconds [7] | `newt_stats_ci` bytes 17–60, kept verbatim, not decoded before this PR | Time of day; Days read's estimate (#238) |

`libraryDaysGraph` and `libraryPeriodsGraph` build both graphs from these
records only. The test builds a ledger with the real seed and folds (archiving
months as the store does) and checks every day and period against it. The
CrossInk period decoder is new code over bytes Newt already stores.

Limits of the library data:

- CrossInk kept *which* days, not how long. Those days draw an estimate
  (below), or a hollow box when there is none.
- Days before the first dated fold, and between CrossInk's last day and Newt's
  first, are **not recorded** and draw as a dotted baseline.
- Reading counted before the clock was set (or seeded from earlier Newt
  reading) has no period. It shows as **Unknown**, so the periods add up to
  the total.

### Per book: today's data supports neither graph

| Data | Where |
| --- | --- |
| Time, pages, qualified sessions (no dates) | progress counters in `nb<hex>` (`NWTR`) |
| First, last and finished day; start position | `NWTB` facts (`sb<hex>`) |
| CrossInk per book: sessions, time, pages, start/finish dates, period seconds [4], weekday seconds [7] | `ci<hex>` (`NWCB`), bytes kept verbatim, bound to the book's digest (unverified) |

Newt keeps no per-book day or period data. CrossInk's per-book periods can
feed graph 2 for imported books (`crossInkBookPeriods`); nothing gives graph 1.
Section 5 designs the smallest record that feeds both.

### What CrossInk shows (commit `9914146`, MIT, read only, no code copied)

`src/activities/reader/BookStatsView.cpp`:

- An "All time" card: sessions, time, pages per minute, average session,
  current streak, completed books.
- A per-book card: sessions, time, progress, average session, time left,
  started and finished dates (editable).
- **Time of day:** four horizontal bars. **Day of week:** seven horizontal
  bars. The same two charts on the per-book and the all-time page.
- Reading-stats sleep screens.

CrossInk draws no per-day history. Newt's day graph is new.

## 3. The graphs

One code path: `drawStatsDays` and `drawStatsPeriods` draw into a box. The
screens only choose the box and the data.

### Days read

- One bar per calendar day, the newest at the right. The window is whole
  weeks at 13 px or more per day (`statsGraphDays`): 4 weeks in portrait
  (Stats screen and panel), 6 weeks on the landscape panel.
- Bar height is time read. The axis top is the first of 5, 10, 15, 20, 30,
  45 min, 1, 1.5, 2, 3, 4, 5, 6, 8, 10, 12, 15, 18, 20, 24 h that holds the
  tallest bar and divides into 3 to 6 steps of a nice scale (1, 2, 5, 10, 15,
  30 min, 1, 2, 3, 4, 6, 8 h); the finest such step wins (#238,
  `statsDayAxis`). Each step has a dotted line labelled in the UI font ("30
  min", "1 h", "1.5 h"; minutes throughout below a 30-minute step), and "0"
  sits at the baseline. A box with less than 22 px a line takes fewer lines,
  never fewer than 3, so labels never touch each other or the dates. Any
  reading is at least 3 px tall, so a short day stays visible next to a very
  long one.
- Marks, all 1-bit, no grey: a solid bar (read), an outlined bar (estimated
  from CrossInk, below), a low hollow box on the baseline (read in CrossInk,
  no estimate), a solid baseline (known, not read) and a dotted baseline (not
  recorded). A legend line appears only for the marks in view.
- **CrossInk's estimate (Stats screen only, #238).** A day CrossInk marked
  read and Newt has no time for draws an outlined bar at CrossInk's seconds
  for that weekday divided by the days its 730-day history marks read on that
  weekday (at most a day; weeks start on Monday, as CrossInk's
  `readingStatsDayOfWeekIndex` at `9914146`). A day with Newt time shows only
  that time. The axis fits the estimates; the caption's total, "Read on N of
  M days" and "Longest day" count measured time only (a CrossInk day still
  counts as read). Legend: "Estimated from CrossInk". The book panel has no
  CrossInk days.
- Each Monday carries its date ("12", or "2 Nov" where a month starts and on
  the first label). A label that would touch the previous one is skipped; one
  that cannot fit whole is left out, never cut.
- Caption: "7 Oct - 3 Nov · 7 h 45 min".
- Empty: "No days recorded yet" (nothing known), "No reading on these days"
  (known days, no reading) or "Dates need a set clock" (no trusted clock),
  each with one line of what comes. No empty axis.

### Time of day

- Four rows, Morning to Night, each with its name and hours (taken from
  `statsBucket`, not restated), a bar and its time. A fifth row, **Unknown**,
  appears only when part of the total has no period; its bar is hollow.
- Bars share one scale (the largest row). Caption: "All time · 21 h 46 min",
  the same total as the summary.
- Empty: "No reading yet".

## 4. Screens and buttons

The owner moves with buttons only. Both screens have three views: Summary →
Days read → Time of day. Prev and Next step through them and stop at both
ends. On Days read, Select ("Earlier") steps back one window; after the window
that holds the oldest kept day it wraps to the newest. Nothing is kept for a
view but its number and window count, in spare bits of existing UI state
(`AppState::listFocus`, `ReaderUiState::selected`).

### Stats screen (portrait, like every SystemPage, docs/078)

Summary (today's page, unchanged) → Days read → Time of day. Each graph gets
the whole card; Days read adds "Read on 16 of 28 days" and "Longest day:
14 h 15 min, 25 Oct". The rail names the presses: Back, Earlier, Prev, Next.
The oldest kept day is the first dated Newt day or CrossInk's oldest day.

### Reader menu → Reading statistics (portrait and landscape)

The panel keeps its box and frame (`readerOverlayGeometry`: 474 × 402 portrait,
637 × 402 landscape) and has the same three views. The view's name sits on the
first line in small capitals, with the graph's caption at the right. The days
window ends today, or on the book's last day read when that is older than one
window. Time not yet folded makes today the last day read. Its oldest kept day
is the record's `sinceDay`.

The panel shows Newt's records at once (#238). It reads the book under the
identity it opened with while the book's file check still runs, and counts
the open session before this open's first fold, so the first panel after
opening a book shows today's time. The paint writes nothing and reads no
CrossInk source. The book's CrossInk history comes from its import below,
which runs a few seconds after the file check; an open panel repaints once
when that import is written, so the history fills in without leaving it.

Copy (same facts as before):

| Before | Now |
| --- | --- |
| THIS BOOK / SAVED + ACTIVE SESSION | THIS BOOK |
| Active time: 1:02:03 | Time read · 1 h 02 min |
| Forward pages: 412 | Pages turned · 412 |
| Qualified sessions: 9 | Sessions · 9 |
| Started: 21 Oct 2026 (and Last read, Finished) | Started · 21 Oct 2026 (same rows) |
| Menus and sleep do not count. | Includes this session. Menus and sleep don't count. |
| — (imported books) | Includes 9 h 05 min read in CrossInk. |
| Back: menu / Power: sleep | Next: days read · Back: menu |

Values sit at the right in bold, as on the Stats screen. Imported CrossInk
time, pages and sessions are added to the rows, as the Stats screen already
adds CrossInk's totals. The open session's time not yet folded counts on
today and in the current period, as on the sleep screen.

## 5. Per-book record

Every reading day of every book is kept exactly; nothing is merged or
dropped. A book has one head record, which holds its current block of days,
and one archive record per older block, like the library's `NWTS` and
`NWTM`. Same conventions as the other statistics records: A/B slots,
little-endian, generation at byte 8, FNV-1a at the end, in `/Newt/Stats/`.

A **block** is 62 calendar days (block = day / 62), 2 bytes a day (seconds,
saturating at 18 h 12 min as in `NWTS`).

**`NWTD` v1, `sd<13 hex>`, 232 B (the NRS payload limit):**

| Off | Field | Type | Rule |
| ---: | --- | --- | --- |
| 0 | magic `NWTD`, version 1, generation | 3 × u32 | |
| 12 | `digest` | 32 B | The book's SHA-256; must match. |
| 44 | `sinceDay` | day | The first dated fold's day; unset until then. Earlier days are not recorded. |
| 46 | `block` | u16 | The block held below; 0 before the first dated fold. |
| 48 | `flags` | u16 | Bit 0: CrossInk's records were checked for this book. |
| 50 | reserved | u16 | 0 |
| 52 | `watermark` | u64 | The book's milliseconds already in this record. |
| 60 | `periodSeconds[4]` | 4 × u32 | Newt's dated time by `statsBucket`. |
| 76 | `imported` seconds, pages, sessions | 3 × u32 | CrossInk's sources bound to this book, summed. |
| 88 | `importedPeriods[4]` | 4 × u32 | Their period seconds (v4/v5 files). |
| 104 | `daySeconds[62]` | 62 × u16 | The current block. |
| 228 | FNV | u32 | |

**`NWTE` v1, `se<10 hex of the digest><3 hex block>`, 176 B:** magic,
version, generation (12), `digest` (32), `block` u16, reserved u16,
`daySeconds[62]`, FNV. The full digest must match, so two books sharing the
40-bit name prefix never read each other's days: the second one's archive
for that block reads as not recorded and is never written (undercount).

**Why its own records.** `NWTS` is the library ledger and cannot hold every
book. `NWTB` holds the facts that count finished books; a v2 `NWTB` would
read as unsupported in an older Newt, which would then stop handing over. New
records change nothing an older Newt reads (as `NWTK` did).

**Card space.** A year of daily reading spans 6 or 7 blocks: the head and 5
or 6 archives, about 1.3 KB of records. Each record is one 4 KB NRS file, so
24–28 KB of files, and one cluster each on the card (on a card with 32 KB
clusters, about 224 KB). A book read for two weeks uses one or two files.

**Writes.** Only at the existing folds of the leased book (idle every 5
minutes, Back, Sleep, a book switch), after `NWTS` is committed, never at a
page turn:

- The fold credits `C − max(record watermark, NWTS's watermark before this
  fold)` whole seconds, on the fold's day and period, with the same midnight
  split as the ledger (below the record's watermark, after a progress reset,
  `NWTS`'s alone). So the record never gets more than `NWTS` dated in the
  same fold.
- One head write (create, write, fsync, read-back) when the record changed.
  When the fold's day is in another block, the head's block is archived
  first (one more write, confirmed) and the target block's archive, if any,
  is read into the head.
- The first fold of a book makes the head. Once per open, a quiet reader-idle
  duty that input cancels (between sources, and before its write) sums the
  `ci` records bound to the book into `imported*` and sets the flag. It runs
  after the open's first fold (normally the handover, due once the file check
  passed and input has been quiet for 2 s) and once the library import
  completed (docs/076 section 6): this boot's at Home, or an earlier boot's
  (`newt_import` exists), so a boot that resumed into the book needs no Home
  visit. After this boot's import failed it does not run, so a later boot
  sums the complete sources. An open panel repaints once its write lands.

**Reads per paint.** Nothing is kept in RAM.

- Panel: `NWTS` (and `NWTB` when the lease names another book) as before,
  `NWTS` once more on a graph view (the unsaved session), the head, and at
  most 2 archives for the days window (42 days span at most 2 blocks): at
  most 3 per-book records. Never the `ci` listing or a source.
- Stats screen: Days read reads `NWTS`, at most 2 `NWTM` and CrossInk's
  record; Time of day reads every month in the dated range (about 12 a year),
  as the summary already does.

**Failures (durable-commit rule).** Every step can undercount, never
overcount, and never moves time to another day.

| Case | Result |
| --- | --- |
| Cut after `NWTS`, before `NWTD` | That fold's time is missing from the book's days and periods; the next fold starts from `NWTS`'s watermark. |
| A book switch's handover fold (after the exit fold failed) | Credited to `NWTS` only; missing from the old book's days. |
| Unconfirmed write | The record holds the old or the new value; the next fold starts from the larger watermark. |
| Archive write fails at a block change | The head keeps its block; the fold's time is missing from the days. |
| Counters below the record's watermark (a progress reset) | Credits what `NWTS` credits from its own watermark. |
| Counters below `NWTS`'s watermark (rollback) | Watermark moves down; nothing is added. |
| No trusted clock, or a day before `sinceDay` | Watermark advances, no day or period: shows as Unknown. |
| Absent | Made at the next fold; earlier reading shows as not recorded and Unknown. |
| Corrupt head | Set aside as `.bad` and made again from that fold (undercount). |
| Corrupt archive | Reads as not recorded; at a block change it is set aside and started empty. |
| Newer version | Never written; the panel says "From a newer Newt". |
| Unavailable | Skipped. After `NWTS` committed, that fold's time is missing from the days (as a cut); otherwise the next fold counts it. |

The book's totals still come from its progress counters, so these records
never move them. Days before `sinceDay` draw as not recorded, and their time
is in Unknown.

## 6. Cost

Flash record 103 (code-health-baseline.json): +18,928 B of flash for the
drawing, data, records and wiring. RAM (125,312 B), RTC (300 B) and every
selected stack ceiling are unchanged; nothing is held while reading. The
panel's view and its record reads live in their own frames, entered only
while the panel paints, so a page turn's paint is smaller than before
(`renderReaderUi` 272 → 256 B). SD writes: one per fold while a book's time
moves, one more every 62 days.

## 7. Tests

- `tests/book_days_test.cpp`: the record codecs (layout, limits, every
  corrupt cause, newer versions), the fold rule against the ledger, the
  window and periods.
- `tests/book_days_store_test.cpp`: the failure table above through the
  real fold on the real SD backend (a cut at every write, with cold boots
  that discard RAM), the book's CrossInk sum, and negative controls.
- `tests/reading_stats_firmware_test.cpp` (`book_days_wiring.h`): the idle
  fold writes the record, both screens read it, the import runs once after
  input, a cold boot keeps the days, unsaved time shows on today a window
  after the last read, a failed Home import leaves the book's sum to a later
  boot, the first panel after an open (file check running, no fold or import
  yet) already shows the session, writes nothing and opens no more files with
  128 other books' CrossInk sources on the card, the open panel repaints once
  when the book's import lands (also on a boot that resumed into the book),
  and negative controls.
- `tests/stats_graphs_test.cpp`: drawing in every box (the landscape panel's
  in a landscape frame), labels never cut, the empty state, the axis scale and
  its labels apart, CrossInk's estimate (its rule, its mark, kept out of the
  captions, inside the axis).

## 8. Previews

`tools/stats-graphs-preview.py <dir>` writes PNGs into a private (0700)
directory through the shared painters:

- `screen-<view>-<fixture>.png`: views `summary`, `days`, `periods`;
  fixtures `empty`, `few-days`, `imported-gap` (two years of CrossInk days to
  12 Oct, a gap, Newt from 20 Oct, 3 h undated), `long-day` (14 h 15 min
  on one day), `crossink-weeks` (two years of CrossInk days with weekday
  time, then three Newt weeks), `crossink-uneven` (CrossInk only, very uneven
  weekdays, one with no time) and `short-days` (minutes a day);
  `screen-earlier-<fixture>` is Days read one window back for `imported-gap`
  and `crossink-weeks`.
- `panel-<portrait|landscape>-<view>-<fixture>.png`: views `summary`, `days`,
  `periods`; fixtures `started`, `long-read` (record began mid-book, a week
  away), `finished` (the window ends on its last day), `imported` (CrossInk
  only), `long-day` (11 h 20 min on one day), `year` (a year of reading
  across 7 blocks), `crossink-weeks` (CrossInk totals, then three Newt
  weeks), `short-days`, `empty` (opened, never read) and `unsaved-only` (read
  today, nothing folded yet); `earlier` (one window back) for `long-read` and
  `year`.

Fixtures are invented; the panel shows no title ("Your SD book", Newt's words
for a title it cannot show). These are host pixels from the shared C++ code,
not device evidence.
