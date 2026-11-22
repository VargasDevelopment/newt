# Back paints Home first (#132)

Back (and Menu → Home) from a page now shows Home as soon as the reading
position is safely saved. The two optional records Home reads as hints, the
chapter identity and the progress estimate, are written right after Home is
on the panel. The "Saving your place…" card is gone from this path except
while the book check is still pending.

## Where the time went

Measured over DEV USB on the bench before the change (today's main, n = 5
Backs per book from a page in the book; per-Back times from the device's
own phase rows):

| Part | Short, no cover | Mid-size, cover | Long, cover |
|---|---|---|---|
| "Saving your place…" card (one full refresh) | 0.64 s | 0.64 s | 0.64 s |
| Exit save, all of it | 0.34-0.36 s | 2.02-2.23 s | 4.08-5.35 s |
| of which chapter identity record | 0.17 s | 1.78-1.97 s | 3.40-3.74 s |
| of which progress estimate record | 0.10 s | 0.15 s | 0.59-1.87 s |
| Home compose and refresh | 1.20-1.21 s | 1.14-1.15 s | 1.14-1.16 s |

The rest of the exit save (progress commit and verification, statistics
fold, resume picture, Home destination record) is under 0.1 s.

The two derived records were most of the wait. Each rewrites a small file in
the book's cache folder, and on a book with many cached chapters the folder
lookups and the synced write take seconds. The panel refresh itself is about
0.6 s of Home's 1.2 s.

## What changed

- `finishReaderExitAction` arms a capture before the exit save
  (`sdSelection.deferExitMetadata()`), unless the exit is Sleep.
- The exit save is unchanged. Its progress commit still runs first and still
  verifies. Only after a verified progress save does the checkpoint ask for
  the identity and estimate (`saveReadingCheckpointMetadataCore` writes them
  only `if (saved)`), and the selection then copies them into the capture
  (`ReaderExitMetadata`, 392 B on the heap) instead of writing them.
- The statistics fold, the resume picture, the Home destination record and
  the reader's close run as before, then Home paints.
- `loop()` publishes the capture after the paint (on DEV USB, after the press
  is reported), through the same ownerless cache route a closed reader's
  checkpoint already uses. Before any later press is handled, the loop
  publishes first, so a press queued behind Back finds the records written.
- The firmware switch publishes right after its exit save, since no Home
  follows. Sleep never arms a capture: its records stay inside its save,
  before power-off.
- A capture whose book is no longer the selected book is dropped. If the
  allocation fails, the records are written inside the save as before.
- The Saving card stays only while the book check is pending
  (`sourceVerificationPending()`), because the save must then finish the
  check first, which can take seconds.

## Durability

The save acknowledgement is unchanged: Home's position and any save warning
come from the same verified progress commit as before. The change only moves
when two optional, derived records are written.

- **Failed save.** No identity or estimate is captured (the checkpoint asks
  for them only after a verified save), so nothing is published. The warning
  path is unchanged.
- **Power loss during the save.** As before: the progress store's A/B commit
  leaves the old or the new complete position.
- **Power loss after the save, before publication.** The new position is
  durable. The identity record still describes the previous position, so a
  cold Home lookup for the new position misses and shows the generic label,
  never a wrong chapter. The estimate record keeps its previous complete
  chapter weights, which Home reads as an estimate for the saved chapter or
  misses.
- **Power loss during publication.** Each record uses its existing atomic
  write, so it is either the old or the new complete record.

## Tests

- `tests/back_exit_metadata_test.py` runs the real progress store,
  checkpoint core, chapter identity record and capture over the crash
  harness. It checks that Back's checkpoint does no identity I/O, that the
  published files are byte-equal to the in-save path, that a failed save
  publishes nothing, and that a cut or failure at every operation of the save
  and of the publication leaves the old or new progress and a truthful
  identity. Controls: copying before the save, and a copy that answers any
  position.
- `tests/sleep_first_firmware_test.py` composes the actual exit, dispatch and
  loop lines: Back saves before Home, paints Home before publishing,
  publishes before a queued press, shows the Saving card only while the
  check is pending, and Sleep never defers. Each has a removal control.
  `exit_publications` (tests/source_fixture.py) takes both publication
  lines from where they sit in the actual `loop()`; moving either one in
  the production source (before Home's paint, or out of the input loop)
  fails the composition.
- `tests/sd_power_on_resume_test.py` runs the actual exit over the SD
  fixture: after Back's exit and before the loop's publication, a cold load
  returns the new position without the new chapter record; after it, both.
  Controls: writing the record inside the save, and dropping the loop's
  publication.

## Measurements

Same method before (main `fcafa09c`) and after, on the bench X3 over DEV
USB, n = 5 Backs per book. "Home shown" is the device's own time from the
press to the end of Back's action, with Home on the panel. "Next press
ready" is the DEV reply to the press, which the loop sends after the
publication, so it is also when a following press is handled.

| Book | Home shown p50 / max, before | after | Next press ready p50 / max, before | after |
|---|---|---|---|---|
| Short, no cover | 2.29 / 2.50 s | 1.40 / 1.64 s | 2.32 / 2.54 s | 1.71 / 1.95 s |
| Mid-size, cover | 4.35 / 4.36 s | 1.59 / 1.59 s | 4.38 / 4.39 s | 3.56 / 3.74 s |
| Long, cover | 6.65 / 7.53 s | 1.63 / 1.80 s | 6.69 / 7.56 s | 6.16 / 6.94 s |

Free heap low while reading: 63,088 / 63,924 / 63,508 B before and
63,088 / 63,924 / 63,476 B after, above the 50,468 B floor.

Home now appears in about 1.4-1.8 s whatever the book. The publication
still costs what the two records cost before (0.3 s to 5.3 s), so on a
book with a crowded cache folder a press made right after Home waits for
it. Making those writes cheaper, or stoppable by input, is follow-up work.
