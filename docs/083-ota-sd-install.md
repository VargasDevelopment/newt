# 083: Installing updates from the SD card (#103 phase 1)

**Status:** implemented on `feat/ota-sd-install`. Host tests, compile and
resource evidence only. No device was used. Bench checks and the owner's
three power-condition checks are pending, so this is not ready for owner
acceptance. Design and owner decisions: [082](082-ota-updates.md).

Phase 1 lets the X3 install a Newt image, or since NEWT-71 any other X3
firmware's `.bin`, from `/Newt/Update/` on the card. It keeps the partition table and writes the slot
that is not running. Not in phase 1: Wi-Fi, phone setup, a boot combo, and
going back to the previous Newt.

## The flow

1. The publisher builds clean and writes `newt.bin` and `newt.manifest`
   ([RELEASING](RELEASING.md#publish-an-update)). The owner copies both to
   `/Newt/Update/`.
2. Settings → **Update** runs the reader's exit save, as the firmware switch
   does. An unconfirmed save keeps Newt open and says so on the row.
   Settings → **Wi-Fi** does the same, and its request opens update mode on
   Wi-Fi networks instead ([085](085-wifi-setup.md)); Update lists no Wi-Fi.
3. Otherwise Newt sets a one-shot request and restarts.
4. The next boot sees the request as soon as the panel is ready and runs
   update mode instead of the app, the reader or the library.
5. Update mode lists the card's offers. Only a newer Newt can be installed
   directly. The same version needs an explicit reinstall confirm. An older
   one is shown but refused.
6. Replacing another firmware in the other slot, or installing another
   firmware's `.bin`, asks once.
7. Installs run on USB or battery ([082](082-ota-updates.md), rule 8). The
   installer checks the whole file, then erases and writes the other slot,
   reads it back and selects it.
8. Newt restarts into the new image. That image confirms itself at its
   first finished screen, whichever screen it is. If it never shows one, or
   its confirmation cannot be verified, the next boot returns to the
   previous image.
9. The first time a new build reaches Home, it shows "Updated to <version>"
   with What's new, once.
10. Back, Sleep or ten idle minutes leave update mode (a restart into Newt).

## Card files: one release format

A Newt update is two files, the same for every source (the card, a release
and the online check): `newt.bin` and `newt.manifest`. The manifest is
LF-terminated
`key=value` lines in a fixed order (`include/newt/update_manifest.h`):

```text
format=newt-update-1
kind=newt
version=0.1.0-alpha.1
build=0123456789ab
size=2414608
sha256=<64 lowercase hex>
```

Nothing in it names a source or a path. Anything else, including a missing,
repeated or extra line or the retired `kind=crossink`, is refused.
`tools/update_publish.py` writes the pair.

Newt's two card paths (`firmware/update_card.h`) are built and sized from
the directory, the name and the extension at compile time, never formatted
into a buffer at run time. 287eedea formatted them into one 16 bytes past the
directory, which cut `crossink.manifest` to `crossink.manife`, so update mode
never offered CrossInk.

### Another firmware's file (NEWT-71)

Any other firmware installs from its own release file, as published, with no
manifest: copy the `.bin` into `/Newt/Update/`, the folder Newt's own update
uses, so there is one place to look. Update mode lists every `*.bin` there in
any letter case except `newt.bin`, hidden files (macOS leaves `._` copies) and
names of 48 bytes or more. It shows the first three by name (byte order) and
says when there are more. Each row reads the file's first 512 bytes: a known
firmware is named with its version ("CrossPoint 2.0.0"), others by file name.
A file that is Newt's image is listed but not installable (Newt updates need
its manifest), and a file that is not an X3 app says so.

Before writing, the installer checks the whole file: no larger than the
other slot (`0x770000`), an ESP32-C3 image header with chip ID 5, 1 to 16
segments inside the file, placed as the bootloader requires (each length a
multiple of 4; a cache-mapped segment at the same offset in its 64 KiB page
as its data), the XOR checksum and, when the header says one is appended, the
SHA-256 of every byte before it. Its head must still be another
firmware's (no Newt mark) with the version and ELF SHA-256 the list read, so a
file swapped after the list is refused. The digest of the checked bytes is
kept, and the slot's readback must match it. There is no approved list: the
owner chose (2026-11-04) to let people install any firmware after one
consent.

The consent page names the file, says what it replaces ("It replaces
CrossInk on this X3.", the spare Newt, an empty or a damaged slot) and that
"Newt stays installed. The X3 then restarts into the new firmware. To come
back to Newt, install newt.bin from that firmware or a computer." (for an
unnamed firmware: "install it from a computer."). The page never says "slot"
(owner, #365). After a
verified install the X3 restarts into it; a failed write keeps Newt running and
says so; an unconfirmed one says either may start.

Versions are SemVer 2.0.0 (`0.1.0-alpha.N`). Numeric pre-release parts compare
as numbers. The X3 compares the manifest's version with its own and offers
only a newer one; the same version is an explicit reinstall.

## Update mode is its own boot

Setup takes the request right after the panel is ready
(`enterRequestedUpdateMode`, `firmware/update_mode_runtime.h`). Update mode
never returns: it leaves only by restarting. Reading therefore holds none of
its memory, and it never meets an open book.

The request is three words in RTC memory. It rides in the SD resume record's
storage while that record is empty (magic 0): version `0x5544574e` and its
complement as the checksum (`firmware/update_request.h`). It adds no RTC
bytes. Only a software restart honors it. Any reset clears a matching
request, so one request starts update mode at most once. A resume record is
never touched. A lost request just boots Newt normally.

On entry, update mode starts the buttons, reads the battery and clock, and
scans the card: each `<kind>.manifest`, what the other slot holds, and
whether the last install or switch rolled back. The page
(`include/newt/update_screen.h`) is shared by the device and the host preview.

**USB power.** None needed since 2026-10-31 (owner decision, [082](082-ota-updates.md)
rule 8). Until then update mode asked for USB to a computer. The install is
power-loss safe, so there is nothing to gate: a battery that dies mid-install
loses only that attempt, and the running Newt still starts. Battery current,
gauge accuracy and brownout at low charge are not measured (a known gap, not
a gate). Installing on battery is a separate hands-on check, still pending.

## The installer

One writer for every source and both kinds (`include/newt/slot_installer.h`),
plain C++ over ports, so the host fault harness runs the device's code.

- **Checks first, on the card file:** size, the image check (segments,
  checksum and appended digest) and the identity: for Newt, the manifest's
  SHA-256 of the whole file and Newt's mark and version/build block; for
  another firmware, its descriptor as listed. Older Newt is refused; the same
  version needs consent.
- **Then the device:** a readable head on the other slot (one that cannot be
  read may hide another firmware, so it is refused, `TargetUnreadable`),
  consent when that slot holds another firmware or the file is one, and a
  settled running selection (a pending image is refused).
- **Write:** erase the other slot in 64 KiB steps and write from its second
  sector on. The first sector (the image header) stays in RAM and is written
  last, so the slot's head reads empty until the end.
- **Verify and select:** read the whole slot back and check digest, image and
  identity. A failure after the first erase erases the first sector again. A
  fresh state-new record then selects the new image.
- **Result:** None, or a reason. Reasons before the write change nothing.
  Write, readback and selection failures leave the running Newt selected.
  Unconfirmed means the next boot may start either image.

Update mode holds three 4 KiB sectors of heap only during one install, and
reads the card through one of them.

Until NEWT-71, CrossInk installed only as `crossink.bin` with a manifest and
a whole-file SHA-256 on a compiled approved list of two reviewed images. Both
are gone: any firmware's own `.bin` installs after consent.

There is no check code on the card. The owner copies the files to their own
card, so the page shows the version and build only. The check code
([082](082-ota-updates.md), section 6) belongs to the Wi-Fi phase, where the
bytes come over a network.

## Confirm after the first finished screen

The rollback bootloader starts a newly selected image as pending. Arduino
normally confirms it before `setup()`. Newt overrides the weak
`verifyRollbackLater()` hook to return true (`firmware/app_slots.h`) and
confirms in `finishFirstPaint` (`firmware/update_seen.h`), the one place
that decides. It acts on the boot's first finished screen, whichever it is:
Home, a book page or reader error, Library, Settings, Stats, an app, a
notice or the sleep image. Finished is a property of the frame on the panel,
not a list of screens: a completed paint that ended its press. A working
frame, such as "Opening..." while a changed book reopens in the loop, leaves
its press answered, so it never counts. The loop calls the check before each
input and `enterSleep` before the panel sleeps, so every finished screen
meets one of them first (Power behind a working frame shows the sleep image
as the first finished screen). Besides working frames, only two screens
never confirm: a fatal startup failure halts in place, and update mode, its
own boot, restarts. Neither reaches the check.

Confirming is a commit like any other. Newt marks the image valid, then reads
its state back up to three times. A lost reply counts once the state reads
settled. A refused mark, or a state that stays pending or unreadable, leaves
the image unconfirmed: no "Updated to", no record, and the next boot may
return to the previous image (or keep this one, if the mark landed). A build
that never shows a finished screen stays pending, and the next boot returns
to the previous image. `tools/verify-nvs-guard.py` checks the linked hook: one
strong definition that returns 1, reached from `initArduino`.

DEV builds poll their bridge on every loop, but answer only once the image is
confirmed. Before that, the bridge reads each request line and drops it, so
an unconfirmed image never answers HELLO, yet unread bytes never hold back
work that waits for quiet input, such as a changed book's reopen (and with
it the confirmation). `bench_device` resends HELLO until the readiness
budget ends, and its timeout says that Newt answers only once confirmed. A
HELLO answer therefore means the new build showed a finished screen and
confirmed itself. `bench_flash` installs end confirmed and HELLO-verified
([bench-flash](bench-flash.md)). Update mode is its own boot and polls its
own bridge.

## "Updated to" once

The last build this X3 confirmed and noted is a durable SD record in its own
namespace, `newt_seen`: version and exact build, A/B slots and a checksum. It
never shares a payload or commit with a preference, a book or a checkpoint.
There is no RTC copy; every boot reads the card.

Once the image is confirmed, on the first finished Home of the boot:

- A record naming another build shows "Updated to <version>" with What's new
  over Home, then records this build. Any other first screen, such as a
  resumed book's page, keeps its place; the check waits until the owner is
  Home, in this boot or a later one.
- A missing record records this build and shows nothing (the product default
  for a first boot or a new card).
- A damaged record of this format shows nothing and is replaced with this
  build: it is only a note. A record of an unknown format is never replaced.
- An unconfirmed image does neither.
- Nothing reports the commit. A failed or unconfirmed commit leaves the old
  or new record, so the page may show once more on a later boot.

A dirty build carries an empty build field, so it differs from the exact
build of the same version.

## Slots and tools

Beside one other firmware, the tools write only Newt's slot. Without one
(after SD installs of Newt), the slot that boots must hold a selected,
confirmed Newt, and the tools write only the other one. A
damaged other slot needs an explicit `--repair-invalid`.

Every USB install, like an SD one, keeps a verified fallback and writes a
fresh *new* selection for the written slot. A failed first boot returns to:

| Layout | Slot written | Fallback |
| --- | --- | --- |
| bench: CrossInk app0, Newt app1 running | app1, in place | CrossInk, app0 |
| main: Newt app0 running, CrossInk app1 | app0, in place | CrossInk, app1 |
| another firmware running | Newt's slot | that firmware |
| no other firmware | the slot that does not boot | the running, confirmed Newt |

In place, the tool first selects the other firmware with a settled record and proves
it on chip, so the running Newt's record never points at new bytes. An SD
install never writes the running slot; its fallback is the running Newt. See
[bench-flash](bench-flash.md#a-failed-first-boot),
[077](077-return-to-crossink.md) and [080](080-main-device-switch.md).

## Memory

Measured on a clean `--health-report` build (flash record 70, against
`ff4174d0`):

| | Before | After |
| --- | --- | --- |
| Flash | 2,398,048 B | 2,414,448 B (+16,400 B) |
| Static RAM | 133,440 B | 133,440 B |
| RTC | 300 B | 300 B |
| Heap held while reading | 0 B | 0 B |

Flash record 71 (the card path fix, against `287eedea`) measured
2,414,448 → 2,414,592 B (+144 B: the four path arrays and the identity
refusal, less two run-time formats). RAM, RTC and every selected stack are
unchanged.

- Update mode runs only in its own boot. It allocates three 4 KiB sectors
  (12,288 B) of heap for one install and frees them after.
- It traces free heap, largest block and lowest heap at start, at install and
  after (`Newt update mode ...` over DEV USB). The device numbers are pending.
- All 522 selected stack ceilings are unchanged; `setup()` stays at 464 B and
  `loop()` at 192 B. The new frames are unselected: `installUpdate` 1,488 B,
  `checkUpdateImage` 864 B, `scanUpdate` 640 B, `readUpdateOffer` 592 B,
  `appImageIntact` 432 B, `runUpdateMode` 352 B, `renderUpdate` 320 B,
  `finishFirstPaint` 256 B and `runningImageState` 32 B. Static frames do not
  prove runtime peaks.
- No NVS: the seen-build record and the request live on the card and in
  existing RTC words.

## Tests

Each test fails with its change removed; the guard-removal controls run in the
gate.

- `tests/slot_installer_test.cpp` and `tests/slot_installer_controls_test.py`:
  the fault harness (power cut at every write step, bad files, consent, power,
  pending selection), with negative controls. A manifest version or build
  longer than the identity field is refused, never cut to match.
- `tests/update_firmware_test.py`: the actual setup, loop head, sleep entry,
  deferred reopen and dispatch statements, with the actual DEV bridge, for
  the request, the Update row, the confirmation and the `newt_seen` record
  (its wait for Home and its repair). It walks every screen a boot can show
  first, every `Screen` value as setup's paint or behind a working frame,
  and the named exclusions; a HELLO waiting behind a changed book's working
  frame, for a valid image and a new install; refused, lost and unreadable
  SDK answers; cold boot with random RTC, every reset class, missing,
  damaged, unavailable, rejected, lost-acknowledgement and unsynced storage.
  Twenty-one firmware controls and eight source controls, including a list of
  confirming screens, the bridge unread or answering before confirmation,
  the sleep check removed or late, and a fatal failure that confirms.
- `tests/update_card_test.py` and `tests/update_card_test.cpp`: the actual
  card code reads both kinds' manifests and images from a scratch card under
  their own names, and opens nothing when only cut names (every prefix of each
  file name) are there. Controls: 287eedea's path buffer, a cut builder, a
  manifest read from the image's path and one kind's files for both.
- `tests/dev_usb_test.cpp`: unanswered polls read whole requests and never
  reply; a line still arriving when answers resume is answered whole.
- `tests/native_rollback_hook_test.py`: the linked hook check.
- `tests/update_screen_test.cpp`, `tests/system_ui_rule_test.py` and
  `tools/gate_previews.py`: the page in every state, as host previews
  (`artifacts/update-*.png`, `artifacts/device-updated*.png`).
- `tests/slot_vectors_test.py` (#362): the Rust image check, card install,
  slot names and OTA selection against the shared slot vectors
  (`installer/test/x3-slot-vectors.json`), including #168's valid foreign,
  wrong-chip, truncated, bad-checksum, bad-SHA-256 and oversized images, with
  thirteen negative controls.
- `tests/x3_slots_test.py`, `tests/bench_flash_layouts_test.py` and
  `tests/update_publish_test.py`: the tools, including each layout's
  first-boot fallback, and the retired CrossInk manifest refused.

## Hardware checks (pending)

On the bench X3 (CrossInk app0, Newt app1), each a separate fact:

- Install a newer Newt from the card; interrupt an install; force a rollback.
- Another firmware from the card: stock, CrossPoint and CrossInk (NEWT-71).
- USB restore and repair for every layout.
- Owner-run USB sleep/wake, unplugged battery sleep/wake and cold power-on,
  with reset evidence, after an SD install.
