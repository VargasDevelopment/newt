# Card swap, missing card and a labelled progress estimate

## Why

Three reliability gaps remained after durable reading state moved to the card
(doc 051):

- **Card swap.** A card removed and reinserted while a book was held left a
  stale file handle. The next Read failed with "This book could not open…"
  until a visit to Books released the handle.
- **Missing card.** A boot without the card could not read the selection, so
  Read silently opened the built-in sample instead of the owner's book. Nothing
  on Home said the card was missing.
- **No percent without counts.** Until the reader has counted a whole book,
  Home and the reader footer showed no percent at all.

The owner chose (2026-10-05):

- Keep the sample available, show "No SD card" prominently and make opening
  the sample explicit. Read must not substitute it, and Home should tell "no
  book selected" apart from "the selected book is on the missing card".
- Show a labelled chapter-size estimate ("~42%"), weighted by reading-order
  chapter text and excluding images and fonts. Count progress within the
  current chapter too, with no extra full-book pass.
- Fix the card swap by closing the stale file and retrying once, preserving the
  saved position.

## What changed

### Card swap (`32fa599`)

Reinserting a card remounts FATFS and invalidates every handle opened before,
while new opens work. `fstat` keeps reporting the old size, so only a read
notices.

- When Read prepares a held book, `ensureSource` reads one byte from it
  (`sourceLive`).
- If that read fails, the stale file is closed and the same filename is opened
  once. Its receipt or full digest is verified again before parsing.
- A cancelled read is not taken for a swap, and a book still being chosen
  (`titleDeferred_`) keeps its handle.
- A failed reopen leaves no held file, so the next Read opens directly. There is
  no loop.
- No progress, selection or resume record is written. The saved position is
  untouched.
- The probe runs only when Read prepares a source, never on page turns.

### Missing card (`3234df5`, `8893d0b`)

**Detection.** `SdLibrary::present()` says the card is missing when it will not
mount, or when a mounted volume answers a `stat` of the Books folder with `EIO`,
`ENODEV` or `ENXIO`. A missing file or folder is not a missing card. At boot,
an unreadable selection together with unavailable reading state means the card
is missing.

**Home** (`homeText`, flags from `homeCardFlags`):

| State | Title | Progress line | Hint |
|---|---|---|---|
| Card missing, book selected | No SD card | Your book is on it | Insert the card, then press Read. |
| Card missing, nothing selected | No SD card | Sample is in Books | Insert the card, then press Read. |
| Sample chosen in Books | The Small Lantern | the sample's progress | No SD card. Read opens the sample. |

The two "No SD card" states draw no cover and no bar fill. The bar's empty
track remains.

**Read** never substitutes the sample for a book on a missing card. It shows "No
SD card. Insert it, then press Read." What it does next depends on the card:

- **Card still missing:** the open stops there.
- **Card back:** Read restores the durable selection as boot would and opens it.
  This also happens after the sample was chosen while the card was missing.
  - Home is reloaded first, so a failed open shows that book, not the sample.
  - A pending checkpoint keeps the source it belongs to; the card's book is not
    restored under it.
  - A book still held from a wake without the card is compared with the
    card's selection first (see "Recovery after a wake").

**Books** shows "No SD card", "Insert the card to see your books." and "Open
reads the built-in sample instead."

- Open is the only way to choose the sample. Home then takes the sample's title.
- A pending checkpoint refuses the sample and says "Position kept. Use Read on
  Home to resume."
- With the card back and no checkpoint pending, Books restores the card's
  selection, or compares a held book with it, and Home reloads.

**Wake.** A wake straight into the sample resumes the sample, with or without
the card. A Home wake that still holds the SD book in RTC memory names it
("Your book is on it"). That book saves again once Read or Books has
compared it with the returned card's selection, or once a book is opened from
Books.

**Sleep image.** The card state is derived again before the sleep image is
drawn. It no longer reuses Home's last paint.

Nothing is written while the card is missing.

### Progress estimate (`0faad73`, `8893d0b`)

Without exact counts (no complete semantic index for this book), the reader
shows an estimate:

    percent = (before + current × min(character, chapterChars) / chapterChars) × 100 / total

The result is capped at 100.

- **Weights.** `before`, `current` and `total` are declared uncompressed
  text-member bytes of reading-order items.
  - A repeated item counts each time.
  - Images, fonts and other non-text items weigh nothing.
  - The open already binds these inputs (`ReaderBookProgressInputs`), so no
    extra pass reads the book.
- **Within the chapter.** `chapterChars` is the current chapter's laid-out
  character count, so progress moves within the chapter too.
- **Exact counts win.** When exact counts exist they replace the estimate
  everywhere.
- **Labels.**
  - The footer shows a plain "N%": readers accept an estimate without a mark
    (owner, #75).
  - Home matches it since NEWT-24 (#76): "Chapter N · N% read", or "N% read"
    when the line is wider than its column. Home no longer marks an estimate
    with "~", and an estimate alone no longer repaints Home.
- **Home after a restart.**
  - Leaving the reader (to Home, Books or sleep) saves the chapter weights as
    an 80-byte record, `pe-<full D>.ntpe`, in the book's cache.
  - It goes through the session's existing storage port. FATFS allows three
    open files, and this needs no extra one.
  - It is written only when the weights change, so most exits write nothing.
  - Home uses the record only when its chapter equals the saved checkpoint's
    chapter and the saved offset fits inside it. Otherwise progress stays
    unavailable.
- **Record format.** Little-endian:
  - `NTPE0001`, the semantic policy version, spine, chapter characters and a
    zero word;
  - the full digest D;
  - the `before`, `current` and `total` weights.

  Storage adds its NTCC CRC footer. A wrong magic, policy, digest, size or
  reserved word, or weights that fail `valid()`, load nothing.

### Recovery after a wake (`6699538`, `10b3241`, `656155b`, `48175d4`, `04d9b69`)

An independent review (Ribbit, 2026-10-06) reproduced two recovery defects on
`246f805` with host probes under ASan/UBSan. The existing suites passed
because they never combined these states.

**Cardless wake, then reinsertion.**

- A wake that kept its SD book in RTC memory compared that book with the card's
  selection at once. Without the card the selection could not be read, so the
  comparison failed and set `selectionSaveFailed_`.
- The returned card reopened the book, but every checkpoint was refused, so
  later reading progress was not saved until a book was opened again from
  Books.
- Now the book stays unconfirmed (`selectionUnconfirmed_`) until the card's
  selection names it. `checkSelection` compares them at the wake, then again
  whenever Read (`prepareSource`) opens the book's file from a card, or Books
  finds a card after a missing one (`resumeAfterCard`, nothing pending).
- A selection naming the book restores saving.
- Another card's selection, a card with none, or a failed read refuses
  ("Book selection not verified. Reading position is kept.") but stays
  unconfirmed. When the book's own card returns, the next comparison restores
  saving, and Read replaces the refusal message.
  - The same holds when the wrong card was already in at the wake.
  - The fresh adversarial review of `6699538` found that a refusal by the
    first card to answer was final. `10b3241` makes it repeatable.
- A card change that Read or Books notices while a book is held starts the
  comparison again. When Read or Books finds the card missing, or a stale
  handle is closed to reopen the book's file by name, the book is unconfirmed
  until the next card's selection names it. An unnoticed swap is not compared
  (see Limitations).
  - The fresh adversarial review of `10b3241` found that a card swapped after
    a match was not compared again. A card whose older slot named the book took
    its place and a resume intent, so a restart resumed the held book instead
    of that card's newest selection. No selection was rewritten. `656155b`
    compares again once the change is noticed.
- A refusal can never bless a book. For a held book, only an exact match of
  filename and digest with the card's selection restores saving. Opening a
  book from Books writes a new selection instead, and comparisons write
  nothing.
- A selection that could not be written when the book was opened stays
  refused until a book is opened again from Books, or until a wake, or a card
  change that Read or Books notices, finds the card's selection naming it.
- A failed open from Books rolls back every selection flag, so a held book
  that was refused or not yet compared is still compared again. A cleared or
  refused message is derived again from the restored flags (`48175d4`): the
  open's own Read can clear a refusal before the open fails, and a refusal
  still shown after the own card restored saving is cleared (`04d9b69` tests
  it).
- Until a comparison succeeds nothing saves, and a sleep takes the path for an
  unverified selection.

**Sample sleep, then the card inserted before the wake.**

- Boot restored the card's durable selection before it validated the sample's
  RTC record. That record then failed its book identity, and the wake went
  Home.
- Now a valid wake into the sample reads the card's selection only to learn
  whether the card is there, then returns. The selection is neither activated
  nor allowed to block the sample, which resumes at its page.
- After that wake:
  - Home shows the sample, and Read opens it. The card was never found
    missing, so Read does not replace the sample.
  - Books lists the card's books. Opening one selects it.
  - A restart, or a sleep and wake from Home, restores the card's selection at
    its saved place.
  - Neither book's place is written.

### A card pulled after a card boot (`9c7e008`, `6e31612`)

The owner's checks on the X3 found this on image `555547a` (part E,
2026-10-06). The X3 booted with the card, the card was pulled on Home, and the
built-in sample was chosen in Books. It did not open; the device log said
"Newt sample unavailable: progress storage unreadable or ambiguous".

- **Cause.** The card's mount is kept for the rest of the boot, so a card
  pulled after this boot mounted it fails reads (`EIO`, `ENODEV` or `ENXIO`;
  which one the X3 gives was not observed). It does not read as nothing, as a
  card missing since boot does. Books had found the card missing. Opening the
  sample then ran the retry for a missing card (`resumeAfterCard`, through
  `prepareSource`). The retry treated the failed read as a card that
  answered:
  - it marked the card's selection unreadable, which blocked it;
  - it cleared the missing-card and chosen-sample flags;
  - the sample's progress load then refused, because the selection was
    blocked. It did not read the card.

  The host fixture had modelled only a card missing since boot.
- **Fix.** `cardSilent` names "no card answered". The reading state must be
  unavailable, and the read either found nothing, or failed while the
  presence check (`cardAbsent`, a `stat` of the Books folder) finds no card.
  An empty read from a present card is not silent. `resumeAfterCard` and
  `checkSelection` use it.
  - A present card whose reads fail still refuses, as before.
  - `restoreOnBoot` keeps its own Empty-only test for the durable selection,
    because a card missing at boot was never mounted and reads as nothing.
    Its wake branch reaches `cardSilent` through `checkSelection`.
- **After the fix** (host tests), once the booted card is pulled:
  - the chosen sample opens from Books;
  - Home names the sample with its missing-card hint, and Read keeps it;
  - the returned card restores its book with the selection verified;
  - nothing is written while the card is out.
- **Cost.** +112 bytes of flash in every variant, recorded in `0184bbb` (see
  Build evidence).
- **On the device.** The owner's part E check passed on image `0184bbb` (see
  Device evidence).

## Host evidence

All of these run in `python3 tools/check.py`:

- `sd_selection_reliability_test`. It runs on the verbatim selection fixture,
  with the authored EPUB and isolated cache files:
  - card swap: reinserted, removed and cancelled;
  - missing card across a cold boot, a Home wake, a Home wake still holding the
    book and a wake into the sample;
  - Books and Read with the card back, including after the sample was chosen;
  - a pending checkpoint with the card back;
  - Home's estimate after a restart, with one publish per changed record
    (counted at the storage rename seam) and none when unchanged;
  - `cardReturnSaves`: 24 combinations of a wake that keeps the SD book
    through the real sleep path:
    - the card's selection: the same, another, or unreadable;
    - the card missing or present at the wake;
    - a Home or reader wake;
    - the card's return seen by Read or by Books.

    Each case checks that Read waits without the card, then reinserts it,
    reopens the book at its place and turns a page. Only a matching selection
    checkpoints that newer page, and a restart reopens it there. A refused
    selection writes nothing and keeps the card's own selection and place.
  - `wrongCardFirst`: 18 combinations in which another card answers first:
    - it selects another book, has no selection, or fails its read;
    - it answers at the wake, to Read, or to Books;
    - the book's own card then returns to Read, or to Books after a missing
      card.

    The first card refuses; the wake and Read name the refusal. The own card
    restores saving and Read clears the message. A newer page saves, and no
    selection is rewritten.
  - `heldBookRollback`: a failed open from Books after a held book was
    refused by another card, or left unconfirmed by a card whose reading
    state stays unreadable, or refused and then restored by its own card while
    the refusal is still shown. The held book keeps whether it saves, and the
    shown message follows it: the refusal, nothing, or the stale refusal
    cleared. Its own card still restores saving. A book whose selection write
    failed at open, and an unreadable selection at boot, are each kept through
    a failed open too.
  - `swapAfterMatch`: 12 combinations. Read holds a book its card names; the
    card is then changed:
    - the next card has the same selection, selects only other books, or has
      an older slot naming the book;
    - in between, Read finds the card missing, or Books with the card in
      closes the book's file and Read then finds the card missing, or Books
      finds it missing; or the swap goes unnoticed and only the held handle
      fails.

    Read compares the next card's newest selection and keeps the place. Only
    the book's own card checkpoints the newer page, no selection is rewritten,
    and a restart follows the card's newest selection.
  - `sampleWake`: sample sleep with the card missing, returned, or returned
    with an unreadable selection. The sample resumes at its page, Home names
    the card only while it is missing, and Books lists the card's book. Nothing
    is written, and a restart reopens the card's book at its own place.
    - Each case runs twice: with the card missing since boot, and with it
      pulled after this boot mounted it (`9c7e008`).
  - Each recovery rule was removed in turn (15 mutations): every removal fails
    at least one of these five regressions.
  - **A card pulled after a card boot** (`9c7e008`, `6e31612`). The fixture
    now models a mounted card. Once this boot has read the card, a pulled
    card fails reads instead of reading as nothing. A present card can also
    fail its reads with `EIO`. The tests' `reboot()` drops the mount, and
    `card()` pulls or inserts the card mid-session.
    - `noCard` pulls the card after it was read. Books opens the chosen
      sample, Home names it, and Read keeps it. The returned card restores its
      book with the selection verified. Nothing is written.
    - In `noCard`, a card that is back but fails its reads with `EIO` blocks
      its selection instead of reading as no card. Home drops the
      missing-card hint, and nothing is written.
    - `wrongCardFirst`'s unreadable first card now fails its reads with
      `EIO`, so its reading state is unavailable too. It still refuses.
    - Each test checks that this boot mounted the card it then lost.
  - **Reproduced first.** The pulled-card checks fail on `4cc0eb8`'s
    unchanged firmware:
    - `noCard`: "Books opens the sample after the card is pulled
      mid-session";
    - `sampleWake`: "card pulled after boot, then missing at the sample wake",
      where the chosen sample does not open from Books.
  - **Mutations.** 11 mutants of the new rule were run against the six
    regressions, and 8 are caught. Three survive:
    - Using the Books catalog instead of the presence check (`no-catalog`:
      `!catalog()` in place of `cardAbsent()`). The fixture has no catalog
      while the card is out, so the two agree there (see Limitations).
    - `checkSelection` keeping the old test (`check-old-test`). This is
      untested. They differ when `checkSelection`'s read fails and the
      presence check also finds no card:
      - a card that mounts at a wake but fails both the read and the
        presence check;
      - an open that skips the book read (cancelled or title-deferred);
      - a pull between a good book read and the selection read.

      The old test calls that a refusal; the new one calls the card missing,
      which changes Home's hint and Read's message. Saving stays off either
      way.
    - Dropping the "not loaded" term (`no-loaded-term`). This is equivalent
      in firmware: any failed slot read makes the load fail, and each
      transaction resets the unavailable flag. So a loaded record never comes
      with unavailable reading state.
- `progress_estimate_record_test`:
  - round trip, an unchanged save and chapter binding;
  - nine damaged records rejected;
  - failed writes keep the published record.
- `frosted_home_test`:
  - Home's exact words for every progress and card state, including both
    shortened forms;
  - card bits for each combination;
  - Home pixels unchanged by the `homeText` extraction.
- `home_refresh_test`: an estimate or card flag change repaints Home.
- `reader_home_overview_test`: an estimate still allows the optional exact
  overview; exact counts clear the "~".
- `library_preview_test` (`7803929`): after reading the authored Library
  EPUB, Home's percent is an estimate; the built-in sample's is exact.
- Existing reader, sample, canonical-metadata, boundary, retention and
  integration tests now assert "estimated" or "exact" rather than "unknown".

## Build evidence

Four clean `--health-report` builds of `8893d0b`. Flash grows by the measured
cost of this layer, recorded in `code-health-baseline.json` with these
artifacts. Static RAM is unchanged in every variant.

| Variant | Flash before | Flash after | Growth |
|---|---|---|---|
| default | 1,306,496 | 1,311,328 | +4,832 |
| dev-usb | 1,326,208 | 1,331,088 | +4,880 |
| regional | 1,310,736 | 1,315,552 | +4,816 |
| regional dev-usb | 1,330,416 | 1,335,312 | +4,896 |

The application slot holds 7,798,784 bytes; the policy's usable limit is
7,016,448 after its 782,336-byte reserve.

**Stack.** Every selected frame is within its ceiling. These are per-frame
numbers; they do not prove whole-task stack safety.

- `loadHomeProjection`: 1,024 bytes against 1,072 at base, in all variants.
  - Home's lookup (`loadSdHomeProgress`) is forced inline.
  - Its estimate fallback (`noteSavedEstimate`, 128) and the route key
    (`cacheRoute`, 128) are out of line. The route key's frame is gone
    before the metadata read.
  - Earlier versions of this layer measured 1,120 and 1,088, over the
    ceiling, and were not kept.
  - Down to its storage calls, the estimate path holds 1,392 bytes:
    1,024, then `noteSavedEstimate` 128, then `ProgressEstimateRecord::load`
    240. The counts path holds 1,024 plus `loadProjection` (224) and its
    record helpers, 48 bytes less than at base. So a Home lookup that falls
    back to the estimate runs at most 96 bytes deeper than base's lookup.
- `setup`: 384 bytes (dev-usb variants 400) against 672 (688) at base.
  - The durable selection load is now shared with `resumeAfterCard`, so it
    is out of line (336).
  - The boot path through that load is 48 bytes deeper: 720 (736) against
    672 (688). Every other call from `setup` starts 288 bytes shallower.
- `renderFrostedHome` (not a selected frame): 496 bytes against 480 at base.
  The battery text's buffer ends before Home's words, which reuse its stack.
  With both buffers live it measured 624.
- The protected audit lowers each ceiling measured below it to the measured
  size and guards every new out-of-line frame of this layer at its measured
  size.

### Recovery fixes (round 5)

Four clean `--health-report` builds of `04d9b69` stop only at the round-4
flash ceilings. `04d9b69` changes only firmware comments after `48175d4` and
measures the same. The record and protected audit (`555547a`) follow them,
and four clean builds pass against it.

| Variant | Ceiling (round 4) | `10b3241` | `656155b` | `48175d4`, `04d9b69` | Growth |
|---|---|---|---|---|---|
| default | 1,311,328 | 1,311,600 | 1,311,680 | 1,311,728 | +400 |
| dev-usb | 1,331,088 | 1,331,360 | 1,331,440 | 1,331,488 | +400 |
| regional | 1,315,552 | 1,315,840 | 1,315,904 | 1,315,968 | +416 |
| regional dev-usb | 1,335,312 | 1,335,584 | 1,335,664 | 1,335,712 | +400 |

- `6699538` measured the same flash as `10b3241` (build not kept).
- The comparison after a card change and the refusal literal add 64–80 bytes
  in `656155b`. Restoring the shown message on a failed open adds 48–64 in
  `48175d4`.
- Static RAM and RTC memory are unchanged in every variant.

**Stack.**

- `dispatchAction` (dev-usb variants) measures 992 bytes, its ceiling.
  - `openSdSelection`, with `openBook`, is inlined into it.
  - At `6699538` one more saved flag in `openBook`'s rollback snapshot
    crossed a 16-byte step, and it measured 1,008, over its ceiling. That
    build was not kept.
  - `10b3241` packs the six flags into one byte, which brings it to 976.
  - `48175d4` derives the shown message again in the rollback, which
    spends those 16 bytes again. The ceiling is not raised.
- The new out-of-line comparison, `checkSelection`, measures 192 bytes in
  every variant.
  - It is kept out of line so its selection record stays off the `setup` and
    `openReader` frames.
  - Those frames are unchanged: `setup` 384 (dev-usb 400) and `openReader`
    144 (dev-usb 560).
- No other selected frame changed. The protected audit guards
  `checkSelection` at 192 and tightens nothing, because no selected frame
  measures below its ceiling.

### Pulled-card fix (round 5, part E)

Four clean `--health-report` builds of `6e31612` stop only at the `04d9b69`
flash ceilings. `6e31612` changes only firmware comments and a parameter name
after `9c7e008`, and measures the same. The record and protected audit
(`0184bbb`) follow them, and four clean builds pass against it.

| Variant | Ceiling (`04d9b69`) | `9c7e008`, `6e31612` | Growth |
|---|---|---|---|
| default | 1,311,728 | 1,311,840 | +112 |
| dev-usb | 1,331,488 | 1,331,600 | +112 |
| regional | 1,315,968 | 1,316,080 | +112 |
| regional dev-usb | 1,335,712 | 1,335,824 | +112 |

- Static RAM is unchanged in every variant.
- `resumeAfterCard` measures 176 bytes against 192, now that its test is
  out of line. That cause is inferred; the older build wasn't disassembled.
  The audit tightens its ceiling to 176.
- `cardSilent` measures 16 and is guarded at 16.
- After a failed read, the presence check adds `cardSilent` 16, `cardAbsent`
  16 and `SdLibrary::present` 112 before the filesystem stat. It runs after
  the selection load returns, not inside it.
- `checkSelection` measures 192, its ceiling.

**Line count.** `firmware/library_reader.h` grows from 496 lines at
`246f805` to 529, against the 500-line production threshold. Its exact
ceiling in `code-health-baseline.json` follows it (512, 519, 522, 528 at
`9c7e008`, then 529), with no headroom. The round-5 maintainability reviews
accepted the exception for these correctness fixes; `docs/CODE_HEALTH.md`
records it. Moving the stateless `hash()` to its own header would retire it,
but its selected stack key would have to move with it, which needs a reviewed
policy change.

## Reviews (round 5)

Every fix commit had fresh, bounded reviews with their own host probes. The
reports and probes travel with the handoff, not in the repository; the
adversarial review of `6699538` survives only as its reproduced finding.

- **Adversarial, `6699538`.** A refusal by the first card to answer was
  final, so the book's own card never restored saving. Fixed in `10b3241`
  (`wrongCardFirst`).
- **Adversarial, `10b3241`.** No blocker.
  - The refusal message stayed after the own card matched. Fixed in
    `656155b`.
  - A card swapped after a match was not compared. Fixed in `656155b`
    (`swapAfterMatch`).
  - Three older findings are listed under Limitations: the refused sleep
    from the reader, the Home resume intent, and Home's missing-card text
    while a checkpoint is pending.
- **Maintainability, `10b3241`.**
  - Must-fix: the rollback of the unconfirmed flag was untested
    (`heldBookRollback`), and the resource gate was red until the flash
    record.
  - Should-fix: a stale refusal message, comments, the packing reason, the
    `checkSelection` stack guard and the docs wording. They were applied in
    `656155b`, the flash record and this document. Its proposed
    `dispatchAction` tightening was overtaken when `48175d4` returned the
    frame to 992.
- **Adversarial, `656155b`.** No blocker.
  - A failed Books open cleared a held refusal. Fixed in `48175d4`.
  - The new Read rule was unpinned. A `swapAfterMatch` case pins it in
    `48175d4`.
  - The unnoticed swap and the own-card refusal after a reseat are listed
    under Limitations.
- **Maintainability, `656155b`.**
  - Must-fix: `swapAfterMatch` never reached Read's rule, and comments
    overstated what a swap compares. Fixed in `48175d4`; the swap is listed
    under Limitations.
  - Should-fix: two rollback mutants survived, the restart check was weak,
    and the line exception had to be recorded. Fixed in `48175d4` and
    `docs/CODE_HEALTH.md`.
- **Combined, `656155b` to `48175d4`.** No blocker. The resource cost is
  within policy.
  - The rollback's clause that clears a stale refusal was unpinned, and one
    message check depended on fixture state. `04d9b69` pins both and checks
    that the silent case shows nothing.
  - Two comments were corrected in `04d9b69`. A message set by a failed open
    itself is listed under Limitations.
- **Adversarial, `9c7e008`** (the pulled-card fix). No firmware defect.
  - A present card failing with `EIO` was untested, so four plausible wrong
    fixes passed. `6e31612` adds `ioError` and pins it.
  - The fixture's missing Books catalog is listed under Limitations.
  - The commit message's +128 bytes for regional dev-usb was a working-tree
    build. The clean builds measure +112; `6e31612`'s message corrects it.
- **Maintainability, `9c7e008`.** No must-fix. Applied in `6e31612`:
  - the mount precondition in each new check;
  - comments and the parameter name;
  - which test helper to use for what;
  - the line exception record (in `0184bbb`);
  - the `resumeAfterCard` tightening (in `0184bbb`).

  `restoreOnBoot` explains its own test in a comment rather than sharing
  `cardSilent`.

  Its S3 found two problems in `9c7e008`'s message:
  - it says `checkSelection` cannot see a pulled card, but it can at a wake
    or after an open that skips the book read;
  - it lacks the host-evidence boundary.

  `6e31612`'s message corrects both. The commit is not amended.
- **Combined, `4cc0eb8` to `0184bbb`.** No blocker. Every number in the
  records matches the build reports, nothing is raised or removed, and the
  pins match.
  - This section is the one the records cite.
  - The untested `checkSelection` difference is now stated (Host evidence).
  - A device claim in `docs/CODE_HEALTH.md` is now marked host evidence.
  - The frame drop's cause is marked as inferred.
  - Two test comments are corrected.
  - `cardSilent` has no `noinline`. If a later edit inlines it, its guarded
    frame disappears and the gate fails. Add `noinline` the next time the
    file is re-measured.
  - Left as is, because changing them would mean re-pinning the records or
    re-measuring the firmware:
    - the pinned flash reason's wording ("over the 555547a ceilings", which
      are the `04d9b69` measurements, and "Books' presence check");
    - the `resumeAfterCard` comment "(cardSilent is false)", which reads
      like a precondition.
- **Adversarial, `4cc0eb8` to `0184bbb`** (fresh). No blocker and no defect in
  `cardSilent`, `resumeAfterCard` or `checkSelection`.
  - Silence can never verify a selection. `resumeAfterCard` returns without
    changing anything, and `checkSelection` keeps the book unconfirmed, so
    the checkpoint and power-on guards stay closed.
  - Its probe's steps:
    1. A power-on with the card, then the card is pulled.
    2. Read with the held book waits for the card, and Books says it is
       missing.
    3. The sample is chosen there (it opens, and Read keeps it), or not
       (Read still waits, with no fallback).
    4. The card returns, seen by Read or by Books. A newer page checkpoints,
       and a restart reopens at it.

    Steps 3 and 4 make four combinations. All four pass with nothing written
    while the card is out. The probe fails with `resumeAfterCard`'s old
    test.
  - The fix relies on the driver reporting a pulled card as `EIO`, `ENODEV`
    or `ENXIO`. Reading the bundled SD driver and FatFs supports that, but
    only the device can show it. The owner's part E check then passed on
    `0184bbb` (Device evidence).
  - The power-on race, the fixture's state differences and the cost of the
    presence check are listed under Limitations.
- **Docs accuracy, `a8b0f36`** (fresh; this document and
  `docs/CODE_HEALTH.md` checked against the evidence). Four must-fix and nine
  should-fix findings, all fixed in this document.
  - The must-fix claims went beyond the evidence:
    - `no-loaded-term` needed a split slot read; it is equivalent in
      firmware;
    - `no-catalog` ignored the catalog; it tests the catalog instead;
    - a present card fails with `EIO` "as hardware does"; that was never
      observed;
    - part E's retry followed Read; Books found the card missing, and the
      sample's open ran the retry.
  - The should-fix findings covered:
    - the fix's predicate and the unobserved errno;
    - host results that were not labelled as host evidence;
    - `check-old-test`'s missing cases;
    - the restore opened from Books;
    - which frames were recorded;
    - how part E's pass rests on its setup;
    - the sleep not run on the device;
    - the maintainability review's S3.
  - It confirmed every image hash and size, device value, build number and
    mutation count.
  - The section heading keeps its commit IDs after the title the records
    cite.

## Device evidence

### Round 4 (2026-10-05)

The owner's X3, under the owner's grant.

- Image: `8893d0b` regional dev-usb (`8893d0be288d-regional-experiment`),
  1,335,312 bytes, sha256 `bead6d927830dcb07e206015f28e1978e0920886d89a61cf164616ad744dcc83`.
- It was written to the app1 slot, and esptool verified the written hash.
- No later round-4 commit touches firmware sources.
- The run was driven over the dev-USB bridge. It recorded numbers, and
  frames only of the CC0 fixture; no owner text was captured.

**Selection restore.**

- In both sessions, the first Read after the connection's restart opened the
  owner's selected book at its saved place (spine 8, character 0).
- At the end, the owner's book was reselected from Books. After another
  restart, Read opened it at the same place.

**Footer estimate.** The damaged-chapter fixture has two chapters of 2,479
and 2,439 XHTML bytes. It opened at its saved place: spine 0 (chapter 1),
character 2,203, page 5 of 5.

- Its footer read "~49%" after a warm reopen and again after a restart
  (frames `r4-fixture-warm` and `r4-fixture-cold`).
- That is the chapter-size estimate: chapter 1 is about half the
  reading-order bytes, and the page is near its end.

**Timing.** Device action totals in ms; round 3 is the same device on
`1d5e298`.

Round 4 purged no caches. Every open after a restart, and every open from
Books, reused the book's retained import receipt, so the table labels them
cached opens. None of them is a first import (a cold open).

| | Round 4 | Round 3 |
|---|---|---|
| Book 7: warm reopen | 961, 962, 961 | 973–975 |
| Book 7: cached open after a restart | 1,274 | 1,293 |
| Book 7: turns | 599, 600, 606; 710 and 703 across a chapter boundary; 903 | 602–965 |
| Book 7: cached open from Books | 1,671 | — |
| Fixture: warm reopen | 923, 921 | — |
| Fixture: cached open after a restart | 1,265 | — |
| Fixture: cached open from Books | 1,250 | — |
| Owner's book: Read after a restart (cached) | 1,744, 1,741, 1,745 | — |
| Owner's book: cached open from Books after a restart | 4,024 | — |
| Back to Home (gray) | 1,154–1,305 | — |

- Within noise, this layer leaves the reader's speed where round 3 left it;
  it did not aim at speed.
- First imports were not repeated in round 4. They come from earlier rounds
  on this device and stay separate from the cached opens:
  - book 7 (482 KB): 3.0 s, and book 55 (7.9 MB): 11.9 s (round 3);
  - book 24: 11.1 s, and book 6: 18.8 s (round 2).

  Both rounds ran image `1d5e298`.

  A first import of a large book still exceeds the 5 s cold-open target.
- Warm reopen is still above the 700 ms ceiling.
- Wake time was not measured.
- Three values come from the device's own action trace, because the bridge
  reply lapsed: book 7's restart reopen, its third warm reopen, and the owner's
  first Read in the second session.

**Reader stack.** The minimum free stack was 6,980–7,068 of 16,384 bytes,
the same values the round-3 image logged.

**Bridge lapses.** The dev-USB bridge lapsed six times in two sessions (one
in the first, five in the second): a reply arrived about 60 s late, a stale
reply arrived, or a record was damaged.

- Round 4 changes no bridge code, and round 3's sessions show the same
  lapses.
- The first session stopped at its first lapse. Its driver gave up at the
  protocol timeout, so it never re-handshook.
- The driver now re-handshakes and observes the state again before
  continuing.
- One frame was lost: the first fixture open.

**Not run on the device:** a card swap with a held book, a boot and a wake
without the card, Books and Read without the card, and Home's estimate line
after a restart (Home is gray, which frames can't capture). Host tests cover
these.

### Round 5 (2026-10-06)

The owner's X3, under the owner's grant. The checks ran from the wizard
`x3-r5-checks.sh` over the dev-USB bridge. The owner moved the card and
reported what the screens said. Only numbers and three frames were recorded:
two Books "No SD card" screens and one page of the built-in sample. Every
frame of the CC0 fixture timed out. No owner text was recorded.

**Image `555547a`.**

- Image: `555547a` regional dev-usb (`555547aa954b-regional-experiment`),
  1,335,712 bytes, sha256 `0c8521425ce7c1c2f27f48085326c0e3a76b6f96192520e4632449317ed5a274`.
- It was written to the app1 slot, the booting slot. The readback matched the
  image, and no other byte of flash changed.
- One bridge run after the install; it re-handshook once after a lapsed
  keepalive. The handshake named that build. Read opened the owner's selected
  book at its saved place (spine 8, character 0) in 1,741 ms, a cached open
  after a restart as in round 4. Back returned Home with no save failure. No
  frames were captured.
- **Part D: PASS.** The first attempt stopped at a bridge lapse before its
  first check. The wizard now waits through lapses, and the rerun passed in
  full:
  - A restart without the card:
    - Home: "No SD card · Sample is in Books".
    - Read: "No SD card. Insert it, then press Read." No book opened, so
      there was no silent sample.
    - Books: "No SD card", and Open reads the built-in sample instead.
  - The card put back without a restart: Read reopened the test book (the
    damaged-chapter fixture) at its place, spine 0, character 2,203.
  - The card pulled, then a sleep and a wake by hand:
    - Home: "No SD card · Your book is on it".
    - Read and Books said the card was missing.
  - The card put back:
    - Read opened the test book.
    - A page back showed no UNSAVED.
    - Home showed a percent marked "~".
  - After a restart:
    - Home showed the "~" percent.
    - Read reopened the test book at the newer page (spine 0, character
      1,602), so saving had resumed. The diagnostics name the test book.
  - The test book's place was put back (diagnostics: spine 0, character
    2,203). The wizard's own record of it reads "closed", because it read the
    state after Back. The owner's book was reselected, at spine 8, character 0.
- **Part E: FAIL.** This is the defect in "A card pulled after a card boot".
  The wizard stopped and returned Home. The owner then reselected their book
  with the wizard's restore step, which opened it from Books at spine 8,
  character 0 in 4,146 ms.

**Image `0184bbb`.**

- Image: `0184bbb` regional dev-usb (`0184bbb0da20-regional-experiment`),
  1,335,824 bytes, sha256 `0581864f01ac015ec5c11be300d13a1b11b3f168fff187a9c4f3910b3b52b0e7`.
- It was written to the app1 slot, the booting slot. Before the write, the slot
  held what the `555547a` install had left there. The readback matched the
  image. Later commits change only documentation and test comments.
- One bridge run after the install. The handshake named that build. It
  re-handshook three times after lapsed keepalives.
  - Read opened the owner's selected book at its saved place (spine 8,
    character 0) in 1,787 ms, a cached open after a restart.
  - Back returned Home with no save failure.
  - The reader's minimum free stack was 7,068 of 16,384 bytes. No frames were
    captured.
- **Part D was not repeated.** The fix changes behavior only after a failed
  read in a boot that had mounted the card, with no card present. Part D
  never reaches that state, by reading of the code:
  - its card-out states follow a restart or a wake, where nothing is
    mounted;
  - when the card was pulled, the book it held had already been verified.
- **Part E: PASS.** This is the sequence that failed on `555547a`:
  - The X3 booted with the card, by the wizard's preflight, which the owner
    confirmed; no log line records the mount. The card was then pulled on
    Home.
  - Books found the card missing. This is inferred: Books listed no books
    and Open reached the sample. Its frame timed out, and the owner was not
    asked.
  - A card missing since boot would pass on the old firmware too. The same
    steps failed on `555547a`.
  - Open in Books opened the built-in sample. The diagnostics name the
    sample, at spine 1, character 1,500. The frame shows its page 4 of 5,
    with an exact 84%.
  - The sample open took 2,233 ms of device time. The presence check's share
    of that was not separated.
  - The X3 slept from the sample, and the card went in before the wake.
  - After the wake, by hand:
    - the sample was on the same page;
    - Home showed the sample;
    - Books listed the owner's books.
  - After a restart, Read opened the owner's book at its saved place (spine
    8, character 0) in 1,747 ms. Back returned Home with no save failure.

**Not run on the device:**

- a sleep from an SD book's reader with the card out (part E slept from the
  sample's);
- a present card that fails its reads (it needs a damaged card);
- a card swap with a held book, and the two limitations about a swapped card;
- wake timing.

No battery disconnection or improvised power cut was tested, by instruction.

## Limitations

- **The estimate is saved only when the reader is left.** A reset or power loss
  inside the reader keeps the previous record.
  - No record is saved while the optional revision session is absent (after a
    strict heap fallback).
- **The optional exact Home overview is still not called by the firmware**
  (`prepareHomeOverview`). Exact Home percents come from counts the reader has
  already saved.
- **A cold boot without the card cannot name the book.** The selection lives on
  the card, so Home says "Sample is in Books".
- **Card changes are noticed only by Books, Read or a boot.** The `EIO`,
  `ENODEV` and `ENXIO` branch of the presence probe is reachable only on
  hardware. Host tests model a card missing since boot, whose reads find
  nothing, and a card pulled after this boot mounted it, whose reads fail.
  - The host fixture has no Books catalog while the card is out. Firmware
    keeps the catalog, with an unavailable status, while the card stays
    mounted; `SdLibrary::scan` releases it only when a mount fails. Both say
    "no card" to Books, but a mutant that tests the catalog instead of the
    presence check survives (`no-catalog`; adversarial review of `9c7e008`).
  - A card returned while Books is open still counts as missing until Books
    rescans or Read runs. The sleep image follows that state.
  - After the sample is chosen, Home keeps the sample's hint until Read or
    Books sees the returned card.
  - After a wake into the sample with the card already back, Read keeps
    opening the sample. The card's book returns when it is opened from Books,
    after a restart, or after a sleep and wake from Home.
  - A book held from a wake without the card saves nothing until Read or
    Books compares it with the returned card's selection.
  - Books compares a held book only after the card was seen missing (by the
    wake, Read or Books). A Books visit that finds a card already present
    leaves the comparison to Read.
  - After Books finds the returned card, the Device screen keeps the
    missing-card message until Read or an open replaces it (present at
    `246f805`, before the round-5 fixes). While a checkpoint is pending, Home
    also keeps saying the card is missing until Read runs, though Books lists
    the card.
- **A refused book cannot sleep from the reader** (found by the adversarial
  review of `10b3241`; present at `246f805`, before the round-5 fixes).
  Suppose the card that is in selects another book, or its selection can't be
  read, and Read opens the held book from it. A sleep from the reader then
  refuses the checkpoint. Because the card is present, the sleep is abandoned:
  the device stays awake in the reader with "Battery resume not saved.
  Existing position kept."
  - Back to Home, then sleep, works.
  - Changing this means admitting an unacknowledged reading sleep. A tested
    negative control forbids that, so it was left for review rather than
    changed here.
- **A card swapped while the book's file is closed, and never seen missing, is
  not compared** (found by both reviews of `656155b`; present at `246f805`,
  before the round-5 fixes). After a Books visit with the card in, a power-on
  restart or a Home wake, no handle is held. Swap the card for another that
  holds the same book file, with no Read or Books seeing it missing in
  between, and Read opens the file by name and saves without a comparison. The
  same happens when Books runs first after a swap (its scan closes the stale
  handle unread), and when the periodic checkpoint runs while reading.
  - A card whose older slot names the book takes its place and a resume
    intent, so a restart resumes the held book instead of that card's newest
    selection. A card that never selected the book refuses every checkpoint,
    but saving still counts as verified, so a sleep from the reader meets the
    refused-sleep limitation above.
  - No selection is rewritten and no other book's place is written. It needs a
    second card holding the same book file.
  - The firmware has no card identity, and the mount is cached, so only a
    stale handle or a missing card reveals a swap.
  - The reviewers' proposed fix compares with the card's power-on authority
    (its committed resume intent's book, else its newest selection) before
    each save and on every by-name open. The host fixture shares one store
    across cards, so it can't test that rule; it is left for review.
- **The held book's own card can refuse after a reseat** (adversarial review
  of `656155b`; new with the card-change comparison).
  1. A book is committed, then Books opens another, which becomes the card's
     newest selection.
  2. Power is lost before the new book's first committed checkpoint.
  3. The power-on restores the committed book from its older slot, as designed.
  4. The same card is then reseated, and Read or Books sees it missing.

  The comparison reads the newest selection, so it refuses the restored book.
  Nothing is written; the place is kept, and "Book selection not verified"
  shows until a book is opened from Books. It needs a true power-on reset in
  that window and then a reseat. The authority rule above would close it too.
- **A failed open can still hide a held refusal behind its own message**
  (combined review of `48175d4`; predates it). If the open's own Read clears
  the refusal and the reader session then fails, "Reader session
  unavailable" stays shown after the rollback while the held book is refused.
  The next Read shows the refusal again. Re-deriving every message would hide
  a real write failure, and `dispatchAction` has no stack headroom, so it was
  left.
- **A read error on a held handle counts as a card change.** Read compares
  again; if the selection read also fails, saving stays off until the next
  comparison. While a refusal stands, it is shown instead of an NVS storage
  warning (as before).
- **A refused session's Home sleep replaces that card's resume intent with
  Home** (adversarial review of `10b3241`; present at `246f805`). This happens
  while the card that is in selects another book: a Home or Library sleep, or
  Back from the reader, records Home. The card's selection and every place are
  kept, and Read still opens its selected book there. Only a power-on resume
  straight into the reader is lost.
- **After a wake into the sample with a card whose selection can't be read,**
  the card's book stays blocked after a Home sleep until its selection reads
  again. Nothing is written.
- **A card without a reading-state file reads the legacy NVS records**
  (`SdReadingState`). If legacy NVS names the held book and that card has the
  book's file, the comparison accepts it. This path is firmware-only, and the
  host fixture does not model it.
- **While the card is missing, each sample open first asks for it** (a failed
  mount). This includes a wake straight into the sample. After a card pulled
  since boot, the ask is a failed read and a `stat` of the Books folder. Both
  fail card initialisation, and the driver retries each command three times
  with 100 ms delays. The adversarial review of `0184bbb` estimates the
  presence check adds roughly 0.3–1 s to each Read of the chosen sample.
  Neither cost was timed on hardware.
- **A card pulled during a power-on can block its selection.** The power-on
  reads the committed resume intent first, which mounts the card, and then
  reads the selection. The adversarial review of `9c7e008` raised this (S1),
  its maintainability review noted the power-on mount (S1), and the
  adversarial review of `0184bbb` probed it.
  - A card pulled between those two reads fails its read instead of reading
    as nothing, so the selection is blocked. The probe's boot reports "Saved
    book selection unavailable. Existing data kept." until Books finds the
    card missing ("No SD card. Insert it, then press Read."). Choosing the
    sample there recovers.
  - The reviewer estimates the window at about a millisecond. The fixture
    keeps the resume intent in a separate store, so the race was probed only
    by a reviewer's harness, not by the suite.
- **The host fixture's reading state is fixed per test step.** Firmware
  reports unavailable reading state for its last transaction only.
  - One `heldBookRollback` case lists the card's books while its reading
    state reads as never mounted. Firmware reaches that state only in a race,
    and there it would refuse, the safer outcome.
  - With `ioError` set, the fixture would count a read that finds nothing as
    silent, which firmware would not. No test combines the two.
- **The no-card Home keeps the progress bar's empty track.**
- **The `.ntpe` key carries no SDK commit,** unlike the NTSM metadata record.
  The policy version is stored and checked. If an SDK change altered chapter
  character counts, an old record's offset check still bounds it, and the next
  reader exit rewrites it.

## Integration notes

- **Landing order** (base `2ef3148`):
  1. fix/sd-source-descriptor `3a7146a`
  2. perf/reader-latency-claude `218ea27`
  3. fix/sd-reading-state `567e737`
  4. feat/overlay-home `954d9b6`
  5. this branch (round 4, the round-5 recovery fixes and the pulled-card
     fix)
- **Land steps 3–5 together.** fix/sd-reading-state alone still carries the
  boot without a card where Read opens the sample (doc 051, "Card removal").
  It ends when this layer lands, so it must never be the landed intermediate
  state.
- **Each step carries its own measured flash record.** After a rebase, rerun
  the full gate and the four clean `--health-report` builds. Keep the commits,
  because the docs cite their hashes; update the citations in the same step.
- **`ReaderFooter` gains `overallEstimated` before `sectionLabel`.** Positional
  initializers elsewhere must add it, or a label pointer converts to the bool.
- **Overlap.** `src/frosted_home.cpp` and `include/newt/frosted_home.h` overlap
  with #19 and the queued Home redesign (#29).
- **Committed artifacts.** The committed `artifacts/frosted-*.png` already
  differ from what the gate regenerates at `954d9b6`. This branch leaves every
  regenerated Home pixel identical to `954d9b6`.
