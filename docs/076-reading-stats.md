# NEWT-40: reading statistics (design)

Round 2 replaced round 1's two-record fold with a single-record lease
(review r1). Round 3 removes the epochs and per-book watermarks: the seed now
completes before any lease exists (review r2). v1 is implemented; section 9
lists where it differs from this design and what it can undercount.

The owner is moving their daily X3 from CrossInk to Newt and cares more about
reading statistics than about the firmware. Newt records its own robust
statistics from the first day. Importing CrossInk history comes after the
switch; its data stays on the CrossInk card.

## 1. Inventory

### Newt today

| What | Where |
| --- | --- |
| `ReadingTracker` counts `activeMilliseconds`, `forwardPages` and `qualifiedSessions`. A page turn needs 2 s dwell; a session qualifies at 60 s; activity stops 300 s after the last interaction, so an abandoned page earns up to 300 s. | `include/newt/reading_progress.h`, `src/reading_progress.cpp` |
| Opening a menu ends the session; returning starts a new one. | [reader-menu-settings](reader-menu-settings.md) |
| The counters are stored in the per-book progress record (`NWTR` v1, 52 bytes), in A/B slots in `/Newt/State/nb<13 hex>.nrs`, bound to the SHA-256 of the book file. | `encodeProgress`, `firmware/external_progress_store.h`, [051](051-sd-reading-state.md) |
| Saved at idle checkpoints, open, Back and Sleep. | [073](073-idle-progress-save.md) |
| Shown only in the reader menu's statistics panel. | `src/reader_overlay.cpp:92` |
| The DS3231 holds UTC; Home reads it (read-only) and shows it in the `newt_zone` time zone. | [066](066-read-only-home-clock.md), [068](068-home-clock-time-zone.md) |

Gaps: no dates, library totals, streaks or history. An abandoned page earns
up to 5 minutes. Menus split sessions.

### CrossInk (commit `9914146`, MIT; read once, no code copied)

| What | Where |
| --- | --- |
| Per-book `stats_v5.bin`, 73 bytes. Fields: sessions, seconds, pages, completed, pace, start/finish dates with manual flags, time of day `[4]`, day of week `[7]`, time left. `.tmp`/`.bak` saves; no checksum. | `src/activities/reader/BookReadingStats.{h,cpp}`, `docs/file-formats.md:518` |
| Global `global_stats.bin` v3, 159 bytes. Fields: totals, completed books, buckets, a 730-day "read that day" bitmap, longest streak. Also `.bak` and `synced_stats/`. | `GlobalReadingStats.{h,cpp}` |
| A page interval longer than the idle threshold (default 300 s) earns nothing. A day is marked read from a 10 s session. | `EpubReaderActivity.cpp:1644-1673`, `ReadingSessionStats.cpp` |
| Stats are saved only on reader exit. Identity is the book's path (`epub_<fnv64(path)>`). | `EpubReaderActivity.cpp:1534`, `lib/Epub/Epub.cpp:496` |
| Buckets: Morning 5–12, Afternoon 12–17, Evening 17–21, Night 21–5. Days are counted from 2000-01-01; weeks start on Monday. | `ReadingStatsUtils.cpp:166,218` |

## 2. What to record

**Time.** Durations come only from monotonic `millis`.

- An *interval* runs from one reader interaction (page shown, page turn,
  same-locator navigation) to the next interaction or a closing event: menu,
  Back, Sleep, book switch or error.
- A closed interval earns its length if it is 300 s or less, otherwise
  nothing. An idle save does not close or credit the open interval.
- A forward page still needs 2 s dwell.
- A *session* starts with the first credited interval after open or wake. It
  ends at Back, Sleep, switch or an expired interval. Menus, errors and
  same-book rebuilds (a settings save, orientation, font, reflow, cache
  rebuild or recovery) pause a session without ending it.
- A session counts once it has 60 s of credit. Shorter sessions still add
  their time.

**Two sources of truth, each with one job.**

- *Per-book totals* (time, pages, sessions) **are** the existing durable
  progress counters, written atomically with the reading position. Nothing
  copies them per book.
- *Statistics records* store only what progress lacks:
  - library totals;
  - dated data (time per day, time-of-day buckets);
  - per-book facts (first read, last read, finished).

**Clock.** The fold that writes dated data reads the DS3231 once, the same
pointer-only 10 ms read that Home uses. The reading is trusted only if Home
would show it (OSF clear, plausible) and the UTC year is in 2026..2099. It is
converted with the published zone.

- If the clock is not trusted, the delta goes to totals only and is
  *undated*.
- Statistics never write the RTC (only Update's Check sets it, [066](066-read-only-home-clock.md#setting-the-clock-261)).
- Streaks are computed from stored days, never stored, so a clock jump cannot
  corrupt them. Moving to another month archives the current one and loads
  the target month (section 3), so nothing is dropped.

## 3. Robustness

### Invariant

**Each acknowledged credited second, page and session is counted in library
totals at most once.** A credited unit is a positive step in a book's
acknowledged progress counters `C` after the seed. Statistics keep reading
that was credited, even if that book's progress is later reset or rolled back
(*retained history*): counters below the watermark move the watermark down
and change no totals. Every failure undercounts; none overcounts and none goes
negative.

Progress really does roll back: a power-on resume can restore an older
acknowledged checkpoint as a whole (`firmware/reader_checkpoint.h:73`).
Library totals stay correct without changing that, but the book's own totals
(its progress counters) drop below what the library counted. **Recommended,
not part of this design:** when a power-on resume restores an older
checkpoint, keep the larger counters from the newer valid peer slot of the
same book, so per-book totals never go backwards.

### Records

There are three namespaces. They use the existing `SdReadingState` backend
and `DurableRecordStore` (A/B, generation at byte 8, final FNV; NRS copies
with CRC-32C, `fsync`, read-back) and stay within the 232-byte NRS payload
(`firmware/reading_state_backend.h:67`). They live in `/Newt/Stats/`, routed
by name, so their `fsync` never rewrites `/Newt/State` directory sectors. No
NVS is used.

Shared rules:

- All integers are little-endian.
- *Day* = u16 local days since 2000-01-01. `0` = unset, `0xFFFE` = set but
  undated, `0xFFFF` = unknown (recovered); otherwise 9497..36524 (2026..2099).
- *Month* = u16 `year*12+month-1`, `0` = none, otherwise 24312..25199.
- Counters saturate at their maximum and never wrap.

**Facts**, 12 bytes, stored in `NWTB` and copied into `NWTS` while the book
is leased. Facts never affect reading totals.

| Off | Field | Type | Limit / rule |
| ---: | --- | --- | --- |
| 0 | `firstDay` | day | Set once at the first credited fold. If the book had qualified sessions when its lease began: set but undated. |
| 2 | `lastDay` | day | The latest fold's date. |
| 4 | `endDay` | day | Set when the shown page is the last page of the last spine, however it was reached (open, restore, turn, jump or recovery). |
| 6 | `manualFinishDay` | day | Set by Reading menu → Mark as finished (#105); kept if already marked. Undo unsets it. |
| 8 | `startPermille` | u16 | 0..1000 at the first fold; `0xFFFF` unknown. |
| 10 | `factFlags` | u16 | bit 0 `manualFinished`: marked in the Reading menu (#105); bit 1 `crossInkFinished`: CrossInk's imported finished count already holds the book, so `booksFinished` does not (#105); other bits zero. |

**`NWTS` v1, `newt_stats`, 180 B** (library). It exists only once seeded; a
normal fold writes only this record.

| Off | Field | Type | Limit / rule |
| ---: | --- | --- | --- |
| 0 | magic `NWTS`, version 1, generation | 3×u32 | |
| 12 | `oldestMonth`, `newestMonth` | 2×month | The dated range: the earliest and latest month any fold dated. Both none until the first dated fold; otherwise the range holds `month`. |
| 16 | `totalSeconds`, `totalPages`, `totalSessions` | 3×u32 | Saturate (136 years). |
| 28 | `booksFinished` | u16 | Saturates. |
| 30 | `month` | month | The month held below. |
| 32 | `leaseDigest` | 32 B | The leased book; all zero means no lease. |
| 64 | `leaseMs` | u64 | Watermark of folded progress counters. |
| 72 | `leasePages`, `leaseSessions` | 2×u32 | Watermarks. |
| 80 | lease facts | 12 B | Facts of the leased book. |
| 92 | `monthPages` | u32 | Saturates. |
| 96 | `bucketSeconds[4]` | 4×u32 | CrossInk's hour bounds; saturate. |
| 112 | `daySeconds[31]` | 31×u16 | Saturate at 65,535 s (18 h 12 min a day). The excess is still in totals. |
| 174 | reserved | u16 | 0 |
| 176 | FNV | u32 | |

**`NWTM` v1, `sm<yyyymm>`, 104 B** (closed month): magic, version and
generation (12); `month` u16; reserved u16; `monthPages` u32;
`bucketSeconds[4]`; `daySeconds[31]` u16; reserved u16; FNV. Rules as in
`NWTS`.

**`NWTB` v1, `sb<13 hex>`, 64 B** (per book, facts only): magic, version and
generation (12); `digest` 32 B (the full SHA-256; must match the book); facts
12 B; reserved u32; FNV.

### Seed first, then lease

**Seed.** No lease, fold or handover exists until the seed has written
`NWTS`. This happens on the first boot with statistics, and again after
`NWTS` is recreated.

- While unseeded, reading continues as normal; only the progress counters
  grow.
- The seed is a quiet duty (Home, or reader idle) that reads one file per step
  and can be stopped by input. Each run starts again from scratch until its
  single write lands. With no books (the owner's main device on day one) it
  completes at once.
- It writes one `NWTS`:
  - totals = the sum of every `nb*` book's current counters, undated (the
    owner decision on earlier Newt reading);
  - `booksFinished` = the number of `NWTB` records that are finished
    (`endDay` set or `manualFinished`) and not `crossInkFinished`;
  - lease = the open book, with watermark = its acknowledged counters (the
    same value that went into the totals) and facts from its `NWTB`; or no
    lease.
- Only the open book's counters can change during a seed. Its contribution
  and its watermark come from one read, so a checkpoint that lands mid-seed is
  picked up by its first fold.

**Lease.** `NWTS` leases the book being read: its watermark (`lease*`) and its
facts. Let `C` be that book's acknowledged durable progress counters.

- **Fold** (lease = this book):
  - If any field of `C` is below the watermark (reset or rollback), set the
    watermark to `C` and change no totals.
  - Otherwise add `C − watermark` to the totals, and to today's day and
    bucket if the clock is trusted. The watermark advances only by whole
    seconds.
  - Update the facts. Only a change from not finished to finished adds one
    to `booksFinished`; only an undo (below) takes one away, never below 0.
    A book CrossInk's import bound and completed is in the imported count
    already: the fold that would add it sets `crossInkFinished` instead, so
    it counts once through mark, undo and re-mark.
  - All of this is one `NWTS` write.
- **Handover** (opening book B while the lease names A or no book). It runs as
  the first quiet duty after B opens, and at the latest just before B's first
  save that changes its counters. B's counters are unchanged until then,
  because the open save credits nothing. Steps:
  1. Read A's counters and B's `NWTB`. Missing → facts unset; corrupt → facts
     unknown.
  2. Write `NWTB(A)` with the lease facts.
  3. Write `NWTS`, adding A's final delta and setting lease = B,
     watermark = `C_B`, facts = B's.
- **Month change:** write `NWTM(old)` from `NWTS`'s month block, and confirm
  it, before writing `NWTS` with the target month. The target month's block is
  loaded from its archive if one exists, otherwise zeros. Both writes store
  absolute values, so they can be repeated.

- **Mark** (Reading menu → Mark as finished / Mark as not finished, #105):
  first the lease check (a handover if the lease names another book), then
  one `NWTS` write: a fold at the watermark, so it credits no reading. Mark
  sets `manualFinished` and `manualFinishDay` (kept if already set). Undo
  clears both, and `endDay` too, and ignores the last page for the rest of
  the open (Return to reading shows it again). The book is then finished
  again only by a new mark, or by its last page shown in a later open
  (reopening a book saved on its last page counts as shown). An unknown
  finish (a corrupt `NWTB`) shows 100% but no row, and the fold ignores its
  undo: the count may not hold it, so an undo could only uncount another
  book. The built-in sample, which statistics exclude, has no row either.
  The last page is judged against the readable end: a book whose final
  chapters are empty finishes on its last text page, found by the first
  Next there. The row and Home's 100% follow the stored facts: they change
  only after a verified write, and every Home load and reader open reads
  them from the card. A failed write says "Could not save"; an unconfirmed one
  says a restart may show the old or the new value. Right after an open the
  book's file check and first save run while the reader is idle, and presses
  defer them; a press before both finish writes nothing and says the book
  is still opening (found on the bench: a quick mark after an open).

Counting reads only `NWTS` and progress counters. `NWTB` feeds only
`booksFinished`, through the seed's count and the finished rule, so
losing or repairing it never moves reading totals.

### Why this stays closed

The invariant holds by structure, not by care at each call site. Library
totals and `booksFinished` live in `StatsLedger`, whose fields are private
(`include/newt/reading_stats.h`). Only `seedLedger` and `foldLedger` change
them; `handoverLedger` reuses `foldLedger` for the old book's final delta and
then only moves the lease, and the month change copies absolute values. The
ledger has no public constructor: one exists only from a valid stored `NWTS`
(the decoder) or from the seed, so no fold, handover or lease can run before
the seed. Every fold is `C − watermark` against the stored ledger, so a retry
of a landed write adds zero. "Finished" has one definition, `finished(facts)`:
the fold's finished rule (+1 when it becomes true, −1 only when an undo
makes it false; both skip a book CrossInk's import counts), the seed's
count and the screens all call it. A new writer, a second finished rule or a fold without a stored ledger
therefore needs a visible change to these types, not a missed check.

### Crash points

A *write* means: NRS copy write, `fsync`, read-back. A statistics write is
acknowledged only after its own `fsync` succeeds: a read-back after a failed
`fsync` can match bytes that a power cut still loses, so it is not proof. A
failed write is repeated once (write, `fsync`, read-back); if that fails too,
the result is *unconfirmed*: either value may be stored, the operation is not
done and its next attempt rereads the record. Every step stores absolute
values against the stored watermark, so a repeat counts nothing twice.

| Operation | Cut after… | Result |
| --- | --- | --- |
| Seed | before `NWTS` | Still unseeded; reading goes on and the seed starts again. Totals include everything at that later seed. |
| Seed | `NWTS` (or unconfirmed but landed) | Seeded. A valid `NWTS` means the seed never runs again. |
| Fold | nothing written | `NWTS` unchanged; the next delta includes this one. |
| Fold | `NWTS` | Done. A retry computes `C − watermark` = 0. Same-book retry has no second record (r1 P1-1 stays closed). |
| Fold, unconfirmed | `NWTS` either value | The next fold uses whichever watermark is stored. |
| Month change | `NWTM` only | `NWTS` still on the old month; the repeat writes the same archive, then `NWTS`. |
| Handover | `NWTB(A)` | Facts written (repeatable); the lease is still A, so the handover repeats. |
| Handover | `NWTS` | Done. If A's progress was unreadable, A's final delta is dropped (undercount). |
| Handover cannot write `NWTB(A)` | — | It waits; B stays unfolded. A later handover sets the watermark to B's counters at that time, so B's reading before it is not counted (undercount). |
| Progress reset or rollback, leased book | — | Watermark moves down; totals unchanged (retained). Later reading counts once from the new base. |
| Progress reset or rollback, other book | — | Its next handover sets the watermark to its current counters; earlier credit stays counted once. |
| `NWTB` corrupt → repair | any point | Repair keeps the file present (below), so facts read as unknown or valid and never as unset. Totals never move. This closes **r2 P1** by construction: no seed can run (`NWTS` exists) and `NWTB` carries no counters. |
| `NWTS` corrupt → repair | any point | Set aside → unseeded → seed. Totals become Σ current counters: retained credit from rollbacks is lost (undercount), never doubled. |
| Finished book leased again, any time | — | Facts load with `endDay` set, so there is no unset → set change and no second count. The seed already counted it: it counts every `NWTB` with `endDay` set, with no epochs. This closes **r2 P2 (finished)**. |
| r2 P2 (rebase) example: 20 s folded, counters reset to 10 | — | No seed can be pending once leases exist. Totals keep 100 + 20 (retained, each unit once); the watermark is 10. The test oracle is the invariant above, not Σ current counters. |

### Loading and repair (P2-4)

A stats load returns one of the following. Only `Corrupt` triggers repair.

- **Loaded.**
- **Absent:** no file.
- **Corrupt:** the file opened and both keys were read without I/O error, but
  no slot passes the codec. Causes: bad FNV, known magic and version with a
  reserved byte not zero, unknown flag bits, a value out of range, a zero or
  mismatched digest, or equal generations with different bytes. A present
  file with no valid slot counts as corrupt. For stats namespaces the backend
  also reports an outer-damaged file (wrong size or header) as corrupt rather
  than absent.
- **Unsupported:** valid FNV, known magic, newer version.
- **Unavailable:** a mount, open or read error.

| Result | `NWTS` | `NWTB` | `NWTM` |
| --- | --- | --- | --- |
| Corrupt | Rename to `.bad`, then seed | Copy to a fresh `.bad`, then overwrite in place with facts unknown. The file is never absent. | Rename to `.bad`; that month's days are lost; totals unaffected |
| Unsupported | Never written; folds stop and the screens say "from a newer Newt". Progress counters keep growing. | Never written; the handover waits | Never written |
| Unavailable | Retry at the next fold; never repaired | same | same |

Both repairs are new backend operations. The existing `DurableRecordStore`
refusal (`durable_record_store.h:68`) would otherwise block that record
forever. Nothing is deleted. A `NWTB` deleted by hand reads as absent; that
book could then count as finished twice. Reading totals are unaffected.

### What can be lost (P2-5)

| Data | Bound |
| --- | --- |
| Per-book totals | Unacknowledged reading (the progress checkpoint age, today's exposure for the reading position) plus the open interval (≤ 300 s). A rollback lowers them (see the recommendation above). |
| Library totals | The same, plus the undercount rows of the crash table. Acknowledged reading on the leased book is never lost. |
| Dated data | Reading while unseeded is undated. A RAM midnight mark splits closed, credited intervals and forward pages between the two local days at the next normal fold, including a month rollover. The UTC session clock and monotonic millis locate midnight using the published zone and its DST rules. The turn at midnight belongs to the new day; a qualifying session still counts once, on its start day (the format stores only total sessions). No SD operation or allocation is added to a turn. Losing the mark (cold boot, crash or reader rebuild), an invalid clock, a changed published zone, a UTC clock jump beyond the projection tolerance, or reaching a second midnight without a fold uses the existing fold-time dating. That fallback can misdate the full fold age; it never creates reading counters. Invalid clocks keep the fold undated. Time-of-day buckets still use each part’s fold hour (23 for the earlier day), rather than a time-of-day log. DEV telemetry reports total delta, fold age and carry. |

### When folds run

- **Seed:** as above, before anything else.
- **Idle:** an optional quiet duty after the progress core (073). It is due
  when the acknowledged counters differ from the watermark and 5 minutes have
  passed. Input before its first effect cancels it.
- **Back, Sleep and switch:** synchronously after the existing synchronous
  progress save.
- **Handover:** as above.
- **Sleep and deep sleep:** Sleep closes the interval and the session; deep
  sleep never earns time.

**Identity.** Records use the same full SHA-256 of the book file as progress.
Titles and paths are never used. The built-in sample is excluded.

**Durability rules.** Statistics are not preferences; the AGENTS.md rules
apply as follows:

- Commit and verify before a value is shown as saved.
- Reconcile a lost acknowledgement with one bounded repeat of the write and
  its `fsync`; otherwise report it unconfirmed.
- Retry an unconfirmed result against the stored watermark; never call it
  lost.
- Read a missing record as zero, and never overwrite an unknown version.

## 4. Cost

Measured at `154aea94`, four clean lanes (flash record 59, protected update
36; [CODE_HEALTH](CODE_HEALTH.md)), after review r2's fixes. The design
estimates are kept where noted.

| Resource | v1 |
| --- | --- |
| Heap while reading | Since #130, **32 B per live reader** for one bounded midnight mark (tracker 64 → 96 B), released with its reader storage; no extra allocation and no storage in a closed reader. Folds use stack and the existing short-lived 112-byte NRS `Active`. |
| Stack | #130 adds a separate clock/split frame and a bounded prefix fold; compiler frames are recorded with flash record 67 and protected record 43. These are not runtime peak measurements. Original v1 measurement: folds are sibling frames after the progress save returns. The deepest path below `finishReaderExitAction` is about 2,256 B (creating a book's facts file at a handover) and about 2,166 B for the ledger save, against the 2,320 B Back/Sleep save path; about 2,464 B only if `NWTS` vanishes between load and save. The save leases add 16 B on their own paths (the cancel source). Four existing selected frames grow 16 B each; 32 new frames are selected. |
| Static RAM / RTC | No additional static RAM / RTC (#130: 133,440 / 300 B for the one build). Nothing is held in a closed reader. The dated range lives in the stored ledger. |
| Flash | +22,896 B release, +23,536 B DEV (+23,552 regional DEV) against record 58: about 2.3 times the 7–10 KB estimate, still well under the 7,016,448 B usable limit. |
| SD writes | A fold is 1 write (2 when applying a midnight mark), at most about 12 normal folds an hour while reading, plus one at each close; an unchanged fold still writes. A failed write is repeated once. A handover is 2 reads and 2 writes, once per book switch. A month change adds 1. A seed reads every `nb*` and `sb*` file once, on the first statistics boot and again only after `NWTS` is repaired. |
| Time | Estimated 20–60 ms per fold, 60–150 ms per handover and roughly 50 ms per book for a seed; not yet measured on the device. DEV STATS rows report each one's duration. |
| Library page | Reads `NWTS`, plus the archives inside its dated range (about 12 a year) for streaks and "last 7 days". Months outside the range hold no days and are not opened. |

## 5. What the owner sees

**v1**

- **Reader menu → Reading statistics.** Time read, sessions and average
  session, and pages come from progress plus the live session, as today.
  Started, last read and finished dates come from the lease or `NWTB`.
  - "No date" for reading from before statistics or without a set clock;
    "Not yet" while unfinished; "Unknown" for a repaired record.
  - One line instead when there are no dates: counting, from a newer Newt,
    or unavailable.
- **Home → Settings → Reading statistics.** The bottom Settings row.
  - All-books time, today, the last 7 days, current and longest streak,
    books finished, sessions and pages.
  - "Counting…" until the seed is done.
  - "Dates need a set clock" when the clock is not trusted. Totals stay; the
    dated rows are hidden.
  - "From a newer Newt" and "Statistics unavailable" (card not readable).
  - "Includes your CrossInk history." under the totals once the CrossInk
    import (section 6) has frozen a global file.
- Both screens read the card only while painted and keep nothing in RAM.
  Previews: `artifacts/device-stats-<state>.png` (including `crossink` and
  `crossink-undated`) and
  `artifacts/reader-stats-<state>.png` (host pixels from the shared C++
  screens; not hardware evidence).

**Later** (● = recorded from day one)

- Time-of-day views ●, and calendar and charts ●.
- Streak and stats sleep screens ●.
- Time left ● (from `startPermille`).
- Date editing.
- Per-book list, reset and export.
- A per-session log.
- CrossInk history per book (the reader menu), from the frozen sources.
- A clock setting.

## 6. CrossInk import (P2-6..8)

**At the switch (insurance, no firmware).** Add this to the NEWT-46 switch
checklist, right after the whole-card private backup that includes
`/.crosspoint` ([BRINGUP](BRINGUP.md)) and before the card changes. A host
script (a later item) builds `crossink-manifest.tsv` from that backup. For
each EPUB it lists:

- path, size and SHA-256 of the file bytes (equal to Newt's book digest,
  `hashCompleteSource`);
- both CrossInk cache directory names (`fnv64` and the older `std::hash`);
- each stats file found, with its SHA-256.

Store the manifest beside the backup and copy it to `/Newt/Import/`.

**Import** reads `/.crosspoint` read-only and never repairs or renames
anything on it.

- **Identity.** One frozen record per CrossInk *source* (cache directory),
  `ci<13 hex of SHA-256(dir name)>`. It holds:
  - the directory name;
  - the CrossInk bytes verbatim, with their length, version and which
    candidate file was used;
  - the Newt digest bound **from the switch-time manifest**. Without a
    manifest, the binding uses current files and is marked "unverified".

  Two paths with identical bytes stay two sources; nothing is discarded.
  `si<digest>` is the rebuildable per-book sum of its sources.
  `newt_stats_ci` holds the global file verbatim (159 bytes fit). Import is
  complete only when `newt_import` records the manifest hash with a status
  for every source.
- **Recovery precedence (P2-8).** For each source, read the primary, then the
  `.tmp` candidate (only a complete 73-byte v5 file), then `.bak`, then the
  older versions (`stats_v4.bin`, `stats.bin`). This is CrossInk's own load
  order, re-checked at `9914146` (`BookReadingStats.cpp:77-131`); the global
  file is the primary, then `.bak`, and a newer global stops the search
  (`GlobalReadingStats.cpp:286-311`). Accept the first candidate that
  passes exact size, version byte and range checks. Record which one was
  used, or "unrecoverable".
- **Totals.** Screens show native + imported. Library totals add CrossInk's
  *global* totals, never the sum of its per-book files.
- **Streaks (P2-7).** Current and longest streak are computed over the union
  of Newt's qualifying days (60 s) and CrossInk's bitmap days. CrossInk days
  count as qualifying even though its rule was 10 s. Both use 2000-01-01 day
  numbers, so streaks join across the switch. Longest =
  max(union longest, CrossInk's stored `longestReadingStreak`), which is valid
  because the stored value is a true lower bound beyond the 730-day bitmap.
- **Days read (#238, docs/090).** Time totals and Longest day are
  measured-only; "Read on" counts every day any source recorded as read,
  CrossInk's bitmap days included.
- `synced_stats/` (other devices): not imported in this design.

**As built (#104).** Source re-checked at CrossInk `9914146` (MIT); no code
copied.

- **Corrections to the design above.** CrossInk tries `.tmp` before `.bak`
  and uses the first candidate it can *open*, renaming a recovery file into
  place (`BookReadingStats.cpp:77-108`). Newt uses the first candidate that is
  *valid*, and never renames. A per-book file must be exactly one layout's
  size (CrossInk also reads a longer v5 file). CrossInk's own legacy cache
  name is `std::hash` of the path (`Epub.cpp:307-309`, `Xtc.h:31`); the
  current one is `epub_<fnv1a64(path)>` (`Epub.cpp:494-497`,
  `ZipFile.h:93-100`).
- **Records** (all in `/Newt/Stats/`, the statistics A/B format, version 1,
  FNV at the end):
  - `newt_stats_ci`, `NWCG`, 180 B: status (imported, unrecoverable, newer),
    candidate (primary or `.bak`), size and the 159 global bytes verbatim.
    A newer global keeps no bytes and adds nothing.
  - `ci<13 hex>`, `NWCB`, 160 B: the source name, status, candidate, size,
    the per-book bytes verbatim, and its binding (none or unverified) with
    the bound Newt digest.
  - `newt_import`, `NWCI`, 56 B: written last; it holds the number of
    sources and a zero manifest hash.
- **Binding without a manifest.** The import walks the card's EPUBs (not dot
  folders; under `/Newt` only `Books`, depth 6), names each path's CrossInk
  source and, when that folder exists, binds it to the SHA-256 of the file
  (Newt's book digest), marked unverified. Other sources stay unbound. The
  manifest builder and `si<digest>` are not built: nothing reads per-book
  history yet.
- **When it runs.** At Home and on the Settings screens, after the boot's
  statistics condition is seeded, as a quiet duty that input cancels before
  any write. Any other result settles it for this boot in a spare save-notice
  bit (no RAM, not kept across a wake). A completed result (done, or nothing
  to import) also sets a second such bit; an open book's CrossInk sum waits
  for it (docs/090 section 5). Each boot rechecks `newt_import` and
  `newt_stats_ci`; once both exist nothing on `/.crosspoint` is opened. A card
  without CrossInk statistics writes nothing.
- **Resumable.** A record, once frozen, is never read from the card again,
  and a damaged one is set aside as `.bad` and frozen again. Only a missing
  name (`ENOENT`, `ENOTDIR`) counts as absent; any other read error stops the
  run without `newt_import`, so the next boot retries it. `newt_import` is
  written only after every source is frozen.
- **Counting.** Summaries add `newt_stats_ci` at read time; no ledger holds
  it, so it counts once however often the ledger folds. Today and the last 7
  days count Newt's reading only.
- **Known limits.** CrossInk sessions are book opens, and its finished books
  are manual marks. Reading done in CrossInk after the import completes is
  not imported. Two source names whose SHA-256 share their first 52 bits
  would share one record; this is not handled.
- **Tests.** `tests/crossink_stats_test.cpp` (layouts, records),
  `tests/reading_stats_test.cpp` (union streaks, totals once),
  `tests/crossink_import_test.py` (synthetic heterogeneous card; every power
  cut, failed sync and cancellation resumes to the clean records; read-only
  card; eight negative controls) and the Home duty in
  `tests/reading_stats_firmware_test.py`.

## 7. Test plan

Host regressions, using several heterogeneous fixture books:

- **Tracker:**
  - closed versus expired intervals; idle snapshot credits nothing;
  - a menu pauses the session; Sleep and expiry end it;
  - 2 s dwell, `millis` wrap, saturation;
  - negative control: the old cap.
- **Codecs:** exact layouts, every limit and saturation; each corrupt cause
  versus unsupported versus unavailable.
- **Crash table:** a fault backend cuts power at every write, `fsync`,
  read-back, rename and copy of a seed, fold, handover, month change and all
  three repairs, and just after each acknowledgement, then cold-boots with all
  RAM and RTC discarded. The backend models what the card holds (section 9),
  and the cold boot keeps all or none of the unsynced work; the repairs are
  also swept with every prefix, all-but-one and single-one choice. Every row
  of the table is a case.
  - Oracle (the invariant): library totals ≤ seed sum + Σ positive steps of
    each book's acknowledged counters after the seed, with equality when no
    write fails.
  - Also: retries after each unconfirmed write; progress reset and
    power-on checkpoint rollback on the leased and an unleased book; reading
    while unseeded; a checkpoint landing mid-seed; a finished book leased
    again; `NWTB` repair cut at every step.
  - Also: an acknowledged write survives a cold boot; a record file is never
    left partly created; a rewritten damaged record leaves its `.bad`
    evidence.
  - Transient faults: every `fsync` of each step fails once or twice while
    later calls work and reads see the buffered bytes; the step is retried
    before or after a cold boot that keeps any choice of the unsynced work.
    An acknowledged step must survive the boot and the retry must reach the
    clean ledger. Named regression (r3 P2-1): an `fsync` fault in a finished
    book's handover, a cold boot and a revisit count one finished book.
  - Negative controls: a two-record fold (r1) fails same-book retry; a folding
    seed racing a lease (r2) double-counts the repaired-`NWTB` case; deleting
    the seed-before-lease guard fails; removing any one of the backend's six
    `fsync` calls fails a case; a repair before the cancellation check fails;
    a read-back accepted after a failed `fsync` fails r3 P2-1; skipping an
    identical record fails the transient matrix.
- **Clock:** trusted, OSF, implausible and out of range; zone and DST;
  midnight, month and year; jumps back and forward with archive load.
  Streaks derived across months. Named regressions (r3 P2-3): days read in
  March with the clock then set back to February keep the longest streak; a
  clock set forward to November and corrected back keeps it too, while today,
  the week and the current streak follow the corrected clock. Negative
  controls: a scan that starts at today or stops after twelve months fails.
- **Wiring:** the real idle, Back, Sleep, switch and handover paths call the
  fold. Negative control: removing a call fails. Input already pending lets
  no effect reach the card in a seed, fold, handover, month change, any
  repair or the idle save's lease (counted in the fault backend).
- **Session and end:** an orientation change and a progress-bar setting each
  keep one session; a reader failure keeps it; a one-page book and a restore
  onto the last page reach the end. Named regression (r3 P2-2), through the
  real UI handler: 31 s, a failed turn, the menu, recovery and more reading
  stay one session. Negative controls: a rebuilt reader or a verified save
  dropping the session, a failure or a failed reader's menu ending it, and the
  end judged without the shown page.
- **Cost:** no reader-path allocation; fold stack measured; resource record.
- **Pixels:** both screens: valid, counting (unseeded), undated, unavailable
  and newer.

**Device checks for the Lead** (bench X3, after implementation):

1. USB power:
   - compare stats with a stopwatch, and check the DEV `STATS` rows (kind,
     delta, fold age, carry, results, ms);
   - a page left untouched for more than 5 minutes earns nothing;
   - a menu visit stays one session.
2. Book switch: the handover rows appear, and both books' totals stay
   consistent.
3. Fold and handover latency; minimum free heap while reading against the
   50,468 B floor; seed duration with the real library.
4. The day matches the DS3231 date in the zone.
5. USB sleep and wake, battery sleep and wake, and cold power-on as separate
   checks, with reset reasons.
6. An agreed safe power cut: the loss stays within the bounds.

## 8. Decided (owner, 2026-10-25)

1. An interval longer than 300 s earns nothing; the threshold is fixed with
   no setting.
2. Folds run every 5 minutes of reading and at every Back, Sleep and switch;
   Sleep paints the sleep image first, then saves progress and folds statistics
   before preparing the resume record. Back folds before Home.
3. The built-in sample is excluded.
4. Finished = reaching the last page, set automatically, or Reading menu →
   Mark as finished (#105), which uses the reserved `manualFinished` and
   `manualFinishDay`: no format change. A finished book reads 100% on Home
   and the sleep screen; the book's last page reads 100% in the reader.
5. The library page is Home → Settings → Reading statistics.
6. A damaged record is kept as `.bad` evidence and rebuilt. Totals are kept,
   except that rebuilding `NWTS` itself resets totals to the books' current
   counters.
7. No per-session log in v1.
8. A streak day needs 60 s of reading.
9. Earlier Newt counters are counted once, as undated.
10. Statistics never write the RTC.

## 9. As built (v1)

**Differences from this design.**

- **Seed.** One cancellable call reads every `nb*` progress file and every
  `sb*` facts file, then writes `NWTS`. It is not split into one file per
  duty step. Input cancels it before the write; the next quiet duty starts
  again. Only `nb*` files are summed: legacy progress kept only in NVS is not
  seeded (undercount).
- **Where the seed runs.** At Home and on the Settings screens while the
  boot's statistics condition is unknown or unseeded, and at reader idle while
  unseeded. An open Reading statistics page repaints from "Counting…" when it
  lands.
- **Condition.** Seeded, unseeded, newer or unavailable is kept in spare bits
  of the existing save notice: no new RAM, and not kept across a wake. After a
  wake it is unknown, so Home checks it again.
- **Idle fold.** Due once per open (normally the handover) and then every
  5 minutes, without first comparing counters with the watermark. Every save
  writes and syncs, even an unchanged record, so a later retry also makes
  durable a value an earlier failed `fsync` left only in the card's cache.
- **Lease before save.** Both actual progress saves (Back/Sleep/switch and
  idle) call the lease after their guard and before they read the candidate,
  so the handover lands before the book's counters change.
- **Cancellation.** Every statistics effect (create, write, sync, rename,
  copy, repair) takes a `StatsEffects` token. Only
  `StatsStore::beginEffects` issues it, and it refuses when input is pending,
  so each operation checks input once, after its reads and before its first
  effect. Once the first effect starts, the operation finishes (a cut
  mid-commit is a crash-table case). The idle save's lease passes the save's
  own admission check; a cancelled lease defers the save, so the lease still
  lands before the book's counters change.
- **Session across rebuilds.** The session's credit lives in the progress
  value (`sessionMilliseconds`, in former padding; never stored). Every
  same-book rebuild seeds its tracker from that value, and a verified save
  keeps it, so only Back, Sleep and a switch end a session. In the reader UI
  only `handleReaderUi` ends one, and only for an exit (Home or Sleep); an
  unopened or failed reader, its menu and its recovery only pause.
- **End.** Whether the end is reached is judged in one place, from the shown
  page, which every path passes (open, restore, turn, jump and recovery).
- **Streaks.** `NWTS` keeps the dated range (oldest and newest month any fold
  dated), so the scan finds the newest retained month without opening every
  possible month. It runs from the later of today and the newest month down to
  the oldest. The longest streak counts every retained day, even days after
  today when the clock was set back. Today, the last 7 days and the current
  streak count only days up to today; the current streak stops at its first
  gap.
- **Clock.** A fold is dated only if both the UTC and the local year are in
  2026..2099. When an SD reader opens, before any deferred source verification,
  the live tracker anchors its next local midnight to the trusted UTC snapshot and monotonic millis. The tracker
  records the counters before the first turn at/after midnight, adding only
  the credited pre-midnight part of its interval. The next normal fold
  commits that prefix, then the remaining counters; the old month is archived
  through the existing path. Each commit advances the same durable watermark,
  so retries and power loss between the two cannot replay reading. The mark
  is capped at acknowledged progress, never ahead of a failed save, and stays
  until both saved time and page counters have reached its boundary. An idle
  fold of an older save, including after an unconfirmed save, cannot consume
  the unsaved part of the mark. A delayed
  handover keeps a crossed mark until the current book can fold. A fold
  before the midnight-spanning interval closes dates its already closed
  counters on the old day and retains the armed mark for that interval’s
  eventual credited close; it never guesses future reading. One mark
  is retained. Each fold compares its UTC reading with the monotonic
  projection from the clock anchor (stored as UTC/millis at midnight in the
  existing 32-byte mark). The 10-second tolerance allows about 50 ppm drift
  over its at most 49-hour lifetime plus one second of quantization. A larger
  clock jump, mark loss, a second midnight, or a changed zone uses fold-time
  dating; an invalid or stopped clock stays undated. It is not in the progress codec, sleep record or RTC memory.
- **Reader menu dates.** They show stored facts (the lease or `NWTB`), so the
  current visit's "Last read" appears after its next fold. A corrupt `NWTB`
  shows "Unknown" here; the next handover repairs it. The menu's former
  "Page dwell 2s / session 60s" line was removed for space.
- **Unconfirmed.** A statistics write whose `fsync` failed twice returns its
  own result. Callers treat it as not done: the handover and the fold are
  retried, and the screens show the stored value.
- **Unavailable at Home** is not retried until the next fold or boot, so a
  missing card is not polled.

**Durability assumptions.** The fault backend in the tests models the card
as follows; the backend relies on nothing more.

- A write, truncate or create is durable only after a successful `fsync` of
  that file, which also makes its directory entry durable. Until then, any
  subset of those operations may survive a power cut. FAT writes back in
  order and flushes at close; the model is deliberately stricter, so the
  backend never depends on either.
- Rename and unlink are atomic and durable when they return (FAT syncs the
  volume in both).
- The card completes each sector write before the next; a cut write may be
  torn. A torn record copy fails its CRC and reads as absent.
- Directory creation is durable when it returns.

**Known undercounts** (each loses counts; none counts twice except the last).

- Reading while unseeded is in totals but undated.
- A book finished while unseeded may not count as finished.
- Rebuilding `NWTS` (after corruption) drops retained history: totals become
  the sum of current progress counters.
- Admissible legacy NVS progress is preserved on SD before seeding; an
  unmigratable record is retained without blocking the seed.
- A `NWTB` deleted by hand can let its book count as finished twice. Reading
  totals are unaffected.

## 10. Sleep screen (#202)

Settings → Sleep screen can show the last 7 days and the time of day the owner
reads most on the sleep screen. Both are Off by default; while they are Off,
the sleep paint and Home read nothing for them.

**The Stats page still computes streaks fresh** from stored days on every
paint (section 9, Streaks). Only the sleep screen uses the cache below, so a
sleep never scans months.

**`NWTK` v1, `newt_stats_sk`, 28 B** (library): the streak cache, its own
record so `NWTS` stays v1 for every Newt that reads it (an older build that
rolls back keeps its statistics). Header and FNV as `NWTS`.

| Off | Field | Type | Limit / rule |
| ---: | --- | --- | --- |
| 12 | `day` | u16 | The build day `T`; `0xFFFF` none (all fields zero). |
| 14 | `run` | u16 | The run of reading days ending the day before `T`. |
| 16 | `before` | u32 | `NWTS` seconds credited before `T`: total less stored days from `T` on. |
| 20 | `days` | u8 | Bit i, day `T - i` (i 0..7) was a reading day. |
| 21 | reserved | 3 B | 0 |

A record whose `days` and `run` disagree (the days before `T` must match the
run, up to 7) is corrupt and set aside as `.bad`.

**Build.** A reading day is 60 s of Newt reading or a CrossInk day, so
today's imported reading is kept. Home idle writes the cache once per boot
while Last 7 days is On, after the CrossInk import, from a full summary dated
today; it skips the write when the cache already holds that value. A summary
that could not read a month or the CrossInk history writes nothing and shows
no count; the next boot or wake tries again. A clock set back into an archived
month writes nothing either (`before` would be unprovable). The CrossInk
import clears the cache before it commits.

**What one sleep reads.** The previous month's archive is read at most once
per sleep and shared by both extras: every day while When you read is On,
else in a month's first 6 days. It is read only when archived (the ledger has
moved past it and holds it in range). Record bytes per sleep:

| Extras On | Card opens | Bytes |
| --- | --- | --- |
| None, or Book progress alone | 0 | 0 |
| When you read alone | `NWTS`, + last month when archived | 180, up to 284 |
| Last 7 days alone | `NWTS`, `NWTK`, + last month in a month's first 6 days | 208, up to 312 |
| Last 7 days and When you read | `NWTS`, `NWTK`, + last month when archived | 208, up to 312 |

A month with no file holds nothing. A clock behind the ledger (today's month
archived, after a clock set back or a zone change) shows no dots, count or
mark: today's month is unread. Today's seconds add the open book's
reading the exit fold has not saved yet (the sleep paint runs first), from
the live session. For the same reason, Book progress after Sleep from the
reader takes the chapter and percent from the live page, as the reader shows
them; other sleeps show Home's.

- **Dots:** each of the last 7 days is read with 60 s of Newt reading that
  day, or when the cache marks it read.
- **Count:** only when all 7 are read, `T` lies within the last 7 days, and
  the ledger's seconds before `T` still equal `before`: the run plus the days
  from `T` to today. A credit to a day before `T` (a clock set back), an
  unread archive it needs, a missing or out-of-window cache, or an untrusted
  clock shows no number, never a wrong one.
- **Time of day:** the most-read bucket over this month and last month (ties
  go to the earlier one), counting today's unsaved seconds in the current
  hour's bucket; nothing under 30 minutes over both. A ledger still on last
  month counts as last month, with this month empty; an older ledger counts
  only today's unsaved seconds. A last month that exists but cannot be read
  shows no mark. Night inverts the whole sleep screen.
