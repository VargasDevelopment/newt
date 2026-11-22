# Newt and another firmware side by side (NEWT-39, NEWT-71)

The X3 has two app slots, app0 (`0x10000`) and app1 (`0x780000`), each
`0x770000` bytes, and one OTA selection area, otadata (`0xe000`, two 4 KiB
sectors). Newt lives in one slot and another firmware (stock, CrossPoint,
CrossInk or any other X3 app) may live in the other. Newt can restart into it
from Settings. Other firmwares cannot switch back: the way back to Newt is
installing `newt.bin` from that firmware or USB from a computer.

NEWT-39 built this for CrossInk only. NEWT-71 made it generic: any valid app
without Newt's mark counts, and a display-name table only chooses the words.

## One slot-identity rule

Slot identity is read from the device, never assumed. The rule reads each
slot's first 512 bytes and the two selection records:

- **Newt**: a fixed 16-byte mark, `NEWT-X3-APP\0` and format 1, at image
  offset `0x120`. Firmware places it in `.rodata_custom_desc`, which the linker
  puts right after the ESP-IDF app descriptor (magic `0xabcd5432` at image
  offset `0x20`; `firmware/app_slots.h`).
- **Other**: any other valid app: another firmware.
- **Empty** (all `0xff`) or **invalid** (bad image or descriptor magic).

Another firmware's name comes from its descriptor's project:
`CrossInk` is "CrossInk", `crosspoint-reader` is "CrossPoint", and anything
else is "the other firmware". Stock X3 firmware reports `arduino-lib-builder`,
which every Arduino build shares, so it stays unnamed. The table is
`firmwareName` (`include/newt/app_slot_identity.h`) and `FIRMWARE_NAMES`
(`tools/x3_slots.py`). Since NEWT-71 Newt's own descriptor names the project
`Newt` and its version, so other firmwares' tools see "Newt" too.

Newt's mark is followed by its identity block: the version and the exact
build (docs/083). Tools report it; no slot decision trusts it.

When exactly one slot holds another firmware and neither slot is invalid,
Newt's slot is the other one. Without another firmware (Newt+Newt or
Newt+Empty after SD installs, [083](083-ota-sd-install.md)) the slot that
boots must hold a selected, confirmed Newt and the tools write only the other
one ([bench-flash](bench-flash.md)). Two other firmwares, or an invalid slot
beside one, is refused: no tool chooses which firmware to replace. Selection follows the
ESP-IDF bootloader: a record counts when its sequence is set, its CRC matches
and its state is not invalid or aborted. The highest sequence boots slot
`(seq - 1) % 2`; a tie goes to the first record. The bench bootloader has
rollback on (its OTA routines match the pinned bootloader): a *new* record
boots as *pending verify* until its app confirms it. Newt does that after its
first completed paint (docs/083); Arduino's default, before `setup()`, is
turned off for Newt. A record still pending at the next boot was never confirmed, so the
bootloader marks it *aborted* and follows the other record. The rule models
this: when the highest record is pending, the next boot follows the other one,
which must count and must not be pending itself; otherwise no safe choice
exists and the rule refuses.

The rule exists twice, with one test over shared byte vectors
(`tests/x3_slots_test.py`):

- `tools/x3_slots.py` for the bench tools.
- `include/newt/app_slot_identity.h` for firmware: `classifySlot`,
  `firmwareName`, `otherFirmwareSlot`, `otaSelect` and `appImageIntact`.

| Unit | app0 | app1 | Newt's slot |
| --- | --- | --- | --- |
| bench X3 | CrossInk v1.4.0.1 | Newt | app1 |
| main X3 | Newt | CrossInk | app0 |

### Tools

`tools/bench_flash.py` ([bench-flash](bench-flash.md)) decides the layout on
each run before any write. It refuses when it cannot tell: both slots hold
another firmware, a slot is invalid, or no selection record counts.
`bench_device.Device.write` refuses the other firmware's slot outright. Its
only other writes are Newt's slot and the 4 KiB selection sectors. An install
into the Newt slot that boots first writes a settled record selecting the
other firmware, so a new image that never confirms returns to it
([bench-flash](bench-flash.md#a-failed-first-boot)). `slots` only reads;
`boot-newt` and `boot-other` write only the selection. To put another
firmware in the other slot, install its `.bin` from Newt's card update
([083](083-ota-sd-install.md)); on the bench, `install-other` writes a pinned
image into the slot that does not boot ([bench-flash](bench-flash.md)).

### A selection write

To boot slot *s*, the writer takes the record the next boot follows, picks
the smallest higher sequence for *s* and writes it, state new and label
`0xff`, into the *other* record's sector. That sector holds an older record or
a pending one the bootloader would abort, so the record the device falls back
to is never touched: with CrossInk confirmed at sequence 7 and Newt pending at
8, selecting Newt rewrites record 8 and keeps record 7. CrossInk's own switch
(`src/network/OtaBootSwitch.cpp`) writes the same record the same way when
nothing is pending.

## Restart into the other firmware

Settings ends with a **Restart into <name>** row, named from the table above
("Restart into the other firmware" when unnamed; "Other firmware" when the
other slot holds none). Select opens one confirm page, "Switch firmware":
"Newt saves your place, then restarts into <name>." and "Newt stays on this
X3 unless that firmware updates over it. To come back, install newt.bin from
that firmware or a computer." An unnamed firmware, such as the X3's own, may
not install from the card, so its page ends "install Newt from a computer."
Back returns to Settings without saving, checking or switching. Sleep sleeps
as usual.

An older Newt in the other slot (the one each update leaves behind) does not
count: the row stays disabled. Restarting into it would be a downgrade the
updater refuses, and a hidden rollback; an older build goes back on through
the card install, which says what it replaces.

Select on the confirm page, in order (`firmware/firmware_switch.h`):

1. **Save.** A book still open takes the reader's existing exit path
   (`finishReaderExitAction`): progress and statistics, nothing new. Then the
   exit's outcomes must be verified. The reader's save notice keeps an
   unverified page or Home record, and a statistics fold that did not complete,
   until a later save verifies it, so a failed save from the Back that left the
   book also counts. Otherwise the page says "Save unconfirmed. Newt stays
   open. Open your book and go back to save again." and nothing else happens:
   a restart would lose what only RAM holds. The check reads outcomes, not a
   particular store. NEWT-50 makes a verified save depend on an effective sync
   of those exact bytes; the switch takes the reader exit's `SaveOutcome` unchanged.
2. **Find.** The other slot must still hold another firmware under the rule.
3. **Show** "Switching to <name>…".
4. **Verify** the whole image: an ESP32-C3 header, 1 to 16 segments inside
   the slot, the XOR checksum and, when the header says one is appended (every
   X3 firmware seen has one), the SHA-256 of every byte before the digest.
5. **Select** the slot (above). After the erase and write, whatever they
   returned, the records are read back up to three times and the rule decides
   from the bytes the bootloader will read: switched, unchanged, or
   unconfirmed when no readback succeeds.
6. **Restart**, only when the readback shows the switch. As sleep does: drain
   DEV USB, cancel the sleep session, clear the warm SD record, close the
   reader and card, put the panel to sleep, then `esp_restart`.

A missing image keeps its existing missing-image message. An image-check
failure says "Could not switch to <name>. Newt stays open." only while no
previous selection is unresolved. Unreadable otadata, including a later attempt
that writes nothing, or readable records with no safe boot choice show "Switch
unconfirmed. Newt stays open; the next restart may open <name>." A boot-lifetime
RAM bit keeps that uncertainty across Back, Home and page re-entry. A later save
or missing-image message does not clear it; a failed image check retains the
switch-unconfirmed message. Only a safe readable selection resolves it: the
other firmware restarts, while a confirmed unchanged Newt selection shows failure. Reboot clears
the RAM bit after the bootloader chooses the app. Select retries from step 1.
When no other firmware is installed the row stays visible but disabled, with
a note pointing to the card install, and no Select hint.
Setup checks once (two 512-byte reads); the row checks again on each Select.

Newt does not call `esp_ota_set_boot_partition` or `esp_image_verify`.
CrossInk links its build with a wrapped
`bootloader_common_check_efuse_blk_validity` (`platformio.ini`), and its own
flasher avoids `esp_image_verify` because it "reject[s] our patched image on
X4 silicon" (`src/network/FirmwareFlasher.h`). Whether the X3 is affected is
untested, so Newt checks what the bootloader relies on, without the
chip-revision gate, and writes the record itself, as CrossInk does.

The slot ports keep one ordinary-RAM uncertainty bool for this boot, with no
new RTC or durable record. Final record 64 measures unchanged SRAM totals in
all four lanes; see [CODE_HEALTH](CODE_HEALTH.md). The shown state uses existing
`DeviceInfo` padding, the page reuses `rowOpen`, and an owed statistics fold uses
the save notice's last spare bit. That fold bit is not kept across a wake; the
next fold retries from the stored watermark either way ([076](076-reading-stats.md)).
The confirm page and its states are
host previews from the shared screen: `device-switch-row`, `-confirm`, the
CrossInk and unnamed variants, `-cancelled`, `-switching`, `-failed`,
`-save-unconfirmed`, `-switch-unconfirmed` and `-missing`
(`tools/gate_previews.py`).

## Coexistence

The table below is CrossInk's; other firmwares keep their own stores and are
not audited here. Newt never deletes another firmware's files.

CrossInk read at v1.4.0.1 (`54ba245f`, the bench build), `0de623ed`,
`31ce7704` and HEAD `9914146e` (v1.6.1) of
https://github.com/uxjulia/CrossInk (MIT). Read only; no code copied. The
clone was deleted after reading. Start from [013](013-crossink-migration.md).

| Store | CrossInk | Newt | Shared? |
| --- | --- | --- | --- |
| NVS namespaces | `cphw` (device type), Wi-Fi with `persistent(false)`; on the bench also `crosspoint`, `eepUser` and ESP-IDF's `nvs.net80211`, `phy`, `nimble_bond` | `newt_*`, legacy `nb<id>` | Partition only; no namespace overlap |
| SD | `/.crosspoint/…` (settings, caches, stats, synced stats), `/.fonts`, `/fonts`, `/sleep*`, `/screenshots`, `/Read` | `/Newt/…` | Card only; no path overlap |
| Books | Any path; identity is the path | `/Newt/Books`; identity is the file's SHA-256 | Neither writes book files |
| RTC memory | Its own use | Validated warm records | Newt clears its warm record before the switch and validates on boot |
| App slots | Updates write `esp_ota_get_next_update_partition` | — | **A CrossInk update overwrites Newt's slot** |

Both use NVS from the same 20 KiB partition (`0x9000`). Neither erases or
formats NVS or the card. CrossInk's USB file transfer can delete any SD path
on request, including `/Newt`; that is user action, not automatic. A CrossInk
firmware update replaces Newt (see USB recovery).

## USB recovery

The bench is selected by default. A main-unit session must explicitly select
its own root-owned udev identity rule on every command, after owner approval
([080-main-device-switch](080-main-device-switch.md)). All identity, lease,
slot and recovery checks still apply. The commands here use the bench default;
follow 080 for the main unit.

On the bench X3, from the other firmware, connect USB and wait at least 30 seconds after
the switch before opening any port: opening one resets the X3, and a reset
before that firmware confirms its new selection makes the bootloader abort it.
Then run, on the computer:

```sh
.toolchain/platformio/penv/bin/python -B tools/bench_flash.py boot-newt CANDIDATE \
  --manifest-sha256 REVIEWED_SHA256 --session .build/bench/SESSION
```

`boot-newt` proves by on-chip MD5 that Newt's slot holds exactly that
candidate, then writes only the selection sector, checks the result boots
Newt and resets. It writes no app bytes. The DEV handshake runs on the same
port. Every variant holds that port at least 30 seconds after reset before
closure, to give Newt time to confirm; a non-DEV boot still needs `slots`
readback to establish VALID. If Newt's
slot holds another Newt build, `install` writes the candidate there instead.

Another firmware's update writes the slot it is not running from, which is
Newt's, so both slots then hold another firmware. The rule cannot tell which
slot is Newt's: `slots` reports "Both slots hold another firmware" and
`install`, `restore` and `boot-newt` refuse before any write. No tool picks a
slot. The simplest way back is that firmware's own install of `newt.bin`.
Otherwise recovery is operator-led, on the bench X3 only. The owner decides
which copy Newt replaces, normally the one the bootloader does not follow.
The Lead then writes Newt into that slot with esptool outside these tools,
keeping the session's full backup, and runs `slots` to confirm that exactly
one slot holds another firmware before any further tool use. Newt's slot then
changes, which `decide` reads from the device on the next run.

## An interrupted switch

- Before the selection write, nothing has changed; Newt boots.
- During it, only the record Newt does not follow is erased and written. A
  torn write fails its CRC, so the bootloader keeps following Newt's record.
- After it, the next boot starts the other firmware. Progress was saved and
  verified first.
- The bootloader has rollback on. If that firmware never confirms itself, or the
  X3 resets again before it does (a port open counts), the bootloader aborts
  the new record and boots Newt again. The same holds for Newt after
  `boot-newt`.

## Backups

`bench_flash.py` takes a full 16 MiB backup on each session's first write,
confirms it on chip and checks every byte outside Newt's slot and otadata
against it before and after each write. Backups are never restored
automatically. Keep each unit's pre-Newt backup (the main X3's was taken on
2026-10-25) and the candidate that is installed.

## What this does not claim

Host tests prove the rule, the selection record, the image check and the
dispatch order. They do not prove device behavior. The bench section below
records USB-powered checks only: no battery sleep/wake or cold power-on
claim.

## Bench evidence (2026-10-25, bench X3, USB power only)

NEWT-39's CrossInk-only build; kept as history. Its tool flags and copy have
since changed.

The DEV candidate `44270202640c` was installed into the bench's app1 with
`--replace-app` (the older Newt there had no mark). The shared C++ image
check passed the real CrossInk v1.4.0.1 image from the install backup.

Round trip 1:

1. Newt read the first Library entry to spine 13, character 22,975 and saved
   it (it survived a reboot).
2. Settings → Restart into CrossInk → Select: the last press was consumed with
   no render, and the unit restarted.
3. Otadata read back: record 0 seq 7 for app0. Newt writes it as *new*; it
   read back *valid*, so CrossInk's boot confirmed it. CrossInk booted.
4. `boot-newt` wrote record 1 seq 8 (*new*, app1) with every byte outside
   app1 and otadata unchanged, then reset. Newt's DEV HELLO did not answer
   within 20 s. The tool latched STOP (Lead decision, outside flash intact).

Review r1 found the likely cause. The tool reset the X3, closed the port and
opened a new one for HELLO, and that open reset the X3 a second time, before
Newt could confirm seq 8 (the bench bootloader marks a new record pending on
its first boot and aborts it on the next). The bootloader then followed record 0, so
CrossInk booted, and HELLO had no answer. Round 2 keeps one port through the
reset and the handshake, resends HELLO within its 20 s budget, and never
writes over CrossInk's confirmed record 7 while record 8 is pending. This
diagnosis is from the bootloader's code and the tool's order, not from a
device readback; the Lead's first read settles it.

Not shown: Newt's boot after recovery, the same book and page after the
round trip, and round trips 2 to 5. NVS was byte-identical before Newt's
install and after CrossInk's boot. The log is kept outside the repository.
