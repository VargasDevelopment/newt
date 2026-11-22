# NEWT-50: durable sync proof (GitHub #111)

A durable store may report a write as saved only after a successful sync of
those bytes. Otherwise it reports the write unconfirmed: the complete old or
new value may survive, so callers keep their previous runtime value and never
say "unsaved". This closes #111 as a class for every store: reading progress,
resume intent, the selected book, statistics, the Home clock zone and format,
the DEV Home preference and the NVS preference stores.

On battery every X3 sleep is a full power-off, so each wake is a cold boot.
Bytes that only reached the file system's buffers are gone on the next boot.

## 1. What was wrong

`SdReadingState::putBytes` fails when its write or `fsync` fails. The bytes it
wrote still sit in the FAT buffers and read back exactly. Three patterns
treated that readback as proof:

1. **Identical-record skips.** A save whose record matched the stored one
   returned saved without writing. After a failed sync the "stored" record
   was the buffered one.
2. **Lost-acknowledgement reconcile.** Progress, sleep destination, zone,
   format and DEV Home re-read the record after a failed write and reported
   saved when it matched.
3. **No retry.** A single transient failed sync reported unconfirmed
   although one more write would have synced the whole file (only statistics
   retried).

The selection check (`checkSelection`) had the same flaw: a buffered matching
selection restored saving.

Review r1 found two more. A retry picked its target copy from a readback
that could include unsynced bytes, so after repeated failed syncs and a torn
write it could overwrite the only synced copy of the last acknowledged value.
And the statistics seed summed the open book's stored progress, which can
hold an unconfirmed save, while later folds use the acknowledged counters.

Review r2 found that a clean FatFs handle's successful sync is a no-op. It
cannot confirm an earlier handle's failed write. A corrected host model
reproduced false acknowledgements on all eight state-store paths; this was
a proof gap, with no demonstrated hardware failure.

## 2. The rule

`firmware/durable_commit.h` holds the one rule.

- `commitDurableBytes(store,key,bytes,size,readback)` returns
  `DurableCommit` (`[[nodiscard]]`, `Saved` or `Unconfirmed`). `Saved` needs
  the backend's acknowledgement (SD: its `fsync`; NVS: `nvs_set_blob`) and an
  intact readback. On a buffered backend a failed attempt repeats once: that
  sync makes the whole file durable, the first attempt's bytes included.
- Every backend declares `readbackDurable()`. `storedRecordDurable(store)`
  static-asserts the declaration, and is the only gate for an identical skip
  or a readback reconcile. A readback may say which complete value is
  currently readable, never by itself that it is durable.
- `SdReadingState` sets `syncFailed_` on any failed write, sync or repair.
  Before another transaction reads or a retry chooses an overwrite target,
  it writes one byte in unused sector 7 and syncs that same handle. This
  forces the modified-handle volume barrier; failure keeps the operation
  unconfirmed. The barrier never touches a record sector, including on a
  torn write. Its success clears uncertainty, so later transactions need no
  marker. Recovery reads open writable until that barrier succeeds; if
  storage cannot support the write, they fail closed. Only the open
  transaction's unsynced bytes make `readbackDurable()` false.
  `NvsProgressBackend` answers for both media:
  `!state_ || state_->readbackDurable()`.
- Raw writes need a `DurableWrite` capability that only `commitDurableBytes`
  creates. Every `putBytes` takes it first and adapters forward it, so a
  store that writes around the primitive does not compile. Host tests that
  seed a backend directly use `tests/raw_write_test_access.h`.
  `tests/durable_commit_guard_test.py` proves the compile failure and keeps a
  scan as a backstop: every product `putBytes` call, however spelled or
  split, is the primitive's own write or a listed forwarder of its
  capability (full path, reason checked against the call), and firmware
  never names the test access.

Store APIs keep their `bool` (true means `Saved`, false means unconfirmed),
so measured stack keys and callers stay unchanged; statistics map the type
with an exhaustive `switch`.

### Why the SD barrier is effective

The installed SDK identifies ESP-IDF 5.5.2, commit `87912cd291`
(`framework-arduinoespressif32-libs/versions.txt`). At that exact revision:

- `components/fatfs/src/ff.c:4184` skips all sync work unless this `FIL` has
  `FA_MODIFIED`. Lines 4234–4235 clear that flag even when `sync_fs` fails.
  A fresh handle and a handle after a late sync failure can both sync with
  no I/O. `f_close` calls `f_sync` (4261); VFS then clears the `FIL` even on
  failure (`components/fatfs/vfs/vfs_fat.c:623–624`, 350–352).
- Per-file caching is enabled in the pinned SDK. A dirty file's sector
  buffer is written before the volume barrier (`ff.c:4186–4188`); it is
  distinct from the volume window (`ff.c:1060–1077`). VFS discards closed
  handles, and Newt has only one mutable state handle: bytes readable by a
  new handle are below the discarded private buffer. A retry on the same
  handle flushes its own private buffer as part of its dirty sync.
- A nonempty write sets `FA_MODIFIED` (`ff.c:4160`). Its successful sync
  flushes that handle's data and calls `sync_fs` (4234), which flushes the
  volume window and reaches `CTRL_SYNC` (1120–1135). The marker therefore
  confirms prior readable lower-layer writes without rewriting either A/B
  record. It does not claim to flush another live file's private buffer.

The shipped `libfatfs.a` disassembly independently confirms the modified-bit
test (`andi ... 64`, branch around the writes and `sync_fs`), the flag clear
after `sync_fs`, and `f_close`'s call to `f_sync`. The round 3 receipt retains
source URLs, hashes, line-numbered excerpts and those binary excerpts.

## 3. NVS is durable on readback

Checked against the pinned IDF 5.5.2 prebuilt `nvs_flash` library (read-only
disassembly): `nvs_commit` returns without I/O, and `nvs_set_blob` programs
flash through `esp_partition_write` before it returns, with no write cache.
So every NVS record read back is the one a cold boot finds;
`NvsBlobBackend::readbackDurable()` is always true and NVS stores keep their
identical skip. The skip still asks `storedRecordDurable`, so a store moved to
a buffered backend cannot skip silently.

## 4. Sites

| Site | Backend | Before | After | Why |
| --- | --- | --- | --- | --- |
| `ProgressStore::save` (per-book progress via `ExternalProgressStore`; sample progress `newt_reader`) | SD; NVS for the sample | identical skip; write + readback; readback reconcile after `end()` | skip only while durable; `commitDurableBytes`; reconcile only while durable | buffered bytes read back the same |
| `ExternalProgressStore` digest | SD | write + readback | `commitDurableBytes` via `CheckedStore` (declares the owner's durability) | same |
| `DurableRecordStore::save` (resume intent, zone, format, DEV Home, selection) | SD | identical skip; write + readback | skip only while durable; `commitDurableBytes` | same |
| `DurableRecordStore::saveProtected` (selection) | SD | identical skip; write + readback | skip only while durable; `commitDurableBytes` | same |
| `persistSleepDestination` | SD | readback reconcile | reconcile only while durable | same |
| `commitTimeZone`, `commitClockFormat`, `commitDevelopmentHomePreference` | SD | readback reconcile | reconcile only while durable | same |
| `LibraryReaderSelection::checkSelection` | SD | a matching selection restored saving | a match while unsynced stays unconfirmed and owes one rewrite | same |
| `stats_detail::save` | SD | two attempts, readback | `commitDurableBytes`; exhaustive `switch` to `StatsSave` | one rule; retry only on a buffered backend |
| `PackageStore`, `PausePreferencesStore`, `ReaderPreferencesStore` | NVS | identical skip; write + readback | `commitDurableBytes` (no retry: durable); skip asks `storedRecordDurable` | NVS readback is durable (section 3) |
| `SdReadingState` | SD | no durability signal | after a failure, a one-byte unused-sector write plus modified-handle sync before another read or overwrite; `readbackDurable()` is transaction-scoped | an effective barrier preserves the last synced record |
| statistics seed (`reading_stats_runtime.h`) | SD | summed stored progress at any time | reader idle and Home wait while a progress save is unconfirmed | a fold below the seeded watermark lowers it, so stored unconfirmed reading counted twice |
| NVS backend and key adapters | NVS | none | `readbackDurable()` true / forwarded | section 3 |
| `tools/host.cpp` preview backends | RAM | none | `readbackDurable()` true | nothing is buffered |

## 5. Cost

- **Page turns:** unchanged. The primitive runs only inside existing saves;
  no SD write or sync was added to the page-turn path.
- **Healthy boot:** unchanged. Every identical skip still applies. The only
  new I/O is the retry: one more write and sync after a failed SD attempt.
- **Recovery after a failed SD write, sync or repair:** one one-byte marker
  write and one effective sync per barrier attempt. This may occur during a
  recovery read, so that read needs writable storage. Once it succeeds the
  extra I/O stops; there is no boot-long marker overhead and NVS skips stay
  independent. A persistent fault keeps attempts bounded by the existing
  commit retry and caller readback, and reports unconfirmed.
- **Host counts across all eight state paths:** healthy identical save:
  zero writes/barriers; changed save: one 256-byte write and one barrier;
  recovery load: one one-byte write and one barrier; recovered identical
  save: zero writes/barriers. One transient failure costs the failed record
  write, a marker barrier, then the record retry (three writes, 513 bytes,
  three attempted barriers). These are logical requests, not SD-sector
  traffic or hardware timing. Idle progress saves and sleep checkpoints
  retain their healthy cost; only recovery adds the marker.
- Statistics already wrote every save; unchanged.

## 6. Reading statistics bound

docs/076 ("What can be lost") holds: the fold counts only acknowledged
reading. The seed sums the card's stored progress, so it runs only while no
progress save is unconfirmed: then the card holds no reading that RAM has
not acknowledged. Both seed duties (reader idle and Home) wait for the next
verified save; a cold boot clears the notice, and the card then agrees with
what the reader loads. The seed keeps its signature and its selected stack
measurements.

`reading_stats_firmware_test` "unconfirmed seed" runs the actual duties:
acknowledged 100.4 s, an unconfirmed save at 131.9 s, idle folds, Home, then
a verified checkpoint at 160.4 s counts 160 s (base `5e458caa`: 191 s). Its
two controls remove one gate each. `reading_stats_store_test` "unconfirmed
progress cold boot" saves progress with every sync failing, folds,
cold-boots under each keep variant and reads on. Statistics count exactly the
acknowledged reading the card kept: never twice, and never less than that
minus the one unacknowledged span (31.5 s in the test). "Unconfirmed progress
handover" switches books after an unconfirmed save and back after a cold boot
without counting more than was read.

## 7. Tests

All run against the real firmware headers; host evidence only.

- `durable_sync_proof_test` (real `NvsProgressBackend` + `SdReadingState`, the
  shared `tests/sd_fault_fs.h` transient-fsync model): every SD store under
  identical skip, unconfirmed cold boot (each keep variant), transient retry,
  identical after unconfirmed, failed commit, first progress unconfirmed and
  clean-handle no-op sync, barrier cost, and repeated failure tear (two saves whose syncs fail, a retry, then a torn
  write; a cold boot must keep the acknowledged value or the new one). A
  progress save that protects an older checkpoint is a store of its own.
  7 controls each remove one sync-proof line, including the marker write.
  With an effective barrier the stores' guards
  never fire (reads are synced first); the unit fakes below and the wiring
  fakes cover them.
- Firmware wiring, each with a sync-proof control:
  - `time_zone_firmware_test`, `clock_format_firmware_test`: actual boot
    restore and Settings dispatch;
  - `stage_home_firmware_test`: actual startup and DEV Home selector;
  - `sd_sleep_sync_test`: `main.cpp`'s sleep checkpoint and cold power-on
    authorization;
  - `sd_selection_reliability_test` "unsynced match": card leaves and returns.
  - Wiring fakes share `tests/unsynced_slots.h` (bytes buffered this boot;
    a cold boot keeps only the last synced bytes). It is stricter than
    `SdReadingState`: readback stays non-durable until reboot.
- `progress_store_test`, `durable_record_store_test`: a buffered fake shows
  that an identical record is written again and a lost acknowledgement is
  never reconciled from a buffered readback; each fails with its guard
  removed.
- `reading_stats_firmware_test` "unconfirmed seed" and
  `reading_stats_store_test` "unconfirmed progress cold boot" and
  "handover" (section 6), plus direct unconfirmed-save checks for ledger,
  month and book facts, and a failed-close repair acknowledgement check.
  The model includes `f_close`'s dirty sync: creation and evidence syncs
  have that additional barrier, so removing only their explicit sync is
  no longer a missing-barrier negative control. Crash/transient matrices
  still cover those paths and close failures.
- `durable_commit_guard_test`: four ways to write without the capability do
  not compile; six scan bypasses (an unqualified call, a split line, an
  allowed call under the same file name elsewhere, an allowed call outside a
  forwarder, a forwarder with another capability, the test access in
  firmware) and three wrong reasons are caught.
- `reading_state_backend_test`: a single torn SD write is rewritten and synced
  by the retry; torn-write cases that model power loss also fail the retry.
- `checkpoint_identity_transaction_test`: a state write that exhausts the
  EINTR bound is the failed operation; the commit's one retry then lands
  and syncs. Persistent state EINTR still fails the save.

Every round 1 regression fails on `4606a896`; every round 2 regression and
guard bypass fails on, or passes unnoticed through, `5e458caa`.
The clean-handle regression fails on `802d31b0` for all eight state paths.
The stats contracts reject independent mutations that acknowledge a failed
save, omit its record sync, or acknowledge repair after a failed close sync.

## 8. Resources

The following are historical round 1 measurements: clean four-lane health
reports at `621ceb82`, against base `4606a896` (record 59's measurements).
They do not establish the resources of the round 2 firmware.

| Lane | Flash base → now | Change | SRAM | RTC |
| --- | ---: | ---: | ---: | ---: |
| default | 2,231,872 → 2,231,776 | −96 | 133,232 (same) | 300 (same) |
| DEV USB | 2,382,464 → 2,382,368 | −96 | 134,064 (same) | 304 (same) |
| regional | 2,238,528 → 2,238,448 | −80 | 133,728 (same) | 300 (same) |
| regional DEV USB | 2,391,168 → 2,391,056 | −112 | 134,560 (same) | 304 (same) |

Round 1 static frames: every selected frame stayed within its ceiling
(`checkSelection` stays 192 B). The store frames that now call the primitive
shrink 16–32 B (for example `DurableRecordStore::save` 496 → 480,
`ProgressStore::saveInternal` 368 → 352). The primitive is a new, unselected
48/64 B frame below them, so those save paths are about 32–48 B deeper. The
loop task has 16,384 B; its lowest observed free stack is 7,052 B (docs/060).
The round 1 `unsynced_` flag fitted in the padding beside `unavailable_` in
the heap-allocated `SdReadingState`; static RAM was unchanged.

Final integration merges main `442d91e7` with both parents retained. Clean
four-lane measurements at `4720c22d` back flash record **63** and protected
update **40**, appended after main's records 62/39 without changing them.
Both `commitDurableBytes` frames are now selected in every variant: **64 B**
for `ExternalProgressStore<NvsProgressBackend>::CheckedStore`, **48 B** for
`NvsProgressBackend`. The record also tightens 34 verified existing selected
ceilings (8/9/8/9); no existing stack or RAM ceiling rises and no selected
entry is removed. This closes the deferred stack-ratchet P2.

| Lane | Main flash → integrated flash | Change | SRAM before → after | RTC before → after |
| --- | ---: | ---: | ---: | ---: |
| default | 2,232,112 → 2,232,352 | +240 | 133,232 → 133,232 | 300 → 300 |
| DEV USB | 2,386,208 → 2,386,576 | +368 | 134,320 → 134,320 | 304 → 304 |
| regional | 2,238,784 → 2,239,008 | +224 | 133,728 → 133,728 | 300 → 300 |
| regional DEV USB | 2,395,424 → 2,395,792 | +368 | 134,816 → 134,816 | 304 → 304 |

The measured flash growth funds the reviewed durability, retry, seeding and
capability fixes. Floors, reserve, partitions, exclusions and adapters are
unchanged. Static frames do not prove whole-task stack safety or runtime heap
margins. The final receipt records clean verification lanes, adoption against
`442d91e7` and the full/pinned release gates. Earlier interrupted lane outputs
remain unverified and are not acceptance evidence. No hardware acceptance is
claimed.

## 9. Limits and follow-ups

- A recovery barrier may add a write to a read transaction after an error;
  failed writable access keeps the commit unconfirmed. Its latency on a slow
  card has not been measured on hardware. Success retires the overhead.
- Only bytes the open transaction itself failed to sync are not durable on
  readback. The primitive repeats such an attempt once; stores never skip or
  reconcile on them.
- The scan covers `firmware/`, `include/`, `src/` and `tools/`; the
  compiler covers everything that builds against `durable_commit.h`,
  including the preview backends in `tools/host.cpp`.
- Compiled resource evidence and host/release gates are separate from the
  physical power-condition checks required for owner acceptance.
- Fault injection exists only on the host. Device checks confirm the normal
  path (no new warnings, exact resume) under USB sleep/wake, battery
  sleep/wake and cold power-on, each recorded separately.
