# Main X3: one owner session (NEWT-46)

> **NEWT-71 note.** This was the one-time switch, now done: Newt (with its
> image mark) runs in app0. `--replace-app` existed only for that first write
> over the older unmarked app0 image and has been removed; with Newt in app0
> the install below needs no consent flag. `boot-crossink` is now
> `boot-other` and `select-crossink-fallback` is `select-other-fallback`
> ([bench-flash](bench-flash.md)).

**Preparation only. No device was accessed for this document.** Hands-on bench
checks below remain pending;
this is not yet ready for owner acceptance. The Lead keeps the private logs. The private capacity numbers below are still
blank: the main switch is blocked until the Lead fills and verifies them.

Use the **one firmware build** on the main X3: regional DEV, with the DEV USB
bridge (owner decision 2026-10-26; there is no other build). Newt goes in **app0, `0x10000`**; keep CrossInk `31ce770` in **app1, `0x780000`**.
The main serial/MAC is in the Lead's private record (`.build/private/x3/main-device/`); `MAIN_MAC` below means it, uppercase. A different descriptor, MAC,
slot or partition table means stop, even if a tool would otherwise proceed.
The two matching full backups are already held privately by the Lead. Tools
also capture a fresh full backup before a write. Keep all of them and the
reviewed candidate; never put dumps or card copies in Git.

Phase 1 of #103 ([083](083-ota-sd-install.md)) keeps the slot rule: with
CrossInk in app1 the tools write only app0. Newt now confirms a new selection
at its first finished screen, whichever it is, and HELLO answers only once that
confirmation is verified; step 6's VALID readback stays the proof. Every install selects app0 with a fresh
record. When Newt already runs in app0, the install rewrites it in place and
first selects CrossInk with a settled record (`select-other-fallback`,
`fallback-md5`), so a new build that never confirms returns to CrossInk in
app1 ([bench-flash](bench-flash.md#a-failed-first-boot)).

## Before the owner arrives

The Lead finishes the bench rehearsal and ordered hands-on checklist, reviews
the main backup's NVS inventory, and supplies an archived clean candidate of
the one build and its reviewed manifest SHA-256. Its gate receipt, resource
report, firmware identity, binaries and manifest must pass `Candidate` checks.
A successful compile or an old receipt alone is insufficient. Record the
candidate revision, build ID and manifest hash. No firmware source change is
required by this preparation.

The image embeds its build ID (`<12-hex revision>-regional-experiment`) and
answers HELLO with it after boot. `Candidate` requires that clean, non-dirty ID
in the image, and the manifest-pinned clean firmware identity receipt binds
revision, variant and `binary_sha256` to the actual image. The install's
`written-range-md5` then compares the full on-chip candidate plus erased
padding before selection, and its HELLO step proves the running build. Also
record the candidate descriptor's ELF SHA-256
(decode `firmware.bin`'s first `x3_slots.HEAD` bytes with `x3_slots.classify`)
for the later `slots` comparison. This descriptor field is neither the binary
SHA-256 nor the Git revision; it supplements the full-range flash comparison.

Only the owner creates a second rule. These exact lines retain the bench rule
and grant only the owner's account access to this main unit; stop if the new file already
exists rather than overwrite it. The owner runs them on the Linux host:

```sh
set -eu
test ! -e /etc/udev/rules.d/99-newt-x3-main.rules
sudo sh -eu -c 'set -C; umask 022; cat > /etc/udev/rules.d/99-newt-x3-main.rules' <<'RULE'
SUBSYSTEM=="tty", ATTRS{idVendor}=="303a", ATTRS{idProduct}=="1001", ATTRS{serial}=="MAIN_MAC", OWNER="OWNER_USER", MODE="0600"
RULE
sudo chown root:root /etc/udev/rules.d/99-newt-x3-main.rules
sudo chmod 0644 /etc/udev/rules.d/99-newt-x3-main.rules
sudo udevadm control --reload-rules
```

Reconnect the main X3 once **before** starting the session. This applies the
rule without a broad udev trigger. Stop on any setup error. If the rule already
exists, the owner checks its exact contents, root ownership and permissions.
Do not change the bench rule or add a global permission workaround. Afterwards
the owner may remove just this second rule and reload rules again.

## The owner session

The Lead records each numbered result, UTC time, power source, reset evidence,
and receipt path. USB supplies power throughout. Close other serial clients;
keep one lease and never unplug, kill a running command, press Reset or open a
second port during its confirmation window. A port open itself resets the X3.
Every command below runs its final reset sequence once (no esptool
close/reopen retries), then holds the port at least 30 seconds before closing; let it finish. This gives confirmation time but does not
prove the app confirmed. `slots` is read-only for flash and also resets on exit.

From the reviewed worktree set these values supplied by the Lead (no `sudo`):

```sh
py=.toolchain/platformio/penv/bin/python
rule=/etc/udev/rules.d/99-newt-x3-main.rules
session=.build/bench/main-owner-SESSION_ID
candidate=/absolute/path/to/reviewed-candidate
manifest=REVIEWED_MANIFEST_SHA256
```

1. **Approve.** The owner explicitly approves this main unit, candidate and
   app0 replacement for this session. Record approval and the preferred final
   app. Without approval, stop. Default final app is CrossInk until the owner
   explicitly chooses Newt; the switch remains reversible.
2. **Identify and read slots.** Run:

   ```sh
   "$py" -B tools/bench_flash.py slots --identity-rule "$rule" --session "$session"
   ```

   Expect USB descriptor + esptool MAC/security/16 MiB checks to PASS. The
   receipt must name the main MAC and rule. Expect app1 CrossInk, app0 the old
   Arduino image (IDF v5.5.4, 2026-07-03), and a VALID selection booting app1.
   Record both descriptors, app0's **full ELF SHA-256**, selection sequences
   and states, and receipt. Stop for refusal, pending/aborted surprises,
   unexpected image or layout. Never substitute the bench's MAC or slot.
3. **Copy the SD card (owner, on a computer).** Turn off/disconnect the X3,
   remove the card and copy the whole card, including hidden files and
   CrossInk state, to a private directory. Verify file counts/sizes and hashes
   against the card. Then copy the books into `/Newt/Books` on the card. This
   is required: Newt reads books only from `/Newt/Books`
   (`firmware/sd_card_paths.h`), and CrossInk keeps reading `/Books`. Copy,
   never move; keep the original EPUBs and CrossInk directories unchanged.
   Record the verified copy location; stop on a copy/verification error.
   Safely eject, reinsert and reconnect USB. Wait at least 30 seconds.
4. **Check NVS (Lead, offline).** Review the Lead's namespace and capacity report
   described below. CrossInk uses `cphw`, `crosspoint`, `eepUser` and ESP-IDF
   namespaces; Newt uses `newt_*` and legacy `nb<id>` (077). Record the report
   and whether any namespace is unexpected. Fill the capacity worksheet below;
   require every threshold, including the reserved GC page. Blank/insufficient
   numbers, parsing failure or unknown/overlapping ownership mean stop. Do not
   erase NVS or run a firmware update in CrossInk.
5. **Install, then leave it alone.** Set `old_app0_elf` to the exact hash from
   step 2; never an example or a binary SHA-256. Run:

   ```sh
   "$py" -B tools/bench_flash.py install "$candidate" --manifest-sha256 "$manifest" \
     --identity-rule "$rule" --session "$session" --replace-app "$old_app0_elf"
   ```

   Expect a full backup, fallback-image, outside MD5 checks (when Newt already
   runs in app0, then select-other-fallback and fallback-md5), **app0-write**, written-range
   MD5, select-newt, selection MD5, **one reset**, **boot-hello-status-diag
   PASS** with the candidate's build ID, and the port held at least 30 s
   before closure. Newt should appear. Record every PASS, receipt and screen;
   FAIL/STOP means use the recovery rules, never retry or add a reset.
6. **Prove confirmation.** After the install returns, run the same `slots`
   command. Require Newt in app0, CrossInk unchanged in app1 and the new
   highest selection **VALID**, booting app0. Require Newt's descriptor ELF
   SHA-256 to match the candidate descriptor recorded before installation.
   Record sequences/states and receipt. NEW, PENDING_VERIFY, ABORTED, a fallback
   to app1, refusal or a
   blank screen means stop. A passing install or 30-second wait alone is not
   this proof.
7. **Read (owner).** Open a Library book, read and turn pages, then Back to
   save. Record its position and any save notice. Stop for a gray/stuck screen,
   incorrect position or an unconfirmed save; do not switch with an owed save.
8. **Try CrossInk (owner).** Settings → Restart into CrossInk → Select to
   open the confirmation → Select to switch. Expect CrossInk, with its books,
   settings and history still present. Wait at least 30 seconds without a port
   open, then run `slots`; require app1's higher record VALID. Record the
   screen and receipt. If the switch is unconfirmed or CrossInk fails to
   confirm, stop and keep the evidence; no repeated Select or reset.
9. **Return to Newt (Lead, USB).** With the same candidate and main rule:

   ```sh
   "$py" -B tools/bench_flash.py boot-newt "$candidate" --manifest-sha256 "$manifest" \
     --identity-rule "$rule" --session "$session"
   ```

   Expect newt-slot-md5, selection proof, one reset, HELLO with the
   candidate's build ID and the held confirmation window; no app bytes written. Then `slots` must show Newt's record VALID
   and boot app0. The owner reopens the book at step 7's saved position.
   Record both receipts and the position. Stop on any mismatch. If the session
   has expired or outside bytes drifted, stop; the Lead reviews and chooses a
   fresh session rather than bypassing the check.
10. **Leave the chosen app (owner).** For Newt, leave it after step 9. For
    CrossInk (default), repeat step 8 and its wait/VALID check, then disconnect
    only after the command returns. Record the owner's choice, final slot and
    VALID sequence. A later USB return to Newt uses step 9, with fresh approval
    for the main unit and a fresh session when needed.

## Recovery: stop first

- **Newt never confirms / rollback starts CrossInk (unconfirmed selection only).** Do not repeatedly open
  ports or press Reset. Wait at least 30 seconds. Preserve the install's
  receipt and shared journal. If no STOP blocks access, the Lead runs `slots`
  with the main rule and records both descriptors and both selection records:
  expect Newt's sequence ABORTED and CrossInk's older sequence VALID, booting
  app1. PENDING_VERIFY means confirmation was not established; do not assume a
  successful Newt boot. With STOP present, the authority below decides before
  any read. Keep CrossInk running, diagnose the failure and rehearse a revised
  candidate on the bench before a separately approved main retry.
- **FAIL before writing:** flash is unchanged by that attempt. Keep logs,
  stop and have the Lead inspect; do not blindly retry. **STOP after a write
  may have started:** the Lead decides only when the saved outside MD5 proof
  is intact; the owner decides for unknown/changed outside bytes or any other
  case. Follow [bench-flash.md](bench-flash.md#per-unit-lease-and-failure-state)
  to archive STOP and the unconfirmed journal under the same unit lock, only
  after that decision. Never delete/recreate `lock` or clear only STOP.
- **Confirmed Newt cannot reach Settings:** a VALID record stays selected
  across resets; rollback will not rescue it. The owner approves selecting
  CrossInk on the main unit. With no blocking STOP, the Lead runs:

  ```sh
  "$py" -B tools/bench_flash.py boot-other "$candidate" --manifest-sha256 "$manifest" \
    --identity-rule "$rule" --session "$session"
  ```

  Expect CrossInk descriptor + full-slot MD5 against the session backup,
  unchanged app bytes, one safe selection-sector write (or a proven no-op),
  one reset and a 30 s held window. After return, `slots` must show app1
  CrossInk VALID. Neither app is rewritten. USB recovery bypasses Newt's exit
  save: expect the last acknowledged checkpoint, and record any unconfirmed
  reading. Stop on FAIL/STOP; use the same
  Lead/owner authority above before further access. A fresh session is allowed
  only after reviewing any expiry/outside-byte drift, never as a STOP bypass.
- **Keep CrossInk for good:** use step 8, wait and establish app1 VALID. Leave
  Newt inert in app0; no erase, firmware update, NVS or SD cleanup is needed.
  Retain the card copy/backups. If Newt cannot reach Settings after VALID, use
  the owner-approved `boot-other` recovery above; resets alone keep booting it.
- **Restore the old app0:** this is a separate owner-approved Lead recovery,
  not `bench_flash.py restore` (that accepts reviewed Newt candidates). First
  keep CrossInk selected/VALID. The Lead privately verifies both original full
  backup hashes, extracts **only app0, offset `0x10000`, length `0x770000`**,
  and checks its descriptor/ELF hash against step 2. Before doing anything,
  prepare and review a bounded app0-only restore with the same main identity,
  unit lease, fresh full backup, no retries, and before/after on-chip MD5 of
  every byte outside app0. The owner approves that exact recovery operation.
  Write/verify app0 only, keep CrossInk's VALID selection and app1 untouched,
  hold the reset handle for 30 seconds, then read slots: original app0 hash,
  original CrossInk app1 and app1 VALID. Keep the recovery receipt. No raw
  full-flash write, erase, bootloader/partition/NVS restore or unreviewed
  esptool one-liner is authorized by this procedure. If app1 will not boot or
  outside bytes differ, stop for an owner-led recovery plan instead.

## NVS report (Lead only)

Never give this worker the private backup. The Lead privately verifies its
backup hash, extracts NVS at `0x9000` for `0x5000` bytes into a mode-0600
partition file, and runs:

```sh
python3 -B tools/nvs_namespaces.py /private/path/to/nvs-partition.bin
```

The lister prints namespace names/header counts and a TOTAL line with only
physical counts: pages, used slots **including payload spans**, erased slots,
free slots, fully erased pages, and the longest free run after reserving one
fully erased page (`append_run`). Header counts alone do not measure capacity.
Erased slots are not immediately reusable and receive no capacity credit.
The whole image is validated before output. Fixed types/indexes must span one
slot; string/legacy-blob/blob-data spans must equal `1 + ceil(size/32)`.
Erased headers still own their payload. Ambiguous, unsupported or corrupt
metadata stops without output; no ordinary key or payload value is printed.

### NVS capacity stop rule (Lead fills from the private extract)

Five 4 KiB pages provide **630 entry slots**, with **one whole 126-slot page
reserved for GC**. Preserve all existing CrossInk, IDF and legacy Newt data.
No erase, cleanup or fallback into NVS is authorized to make this fit.

Newt's native writers are fixed-size A/B blobs (both `slot0` and `slot1`),
except the saved Wi-Fi network's key, one blob (docs/085):

| Namespace | Maximum bytes per blob | Conservative slots per blob | Blobs + namespace |
| --- | ---: | ---: | ---: |
| `newt_reader` (built-in sample) | 52 | 5 | 11 |
| `newt_apps` | 691 | 45 | 91 |
| `newt_prefs` | 24 | 3 | 7 |
| `newt_pause` | 20 | 3 | 7 |
| `newt_wifi` (one `key` blob) | 32 | 3 | 4 |
| **Native steady maximum budget** | | | **120** |

For a v2 blob, a data chunk needs its header plus payload slots, and its key
needs one blob-index slot. The pinned IDF `nvs.h:317–320` documents `2 + ceil(bytes/32)` (75 steady
slots across all five namespaces); the format permits chunk headers too.
The budget above instead permits **each 32-byte payload slot its own chunk
header**: `2 * ceil(bytes/32) + 1`. Non-final chunks fill whole payload slots;
this deliberately overcounts fragmentation instead of assuming one chunk.
One namespace entry is included for each of the five names. Updating the
largest blob may hold its old and replacement bytes together: reserve another
**45 slots**, giving **165 Newt slots including an in-flight replacement**.
Newt writes one blob at a time, so that reserve also covers replacing the
Wi-Fi key (3 slots).

Source: `include/newt/reading_progress.h` (52), `firmware/package_store.h`
(691), `include/newt/reader_preferences.h` (24), `include/newt/pause.h` (20),
`firmware/wifi_network_store.h` (`kWifiKeyBytes`, 32), and the matching
`firmware/nvs_*_backend.h` and `firmware/wifi_network_device.h` key/size
restrictions. Both A/B keys are included at maximum size, including records a default install has not
created yet. This budget applies to the one firmware build.

**Statistics are included, at zero native NVS growth.** The largest fixed stats
record is the **180-byte ledger** (`include/newt/reading_stats.h`), with two
SD record slots in `newt_stats`; closed months (104 bytes), per-book facts
(64 bytes), selected book, resume, zone, format, DEV Home, the saved Wi-Fi
network (`newt_net`, docs/085) and `nb<id>` are also routed to SD.
`LibraryReaderSelection` attaches the router in its constructor
(`firmware/library_reader.h`); `firmware/main.cpp` shares that router with
progress/resume/stats. `nvs_progress_backend.h` owns these routes even when
allocation/card access fails: no fallback NVS write. Legacy NVS is read-only
and is already charged in the private dump's physical used/erased totals.

Record this worksheet in the private session receipt; **every blank is a stop**:

- Image SHA-256: ______; pages **must be 5**: ______.
- Used `U`: ______; erased `E`: ______; free `F`: ______; `U+E+F` **must be 630**.
- Fully erased pages **at least 1**: ______ (one remains reserved for GC).
- Free outside that page: `F - 126 = ____` **at least 165**.
- Equivalently `F >= 291`, or `U + E + 165 <= 504`: PASS/STOP ______.
- `append_run` **at least 25** for the largest one-chunk blob/index
  plus its new namespace, in an ACTIVE/erased page outside the GC reserve: ______.
- Namespaces understood/separate: ______; Lead/date/approval receipt: ______.

This does **not yet claim the main backup fits**; only the Lead may read it
and fill the numbers. Insufficient space stops the main session. Do not count
erased slots as free or borrow the GC reserve.

Full-NVS host evidence is `tests/nvs_backend_test.cpp`, the `fullNvs` cases:
the real sample/progress, app, reader-preferences and Pause backends, plus the
unattached router with a 180-byte stats ledger, receive pinned native
`ESP_ERR_NVS_NOT_ENOUGH_SPACE` (0x1105). `commitDurableBytes` returns
Unconfirmed, does not commit and leaves the prior bytes readable. Production
stats uses SD; this extra case tests the shared native seam, not a fallback.
`tests/reading_state_backend_test.cpp` (`mountFailure`, `allocationFailure`)
and `tests/reading_stats_store_test.cpp` cover SD-only routing/failure. No
firmware change or full-flash simulation is claimed; actual NVS allocator and
GC behavior remains separate from the host fault shim.

## Bench rehearsal for the Lead

**Pending; bench X3 only (the default bench rule's unit), awake on USB.** Use the final
reviewed **same candidate and manifest** as the main session,
the default bench rule (omit `--identity-rule`), and a fresh private session.
No buttons are needed for steps 1–5. The bench has CrossInk in app0 and Newt in app1; the main has
those roles reversed. Read both, never hardcode bench slot numbers in tools.

1. `slots`: record current descriptors and VALID selection, receipt and power.
   Stop for an unknown layout/STOP. Preserve the current DEV candidate for
   later restoration if needed.
2. `install CANDIDATE --manifest-sha256 HASH --session FRESH_SESSION`: expect
   app1-write, outside/written/selection MD5 PASS, one reset, at least 30 s
   held with no control-line changes, then close. Record timed console output,
   receipt and actual screen. Written bytes are verified against the receipt-bound
   candidate, and HELLO must report its build ID.
3. Before opening another port, observe the close with an **external camera**
   or existing independent reset trace: record from the initial reset through
   30 s after closure, looking for a second boot/repaint. Do not use a serial
   monitor (opening it is a confounding reset). If optics cannot distinguish a
   reboot, record close-reset behavior **inconclusive**, not "no reset".
4. `slots`: require Newt app1, CrossInk app0 unchanged and Newt's sequence
   VALID, boots app1; compare Newt's descriptor ELF SHA-256 with the candidate's.
   On the bench, Newt's slot already boots, so the install rewrites it in
   place: it writes CrossInk's settled record, app1, then Newt's fresh record,
   and this VALID record is the new image's own confirmation. Step 5's round
   trip, `boot-other` then `boot-newt`, with `slots` VALID after each,
   still proves recovery. NEW/PENDING/ABORTED or
   CrossInk fallback means failure: stop, keep evidence and diagnose. A failed
   confirmation may be changed by this diagnostic command's own port open;
   preserve its first readback and do not retry. If no independent reset
   evidence is available, the main switch stays pending that observation.
5. **Lead alone (the confirmation proof):** once Newt is VALID, rehearse confirmed-Newt recovery with
   `boot-other CANDIDATE --manifest-sha256 HASH --session FRESH_SESSION`.
   Expect CrossInk slot MD5 and only a selection-sector write, one reset and
   held window; `slots` requires CrossInk VALID. Return with `boot-newt`, then
   require Newt VALID. Record both app-preservation MD5 PASS steps and selection records.
6. **Buttons needed:** open/read/Back, restart into CrossInk, wait at least
   30 s, `slots` requires CrossInk VALID; `boot-newt` with the
   candidate, wait for completion, `slots` requires Newt VALID. Reopen the
   saved book. Stop for any failure. Record each receipt and optical result.

The hold avoids an early second reset regardless of close behavior. Hardware
must still establish that this build confirms within the window. USB
rehearsal establishes no battery-sleep or cold-power result.

## Owner decisions

- Main build: the one firmware build (regional DEV, with the DEV USB bridge),
  per the owner's 2026-10-26 one-build decision.
- Default final slot: CrossInk app1 unless the owner explicitly chooses Newt.
- Use the conservative 165-slot Newt update budget plus a separate 126-slot GC
  reserve. Missing main capacity numbers block installation; no erase/repair.
- Confirmed-Newt recovery on main needs owner approval; STOP recovery keeps
  the existing Lead/owner decision split and never changes either app slot.
- Any ambiguous identity, namespace or confirmation stops the session; preserve
  both apps and all backups. Do not repair, overwrite or retry by default.
