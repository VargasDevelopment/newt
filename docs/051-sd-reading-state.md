# Durable reading state on SD

## Why progress saves fail

On the owner's X3, every failed save returns `ESP_ERR_NVS_NOT_ENOUGH_SPACE`
(`0x1105`). The NVS partition is 20 KiB, of which 630 entries are available to
records; 499 are written, and only about 5 sit outside the IDF GC reserve page.

| Contents | Entries |
| --- | --- |
| Data from the stock firmware (calibration, PHY, Wi-Fi), not Newt's to delete | ~205 |
| Newt's fixed records | ~40 |
| One `nb<id>` namespace per book Newt has ever saved progress for | 20 × 12 = 240 |

Each book namespace holds a 32-byte digest blob, two 52-byte slots and a
namespace entry. Nothing removes them. 11 of the 20 books were only opened
(sequence ≤ 3, no pages turned). Growth is per book opened, not per page read,
so the partition fills after about 20 books.

Once full, every rewrite needs free entries for the new copy before the old
one is erased. Saves fail as follows:

- **Existing books**: the 4-entry progress slot rewrite fails intermittently.
- **New books**: the 6-entry `newt_selected` rewrite fails, which sets
  `selectionSaveFailed_` and disables every later save for that book.

Existing records must be preserved before cleanup can free space. Moving only the per-book
namespaces would leave `newt_selected` and `newt_resume` blocked on this
device. Reading state as a whole therefore moves to SD.

## Design

**Scope.** Mutable reading state moves to SD:

- per-book progress (`nb<id>`);
- the selected SD book (`newt_selected`);
- the sleep or power-on resume intent (`newt_resume`).

Device settings stay in NVS: `newt_prefs`, `newt_pause`, `newt_apps` and the
built-in sample's `newt_reader`. They are a fixed set and rarely written. NVS
no longer grows with the library.

**The stores are unchanged.** `ProgressStore`, `ExternalProgressStore` and
`DurableRecordStore` keep their code and their rules:

- the A/B slots and sequence;
- the protected checkpoint the resume intent references;
- the full-digest binding;
- read-back verification and torn-slot tolerance.

Only the backend changes. `SdReadingState` (`firmware/reading_state_backend.h`)
implements the existing store interface (`begin(namespace, readonly)`,
`readBlob`, `putBytes`, `end`). `NvsProgressBackend` routes the three
reading-state namespaces to it; every other namespace goes to NVS through
`NvsBlobBackend` (`firmware/nvs_blob_backend.h`) as before. `NvsProgressBackend`
keeps its name because reviewed stack measurements are keyed by it.

The selection, progress and resume stores share one backend instance, so their
transactions run one after another; `begin` ends any transaction still open.

**Location.** Reading state is user data, so it lives beside the disposable
cache tree, not in it. Each namespace has one file:

- `/Newt/State/nb<13 hex>.nrs`;
- `/Newt/State/newt-selected.nrs`;
- `/Newt/State/newt-resume.nrs`.

The short names take three FAT directory entries each, and their 8.3 aliases
differ.

**File layout.** Each file is fixed at 4,096 bytes, in eight 512-byte sectors:

- Sector 0 is the header (`NRS2`, version, namespace).
- Each key (`digest`, `slot0`, `slot1`) owns two sectors, one copy per sector.
- Sector 7 is unused.

A copy is a 256-byte record: tag, key name, length, a 32-bit generation, a
payload of at most 232 bytes, and a CRC-32C. Records use CRC-32C because the
progress and selection payloads already end with their own CRC-32. A record
CRC with the same polynomial could not detect damage confined to such a
payload.

**Reads.** A key's value is its newest intact copy.

- A copy that fails its tag or CRC is ignored. It was never written, or was
  torn by power loss.
- With no intact copy, the key is missing. NVS treats an entry that fails its
  CRC the same way.
- An intact copy that names another key, or claims an impossible length, fails
  closed.

**Writes.** `putBytes` writes the copy that does *not* hold the key's newest
value, with the next generation, then calls `fsync`. A torn record write
therefore damages only the stale copy, and the key keeps its previous value,
as an NVS write that fails. That matters when the protected checkpoint is the
older store slot: the store then overwrites the newest slot, and a power loss
mid-write must not lose it.

FAT's `fsync` also rewrites the file's directory entry (size and time) on every
save. The state files share the directory's sectors, about five files per
sector. A power cut that tears that sector could make those namespaces read as
damaged or absent, and they then fall back to legacy NVS. The hard-reset tests
keep the card powered, so this exposure is unverified; it belongs in the
owner's power-cut test.

After `fsync`, the write reads the header back. FAT's one-sector file buffer
then holds the header, so the stores' read-back of the key comes from the
card, not from the buffer.

**Creation.** The first write to a namespace creates its file:

1. A fresh temp file is opened with `O_EXCL` (`<name>.nrs.<nonce>.tmp`). The
   nonce counter restarts each boot, so taken names are skipped.
2. Every record is written explicitly: the header, any legacy blobs (generation
   1, first copy), and empty records elsewhere. Nothing depends on how the file
   system fills new clusters. Then `fsync` and close.
3. The temp is renamed into place.

A failure before the rename removes that fresh temp. A rename that fails with
an I/O error may already have written the new directory entry, so that temp is
left behind. Temps left by a crash are never opened, truncated or deleted: an
interrupted FAT rename can leave them sharing clusters with the renamed file.

**Damaged files.** A file of the wrong size, or whose header names another
namespace, does not lock that namespace. It reads as absent. The next write
renames it aside as `<name>.nrs.<nonce>.bad`, under a name not already taken
(FAT refuses to rename onto an existing name), and creates a fresh file in
its place. The set-aside keeps the evidence and frees no clusters; earlier
`.bad` files are never overwritten.

**Migration from NVS.** Reading-state writes go to SD. Rust also has a
normal-boot cleanup of recognized legacy per-book NVS records:

- **Reads:** while a namespace has no valid SD file, the backend reads the
  legacy NVS namespace.
- **Creation:** the new file carries the legacy `digest`, `slot0` and `slot1`
  bytes verbatim, as generation 1. Protected checkpoints and resume intents
  that reference legacy sequences then stay valid.
- **Cleanup:** only `nb` followed by exactly 13 lowercase hex digits is
  eligible, and only with Newt's matching full digest and recognized progress
  records. Unknown keys, types, identities or damaged records are retained.
  The first pass preserves every candidate through the existing SD router;
  an existing valid SD authority wins over older NVS. It forces a successful
  modified-handle sync and reopens the file to verify its digest and slots.
  Only after every candidate is preserved does cleanup erase the three
  recognized NVS keys, commit and verify their absence. The namespace ID
  entries remain. No partition erase or cleanup marker is used.
- **Failure:** an incomplete preservation pass erases nothing and defers a
  new statistics seed, so unopened legacy progress cannot be left out. A
  valid statistics ledger is never reseeded. A cut during cleanup leaves the
  durable SD authority and a retry on the next boot. With no candidates, the
  pass only walks NVS once; it does no SD I/O and retains no heap allocation.
- **Scope:** the four fixed NVS device-settings namespaces, the Wi-Fi key,
  other firmware's namespaces and SDK data are not reclaimed.
- **Multiple cards:** cleanup moves legacy progress to the mounted card;
  swapping cards afterwards no longer restores that old NVS fallback.

**No card, or the card removed.**

- **At boot with no card:** reads find no reading state. Nothing is selected, so
  nothing is blocked, and power-on resume falls back to Home.
- **Writes:** reading-state writes fail. Nothing falls back to NVS.
- **Medium unavailable:** a failed mount, or an `EIO`, `ENODEV` or `ENXIO`
  error from the call that failed, marks the medium unavailable. `errno` is
  cleared at the start of each operation, so only that call counts. The
  destination save is retried once. If it still fails, sleep and leaving the
  reader proceed with the warning `Battery resume not saved. Existing position
  kept.` instead of being refused. Deep-sleep wake uses the RTC record, and
  the last acknowledged intent stays on the card. When the checkpoint save
  itself failed, the sleep record is unacknowledged and its save opens
  nothing; that failed checkpoint save's medium state decides.
- **Not the card:** any other failure, such as a descriptor limit, still refuses
  sleep and Back as before.
- **Reinsertion:** Newt never detects a removed card. Mounting is retried on
  each card access until it succeeds, so a card missing at boot is picked up
  later. A swap after mounting relies on the SD layer reinitialising; see
  "Card removal". This predates the change.
- **Unaffected:** device settings and the built-in sample's NVS progress.

**Open files.** The card is mounted with a 3-file limit
(`sd_library.cpp:28`). During reading, the book, its receipt and the page cache
can each hold a descriptor. A state transaction needs one more, so it can fail
if all three are open at once; the save then fails closed with the usual
warning. Device observations are under Evidence.

**Memory.** Static RAM is unchanged in all four variants. `NvsProgressBackend`
grows from 8 to 12 bytes: its two flags fit in `NvsBlobBackend`'s tail
padding, and the store pointer adds 4. At boot it allocates a 16-byte
`SdReadingState`; if that allocation fails, the reading-state namespaces fail
closed. Each transaction heap-allocates a 112-byte `Active` (path, legacy
reader and the newest-copy memory) and frees it at `end`.

## Card removal

Doc 056 changes several behaviours below: a card swap while a book is held, a
boot or wake without the card, and what Home, Books and Read say while it is
missing. The observations here are of this document's image; doc 056's
versions are tested on the host only.

Nothing watches the card, so a removal is noticed only when something reads
it. Device observations are from the owner's X3 on the final image
(2026-10-05).

- The first card session and the first reinsert-and-reboot used the owner's
  own book.
- A later wizard run used the damaged-chapter fixture as the selected SD
  book. There the owner handled the card, the signal presses, the wake and
  device details; Claude drove the rest over dev-USB.
- Screen captures cannot record Home's grayscale screen, so Home was observed
  by the owner or not at all.

- **Removed while on Home.** Home keeps the book, chapter and percent it
  already holds in memory, and Home never writes. Focus moves still respond
  (509 and 549 ms). Books shows the heading "Books are unavailable", then "The
  book folder could not be read." and "Original books have not been changed."
  With no book held (as after a boot), Read shows "This book could not open.
  Your position is kept. Press Back to return Home." with "Saved SD book
  unavailable or changed. Position kept." With the book still held after Back
  from the reader, Read shows the same page without that second line
  (observed after a swap; see below). In the reader, a failed save shows
  `UNSAVED`; that case comes from code reading.
- **Boot without a card (a regression).** This covers every boot that does
  not restore from RTC memory, that is, anything but a wake from sleep or a
  restart after a refused sleep. With no mounted card, a reading-state read
  reports absence rather than failure (`SdReadingState::begin`), so the
  selection is not restored. Home then shows the built-in sample with its own
  NVS progress and no card notice. Before this change the selection lived in
  NVS, so the same boot kept the SD book selected and reported it unavailable
  when the reader opened. That earlier behaviour comes from code reading.
  - On the device, after a USB-reset boot without the card, Read opened the
    built-in sample at its saved place (chapter 2, its 84% page) instead of
    the selected SD book. Home after that boot was not observed.
  - With the card back in the same boot, Read still opened the sample. A
    card inserted later is mounted by the next card access, but the
    selection is decided at boot.
  - The owner's earlier card-removal photo shows this Home: the sample with
    its NVS progress. Which boot led to it was not recorded. The fixture has
    the same title as the sample, "The Small Lantern"; only the progress line
    tells them apart.
- **Wake from sleep without a card.** The wake restores the selection from RTC
  memory (`restoreOnBoot`), so the SD book stays selected. The durable
  selection cannot be read, so the storage warning becomes "Book selection not
  verified. Reading position is kept."
  - On the device: the card was removed on Home, the X3 slept, and the owner
    woke it with the card still out. By the owner's answers (no photo), Home
    showed "Your SD book" and "Progress unavailable", and device details
    showed that storage warning.
  - From code, not observed: Read, or a wake straight into the reader, then
    fails to open the book and replaces the warning with "Saved SD book
    unavailable or changed. Position kept." The reader shows `UNSAVED` only
    if the card is back before it opens.
- **Reinserted, then Read in the same boot.** The SD layer reinitialises the
  volume on the next open, which invalidates descriptors opened before the
  removal. Back from the reader leaves the book held and Books releases it,
  as observed below. From code, a boot or wake straight into the reader also
  holds it, and another book or sleep also releases it.
  - On the device: the selected book was opened, then Back. On Home the owner
    pulled the card, reinserted it after a short count, and pressed Next. The
    X3 did not restart: the boot id was unchanged and the owner saw no
    restart. Read then failed with "This book could not open…", without the
    SD line. After a visit to Books, Read opened the book at its saved place.
  - origin/main (`2ef3148`) also keeps the book held after Back:
    `ensureSource` reuses a held source. So this predates the change. That
    comes from code reading; origin/main was not run.
  - A swap made while no book was held was not run.
- **Reinserted, then rebooted.** The selection and position return. Observed
  twice: after reinsertion and a USB-reset boot, Read restored the selected
  book at its saved place, the second time pixel-identical to the earlier
  capture of that page.

## Not addressed here

- A failed save of the selection at open still sets `selectionSaveFailed_`,
  which blocks later saves for that book until it is reopened. On SD this needs
  a card failure rather than a full partition.
- `openSdSelection` still ignores the result of the accepted-open save;
  `saveReaderProgress` reports a failure through the storage warning.
- Detecting a removed card, and telling a missing card apart from absent
  reading state at boot. The latter is the boot regression under "Card
  removal". Both are addressed in doc 056, as is the card swap.

## Evidence

**Host** (`tests/reading_state_backend_test.py`). Real files, syscall seams and
an in-memory NVS that counts every write. The rename seam refuses to rename
onto an existing name, as FAT does, and the `ftruncate` seam can leave stale
records in new space. The real stores run with the firmware's strict,
protected save forms. Each case starts from reset counters.

- **Wiring:** a source check binds both `main.cpp` aliases to
  `sdSelection.state()`, requires the constructor's attach, and routes Back and
  sleep through it.
- **A new book:** the SD file is created, and an in-place save is one open, one
  `fsync` and no rename. NVS is never written.
- **Legacy migration:** positions load from NVS before migration. The first save
  copies them verbatim, and the acknowledged legacy checkpoint still resolves.
  NVS stays byte-identical. A legacy blob too large for a record fails closed,
  without NVS writes and without marking the card missing.
- **Torn slot writes:** cuts in the tag, name, length, generation, payload and
  padding each fail the save, and the protected checkpoint survives. The next
  strict save recovers. A landed but unacknowledged record loads as newest.
- **Older protected checkpoint:** a torn overwrite of the newest slot keeps the
  newest position.
- **CRC damage:** record-CRC damage that the payload CRC cannot see reads as
  missing, and later strict saves continue. An intact record under another
  key's name fails closed, and a stale `errno` then does not mark the card
  missing.
- **Read-back:** a written copy damaged before the stores read it back is not
  reported as the saved value.
- **Selection and resume records:** torn writes keep the previous record, and
  the next save recovers.
- **Creation crashes:** failures at the header write, `fsync` and rename leave
  no canonical file. Only the temp of a rename that failed is kept, and a retry
  succeeds. Stale records in new space never survive creation. Stale temps are
  never modified; with every candidate name taken, creation fails instead.
- **Damaged files:** short, empty and other-namespace files read as absent. Each
  is set aside as `.bad` and replaced by the next save. Set-aside names from an
  earlier boot are skipped and never overwritten.
- **Card loss:** failed writes, failed reads or `EIO` on open fail the save
  without touching NVS. They mark the medium unavailable, and sleep proceeds
  with the warning. That includes an unacknowledged destination after a failed
  checkpoint save, which opens nothing. A single transient `EIO` is absorbed by
  the retry. A descriptor limit (`ENFILE`) still refuses sleep. The last save
  survives when the card returns.
- **No card at all:** nothing is selected, writes fail, sleep proceeds, and the
  sample's NVS progress still saves.
- **Allocation failure:** a failed boot allocation fails reading state closed
  and never writes NVS.

**Mutation runs.** These were one-off manual runs, not part of the gate, against
the working tree before the source commit. Every injected defect was caught:

- writing over the newest copy;
- ignoring generations;
- ignoring the record CRC;
- failing closed on damaged files;
- deleting damaged files instead of setting them aside;
- never, or always, reporting the card as unavailable;
- refusing sleep without a card;
- dropping the in-place `fsync`;
- routing to NVS on allocation failure;
- skipping the legacy copy;
- a second NVS resume backend in `main.cpp`;
- truncating stale temps;
- trusting a remembered copy without rechecking it;
- skipping the read of the newest copy;
- not clearing `errno` before a write;
- relying on new space being empty;
- removing a temp after any rename failure;
- dropping the destination retry;
- giving up on the first taken set-aside name;
- the sleep policy not detecting the routed backend.

Clearing `errno` in `begin` is defensive only. Its later `mkdir` and `open`
calls always set `errno` before any failure, so dropping it changes nothing the
tests can see.

**Device (owner's X3, regional dev-usb).** Builds are kept as local tags
`evidence/sd-state-*`.

- **This revision, `e1dfe12`** (build `e1dfe127955b`), 2026-10-05. It reused the
  v2 files written by the builds below, since the layout is unchanged.
  - **Book 48:** two same-chapter turns, the 30-second periodic save, a third
    turn and Back. After a reboot, Read restored (7,195).
  - **Book 60:** a first open under SD state loaded its legacy NVS position
    (3,905). Three turns crossed into chapter 4, then Back; after a reboot,
    Read restored (4,0).
  - **Back saves:** 58–86 ms.
  - **Interrupted Back:** a hard reset 10, 30, 50, 60, 70, 90 or 120 ms after
    pressing. Up to 70 ms the previous position came back; from 90 ms the new
    one. Every later save succeeded.
  - **NVS:** the region's SHA-256 was still `27c8fd17…`.

- **v2 layout** (this design without the v2.1 hardening), 2026-10-05:
  - **Migration (`d56a785`):** the owner's book loaded its legacy NVS
    position (6,517). Its first save created the SD files. The periodic save
    ran after three same-chapter turns. Turning back to (6,517) and pressing
    Back saved it.
  - **`0c67039`** adds the newest-copy memory and uses the same layout. It
    reused those files: the owner's book held (6,517) across the same sequence
    and two reboots.
  - **First state files (`0c67039`):** books 3, 12, 18 and 48 each opened and turned three
    pages (crossings included), then Back. After a reboot, Read restored each
    exact position.
  - **Save latency (`0c67039`):** Back saves took 56–65 ms, against 41–53 ms for v1. Each
    key's newest copy is located before writing.
  - **Interrupted Back (`0c67039`):** a hard reset 10, 20, 30, 40, 50, 60, 70 or 100 ms after
    pressing. Up to 60 ms the previous position came back; from 70 ms the new
    one. Every later save succeeded.
  - **NVS:** the region's SHA-256 was still `27c8fd17…`.
- **v1 layout** (2,048-byte files, one copy per key, names keyed by a 64-hex
  constant), 2026-10-04/05. Build `e7b2828` plus a diagnostic print of each
  state-file open:
  - Legacy migration of the owner's book at (6,517).
  - First opens of four unopened books with exact restores across reboots.
  - A periodic save after three same-chapter turns, and every Back save: no
    failure and no exhausted descriptors.
  - Back interrupted by a hard reset at 10–100 ms: the previous or the new
    position, never neither, and the next save succeeded.
  - NVS byte-identical (SHA-256 `27c8fd17…`) from before the first SD build
    through all of those tests.

**Owner (cannot be automated).**

- Actual battery sleep and wake: the owner reports it works, untimed.
- A real power interruption (battery disconnect) during reading: not run.
  There is no verified safe way to isolate the battery, and a reset or USB
  disconnect is not a substitute, because hard resets keep RTC memory and
  power.
- Card removal and reinsertion, and a wake without the card: mostly observed
  with the owner's hands; see "Card removal". Not observed: `UNSAVED`, Read
  after a wake without the card, and a swap made while no book was held.
