# Install page (NEWT-70)

Status: built and tested on the host with a fake X3. Not yet tried on a
device (see "Device checks").

A static web page installs Newt, or any other X3 firmware, from a computer
over USB, with no tools to install. It lives in `installer/` (build and test:
`installer/README.md`). It has no backend, no tracking and no analytics; it
talks only to the X3 and to its own files.

## What a stranger sees

1. **Start.** "Install Newt" and "Install another firmware", three short
   steps, and a fold-out "Install from the card instead" for browsers without
   Web Serial. Safari, Firefox and phones get a plain "This browser can't
   install Newt" with the card path open.
2. **Pick the X3** in the browser's port list (the X3 shows as "USB
   JTAG/serial debug unit").
3. **Checking your X3:** the page reads the firmware that stays and checks
   it whole (see "The firmware that stays").
   **Found your X3:** what runs now and what is in the spare slot.
4. **Consent:** "Install Newt 0.1.0-alpha.11?" with **Stays** (the running
   firmware; "Your books, settings and card stay as they are.") and
   **Replaced** (the spare's app by name, or "Nothing: the spare is empty.").
5. **Writing** with a progress bar, then checking, setting it to start,
   restarting, and "Finishing in N s".
6. **Done**, naming what was installed and what stays as the spare.

"Install another firmware" adds one step before 2: pick a `.bin`. The page
checks it is an ESP32-C3 app image that fits a slot, then names it
("CrossInk v1.6.1", "CrossPoint 2.0.0", or "Another firmware").

Every failure has its own screen in plain words, and says whether the X3 is
unchanged.

## Safety rules

The page writes exactly two things, both checked by one guard
(`guardWrite`, `installer/src/slots.ts`) that throws on anything else:

- the **spare app slot**, from its start, within its size; and
- **one 4 KiB otadata sector**, after the slot is verified.

It never erases, never writes the bootloader, partition table, NVS, spiffs or
coredump, and never takes an address from the user.

Before writing, the page reads and must accept:

- **The chip and flash:** an ESP32-C3 with 16 MiB.
- **The partition table** at 0x8000, byte for byte one of the known tables
  (today only the X3 factory table, `firmware/x3_existing_partitions.csv`).
  Its MD5 entry and erased filler must match too.
- **Which slot boots**, from otadata, by the ESP-IDF bootloader's rule as
  `tools/x3_slots.py` ports it. The followed record must be the newest usable
  one and settled (valid, or undefined without rollback). A pending change is
  refused ("Your X3 is in the middle of a change").
- **The slot that boots holds an app.** The page always writes the other one.
- **That app is whole** (below).
- **A sequence is left.** The record that selects the spare takes the
  smallest higher sequence for it; 0xffffffff marks an erased record, so
  when only that number would do, the page refuses ("This page can't switch
  your X3") before writing anything. Without this, the readback would match,
  the page would say Done, and the X3 would keep starting the old firmware.

The order of writes:

1. Write the image to the spare slot (esptool-js, compressed, no erase-all).
2. Ask the chip for the MD5 of the written range. A mismatch, or no answer,
   stops here; the X3 still boots what it ran.
3. Write one selection record (Newt's `otaSelect`: the smallest higher
   sequence that boots the spare, state *new*) into the sector the next boot
   does **not** follow, so a torn write leaves the old record booting.
4. Read otadata back, twice at most: the planned bytes mean selected; the old
   bytes mean not selected (unchanged); anything else, or no read, means
   *unconfirmed*. The page never calls that "not saved": it says either
   firmware may start and both are whole.
5. Reset once, then keep the port open for 30 s, so a page closing early does
   not reset the X3 before Newt confirms its new slot (`CONFIRM_SECONDS`, as
   `tools/bench_device.py`).

**Blank otadata** (both sectors erased: a unit that never switched slots)
boots app0. The page writes sequence 2 into sector 0 to select app1. The
CrossPoint web flasher writes sequence 1 there, which boots app0 again; the
page must not copy that, and a negative control proves its tests catch it.

**Newt's own image** comes from the page's own files: `firmware/newt.bin` and
`firmware/newt.manifest`, as `tools/update_publish.py` writes them. Both the
build and the page check the size, SHA-256, the whole-image digest and that
the identity block names the manifest's version and build. If Newt already
runs at exactly that version and build, the page says so and writes nothing.

**A chosen file** must be one ESP32-C3 app image with an app descriptor (so
never a bootloader or a merged flash image), intact (segments placed as the
bootloader requires, checksum and, when present, the appended SHA-256), fit
the slot, and carry nothing but erased padding after it.

## The firmware that stays

Otadata names the slot the bootloader tries first, not the one that runs.
The ESP-IDF bootloader checks that image whole before it starts it, and when
the check fails it starts the other slot (`bootloader_utility.c`, ESP-IDF
v5.5.4, lines 594-619). Then the "spare" is the firmware that really runs,
and writing it can leave the X3 with two broken images.

So before it shows anything to consent to, the page reads the image in the
slot otadata selects and checks it as the bootloader does: header, segments
and their placement (each length a multiple of 4; a cache-mapped segment at
the same offset in its 64 KiB MMU page as its data), the XOR checksum and the
appended SHA-256 when the header says one is there (`slotImageIntact`,
`installer/src/image.ts`; the same rule as `x3_slots.image_intact` and the
firmware's `appImageIntact`, checked by shared vectors).
If it fails, the page refuses ("Your X3's firmware didn't pass its check")
and writes nothing.

The read takes only the image's own bytes, following its segment headers:
3.3 MB for Newt 0.1.0-alpha.11, 6.3 MB for CrossInk v1.6.1, plus one small
read per segment header. Its time on a real X3 is not measured yet (B1
times it); the screen says "This can take a minute."

## Known tables

Only the X3 factory table is accepted. The CrossPoint flasher also accepts
the X4 table on an X3 (app1 at 0x650000), so some X3s may carry it. The page
refuses those for now ("This X3 is set up differently") and offers the card
path. Accepting it means adding its exact entries to `KNOWN_TABLES`; the
rest of the page already reads slot offsets and sizes from the table.

## The slot rule's single source

`installer/src/slots.ts` is a TypeScript copy of `tools/x3_slots.py`'s rule.
`tests/installer_test.py` exports `tests/x3_slots_test.py`'s byte vectors,
with the Python rule's answers, to `installer/test/x3-slot-vectors.json` and
fails when that file is stale, so the two cannot drift silently.
`tests/slot_vectors_test.py` holds the firmware's Rust copy
(`rust/core/src/slot_installer/identity.rs`, `slot_installer.rs`,
`update_card.rs`) to the same file, with negative controls.

## The wordmark

The header shows Newt's wordmark: the approved crop `[200,951,628,210]`
(Stage Home's `WORDMARK_CROP`, `tools/make-stage-backdrop.py`;
`assets/frosted-home/source/README.md`) of the frozen, owner-approved
`assets/frosted-home/source/assets/newt-brand-reference.png` (docs/078).
`tools/make-install-wordmark.py` cuts it with the standard library after the
source's freeze-manifest SHA-256 check, keeping its RGB pixels unchanged, and
writes `installer/src/newt-wordmark.png`. The page multiplies it over the
paper colour. `--check` (run by the gate) fails when the committed file's
pixels differ from the crop.

## Hosting

The public repository's GitHub Pages serves the page, rebuilt from each
published release with that release's pair (`.github/workflows/install-page.yml`;
[RELEASING.md](RELEASING.md#install-page)). The page cannot fetch the pair
from the release itself: GitHub sends no CORS header for release assets.

## CI

The gate runs `tests/installer_test.py`: the vector and wordmark checks,
`tsc --noEmit`, vitest and, with controls, five negative controls, each
breaking one rule in a scratch copy: blank otadata selecting with sequence
1; a write guard that allows any write; a selection taking the reserved
sequence; the firmware that stays left unchecked; and an image the
bootloader refuses for segment placement passing the check. Public CI
installs the locked packages first, so it runs all of it. The self-hosted
runner has no pnpm or Node packages, so there, under `GITHUB_ACTIONS`, only the
vector and wordmark checks run and the rest is skipped with a loud line.
Locally a missing `installer/node_modules` fails the gate.

## Device checks

None yet. Web Serial needs a person to pick the port, so these are owner
hands-on checks, later, on the owner's Mac in a real browser with the page
served on 127.0.0.1 (`installer/README.md`):

- **B1:** CrossInk in app0, other firmware in app1: Install Newt writes app1,
  the X3 starts Newt, and CrossInk stays in app0. Time the "Checking your
  X3" screen.
- **B2:** Newt running: Install another firmware with a CrossInk `.bin`
  writes the spare, and the X3 starts CrossInk.
- **B3:** Cancel at the consent screen: nothing is written, and the X3
  restarts into what it ran.
