# 082: Updating Newt without USB (#103)

**Design for owner approval.** This round is docs and research only. It changed
no firmware and wrote nothing to a device. The one device
action was a read-only bench `slots` run (2026-10-27 13:34Z, exit 0, 30.6 s).
This builds on [005](005-ota-firmware-updates.md),
[077](077-return-to-crossink.md) and [080](080-main-device-switch.md).

**Status by phase:**

- Phase 1 (SD install): implemented on `feat/ota-sd-install`
  ([083](083-ota-sd-install.md)); host, compile and resource evidence only;
  bench and owner power-condition checks pending.
- Phase 2 (Wi-Fi measurement): measured on the bench (image (c), 2026-10-28)
  and approved by the owner ("You can build wifi on (c)"). The Wi-Fi base is
  on `feat/wifi-base` ([084](084-wifi-memory.md)): Wi-Fi linked with the
  carve-out, and a scan-only networks list in update mode. Host, compile and
  resource evidence; this build's bench check is the Lead's receipt.
- Phase 3 (Wi-Fi setup): slice A (setup and storage, no join) implemented on
  `feat/wifi-setup` ([085](085-wifi-setup.md)); host and compile evidence
  only, bench and power-condition checks pending. Slice B part 1 (the join,
  its memory and the heap clear) on `feat/wifi-join` (085, "Slice B, as
  built"); its bench run joined only a made-up network. Part 2 (the
  forced-crash test after #157 and slice A merge) not started. Until Phase 3
  setup is whole, main is not offered to the owner and there is no release
  bump.
- Phase 4 (Wi-Fi update): implemented on `feat/ota-wifi-install`
  ([088](088-ota-wifi-install.md)): update mode checks GitHub Releases over
  HTTPS and downloads a newer Newt to the card, where the card installer takes
  it. Resident RAM unchanged; flash record 79 (reviewer pending). Host, compile
  and resource evidence only; the bench checks, the TLS heap peak first, are
  pending.
- Phase 5 (optional): not started.

Labels: **V** = checked in this round (source, config, binary or bench
readback). **I** = inferred from verified facts. **U** = not verified.
Framework paths are under `.toolchain/platformio/packages/`; `sdkconfig` means
`framework-arduinoespressif32-libs/esp32c3/sdkconfig`. "Ledger" is the Lead's
private log; its facts are quoted, not copied.

## Owner decisions

Each item starts with my recommendation.

1. **Slots: keep today's partition table.** Updates always write the slot Newt
   is not running from. The first update replaces CrossInk, after you confirm
   once. After that, the other slot holds the previous Newt. CrossInk comes back
   from an image on the card when you want it (about a minute, **U**). The
   alternative is three slots: a one-time USB re-partition that keeps CrossInk
   one press away but is riskier and caps Newt's size (section 1).
2. **First source: an image file on the SD card**, then Wi-Fi.
3. **Wi-Fi source: GitHub Releases over HTTPS** (decided 2026-10-30; it
   replaces the first plan, a file server on the Lead's computer, section 3).
   The X3 checks the public `VargasDevelopment/newt-releases` repository only
   when you press Check on the Update screen, verifies GitHub's certificate,
   and downloads a newer Newt to the card (088). No host changes, no token.
4. **Entering the network: pick it and type the password on the X3**, CrossInk
   style. Phone setup over a temporary network comes later, only if typing is
   too painful.
5. **Storing the network: an encrypted record on the card, with its random key
   in Newt's own NVS area.** Neither half alone reveals the password. Forget
   network saves an explicit "no network" first, then erases the key (erased,
   not replaced, since 2026-10-30: a replacement needs free NVS space).
6. **Trust without signing.** Before Install, the X3 shows the version, build
   and a 16-character check code. You compare it with the code in the Lead's
   separate update message and install only if they match. Update mode runs
   only while the Update screen is open and stops after 10 minutes without a
   button press while you choose. The time limit only shortens exposure; it
   proves nothing about the file. Once writing starts, it runs to the end.
7. **Memory: measure before building Wi-Fi.** Wi-Fi ships only if reading keeps
   its 50,468 B heap floor with Wi-Fi linked in. A carve-out that lends most of
   Wi-Fi's fixed memory back to the heap while you read is worth trying, but
   it is unproven and does not cover all of it. If reading misses the floor,
   you choose then: find savings, or no Wi-Fi yet (SD updates still work).
   Any memory Wi-Fi holds while you read needs your explicit approval.
8. **Power: no USB needed (owner, 2026-10-31).** Wi-Fi, Check, downloads and
   installs work on battery. This replaces "USB power only at first": the
   install is power-loss safe (see "Power rules"), so there is nothing to
   gate, and no battery threshold replaces the rule. Buttons and sleep wait
   during the write.
9. **A new build counts as good only after it paints its first screen.** If it
   crashes or is reset before that, the X3 goes back to the previous one by
   itself. This also protects USB installs.
10. **Versions: use the existing tag `v0.1.0-alpha.0` as the baseline.** Your
    tag calls it the first reviewed software baseline under RELEASING.md, with
    no hardware acceptance claim (section 7). Each owner update bumps to the
    next alpha (Commitizen proposes `0.1.0-alpha.1` now). The X3 offers only
    newer versions. Reinstalling the same version needs an extra confirm.
    Older versions only by USB, or by going back to the previous slot.
11. **After an update, show "Updated to *version*" and What's new once.**

## The design in ten lines

1. One installer core in Newt, shared by every source.
2. Its input is always a file on the card: you copy it, or Newt downloads it.
3. Check the file first: exact size, SHA-256, image structure, chip, and per
   kind the Newt mark and version or CrossInk's approved digest. Nothing is
   erased before the file passes and the running slot's selection is settled.
4. Write the slot Newt is not running from, first sector last; read it all back.
5. Select it with a fresh NEW record (never "already selected"), then restart.
6. The new build confirms itself at its first finished screen of any kind;
   else the bootloader rolls back (docs/083).
7. Update mode is its own boot, on USB or battery: no reader, Wi-Fi only
   there, outbound only.
8. Wi-Fi pulls GitHub's latest published release over HTTPS (088).
9. The network is typed on the X3 and stored split (card ciphertext, NVS key).
10. Every owner update is an `alpha.N` release; What's new shows once after it.

## 1. Slots

### Facts

- The X3 table has six entries: nvs `0x9000` (`0x5000`), otadata `0xe000`
  (`0x2000`), app0 `0x10000` and app1 `0x780000` (each `0x770000` =
  7,798,784 B), spiffs `0xef0000` (1 MiB), coredump `0xff0000` (64 KiB).
  **V** `firmware/x3_existing_partitions.csv`; the bench backup's table matches
  and its MD5 entry is valid.
- Usable per slot: 7,016,448 B after the 10% reserve
  (**V** `docs/CODE_HEALTH.md:104-112`). Newt today is 2,395,712 B
  (**V** `code-health-baseline.json`, flash record 68).
- Main: Newt in app0, CrossInk `31ce770` in app1 (**V** docs/080). Bench:
  CrossInk `v1.4.0.1` in app0, Newt in app1, boots app1, records 21 and 22
  both valid (**V** this round's bench `slots`).
- A standard A/B update writes the slot that is not running. Arduino's Updater
  and FreeInk's flasher both use `esp_ota_get_next_update_partition`
  (**V** `framework-arduinoespressif32/libraries/Update/src/Updater.cpp:233`,
  `vendor/freeink-sdk/libs/hardware/RecoveryBoot/src/FirmwareFlasher.cpp:270`).
  Here that slot holds CrossInk.
- An app cannot rewrite the slot it runs from: its code executes from that
  flash. Arduino's Updater refuses it (**V** `Updater.cpp:234`).
- Newt's tools refuse unless exactly one slot holds CrossInk, and write only
  the other slot (**V** `tools/x3_slots.py:149-165`).
- CrossInk's own updater already overwrites Newt's slot (**V** docs/077:147).

### Options

| Option | What changes | Newt usable size | Risk | Recovery | What the owner loses |
| --- | --- | --- | --- | --- | --- |
| **A. No table change (recommended)** | Updates write the non-running slot. The first one replaces CrossInk after one confirm. CrossInk returns from `/Newt/Update/crossink.bin` through the same installer. | 7,016,448 B | Low: same table and selection write as today | Previous Newt stays in the other slot: automatic rollback before confirmation; by choice after. USB as today. | Instant CrossInk. "Restart into CrossInk" becomes "Install CrossInk and restart". Installing CrossInk replaces the previous Newt, and CrossInk still cannot switch back (docs/077:6). CrossInk's settings stay: NVS namespaces and `/.crosspoint` are untouched (docs/077:143-144). |
| D. A, plus restore CrossInk after each update | After each confirmed update, write CrossInk into the old slot | 7,016,448 B | Low | No previous Newt after confirmation | Twice the writes and time per update; no going back to the previous Newt |
| B. Three app slots | One USB rewrite of table and otadata. B1 keeps CrossInk at `0x780000` and splits app0 into `0x3B0000` + `0x3C0000`. B2 uses three equal `0x4F0000` slots and moves CrossInk. | B1 3,477,504 B; B2 4,657,152 B (same 10% rule) | Medium-high: a partition write (005 does not authorize one); `(seq-1) % 3` changes what old records mean; FreeInk's switch hardcodes two slots (**V** `RecoveryBoot.cpp:164-166`); CrossInk's own switch probably too (**U**); CrossInk's updater still writes a Newt slot; every tool and the resource gate change; the bench must move CrossInk | USB | Newt's size cap. B1 barely fits Newt plus Wi-Fi (about 3.2-3.4 MB, upper bound) |
| C. Stage on the card, write the running slot | Needs a helper image or a custom bootloader | — | Not possible as an app. A bootloader rewrite risks a brick (FreeInk avoids it: `RecoveryBoot.h:29-32`) | — | — |
| R. Separate updater image | A small Wi-Fi updater lives in the other slot | 7,016,448 B | Medium: a second firmware build, against "one firmware build" | Bootloader falls back to the updater | CrossInk's slot and the previous Newt |

### Recommendation: A

It keeps the table, the full 7.0 MB slot and the selection write that is
already tested, and it gives rollback to the previous Newt. It matches the
2026-10-24 decision (CrossInk comes back as a card image). What changes:

- **Consent.** The first update into a CrossInk slot shows one confirm page:
  "This replaces CrossInk. You can install it again from the card." Back
  cancels. No other path writes a CrossInk slot.
- **Settings row.** "Restart into CrossInk" while CrossInk is in the other
  slot (as today). "Install CrossInk and restart" when it is only on the card.
  No row when neither.
- **Slot rule.** `classifySlot`/`crossInkSlotIndex`
  (`include/newt/app_slot_identity.h:32-50`) and `tools/x3_slots.py` accept
  Newt+CrossInk, Newt+Newt, Newt+Empty and Newt+Other. Writes go to the
  non-running slot; a CrossInk slot needs the explicit consent. `bench_flash`
  follows the same rule, so USB installs also keep the previous build.
- **Damaged inactive slot.** A power loss during the final header write or an
  early erase can leave the non-running slot unreadable (Invalid). Today
  `decide()` refuses any Invalid slot (`tools/x3_slots.py:149-155`), so USB
  could not repair it. Add one narrow repair: write only the damaged
  non-running slot, only when the other slot is the running, selected and
  settled Newt and its whole image still verifies (full-slot MD5 against the
  session backup, as `bench_flash` already checks outside spans). It never
  writes the survivor and never selects the repaired slot before its image
  verifies. The decision follows docs/bench-flash.md's recovery authority.
  On the device, an Invalid non-running slot is writable by the installer
  without the CrossInk consent, since it holds no bootable app.
- **docs/077 and docs/080** are updated in the same change.

Size of a CrossInk image: **U**. The Lead can read it privately from a backup's
app header; no device action is needed.

## 2. Bootloader facts

### Pinned configuration (V, sdkconfig)

| Line | Setting | Meaning |
| --- | --- | --- |
| 331 | `BOOTLOADER_APP_ROLLBACK_ENABLE=y` | A new selection boots once as pending; unconfirmed, the next boot aborts it |
| 332 | anti-rollback off | No secure-version check |
| 387-389 | skip validation only on deep-sleep wake | Every other boot checks the image |
| 400-402 | secure boot and flash encryption off | No signature check; flash readable |
| 3392 | `APP_ROLLBACK_ENABLE=y` | The app has the confirm/rollback API |
| 2562 | `SPI_FLASH_DANGEROUS_WRITE_ABORTS=y` | The flash API refuses bootloader/table writes (a guard against mistakes, not against a hostile image) |

### otadata selection (V)

- Two 32-byte records: sequence, label, state, CRC. States: 0 new, 1 pending,
  2 valid, 3 invalid, 4 aborted. The CRC covers the sequence only. The slot is
  `(seq - 1) % 2`. Newt models this in `otaSeqCrc`, `otaRecordUsable`,
  `otaBootRecord` and `otaSelect` (`include/newt/app_slot_identity.h:75-119`).
- Newt's selection write (`commitOtaSelection`, `firmware/crossink_switch.h:20-36`)
  writes state new into the other record's sector and reads it back up to three
  times: Verified, Unchanged or Unconfirmed. It never touches the record the
  device would fall back to (docs/077:60-66).

### Bench unit (V)

- The bench bootloader (from the newest bench session backup, read in memory
  only) is 18,672 B with a valid appended SHA-256
  (`00925e7fe5d8e2db7bee01cc0275d1c33a434c2068fe977a84104e5c755edb8a`).
  Section by section it equals the pinned Arduino `bootloader_dio_80m.elf`,
  except its descriptor (IDF "v5.5.4", Jul 3 2026) and three assert line
  numbers in two flash helpers. It is the pinned rollback-enabled bootloader.
- The pinned bootloader contains `write_otadata`, `set_actual_ota_seq`,
  `bootloader_common_check_chip_validity`, `bootloader_sha256_*` and
  `try_load_partition`.
- Hardware: on 2026-10-25 a Newt selection reset twice before confirmation read
  back as record 8 aborted, record 7 valid (ledger, 2026-10-25 19:22Z).
  Rollback works on the bench.

### Main unit (I)

- Its bootloader was not read. The release install wrote record 3 as new; it
  later read valid. Arduino confirms only a pending image, so the bootloader
  must have made it pending: rollback is on (ledger, 2026-10-26).
- To verify without touching the device, the Lead can hash bytes
  `0x0-0x48f0` of a main backup privately and compare with the bench hash.

### Image validation (V config, U on device)

- On every boot except deep-sleep wake, the bootloader checks the image header,
  segment checksum and appended SHA-256, and the chip. A failed image makes it
  try the other slot (FreeInk's note: `RecoveryBoot.h:29-30`; **U** that the
  fallback works on these units).
- Newt's `appImageIntact` (`include/newt/app_slot_identity.h:132`) checks the
  image structure from the app: chip 5, hash appended, segments, checksum and
  digest. It is a structure and checksum check, not every bootloader check: it
  omits the chip-revision and eFuse gates on purpose (`:124-129`).

### What Newt does today (V)

- Arduino confirms a pending image in `initArduino()`, before `setup()`
  (`framework-arduinoespressif32/cores/esp32/esp32-hal-misc.c:311-326`), unless
  the weak `verifyRollbackLater()` returns true (`:285-288`). Newt overrides
  neither.
- So a Newt image that crashes in `setup()` is already confirmed and loops.
  Only USB recovers it.
- Phase 1 changed this: Newt now overrides `verifyRollbackLater()` and
  confirms itself, as recommended below (docs/083).

### Recommendation

- Override `verifyRollbackLater()` to return true. Confirm with
  `esp_ota_mark_app_valid_cancel_rollback()`
  (`framework-arduinoespressif32-libs/esp32c3/include/app_update/include/esp_ota_ops.h:360`)
  at the boot's first finished screen of any kind (Home, a book page or reader
  error, Library, Settings, a notice or the sleep image), never on a working
  frame such as "Opening...". docs/083 has the rule as built.
- After a rollback, the Update screen says "The update did not start. Newt went
  back to *version*", using `esp_ota_get_last_invalid_partition()` (`:391`).
- Until the image is confirmed, the DEV bridge reads requests but answers
  none, so `HELLO` and `DIAG` stay silent; tools wait for `HELLO`
  (docs/083).
- Keep Newt's own selection write, not `esp_ota_set_boot_partition`: it runs
  `esp_image_verify`, which rejects some vendor images (`RecoveryBoot.h:78-80`),
  and one selection rule serves the device and the tools (docs/077).
- Tools keep the existing rule: never reset twice before confirmation.

## 3. Transport

### One installer input

The installer only ever installs a verified file on the card,
`/Newt/Update/newt.bin` with `/Newt/Update/newt.manifest`. Wi-Fi only downloads
those files to the card. Then:

- Nothing in the target slot is erased until a complete file has passed every
  check.
- Wi-Fi is off during the flash write: less power and less RAM.
- There is one write path to test.

FreeInk's flasher writes the header first and does not read the slot back
(`FirmwareFlasher.cpp:301-338`). Newt's core differs on both points (section 5).
Credit FreeInk (MIT) for the pattern; reuse Newt's own pieces.

### Order: SD first

The SD source needs no network, no credential, no host change and no extra
memory. It exercises everything risky: the slot rule, writing, readback,
selection, confirmation and rollback. It also brings back CrossInk from the
card. Its one cost: someone moves the card to a computer to copy the file.

### Wi-Fi: pull, not push

| | Pull from the Lead's computer (recommended) | Upload page on the X3 |
| --- | --- | --- |
| Owner steps | Update → Check → compare code → Install | Open page on phone/computer, enter IP and pairing code, pick file, then Install |
| Listener on the X3 | None (outbound only) | An HTTP server while the page is open |
| Agent bench use | The Lead publishes, then presses over DEV USB; no hands | Needs a browser or a client and the X3's IP |
| Device code | HTTP client | HTTP server, upload form, pairing |

Later, once the repo is public (#137), the X3 could fetch from GitHub over
HTTPS without a token. That adds TLS (about 20 KB heap, about 0.3 MB flash per
the NEWT-25 brief) and is out of scope now.

### What the host needs for pull

Each item is a host or network change and needs owner approval:

1. **A file server** for one directory, read-only, on one port (for example
   8783), bound to the home-network address, started only while an update is
   offered. Python's standard `http.server` is enough (no install). A systemd
   unit would be a further change.
2. **A firewall rule.** ufw is enabled on the Lead's host (**V** unit file
   enabled; rules not read). Allow that TCP port from the home subnet only.
3. **An address.** No mDNS responder is installed on the host (**V** no
   avahi unit). Recommended: the owner types the host's address once on the X3,
   and the router gives the host a fixed address (a router change). Router DNS
   names may work (**U**). Installing an mDNS responder would need a dependency
   approval; lwIP can already resolve `.local` names (sdkconfig:2098).
4. **Same network, no client isolation.** Guest networks often isolate
   clients (**U** for the owner's network).
5. **A publish step** (a later tool): copy an accepted candidate's image and
   write the manifest from its identity receipt
   (`artifacts/firmware-identity-regional-dev-usb.json`, docs/RELEASING.md
   "Exact identity and acceptance").

## 4. Wi-Fi setup and storage

### Entering the network

- **Recommended: on the X3.** Scan, pick a network (or type one), type the
  password with an on-screen keyboard. FreeInk ships a keyboard component
  (**V** `vendor/freeink-sdk/libs/ui/FreeInkUI/include/components/keyboard`).
  No access point, no listener.
- **Later, if typing hurts: phone setup.** The X3 opens a temporary access
  point with a page. This adds a listener, SoftAP memory and more code
  (SoftAP is compiled in: sdkconfig:1828).
- The update source address is typed once on the same screen. It is not a
  secret.

### Storage: split (recommended)

- The network name and password go into an encrypted durable record on the card
  (`/Newt/…`), AES-GCM via mbedTLS (sdkconfig:2391, 2397). It uses the
  existing durable commit and readback, like other preferences.
- A random 32-byte key goes into a new Newt NVS namespace, `newt_wifi`. That is
  about 4 entry slots, about 7 with an in-flight replacement, by docs/080's
  formula ("NVS capacity stop rule"). docs/080's Newt budget grew from 161 to
  165 slots: its 45-slot in-flight reserve already covers the key's
  replacement. The main unit's last count had room (ledger: F=315, needs 291).
- Why split: a card copy holds only ciphertext; a flash backup holds only the
  key. Agents read NVS bytes routinely (this round read bytes `0x0-0x10000` of a
  bench backup, which includes NVS; docs/080 parses NVS). Plain NVS would put
  the password in every backup.
- Limit: NVS and FAT mark deleted data rather than wiping it at once. Old bytes
  may remain until NVS garbage collection or the card reuses the space.

### Commit rules (AGENTS.md durable preferences)

The card record is the authority. It always holds an explicit value: a network
(ciphertext) or **"no network"**. RTC never holds any part of it.

- **Key lifetime.** The key is created once, the first time a network is saved,
  and is never replaced while a card record encrypted with it may still be
  acknowledged. Order on save: if no key reads back, write one and verify it by
  readback; only then encrypt (fresh nonce) and commit the record.
- **Save a network.** Commit the record with the existing durable commit and
  bounded readback. Publish the new network to the running update mode only
  after Verified. Until then, keep the previous runtime choice.
- **Forget network.** Commit and verify an explicit "no network" record. This
  is never a delete, so an older network record cannot reappear. Only after
  that record is verified may Newt erase the key, which makes leftover old
  ciphertext unreadable (an erase needs no free NVS space; the next Save makes
  a fresh key). A failed or unconfirmed key erase changes nothing visible: the
  record already says "no network".
- **Failures.** A failed commit keeps the previous runtime choice and says
  "Not saved". A lost acknowledgement is reconciled by bounded readback. If
  storage prevents confirmation, say "Not confirmed. After restart the X3 may
  use the old or the new setting", never "not saved"; either complete value may
  return on reboot.
- **Defaults.** Missing record, corrupt record, missing key, a key that cannot
  decrypt, or a different card: "no network saved" (the product default), and
  setup asks again. Nothing is deleted automatically.

Rejected: a plain file on the card (owner decision 2026-10-21); plain NVS;
IDF's own Wi-Fi store (`nvs.net80211`, `persistent(true)`); a key derived from
the chip ID (anyone with the ID and a card copy could decrypt).

### Keeping Wi-Fi out of NVS

- Start Wi-Fi through the IDF API with `nvs_enable = 0` and RAM storage. Arduino
  leaves `nvs_enable` on except for esp-hosted
  (`framework-arduinoespressif32/libraries/WiFi/src/WiFiGeneric.cpp:265-271`,
  sdkconfig:1806) and keeps Wi-Fi persistent by default (`:376`).
- Never call `WiFi.begin()` without arguments: it uses the stock firmware's
  saved network (NEWT-25 brief).
- Keep Arduino's C3 fix that keeps USB alive when Wi-Fi starts
  (`phy_bbpll_en_usb(true)`, `WiFiGeneric.cpp:288-291`), so the DEV bridge
  survives.
- **Answered (#103 phase 2, fix round 2):** yes, Wi-Fi can write it.
  `nvs_enable = 0` and RAM storage do not cover PHY calibration
  (`CONFIG_ESP_PHY_CALIBRATION_AND_DATA_STORAGE=y`, sdkconfig:1657). At the
  first `esp_wifi_start` of a boot, `esp_phy_load_cal_and_init` reads the
  `phy` namespace read-only; if the data is missing or invalid, it calibrates
  fully and calls `esp_phy_store_cal_data_to_nvs`, which opens `phy`
  read-write and calls `nvs_set_blob` and `nvs_set_u32` (**V** in the
  `eabd52f3` ELF, call at `0x42095552`). Newt fences flash while Wi-Fi may run
  ([084](084-wifi-memory.md)): that read-write open is refused, so the store
  returns before any NVS change, and its caller ignores the result. There is
  no abort, retry or erase (**V** same ELF; `esp_phy_erase_cal_data_in_nvs`
  is not linked). The radio keeps that boot's calibration, and CrossInk's
  `phy` namespace (docs/077:143) is untouched. On the bench (`004d5db0`,
  USB power, one update-mode boot), the first scan's DEV row showed
  `fenced-opens 1, fenced-writes 0` and the second `0, 0`, and the NVS
  bytes were the same before and after (**V**). An open once per boot fits
  the PHY store, not Wi-Fi's own NVS, which would open at each scan's init
  (**I**). So this bench's stored calibration likely fails Newt's load or
  check, and every update-mode boot tries the store again (**I**). The first
  Wi-Fi start took 40 ms and the second 8-9 ms on two fenced boots (**V**):
  about 32 ms per update-mode boot, which fits the once-per-boot
  calibration (**I**); whether it is full or partial is **U**. Before the
  fence, the first start took 93-98 ms on four boots (**V**); the
  difference fits the store's NVS work (**I**). Round 0's `phy` and
  `nvs.net80211` entries were unchanged (**V**).

### Keeping the password out of output

- The password lives only in a `Secret` type with no print, log or serialize
  path. It is cleared after use.
- DEV lines never carry the network name or password. DIAG may say
  `wifi=saved|none`. The bridge has no network or credential commands
  (docs/dev-usb-bridge.md:11) and must stay that way.
- IDF logs are at error level only (sdkconfig:2044-2055). Update mode also sets
  the `wifi` log tag to none.
- The password field is masked. FRAME refuses while a secret-entry screen is
  open, and a full refresh clears the panel on exit.
- Host canary tests feed a fake password through setup and fail if it appears in
  any DEV line, log, frame, durable record plaintext or NVS value.

### Crash dumps and panic output

A crash can copy the password out of RAM into places agents read:

- **Core dumps.** The pinned config writes ELF core dumps of task stacks to
  flash on a crash (sdkconfig:1860-1874), and the writer is linked into Newt
  (**V** `esp_core_dump_write` in the `regional-dev-usb` ELF). It writes the
  64 KiB coredump partition at `0xff0000`, which every bench full backup reads.
  A stack holding the password or the derived Wi-Fi key could land there.
- **Panic text.** On the C3, the panic handler prints registers and 1 KiB of
  stack memory ("Stack memory:") to the console (**V** `print_state` calls
  `panic_print_registers` and `panic_print_backtrace`; the config has no frame
  pointer or EH frame backtracer). Agents capture that serial output.

Design:

- **Secrets exist only in update-mode boots.** Wi-Fi setup and every decrypt run
  there; a reading boot never loads the key or the record.
- **Console output only in a known reading boot; fail closed** (#157, **V**
  in the linked ELF and host tests). The class: any printer that formats
  RAM (an abort message, registers, stack, a corrupted heap word, a failed
  value) reaches the console. Gating each printer missed some (fix rounds 1
  to 3), so the gates sit on the console's channels. One 32-bit .bss word,
  `newt::firmware::crashOutput`, decides; it permits output only while it
  holds exactly `0x52454144`. `enterRequestedUpdateMode` writes that value
  once update mode is ruled out (`runUpdateMode` never returns); nothing
  clears it. Zero, any other value or a corrupted word withholds.
  `firmware/crash_output.h` gates three channels:
  - **Panics and aborts.** Every one enters IDF's handler through
    `esp_panic_handler`, which prints the reason or the abort message (caller
    RAM), the fault's details, registers and stack memory, an address when a
    debugger is attached, and writes the core dump. The link wraps that entry
    (`--wrap`). In any other boot the gate prints one fixed line, `Newt: crash
    output withheld in this boot. Rebooting...`, sets the handler's reset
    reason (panic or watchdog) and restarts through `panic_restart`, the
    handler's own restart. `esp_backtrace_print` (the task watchdog's
    register printout, outside the handler) meets the same check through its
    own wrap. Arduino's core defines an unused `__wrap_esp_panic_handler` for
    a hook Newt never sets; a build define renames it.
  - **ROM print.** `esp_rom_printf`, `ets_printf` and IDF's early logs end in
    the ROM's two printf channels: heap poisoning's `CORRUPT HEAP` line,
    `ESP_ERROR_CHECK`'s failure, the task watchdog's task list, Arduino's
    `log_*`. Startup's first init step (`ESP_SYSTEM_INIT_FN` priority 0,
    ahead of IDF's lowest, 1) installs `newt_console_putc` as channel 1 and
    empties channel 2. The gate is IRAM and reads only the DRAM word, so it
    runs with the cache off; it forwards to `esp_rom_output_putc`, IDF's
    equivalent of the putc the bootloader installs, and otherwise drops the
    character. The link wraps `ets_install_putc1` and `ets_install_putc2` into
    nothing, so Arduino's `setDebugOutput` and `uartEnd` cannot replace it.
  - **stdout, stderr, esp_log and the VFS console.** `runUpdateMode`'s first
    step, before any Wi-Fi call, sets every esp_log tag to none, reopens
    stdout and stderr on `/dev/null` and unregisters `/dev/console`,
    `/dev/uart` and `/dev/secondary` (`withholdConsole`). The console keeps
    its own descriptors on the other two while stdin holds it open and
    forwards every write to both, so all three go: IDF drops every
    descriptor on them (stdin's included) and later opens fail. IDF's and
    libraries' logs, `printf`, `puts` and any `write` reach no console in
    update mode.

  **What still prints:** the bootloader's banner and its own lines (before
  the app; unchanged), the ROM's reset line with `Saved PC` (printed by ROM
  before the bootloader; observed as `esp_restart_noos` on software resets,
  never a RAM read, and it may be any value after corrupted control flow),
  the fixed gate line, and the panic handler's fixed re-entry line. Newt's DEV bridge and Newt's own update-mode
  lines (counts only) use the USB serial driver, not these channels, and
  keep working. **Costs (accepted):** before setup rules out update mode, a
  reading boot's crash prints only the fixed line and writes no dump, and its
  ROM print (early IDF warnings, a heap corruption report) is dropped. Update
  mode has no IDF logs.
- **Nothing reaches the handler, its printers, the dump or the console
  writer around the gates (V).** `tools/verify-nvs-guard.py` fails the build
  unless each protected function (the real handler and `esp_backtrace_print`,
  `print_state`, `print_abort_details`, the register, stack, string and
  number printers, the panic's FIFO writer, every core dump writer function
  down to its flash write, the ROM channel installs, `esp_rom_output_putc`
  and the ROM's console writers, `send_packet` included) and the word are
  referred to only by an exact reviewed set of named functions, aliases
  included. IDF's own copy of the channels (`esp_rom_vprintf`,
  `esp_rom_output_to_channels`) and the UART and USB serial driver installs
  must stay unlinked. The console's VFS tables are referred to only by the
  startup steps that register them, and the paths those steps register are
  exactly the paths update mode unregisters. Every resolved address counts, a `lui` alone included, and must
  name a gate's first instruction: entering past the check fails. No loaded
  data section stores the address of any protected function, start or
  interior. Each gate is the pinned instruction pattern: load the word, build
  the value, `bne` to the withheld path, fall through to one tail jump to the
  real function. The panic gate's withheld path must print the fixed line,
  set the reset reason and call `panic_restart`; the putc gate's must only
  return, and its path to the ROM must be IRAM or ROM. The install must be
  the first init step, for core 0 at the CORE stage (IDF skips an entry
  without core 0), and install only the gate, and `runUpdateMode`'s first
  call must be `withholdConsole`. **Console hardware:** every function that
  builds an address in UART0's or USB-Serial-JTAG's register file (a `lui`
  or `auipc` that reaches it) and every object that holds one is in a
  reviewed inventory, each with why it cannot put RAM on the console in
  update mode (the DEV bridge, the panic's writer, the unregistered VFS
  drivers, pad setup, glyph data equal to an address by chance). A word in
  no object is named by its input section in `firmware.map`; only C++
  exception tables (`.gcc_except_table.*`, the unwinder's call-site bytes,
  never a pointer) are reviewed that way. Anything else fails
  (`tests/native_crash_output_test.py` links real RISC-V fixtures: a shared caller, a stored callback table, a
  call to a gate's interior, a stored pointer to a gate's tail, a nop'd or
  inverted branch, an unwrapped call, an abort path around the gate, a
  channel install or console write around the putc gate, a putc gate without
  its check or outside IRAM, a late install or one without core 0, update
  mode without the withhold or leaving a console path open, a console table
  registered by new code, a RAM byte stored straight into the USB serial
  FIFO, a register pointer table and a new caller of a ROM writer all
  fail). **Limits:** resolved code addresses, register addresses built by
  `lui` or `auipc` and aligned stored addresses; an address computed at run
  time from other values is not seen, and the reviewed IDF functions' own
  control flow is trusted as pinned.
- **Before leaving update mode,** Newt replaces the driver's Wi-Fi config with an
  empty one, stops Wi-Fi and clears its own buffers, then restarts. RAM is not
  cleared by a restart, so this also protects the next boot (V, host; 085).
- **Every boot clears the heap's RAM before registering it** (085, V in the
  linked ELF and host tests), so a panicked update-mode boot's freed blocks
  never reach a later boot.
- **Second line (erase a found core dump in the next boot): superseded.** A
  password byte left by a panicked update-mode boot could reach a core dump
  only through RAM the dump captures. No path remains: the next boot writes
  no dump until setup rules out update mode (#157's gate), and by then every
  heap byte, the startup stack's included, has been cleared (above), the lent
  range reloaded, each task stack filled, and a dump takes task stacks only,
  no DRAM (085's three pinned facts). On the bench, a crash in the first boot
  after a panicked update mode wrote a dump without the fake password (below).
- **Forced-crash test (bench, before any real network is entered):** a
  bench-only test image (never installed for the owner) panics on a fixed key
  sequence while a fake password is live. The Lead then takes a full 16 MiB
  backup and scans **all of it**, plus the serial capture, for the fake
  password and its derived Wi-Fi key. Repeat with the crash in the first boot
  after update mode.
- **Forced crashes (V, bench, USB power, #157 fix round 1):** a bench-only
  image of this change (never merged) put a fake canary, an ASCII marker and
  a 32-byte pattern, in a stack and a heap buffer, then stored to address 0;
  a third case called `esp_system_abort` with a stack message holding the
  marker and the pattern in hex. In update mode, 500 ms into a Wi-Fi scan, on
  the networks page after it, and for the abort, the console showed only
  the bench's own line, `Newt: crash output withheld in this boot.
  Rebooting...` and the ROM's reset banner; no reason, abort message,
  register or stack line. The coredump partition stayed byte-identical, and
  neither the serial capture nor the full 16 MiB backup held a new copy of
  the canary (raw, reversed, hex, 8-byte pieces, or rebuilt from printed
  words). The same crash in a reading boot printed the reason, registers
  and stack memory and wrote a new, valid dump; both held the canary. The
  dump holds task stacks only (no `CAPTURE_DRAM`), so the heap copy appeared
  nowhere. Slice B part 2 covered the fake password and the crash in the boot
  after update mode (next bullet).
- **Forced crashes with a fake password (V, bench, USB power, slice B part
  2, 2026-10-30):** a bench-only image of the merged join (never merged)
  saved a made-up network, a random 32-character name no scan heard, with a
  fake password that the image holds only XOR a runtime key. Twice, Test
  crashed 800 ms into the join, when only the Wi-Fi driver held the
  password: before the ROM's reset banner the console showed only `Newt:
  crash output withheld in this boot. Rebooting...`, and the coredump
  partition stayed byte-identical. Then, in the first boot after that panic
  (reset reason panic), a forced crash of the reading boot printed its
  panic output and wrote a new core dump. Neither the serial stream, the
  dump nor any full 16 MiB backup held the password, its derived key (the
  PBKDF2 PMK) or an 8-byte piece of either (raw, reversed, hex, or rebuilt
  from printed words). Battery power is not covered.

### Bench backups and Git

- Until the forced-crash test passes, treat every full backup taken after a
  Wi-Fi boot as possibly holding the password: keep it private and never copy
  it. After the test passes, backups hold only the key and need no scrubbing.
  Keep backups and card copies apart, as now (docs/080: never put dumps or card
  copies in Git).
- Git holds fake values only. Agents never see or type the owner's password.
- On the bench, the owner enters a network once by hand. A throwaway hotspot
  works if the owner prefers.

## 5. Integrity and authenticity

### Manifest

A small text file next to the image: format, kind (`newt` or `crossink`), size
and SHA-256. A `newt` manifest adds version and build (the 12-hex ID HELLO
reports); the publish step writes it from the clean build's identity receipt.
A `crossink` manifest adds the descriptor's version text and ELF SHA-256.

### Checks before writing (on the card file)

One writer (section 5, "Writing") serves both kinds. Only the validation policy
differs.

Both kinds:

- Exact size; at most the usable size (7,016,448 B) and the slot size.
- SHA-256 of exactly `size` bytes equals the manifest.
- `appImageIntact` (`include/newt/app_slot_identity.h:132`): a structure and
  checksum check (ESP32-C3 header, segments inside the slot, XOR checksum,
  appended SHA-256). It deliberately omits the bootloader's chip-revision and
  eFuse gates, which reject CrossInk's images under Newt's IDF (its comment,
  `:124-129`), so it is not every check the bootloader makes.
- Power (section 6). The reader's save was verified before the restart into
  update mode (section 6), as the CrossInk switch does (docs/077:76-97).

`newt`:

- The Newt mark (`include/newt/app_slot_identity.h:23-28`) and the identity
  block below; its version and build equal the manifest's.
- The version is SemVer2 and newer than the running one (section 7).

`crossink`:

- The descriptor's project name is CrossInk (`tools/x3_slots.py:32-33`) and its
  version text and ELF SHA-256 equal the manifest's. CrossInk has no Newt
  identity block, and its version (`v1.4.0.1-1-g0de623ed` on the bench) is not
  SemVer2, so no version ordering applies.
- The image SHA-256 is on a short approved list compiled into Newt (one entry
  per CrossInk image the owner approved, today the main unit's `31ce770` and the
  bench's `v1.4.0.1`). A new CrossInk image needs a reviewed Newt change. The
  Lead computes the digests privately from backups; a digest is not secret.
- The screen shows CrossInk's version text, not a Newt version.

**Newt identity block.** Today the image carries a 16-byte mark at `0x120`
(`include/newt/app_slot_identity.h:17-28`) but no version or build. Add a
second fixed block right after it, in the same `.rodata_custom_desc` section
(`firmware/app_slots.h:11`): version (24 bytes) and build ID (16 bytes). Keep
the 16-byte mark unchanged, so older Newt builds and tools still recognise the
image. The block sits inside the 512-byte head that tools already read (about
40 B of flash).

### Selection before any erase

Today's selection write does not always arm rollback. `otaSelect` returns
"already selected" without writing anything when the next boot already picks
the target slot (`include/newt/app_slot_identity.h:110-113`), and
`commitOtaSelection` then reports Verified (`firmware/crossink_switch.h:24-25`).
Two cases make that unsafe for an installer:

- **Bootloader fallback.** The records select a slot whose image is damaged, so
  the bootloader boots the other one. That damaged slot is now the
  non-running slot. A new image written there would inherit the old valid
  record and boot with no first-boot rollback.
- **An earlier unconfirmed selection.** Its outcome is unknown, so the record
  the device would fall back to is unknown too.

So the installer needs one settled state before it erases anything:

1. The running image is confirmed (not pending; `esp_ota_get_state_partition`).
2. Readable records make the next boot select the **running** slot through a
   settled (valid) record, and no earlier selection is unresolved (the
   boot-lifetime `selectionUnconfirmed`, `firmware/crossink_switch.h:49-51`).
3. If records select the other slot (case one), first commit a fresh selection
   of the running slot and require Verified. The running image then stays the
   fallback. If that is Unconfirmed or no safe choice exists, refuse: "Nothing
   was installed. Newt could not confirm which app starts next. After a
   restart, this Newt or the app in the other slot may start." It never says
   nothing was changed: an unconfirmed settle write may have landed.
4. After the write, the new image always gets a **fresh** state-new record in
   the other sector. "Already selected" is an error for the installer, never
   success. The running slot's record is never touched.

### Writing

1. Erase as it goes (64 KiB steps), write sectors 1 onward, keep sector 0 (the
   header) in RAM. Until the end, the slot's head reads as empty.
2. Write sector 0 last.
3. Read the whole image back. Check `appImageIntact`, the manifest SHA-256 and
   the identity again. On any failure, erase sector 0, so the slot reads empty.
4. Select with a fresh state-new record, read back as `commitOtaSelection` does.
   Restart only on Verified. Unconfirmed is reported as unconfirmed, never as
   failed (AGENTS.md rule), and blocks a retry until readback resolves it.
5. First boot: confirm at the first finished screen (section 2), or the
   bootloader rolls back to the running image, whose settled record step 2 or 3
   secured.

A power loss before step 4 leaves the old selection in place. After step 4,
the X3 boots the new, fully checked image as pending, and step 5 decides. A
power loss never leaves an unchecked image selected. A power loss during step 2
can leave a torn header; section 8 covers its repair.

### What a LAN attacker can do (no signing)

In update mode, someone on the home network can pretend to be the Lead's
computer (spoofed address or name) and serve their own manifest and image. The
SHA-256 only proves the bytes match *that* manifest, and the mark is easy to
copy. If the owner presses Install, the attacker's code runs with full access:

- it can read the card (books, the ciphertext) and NVS (the key), so the Wi-Fi
  password. On WPA2-Personal, anyone already on the network knows that password;
- it can rewrite the bootloader and table (the flash guard is only an API check);
- it can burn eFuses, for example to disable USB download: **permanent loss of
  USB recovery**.

Outside update mode, there is no Wi-Fi and nothing to attack.

### Cheap mitigations (recommended)

- Update mode starts only from the Update screen and runs only while it is
  open. Before writing starts, it stops on Back, on Sleep, or after 10 minutes
  without a button press while the owner chooses. Download time does not
  count toward the 10 minutes; Back or Sleep during a download cancels it and
  leaves both slots untouched. Once writing starts, nothing stops it except a
  failure, which leaves the running slot selected.
- Outbound only: the X3 never listens.
- Exact size, SHA-256, structure, chip, mark and identity checks.
- Never older (anti-downgrade); equal only by explicit reinstall.
- **Check code:** before Install, show the first 16 hex digits of the image
  SHA-256 (64 bits) with the version and build. The Lead's update message
  carries the same code, sent separately from the image (the Lead's message to
  the owner). A matching code means the bytes are the ones the Lead built;
  forging a match is not practical. The time limit adds no authenticity; it
  only shortens exposure.

### Later (each needs its own approval)

- A shared update key (HMAC) is another credential.
- A signature with a public key in Newt is a "signing stack", which AGENTS.md
  excludes, and its private key is a credential.
- HTTPS from GitHub after #137.

## 6. Memory and power

### Facts

- Reading heap low: 50,872 B worst in the 2026-10-26 heap bench and 50,848 B in
  the 2026-10-27 midnight bench, against the 50,468 B floor: a margin of about
  380-400 B (ledger). Floors never move (docs/CODE_HEALTH.md:137-138).
- Static RAM grows, heap shrinks one for one. "Off costs nothing"; memory a
  feature holds while unused (Wi-Fi is the named case) needs explicit owner
  approval, quoted in the record's `owner_approval` (docs/CODE_HEALTH.md, "Memory policy").
- Newt links Wi-Fi on `feat/wifi-base` for update mode's scan, without
  `esp_netif` or lwIP (**V**: clean health build, [084](084-wifi-memory.md)).
  Wi-Fi's `.data`/`.bss` and libphy's DRAM rodata are one range, padded to
  22,528 B so it holds the SD card's FATFS context, lent to the heap in every
  other boot; libphy and libesp_ringbuf IRAM code run from flash. Flash grows
  466,256 B. Wi-Fi IRAM options stay off
  (sdkconfig:1809, 1811).
- Runtime heap: station plus DHCP about 45-110 KB; Espressif's C3 table shows
  332 K free for an empty project and 219 K with a station (NEWT-25 brief,
  sources there). Arduino lowers buffers to static RX 4 and dynamic RX/TX 32
  (`WiFiGeneric.cpp:273-280`).
- A fresh Newt boot has 134,492 B free with a 114,676 B largest block (bench,
  USB, 2026-10-26, Lead receipt). A reader open needs 128,212 B
  (docs/053-warm-reopen-memory.md:8-10).

### Why update mode is its own boot

- Wi-Fi (about 60-110 KB) and an open reader (128 KB) cannot both fit.
- A restart gives a clean, unfragmented heap and guarantees no reader, cover,
  cache or stats work runs next to Wi-Fi.
- **Save first, then restart.** A restart loses any progress, Home record or
  statistics fold that only RAM still holds. Today's CrossInk switch already
  guards this: it runs the reader exit path and refuses while
  `exitSaved()` is Unconfirmed (`firmware/crossink_runtime.h:9-12`,
  `include/newt/reading_save_notice.h:72-76`, `firmware/crossink_switch.h:66-68`).
  Entering update mode (and Wi-Fi setup, which also runs there) uses that same
  boundary **before** it sets the intent or restarts. If a save is unconfirmed,
  Newt stays open and says "Newt could not save your place. Update not
  started." Then it drains USB, cancels sleep, clears the warm record, closes
  the reader and the card, and restarts, as `CrossInkRuntime::restart()` does
  (`firmware/crossink_runtime.h:17-33`).
- How: the Update screen sets a one-shot RTC intent word (magic plus check) and
  restarts. `setup()` sees it early, clears it and runs only the update screens.
  Back or completion restarts normally. A lost intent (power-off) just boots
  normally, which is safe. A bad check is ignored. This is a transient request,
  not a preference, so the durable-preference rule does not apply. RTC grows by
  a few bytes; the record states it.

### What reading pays

- **Phase 1 (SD):** nothing held. The installer's buffers (two 4 KiB sectors
  and a SHA-256 context) live only in update mode.
- **Wi-Fi linked in (measured, image (c)):** reading pays nothing. The owner
  chose it from three measured images (2026-10-28):
  - **The carve-out (M2).** Wi-Fi's `.data`/`.bss` and libphy's DRAM rodata
    sit in one range that every boot but update mode lends to the heap,
    after the RAM heap, before the reader or a cache allocates. Update
    mode keeps it; the bootloader reloads it on every boot. libphy and
    libesp_ringbuf IRAM code run from flash, and the FreeRTOS timer task
    stays unlinked. Details and guards: [084](084-wifi-memory.md).
  - **This build (compile, clean health build):** resident static RAM
    126,704 → 125,280 B (-1,424 B); lent range 22,528 B (17,424 B Wi-Fi,
    5,104 B pad), with its own ceiling; RTC 300 → 300 B; flash 2,391,536 →
    2,857,792 B (+466,256 B, about 4.2 MB still free in the slot).
  - **Bench (step B, the image (c) spike, USB power):** against alpha.1, free
    heap at Home 145,824 vs 145,656 B; the same book admissions and largest
    blocks; `getMinFreeHeap` lows 62,640-63,068 vs 62,428-62,808 B over three
    heterogeneous books (floor 50,468 B); loop stack lows within 72 B; about
    0.1 s more before Newt's first row per reset (a bigger image). The
    spike's range was 22,504 B because it also linked lwIP.
  - The lent range is a separate heap region: the reader's big blocks never
    use it, so the bench checked admissions and largest blocks, not only the
    total.
- Phase 3's join links `esp_netif` and lwIP. It needs its own measurement
  against the same floor before it ships.

### Power rules

- **No power gate (owner decision, 2026-10-31).** Until then update mode
  scanned, joined, checked and installed only on USB power from a computer.
  The owner removed that rule: it made them stay plugged in to save a network
  or update, and nothing ever showed harm on battery. The install is
  power-loss safe by design: it writes the other slot, checks the whole image
  before selecting it, otadata is two CRC'd sectors, and a new build counts
  as good only after its first screen, or the X3 rolls back. A battery that
  dies mid-update only loses that attempt. No battery threshold or other
  gate replaces the rule.
- **Known gap, not a gate:** battery current during download and write, gauge
  accuracy, and brownout at low charge while Wi-Fi transmits (peaks about
  335 mA, NEWT-25 brief; **U**) are not measured.
- **Battery on the device is a separate hands-on check, still pending**
  (AGENTS.md): Wi-Fi, Check and an install, unplugged. Host tests do not show
  it.
- During the write: no sleep, no idle timer, buttons ignored, "Do not turn off"
  on screen. Wi-Fi is already off.
- A power loss is harmless by the write order above.
- On battery, sleep is a full power-off (ledger); the post-update restart is a
  software reset, so the RTC intent survives it.

## 7. Versions and What's new

### Facts (V)

- `.cz.toml`: Commitizen, `semver2`, version `0.1.0-alpha.0`, version file
  `include/newt/firmware_version.h`, tag format `v$version`,
  `major_version_zero`.
- Tag `v0.1.0-alpha.0` exists at `f008308b` (2026-10-06) and is an ancestor of
  `c83f64b6`. `cz bump --get-next --prerelease alpha` there prints
  `0.1.0-alpha.1` (run read-only this round).
- The tag is annotated by the owner: "First reviewed software baseline under
  docs/RELEASING.md", with exact-source CI, the host gate and four clean
  resource gates passing. It also says "Physical NOT RUN" and makes "no
  firmware installation or new hardware acceptance claim". CHANGELOG.md has
  its `v0.1.0-alpha.0` section. Checked from local git and docs only.
- So it is a reviewed source release, not a hardware-accepted one.
  docs/RELEASING.md described tagging it as a future decision; the
  0.1.0-alpha.1 release branch replaced that stale text.
- Recommendation: use the tag as the bump baseline, say in each release that
  it carries no hardware acceptance, and fix the stale RELEASING.md text in
  the first bump's release branch.

### Owner-update flow

1. The Lead updates the What's new notes (#143's source file).
2. On a release branch from main: `release_tools.py`, then
   `cz bump --get-next --prerelease alpha`, then
   `cz bump --version-files-only --check-consistency --yes --prerelease alpha`,
   review the diff, and commit
   `chore(release): 0.1.0-alpha.N -> 0.1.0-alpha.N+1`. Merge it; once main's
   CI gate passes on the release merge, CI tags it (fallback:
   `python3 -B tools/release_tag.py`; docs/RELEASING.md, "Tag the release").
   Until the tag exists the release gate fails every later merge. No bare
   `cz bump`: its tag would land on the branch commit, not on the reviewed merge.
3. Build the one firmware build clean; publish the candidate with its manifest
   to the card or the host.
4. Send the owner the version, build and check code.

Bench candidates between releases keep their current version label. They go
over USB, or over Wi-Fi as an explicit same-version reinstall.

### How the X3 decides "newer"

- Compare the manifest version with `NEWT_FIRMWARE_VERSION` by semver2
  precedence (numeric prerelease parts compare as numbers).
- Newer: offer Install. Equal: offer only "Reinstall" behind an extra confirm.
  Older: refuse ("Use USB to install an older version").
- Going back to the previous Newt in the other slot is a separate, explicit
  action (Phase 5). Records written by the newer build may fall back to
  defaults in the older one.

### What's new after an update

- On the first confirmed boot of a build the X3 has not seen, show "Updated to
  *version*" and the What's new page once, when Home is first on screen after
  confirmation (docs/083). Back returns.
- A small durable card record stores the last seen build. Missing or corrupt:
  record the current build and show nothing (product default). It works for USB
  installs too. Nothing is held in RAM while reading.

## 8. Testing

### Host (each test fails with its fix removed)

The installer core is plain C++ in `include/newt/`, like
`app_slot_identity.h`, with fakes:

- **Flash:** sector erase to `0xFF`; writes only clear bits; a cut at any
  operation.
- **otadata and bootloader model:** new → pending → valid on confirm; pending →
  aborted on the next boot; an image that fails validation is skipped.
- **Source:** card file and HTTP stream with short read, extra bytes, stall,
  bad hash, wrong size, wrong chip, missing mark, identity mismatch, older or
  equal version, malformed manifest.

Cases:

- Cut power at every step of a full install, then boot the model: the result is
  always the old image or the fully checked new one, never neither.
- Torn header: removing "sector 0 last" makes this fail.
- Bit flip on readback: removing the readback check makes this fail.
- Bootloader fallback: records select a damaged valid slot, Newt runs from the
  other. The install reselects the running slot first, gives the new image a
  fresh state-new record, and a failed first boot still rolls back. Removing
  the reselection or accepting "already selected" makes this fail.
- An unresolved earlier selection, or a running image still pending, refuses
  before any erase.
- Failed first boot: removing deferred confirmation makes the rollback test
  fail. This runs through the real firmware startup wiring, with a link check
  that the `verifyRollbackLater` override is in the ELF (AGENTS.md: exercise the
  actual startup wiring, with a negative control).
- Interrupted download leaves the slot untouched.
- CrossInk consent: no path writes a CrossInk slot without it.
- Per-kind policy through the one writer: a `crossink` image whose digest is
  not on the approved list is refused; a `crossink` image is never compared by
  Newt version; a `newt` image without the identity block or with a
  non-SemVer2 version is refused.
- Canary: the fake password never appears in DEV lines, logs, frames, record
  plaintext or NVS.
- Wi-Fi network record, through the real update-mode startup wiring: cold boot
  with all RTC and global state discarded restores the saved network, and
  restores an explicit "no network" (an older network never reappears);
  missing or corrupt record or key gives the default; a failed commit keeps
  the previous runtime choice; a lost acknowledgement resolves by readback; an
  unconfirmed commit reports unconfirmed and a reboot yields the complete old
  or new value; the key is never replaced before "no network" is verified.
  Negative control: removing the durable restore makes the cold-boot case
  fail.
- Entering update mode through the real entry wiring, like
  `tests/crossink_switch_test.cpp`: an unconfirmed progress save, Home record
  save or statistics fold each keeps Newt open with no intent set and no
  restart. Moving the save check after the restart makes these fail.
- Update-mode heap telemetry lines, like the reading ones
  (docs/memory-floor-measurement.md).

### Bench (no agent-held credentials)

The owner does by hand, in one batched session (≤ 15 minutes):

- Phase 1: copy `/Newt/Update/` files to the bench card (an image of a newer
  test version and CrossInk's image).
- Phase 3: enter a network on the bench once (any network the owner chooses).
- The power-cycle checks below that need hands (unplugging, the power button).

The Lead does the rest over DEV USB (`PRESS` only) and `bench_flash`:

- Install from the card; HELLO shows the new build; `slots` shows the selection.
- Interrupt a write by opening the serial port mid-write (that resets the X3):
  the old build still boots.
- Reset right after selection, before the first finished screen: the next
  boot shows aborted and the previous build, with the "did not start" message.
- Install CrossInk from the card; CrossInk boots; return to Newt by USB.
- Canary on hardware: type a fake password with `PRESS` for a network that does
  not exist; scan serial output, frames and the whole next session backup for
  it and its derived Wi-Fi key.
- Forced-crash test (section 4) with the bench-only test image, before the
  owner enters a real network.
- Wi-Fi: compare NVS bytes in session backups before and after (only
  `newt_wifi` may change; the fence keeps `phy` and `nvs.net80211` unchanged).
- Measure update-mode heap minimum and reading heap minimum after an update.

### Power-cycle acceptance (owner-run, separate facts)

AGENTS.md treats these as separate hardware checks. Record the power condition
and reset evidence (DIAG reset reason and the boot record, read over DEV USB
after replugging) for each. A passing USB check does not satisfy the battery
one. Run them after a Phase 1 update and again after Phase 3 Wi-Fi setup:

1. **USB-powered sleep and wake:** the same build, still confirmed; What's new
   does not show again; the reading page is kept.
2. **Unplugged battery sleep and wake** (a full power-off on battery): the same
   build, still confirmed, no rollback; What's new does not show again; after
   Phase 3, the saved network (or the explicit "no network") still holds.
3. **Cold power-on:** the same results, with a power-on reset reason.

Until these pass, the candidate is not ready for owner acceptance; say so.

### Host durable-restore cases

Through the actual firmware startup wiring, each with a negative control that
fails when the durable restore is removed:

- cold boot with all RTC and global state discarded: the What's new last-seen
  record and the Wi-Fi record restore from the card; no update intent means a
  normal boot;
- a stale or corrupt RTC intent never forces update mode; durable records win
  over anything stale in RTC;
- missing or corrupt records give the product defaults; failed commits keep
  the previous runtime choice (section 4).

### Recovery

`bench_flash` over USB stays the fallback. Phase 1 must prove it for every
layout an install can leave, before any OTA install on the main unit:
Newt+Newt, Newt+Empty, Newt+CrossInk and Newt+Invalid (the repair in
section 1), each with Newt running from app0 and from app1.

Host tests for the tools and the installer model, in both slot orientations:

- a torn header (cut during the final sector-0 write) and an interrupted
  erase of the first block each leave an Invalid or Empty inactive slot, the
  running image still boots, and the repair writes only that slot;
- the repair refuses when the survivor is not running, not selected, not
  settled or fails its MD5; removing any of those checks makes a test fail;
- the repair never selects the repaired slot before its image verifies.

## 9. Phased plan

Costs are estimates (**U**) until measured; each phase that grows a ceiling
carries one measured record for the one build.

| Phase | Scope | Acceptance | Owner decisions | Rough cost |
| --- | --- | --- | --- | --- |
| **1. SD install** | Installer core; update-mode boot; deferred confirmation; identity block; slot rule and tools (x3_slots, bench_flash, docs/077, docs/080); CrossInk from the card (the way back once CrossInk is replaced); "Updated to" + What's new once. Not in Phase 1: Wi-Fi, phone setup, a boot combo or other recovery shortcut, going back to the previous Newt | Host fault harness with negative controls; full gate and clean health build; bench: install, interrupt, rollback, CrossInk from card, USB restore and repair for every layout; owner-run USB sleep/wake, battery sleep/wake and cold power-on with reset evidence; then the main unit with the owner | 1, 2, 8, 9, 10, 11 | Flash about +10-20 KB; static RAM about 0; held heap while reading 0; RTC a few bytes; update-mode heap about 10 KB; no NVS |
| **2. Wi-Fi measurement** | Link Wi-Fi and start nothing; measure static cost; prototype the carve-out (M2) | Clean health report; reading heap minimum on the bench ≥ 50,468 B with Wi-Fi linked; largest free block and reader admissions unchanged | 7 | Flash about +0.8-1.0 MB; SRAM up to about 32.5 KB before the carve-out |
| **3. Wi-Fi setup** | Scan, keyboard, split storage, Forget network; crash-dump and panic-output suppression in update mode; a connect test in update mode | Canary tests; forced-crash whole-backup scan; NVS compare; update-mode heap minimum; reading floor unchanged; durable-restore cases; the three power-cycle checks again | 4, 5 | Flash small; NVS about 7 slots; card one record |
| **4. Wi-Fi update** | Manifest fetch, download to card, install through the core; host publish tool and server | The Lead runs round trips on the bench with no hands; a stopped server leaves the slot untouched; then one owner update on the main unit | 3, 6, host changes | Flash: HTTP client, small |
| 5. Optional | Go back to the previous Newt; a FreeInk-style boot combo; phone setup; address discovery | Each its own | Each its own | Each its own |

## Open facts

- The main unit's bootloader bytes (rollback is inferred, not read).
- Whether the bootloader's fallback on a bad image works on these units.
- Wi-Fi's linked cost once the join adds `esp_netif` and lwIP (phase 3); the
  scan-only cost is measured ([084](084-wifi-memory.md)).
- Why the bench's stored PHY calibration fails Newt's check (a refused store
  on every update-mode boot), and whether that calibration is full.
- CrossInk's image size; whether CrossInk's own switch assumes two slots.
- Flash write time for a full image, and battery current during a download
  and a write.
- Brownout behaviour during Wi-Fi transmit at low charge, and the battery
  gauge's accuracy. These are known gaps since updates opened to battery
  (rule 8), not gates.
- Whether the owner's network isolates clients or offers router DNS names.
- Owner confirmation that the reviewed source tag `v0.1.0-alpha.0` is the bump
  baseline (decision 10).
