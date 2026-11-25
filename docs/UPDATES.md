# Updates and Wi-Fi

Newt installs an update into the X3's spare app slot and keeps the running
image as the fallback. The update comes from the SD card, or Newt downloads
it over Wi-Fi to the card first. Wi-Fi runs only in update mode, a separate
boot. This page states the rules that code keeps and names what enforces
them.

## Slots

Newt keeps the X3's partition table and never installs one
(`rust/firmware/x3_existing_partitions.csv`): app0 at `0x10000` and app1 at
`0x780000`, each `0x770000` bytes, and otadata at `0xe000`, two 4 KiB
sectors. Newt runs from one slot. The other holds another firmware (stock,
CrossPoint, CrossInk or any X3 app), an older Newt, or nothing.

**One slot-identity rule.** Identity is read from each slot's first 512
bytes, never assumed:

- **Newt:** the 16-byte mark `NEWT-X3-APP\0` plus format 1 at offset
  `0x120`, in `.rodata_custom_desc` right after the ESP-IDF app descriptor
  (magic `0xabcd5432` at `0x20`). An identity block follows: version (24
  bytes) and 12-hex build (16 bytes). The installer matches it against the
  manifest; no slot decision trusts it alone.
- **Other:** any other valid app. **Empty:** all `0xff`. **Invalid:** a bad
  image or descriptor magic.

Another firmware's name comes from its descriptor's project: `CrossInk`,
`crosspoint-reader` ("CrossPoint"), else "the other firmware". The rule has
three copies that change together: the device's
(`rust/core/src/update_card.rs` `classify`, and
`rust/core/src/slot_installer/identity.rs`; the mark is defined in
`rust/firmware/native/app_descriptor.h`), the bench tools'
(`tools/x3_slots.py`) and the install page's (`installer/src/slots.ts`).
`tests/x3_slots_test.py` holds the shared vectors; `tests/installer_test.py`
exports them to `installer/test/x3-slot-vectors.json`, and
`tests/slot_vectors_test.py` runs the Rust rules against them.

**Selection** follows the ESP-IDF bootloader. A record counts when its
sequence is set, its CRC matches and its state is not invalid or aborted;
the highest sequence boots slot `(seq - 1) % 2`. Rollback is on: a new
record boots as pending, and one still pending at the next boot is aborted
in favor of the other record. A new image confirms itself at its first
finished screen ("Card install"); a crash or reset before that returns to
the previous image. Newt selects a slot by writing a fresh record
into the sector the next boot does not follow and reading it back
(`commit_ota_selection`, `rust/core/src/slot_installer/selection.rs`). The
record the device falls back to is never touched, and Newt does not call
`esp_ota_set_boot_partition`.

**Only the slot that does not run is written.** The card installer writes
`1 - running` (`install_target_slot`). The bench tools write Newt's slot
beside one other firmware; without one, the booting slot must hold a
selected, confirmed Newt. They refuse two other firmwares or an invalid
slot beside one. The install page writes the slot that does not boot,
beside any firmware. The tools also select with a fresh record, so USB
installs get the same rollback.

**Restart into the other firmware** (`rust/firmware/firmware_switch.rs`).
The Settings row counts only an Other slot (`other_firmware_slot`); an older
Newt does not count, since restarting into it is a hidden downgrade. Select
runs the reader's exit save (an unconfirmed save stops here), checks the
whole image (`app_image_intact`), writes the selection, and restarts only
when the readback shows the switch. Other firmwares cannot switch back: the
way back is their own card install of `newt.bin`, or USB.

**USB recovery.** The install page works from any firmware
([installer/README.md](../installer/README.md)). On the bench,
`tools/bench_flash.py` installs, restores, repairs a damaged spare slot and
switches the selection ([bench-flash](bench-flash.md)). Opening the serial
port resets the X3: wait 30 seconds after a switch, or the bootloader aborts
the unconfirmed image. If another firmware's updater overwrites Newt's slot,
the tools refuse; use the install page or that firmware's card install.

## Card install

**Card files** live in `/Newt/Update/` (`rust/core/src/update_card.rs`). Newt
tries to create this folder after each boot's successful card mount. A folder
error does not change the mount result. A
Newt update is `newt.bin` and `newt.manifest`, the same pair for every
source. The manifest is LF-terminated lines in this exact order, at most 512
bytes (`rust/core/src/update_manifest.rs`):

```text
format=newt-update-1
kind=newt
version=0.1.0-alpha.1
build=0123456789ab
size=2414608
sha256=<64 lowercase hex>
```

The version is SemVer 2.0.0; the build is 12 lowercase hex digits. A
missing, repeated or extra line is refused. `tools/update_publish.py` writes
the pair and refuses what the device would refuse. `newt.bin` must end
exactly where its image ends. Another firmware installs from its own `.bin`,
with no manifest: update mode lists up to three `*.bin` files other than
`newt.bin` (no hidden files, names under 48 bytes). Erased padding after its
image is dropped; any other trailing byte is refused.

**Update mode is its own boot.** Settings → Update (or Wi-Fi) runs the
reader's exit save; an unconfirmed save keeps Newt open. Then Newt writes a
one-shot request into the empty SD-resume words in RTC memory and restarts
(`rust/firmware/update_entry.rs`, `rust/core/src/update_mode_request.rs`).
Startup takes it before any reading allocation (`update_boot::take` in
`rust/firmware/app_startup.rs`). Every reset consumes it and only a software
reset honors it, so a lost request just boots Newt. Update mode never
returns: every exit restarts through `update_mode::leave`
(`rust/firmware/update_mode.rs`), including ten idle minutes. Why: Wi-Fi and
an open book do not both fit in the heap, a restart gives a clean heap, and
secrets never meet a reading boot. Builds without the `wifi-update` feature
have no update mode.

**How the installer writes** (`SlotInstaller`,
`rust/core/src/slot_installer.rs`, one writer for every source):

1. Check the whole card file: size (Newt at most the usable 7,016,448 B),
   image structure (ESP32-C3 header, chip 5, 1 to 16 segments placed as the
   bootloader needs, XOR checksum, appended SHA-256), the manifest's SHA-256
   and identity. Newt must be newer; the same version needs an explicit
   reinstall.
2. Check the device: a readable target head, consent to replace or install
   another firmware, a settled running image and no unresolved selection.
   If the records do not boot the running slot, re-select it first.
3. Erase in 64 KiB steps and write from the second sector on. The first
   sector (the image header) goes last, so the slot reads empty until the
   end.
4. Read the whole slot back and check it again. A failure erases the first
   sector again.
5. Select it with a fresh state-new record. "Already selected" is an error,
   so rollback is always armed.

Failures before step 3 change nothing; later ones leave the running Newt
selected; "unconfirmed" means the next boot may start either image. A power
loss never leaves an unchecked image selected.

**Confirm after the first finished screen.** Newt exports
`verifyRollbackLater` returning true (`rust/firmware/lib.rs`), so Arduino
does not confirm before `setup()`; `tools/verify-nvs-guard.py` checks the
hook in the linked image. Newt confirms at the boot's first finished screen,
whatever it is (`finish_first_paint` in
`rust/firmware/app_runtime/run_loop.rs`): a completed paint that ended its
press, never a working frame such as "Opening…". It marks the image valid
and reads the state back up to three times
(`rust/core/src/update_confirm.rs`). Until then the DEV bridge answers
nothing, so a HELLO answer means the build confirmed itself.

**"Updated to" once.** The last confirmed build is a durable card record,
`newt_seen` (`rust/core/src/update_seen.rs`). At the first finished Home
after confirmation, a record naming another build shows "Updated to
<version>" with What's new, then records this build. A version too long
for one title line, such as a test build's, gets its own line. A missing record is
written silently; a damaged one is replaced.

## Wi-Fi memory

Newt links Wi-Fi for update mode only, and reading must not pay for it.
Wi-Fi's static data lives in one range that every reading boot gives to the
heap.

- `tools/wifi-sections.py` derives Newt's `sections.ld` from the pinned
  ESP-IDF script (the first `-L` in `platformio-rust-app.ini` puts it ahead
  of the framework's) and refuses any other script. It moves the
  `.data`/`.bss` of files only Wi-Fi links (`WIFI_FILES`: radio, lwIP,
  esp_netif, HTTP client, esp-tls, mbedTLS), Wi-Fi-only inputs of shared
  files (`WIFI_SECTIONS`) and libphy's DRAM rodata into `.dram0.newt_wifi`,
  at least `LENT_FLOOR` bytes. It moves libphy, libesp_ringbuf and
  `FLASH_FUNCTIONS` IRAM code to flash, and takes `WIFI_CONSTRUCTORS` out of
  boot's list (only update mode runs them).
- The range is a loaded section, so the bootloader rewrites it every boot.
- A reading boot calls `wifi_memory::lend` (`rust/firmware/wifi_memory.rs`)
  once, before the SD mount. The lent heap has the RAM type's caps one
  priority lower, so only RAM-heap overflow lands there; the first is the SD
  card's FATFS context. Update mode never lends.
- `rust/firmware/native/wifi.cpp` stubs the one timer function Wi-Fi needs,
  so FreeRTOS's timer task never links (`tools/verify-nvs-guard.py`).

**Guards.** `tools/verify-wifi-link.py` checks the linked image
(`tests/wifi_link_test.py`): the range holds only reviewed inputs and
padding; nothing outside Wi-Fi code refers to it or its code except reviewed
entries, and update-mode entries (radio, HTTP, TLS, the saved network's
AES-GCM) are called only from code that only `run_update`
(`rust/firmware/app_startup.rs`) reaches; no moved function is in IRAM or
called from IRAM; and the range fits the FATFS context
(`tools/wifi_lent_heap.py`). Limit: an address computed at run time, an
indirect call's target and ROM code are not followed.

`tools/code_health_resources.py` reports the range as `wifi_lent_bytes`,
apart from `ram_bytes`, with its own reviewed ceiling
([CODE_HEALTH](CODE_HEALTH.md#memory-policy)).

## Wi-Fi never writes flash

Wi-Fi starts with its own NVS off (`nvs_enable = 0`) and RAM storage. That
does not stop the PHY: with missing or invalid calibration data, it writes
the `phy` NVS namespace. So Newt fences flash while Wi-Fi may run.

- **The fence** (`fence_for_wifi`, `rust/salamander/nvs_guard/guard.rs`):
  while up, every partition write or erase and every read-write `nvs_open`
  is refused; reads pass. The PHY's store fails at its open, and the radio
  keeps this boot's calibration.
- **Raise and lower** (`WifiRun`, `rust/firmware/update_radio.rs`): every
  scan and join raises it before the first Wi-Fi call and lowers it only
  after `esp_wifi_deinit` succeeds, which proves Wi-Fi is off. If deinit
  fails, the fence stays up and update mode restarts.
- So no network save, forget or slot install happens while Wi-Fi may run.
  The online download writes only the card, which is not flash.
- **Link check:** `tools/verify-nvs-guard.py` fails the build unless
  `nvs_open`, partition writes and erases go only through their wraps
  (`platformio-rust-app.ini`) and flash write primitives are reached only
  from the partition APIs or the gated core dump
  (`tests/native_flash_writers_test.py`). Limit: a function pointer stored in
  data is not followed.

## Wi-Fi setup

Settings → Wi-Fi restarts into update mode on **Wi-Fi networks**, which
scans as it opens. An open network (`WIFI_AUTH_OPEN` only) saves at once; a
secured one asks for its password (Save from 8 characters). A saved network
adds **Test** and **Forget** rows.

**Storage split.** Neither half alone reveals the password.

- **Card:** record `newt_net` in `/Newt/State/newt-net.nrs`
  (`rust/core/src/wifi_network_store.rs`), 146 bytes, in the durable record
  store. Kind 0 is an explicit "no network". Kind 1 holds a 12-byte nonce,
  the AES-256-GCM ciphertext of a fixed 98-byte plaintext (name and
  password, zero-padded so the length never shows) and the tag. Magic,
  version and kind are authenticated.
- **NVS:** namespace `newt_wifi`, blob `key`, 32 random bytes, through the
  NVS guard. RTC holds no part of the network or the key.
- **Randomness:** each scan mixes 32 bytes from the radio-on RNG into a RAM
  pool; keys and nonces are SHA-256 of the pool, a counter and one more RNG
  word. With no scan this boot, nothing is sealed.

**Commit rules** (`rust/core/src/wifi_network_store/workflow.rs`):

- **Save:** read the key; if missing, write and verify a new one first. Seal
  with a fresh nonce, commit, and show the network only once verified.
- **Forget:** commit and verify "no network", then erase the key (an erase
  needs no free NVS space).
- **Results:** "Saved", "Not saved" (nothing changed) or "Not confirmed"
  (after restart, either setting may return).
- **Restore** (each update-mode boot) writes nothing. A missing or damaged
  record, a missing key or another key's record gives "no network".

**Test** joins once (`join_wifi_network`,
`rust/firmware/update_radio/join.rs`): fence up, one connect, up to 20 s for
an address or a disconnect, no retry; the 802.11 reason gives the result.
Then a full teardown. Check uses the same join.

## Secrets in RAM

**Keeping the password out of output.** The password and key live in
`SecretBytes` (`rust/firmware/wifi_secrets.rs`): pinned, no Clone, Copy,
Default or formatting, wiped in place with volatile stores. A join copies
the password into the driver's config and wipes both buffers before the
driver starts. Every exit runs `update_mode::leave`, which empties the
driver's config, stops Wi-Fi and wipes the password and entropy pool before
the restart: `esp_restart` runs no destructor, and RAM survives it. DEV rows
carry codes and counts only; DEV FRAME refuses the networks and password
pages.

**Crash dumps and panic output.** A crash could copy a secret into the
coredump partition or onto the console. One fail-closed word, `crashOutput`
in `rust/firmware/native/crash_output.h`, decides: RAM-bearing output passes
only while it holds exactly `0x52454144`. A reading boot sets it once update
mode is ruled out (`nb_allow_reading_crash_output`,
`rust/firmware/app_startup.rs`); nothing clears it. The gates sit on the
console's channels, not on each printer:

- **Panics and aborts** reach IDF's handler only through the wrapped
  `esp_panic_handler`. Otherwise the gate prints one fixed line, sets a
  panic reset reason (so no update request is honored) and restarts with no
  core dump. `esp_backtrace_print` is gated the same way.
- **ROM print:** startup's first init step installs an IRAM gate as the
  ROM's printf channel; the link turns every other install into nothing.
- **stdout, stderr, esp_log, VFS console:** update mode's first step
  (`withholdConsole`, from `run_update`) silences them and unregisters the
  console's paths. The DEV bridge uses the USB serial driver and still works.

`tools/crash_output_guard.py` (run by `tools/verify-nvs-guard.py`) proves
from the linked image that nothing reaches the handler, its printers, the
core dump writer, the ROM channels or the console's VFS tables except
through these gates (`tests/native_crash_output_test.py`).

**The next boot.** A panic skips `leave`, and a reset keeps RAM.
`tools/verify-next-boot.py` checks in the linked image
(`tests/next_boot_test.py`) that the load segments cover
`.dram0.newt_wifi` (every boot rewrites it), that new task stacks are
filled with `0xa5`, that `CONFIG_ESP_COREDUMP_CAPTURE_DRAM` is off (a dump
holds task stacks only), and that every boot clears the heap's RAM before
IDF registers it: `rust/firmware/native/heap_clear.h` wraps
`soc_get_available_memory_regions` and `heap_caps_enable_nonos_stack_heaps`
(`tests/heap_clear_test.py`).

## Updates over Wi-Fi

Update mode's list ends with **Check GitHub for updates**
(`rust/device-ui/src/update/paint.rs`); it needs a card and a saved network.
Check joins once (`rust/firmware/update_online_runtime.rs`) and reads
`LATEST_RELEASE_URL` in `rust/core/src/update_online.rs`: GitHub's
`/releases/latest`, no token, which skips drafts and pre-releases.

**Test builds.** Test builds are GitHub pre-releases: CI publishes a
nightly build of `main`, versioned `<next>-dev.YYYYMMDD.N` like
`0.2.2-dev.20261123.1` ([RELEASING](RELEASING.md#nightly-test-builds)), with
the same assets and checks as a release. A test build ranks above `0.2.1`
and below `0.2.2`, so a later nightly, or a release above it, replaces it.
Devices ignore them unless Test builds is On. That switch is on a hidden
Developer page: open About's first newt, then press Previous, Next,
Previous, Next. With it On, Check reads `NEWEST_RELEASE_URL` instead:
`/releases?per_page=1`, the newest release of either kind, as a list of one.
Turning it Off does not install anything. On a nightly, Check can offer
**Return** to the latest official release, even when it is older. A separate
confirmation shows both versions and says it is older; Back keeps the current
image. A stable build never offers a stable-to-stable downgrade.
The choice is the card record `newt_builds` (`TestBuildsCodec` in
`rust/core/src/durable.rs`), Off when missing or corrupt. Update mode
restores it with the clock choices at its own boot. Versions, like
every manifest version, are at most 23 characters.

**Returning to official Newt.** The minimum compatible official release is
`0.5.0` (`official_return::MINIMUM`). Check refuses an older target that cannot
read the saved-data formats written by this build. A successful Check syncs
and verifies a small `Newt/Update/official.return` sidecar, using a temporary
file and rename. It binds the staged image digest to the running nightly's
version and build. After a restart, Newt validates it again and asks for
confirmation again. It is deleted after installation or a mismatch. A missing,
corrupt or unconfirmed sidecar means **Check again**, before any erase. Merely
copying an older image and manifest onto the card does not grant a return.
The sidecar prevents accidental installs; it does not authenticate a card
against someone who can replace its contents.

**Assets.** A release has three: `newt.bin`, `newt.manifest` and
`NOTICES.txt` (`tools/release_publish.py`;
[RELEASING](RELEASING.md#publish-a-github-release)). The device reads only
`newt.manifest` and `newt.bin` by exact name and refuses a repeated name.
The release JSON is parsed as a stream (`rust/core/src/update_release.rs`).

**Checks** (`rust/core/src/update_online/check.rs`). The manifest must parse,
be kind `newt`, fit the usable slot, match the release's size for
`newt.bin`, and be newer than the running version, or qualify for the explicit
official return above. `newt.bin` must match its
size (and Content-Length, when sent) and SHA-256. Newt follows redirects
itself, at most 5 per file, only to `https://` URLs.

| What | Bound |
|---|---|
| Release JSON | 512 KiB, 30 s |
| Manifest | 512 B, 15 s |
| Image | the manifest's size, 300 s |
| Each socket step | 10 s, less near the file's limit |
| Headers per response | 16 KiB |
| URL or redirect / asset URL | 2,048 B / 512 B |
| Image tries | 3, only when a card write fails |

A file's limit covers its redirects, headers and body. Socket timeouts alone
do not stop a server that trickles bytes, so `rust/firmware/update_https.rs`
runs a one-second timer that shuts the sockets past the limit.

**HTTPS** (`rust/firmware/native/update_https.cpp`): `esp_http_client` over
mbedTLS, TLS 1.2. Every request attaches the certificate bundle; no path
skips the certificate or hostname check. `tools/crt_bundle.py` writes the
bundle from Mozilla's CA list in the pinned certifi and refuses any other
copy (`tests/crt_bundle_test.py`). There are no date checks: the X3 has no
trusted clock, so an expired certificate is accepted.

**One installer input.** The network never writes a slot. The download goes
to `/Newt/Update/` (`rust/firmware/update_download.rs`): remove the old
manifest, write and sync `newt.bin`, then write and sync `newt.manifest`
last. A manifest therefore only names a whole, hash-checked image. With
Wi-Fi off, the list offers it, and the card installer checks it all again.

**Memory.** TLS needs more heap than the join leaves, so Check lends the
idle logical frame to mbedTLS during a session
(`rust/firmware/update_tls_frame.rs`) and repaints only the download bar.
If a block is still held after the last session, update mode restarts.

## What is not protected

- **No signing.** The checks prove an image matches its manifest, not who
  built it. Anyone who can write the card can offer an image; the Install
  press is the only gate.
- **Over Wi-Fi,** a LAN attacker cannot serve a fake release without a
  certificate for GitHub's hosts from a bundled CA. Whoever controls the
  release repository, or a trusted CA, can. Without date checks, a leaked
  key of an expired certificate still works.
- **Code that runs has full access.** Secure boot and flash encryption are
  off. An installed image can read NVS (the Wi-Fi key) and the card (the
  sealed network), rewrite the bootloader and partition table, and burn
  eFuses, which could end USB recovery for good. USB access gives the same.
- **Downgrade refusal is Newt's rule,** not the bootloader's: there is no
  anti-rollback eFuse.
- **Outside update mode there is no radio.** In update mode Newt only makes
  outbound connections; the X3 never listens.
