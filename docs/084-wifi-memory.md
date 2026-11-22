# 084: Wi-Fi memory (#103 phase 2)

**Status:** on `feat/wifi-base`. Host tests, compile and resource evidence
only in this repo. This build's bench check is the Lead's receipt; USB
sleep/wake, battery sleep/wake and cold power-on are separate checks and not
covered. Design: [082](082-ota-updates.md) section 6.

Newt links Wi-Fi for update mode only. Reading must not pay for it, so
Wi-Fi's static data lives in one range that every other boot gives to the
heap. This is image (c) from the phase 2 measurements: the carve-out (M2),
libphy and libesp_ringbuf IRAM code in flash, and the timer-task stub, on the
build without the RGB LED path.

## The carve-out

- `tools/wifi-sections.py` derives Newt's `sections.ld` from the pinned
  ESP-IDF 5.5.2 script (framework-arduinoespressif32-libs 5.5.0+sha.87912cd291)
  into `.build/regional-dev-usb/ld/`. The build's first `-L` flag
  (`platformio.ini`) makes the linker take it before the framework's copy.
- It refuses a pinned script or framework package with another SHA-256, and
  any anchor it does not find exactly once. The derived file keeps
  Espressif's Apache-2.0 notice and says what Newt changed.
- Three edits, nothing else:
  1. `.data`/`.bss` of the files only Wi-Fi links (`WIFI_FILES`), the named
     `.bss` inputs only Wi-Fi uses in a shared file (`WIFI_SECTIONS`), and
     libphy's DRAM read-only data go to `.dram0.newt_wifi`, between
     `_newt_wifi_start` and `_newt_wifi_end`.
  2. libphy's and libesp_ringbuf's IRAM code, and the IRAM functions in
     `FLASH_FUNCTIONS`, go to flash.
  3. The constructors in `WIFI_CONSTRUCTORS` leave boot's list for their own,
     which only update mode runs (`runWifiConstructors`).
- The range is a loaded section, so the bootloader writes it fresh on every
  boot, zeros included. Update mode always starts with clean Wi-Fi data.
- Wi-Fi's event groups referenced FreeRTOS's `timers.c`, which would start
  the timer task in every boot. `firmware/update_wifi.h` defines the one
  function they need as a stub; `tools/verify-nvs-guard.py` still fails the
  build if `timers.c` links.

## The lend

- `enterRequestedUpdateMode` is setup's first step once the panel is ready.
  Update mode never returns from it: it leaves only by restarting. Every
  other boot then calls `lendWifiMemory()`, which gives the whole range to
  the heap. This happens before the reader, a cover or a cache allocates.
- The lent heap gets the C3 RAM type's caps one priority lower
  (`heap_caps_add_region_with_caps`). IDF tries every heap at priority 0
  before any at priority 1, so boot places each allocation where a build
  without the lend does until the RAM heap cannot hold it. Only overflow
  takes the lent heap, and it does so before the retention heaps, which
  include the large heap the reader opens books in.
- The first overflow is the SD card's FATFS context. `SD.begin` allocates it
  through `esp_vfs_fat_register` when setup first mounts the card, after the
  lend, and keeps it for the whole boot: 8,832 B of VFS state plus three
  4,136 B file objects, 21,240 B, a 21,504 B block. alpha.1 keeps it in its
  RAM heap, which is 16,000 B bigger. Here the RAM heap's free space at the
  mount is too small, so the range is at least 22,528 B (`LENT_FLOOR` in
  `tools/wifi-sections.py`): zero padding after Wi-Fi's 20,672 B (17,424 B
  before slice B's lwIP) leaves room for the block and the lent heap's own
  overhead. The pad comes out of the
  RAM heap, so resident RAM does not change. A bigger Wi-Fi range needs less
  pad or none (slice B's lwIP left 1,856 B); a bigger context fails the
  build (Guards).
- Without the pad, the context took part of the large heap's free block, and
  Home's largest block fell from alpha.1's 114,676 to 90,100 B. That
  happened with `heap_caps_add_region`, whose address-derived caps put the
  lent heap first at priority 0, and with priority 1 alone (bench,
  2026-10-28). The step B spike's 22,504 B range held it.
- Update mode does not lend: Wi-Fi uses 20,672 B of the range there, and the
  1,856 B pad stays idle, so update mode's heap is that much smaller.
- Before the lend, only startup code runs. The linked image has one startup
  root into Wi-Fi code, `s_set_default_wifi_log_level`. It sets four log
  tags (`wifi`, `mesh`, `smartconfig`, `ESPNOW`) through
  `esp_log_level_set`. Arduino's `initArduino` then calls
  `esp_log_level_set("*", ...)`, which frees those entries before `setup()`,
  so the root holds no heap and nothing in the lent range.
- DEV builds print `Newt Wi-Fi memory lent: N bytes, error E`, one row per
  heap (size, free, largest block), then one row per used block of 8 KiB or
  more with its heap's index, once per boot while the bridge is quiet.

## Update mode's scan

- Settings → **Wi-Fi** opens update mode on **Wi-Fi networks**, which scans
  once as it opens on USB power: station on, one blocking scan, stop, deinit,
  event loop deleted. Select (**Scan**) scans again; Back leaves update mode.
  Picking a network, a password and joining are phase 3
  ([085](085-wifi-setup.md)).
- The IDF API only: Wi-Fi's own NVS off (`nvs_enable = 0`), settings in RAM,
  the `wifi` log tag silent, and `phy_bbpll_en_usb(true)` so USB and the DEV
  bridge stay up. The scan starts only on USB power.
- Wi-Fi never writes flash. Before its first Wi-Fi call the scan raises the
  NVS guard's fence (`fenceFlashForWifi`). While it is up, every partition
  write or erase and every read-write `nvs_open` is refused; reads pass. The
  pinned PHY stores its calibration to NVS when the stored data is missing
  or invalid, which `nvs_enable = 0` does not stop; under the fence that
  store fails at its open, before any NVS change, and the radio keeps this
  boot's calibration (082, "Keeping Wi-Fi out of NVS"). The scan lowers the
  fence only after `esp_wifi_deinit` succeeds. The DEV row ends with
  `fenced-opens N, fenced-writes M`: what the fence refused during that scan.
  A refused PHY store shows as one open on a boot's first scan.
- `esp_wifi_deinit` refuses while Wi-Fi still runs, so its success is the
  proof that Wi-Fi is off. If it fails, flash stays fenced and update mode
  restarts into Newt instead of showing a page that could install.
- Network names are other people's data. The DEV row carries counts and
  times only, DEV FRAME refuses while the list shows, and previews and tests
  use invented names.

## Guards

Each runs in the gate and has a negative control.

- `tools/verify-wifi-link.py`, after every health build (receipt
  `.build/regional-dev-usb/wifi-link-verification.json`;
  `tests/wifi_link_test.py`):
  - the lent range is all of `.dram0.newt_wifi` and holds only the reviewed
    files' inputs;
  - no function outside Wi-Fi code calls Wi-Fi code or refers to its data or
    to any address in the range (padding and merged constants included),
    except the scan (only from update mode), the lend, and the saved Wi-Fi
    network's AES-GCM seal and open ([085](085-wifi-setup.md)). mbedTLS's
    allocator hooks and AES DMA state are Wi-Fi's, so those two are library
    entries: mbedTLS code only Wi-Fi and they reach counts as Wi-Fi's, and
    every caller of each must be update-mode code (`runUpdateMode` and what
    only it reaches). Update mode never lends the range;
  - each startup root into Wi-Fi code calls only reviewed functions outside it;
  - no libphy or libesp_ringbuf function is in IRAM, and no IRAM function
    calls one or takes its address;
  - the range holds the SD card's FATFS context (`tools/wifi_lent_heap.py`):
    `vfs_fat_ctx_t` and `FIL` from the ELF's DWARF (IDF's `vfs_fat.c`),
    `max_files` from the only `SD.begin` call, rounded up to the lent heap's
    TLSF size class, plus 1,024 B for the lent heap's own overhead.
- **Limit:** the check reads what the linked code spells: calls, resolved
  addresses, `lui`/`auipc` pairs, gp offsets and aligned data words. A
  pointer built by other arithmetic, an indirect call's target and ROM code
  are not followed. The receipt counts IRAM's indirect calls (198 in this
  build). The 1,024 B overhead is a margin over the 740 B the bench
  measured (the context's block starts 740 B into the lent heap); TLSF's
  control grows about 68 B each time the heap doubles, far less than the
  range itself. Whether something else overflows into the lent
  heap before the mount is not a link fact; the DEV block rows show it on
  every boot.
- `tools/verify-nvs-guard.py`, after every health build
  (`tests/native_flash_writers_test.py`): `nvs_open` is reached only through
  `__wrap_nvs_open` (the PHY's store included), NVS's own opens only from
  `nvs_open`, partition writes and erases only from their wraps, and the
  flash write primitives (`esp_flash_*`, the legacy `spi_flash_*`, ROM and
  bootloader writers) only from the partition APIs or the panic core dump.
  The core dump writer is reached only from the panic handler, behind a
  gate that runs it only in a known reading boot (082, "Crash dumps and panic output";
  `tests/native_crash_output_test.py`). Tail jumps and address loads count. **Limit:** direct references only; a
  function pointer stored in data, such as the flash chip driver's table
  that `esp_flash_write` calls through, is not followed.
- `tests/nvs_guard_test.cpp`: the raised fence refuses every partition write
  and erase and every read-write open before it reaches flash, and passes
  reads; lowered, writes pass again.
- `tools/code_health_resources.py` reports the range as `wifi_lent_bytes`,
  with its own ceiling, apart from resident `ram_bytes`
  ([CODE_HEALTH](CODE_HEALTH.md#memory-policy); `tests/code_health_lent_test.py`).
- `tests/update_firmware_test.py`: the actual setup wiring lends in a
  reading boot and not in update mode; removing the lend fails.
- `tests/wifi_lend_test.py`: the actual lend against a model of IDF's heap
  list with this build's heaps. Every request the RAM type serves tries the
  RAM heap, then the lent heap, then the other heaps as before; other memory
  never comes from the lent heap. The plain `heap_caps_add_region` lend, a
  wrong priority and missing or extra caps fail.
- `tests/update_wifi_test.py`: the actual scan, its caller, update mode's
  loop and the DEV frame port against a Wi-Fi model that fails one step at a
  time. Wi-Fi is fully off before any flash write, or update mode restarts.
  It models the PHY's calibration store with missing, invalid, corrupt and
  valid data through the real scan wiring and fails on any flash write while
  Wi-Fi may run. A corrupt entry makes NVS clean it up inside the PHY's
  read (`eraseEntryAndSpan`); the fence refuses that write, the read fails
  and the PHY calibrates as for invalid data. Dropping the raise, lowering
  the fence after a failed deinit, never lowering it, or a fence that
  guards only read-write opens and non-NVS writes fails.
- `tests/update_screen_test.cpp`, `tests/system_ui_rule_test.py` and
  `tools/gate_previews.py`: the row and the page (`artifacts/update-wifi*.png`).

## Measured

Clean `--health-report` build against `ae405596` (flash record 74, protected
update 46). Compile evidence only:

| | Before | After |
| --- | --- | --- |
| Flash | 2,391,536 B | 2,857,792 B (+466,256 B) |
| Resident static RAM | 126,704 B | 125,280 B (-1,424 B) |
| Lent range (`wifi_lent_bytes`) | none | 22,528 B (17,424 B Wi-Fi, 5,104 B pad) |
| RTC | 300 B | 300 B |

- The flash fence and its DEV counts (fix round 2) add 272 B of flash and no
  RAM: the counts are two bytes in existing `.sbss` padding.
- All 522 selected stack ceilings are unchanged; `setup()` stays 464 B and
  `loop()` 192 B. The new frames are unselected: `scanWifiNetworks` 304 B,
  `WifiLendReport::publish` 256 B (DEV), `scanUpdateNetworks` 64 B,
  `lendWifiMemory` 16 B. `runUpdateMode` grows 352 → 704 B: its view holds
  ten network rows. Static frames do not prove runtime peaks.
- The scan path does not start `esp_netif`, so lwIP is not linked. Phase 3's
  join links it (below).

The bench X3 (2026-10-28, USB power only; battery sleep and cold power-on
are not covered) ran this build's image (`ad1965e2`) and alpha.1
(`fb1e4a03`) through the same journeys: five resets, then three books by
Library row (39: small, many short chapters; 26: the largest, multi-MB;
1: multi-MB, cached, last read), each opened and read for 60 s.

| | alpha.1 | This build |
| --- | --- | --- |
| Largest free block at Home (5 boots) | 114,676 B | 114,676 B |
| Free heap at Home (DIAG) | 145,656 B | 146,336 B |
| Largest block while reading, rows 39 / 26 / 1 | 53,236 B each | 53,236 B each |
| `getMinFreeHeap`, rows 39 / 26 / 1 | 62,808 / 62,428 / 62,472 B | 63,576 / 63,196 / 63,204 B |
| Loop stack low, rows 39 / 26 / 1 | 6,400 / 6,400 / 6,960 B | 6,408 / 6,408 / 6,984 B |

- Every boot lent 22,528 B. The DEV rows showed the same layout each time:
  the lent heap holds only the 21,504 B FATFS block, which starts 740 B in
  and leaves 276 B free, and the large heap holds no used block of 8 KiB or
  more. The memory floor is 50,468 B.
- Without the pad, the largest block at Home was 90,100 B on all five boots,
  with the address-derived lend and with priority 1 alone. With the first,
  it was 40,948 B while reading.
- Update mode scanned twice with error 0, and Back restarted into a boot
  that lent and read row 1 normally. Update mode's free heap: 154,576 B at
  start (159,680 B without the pad), 124,716 B before the first scan,
  lowest 70,468 B.
- With the fence (`5d294215`, then `004d5db0`; one update-mode boot each),
  the same scan journey passed: both scans error 0, Back restarted into a
  boot that lent 22,528 B and read row 1 (`getMinFreeHeap` 63,204 B), and
  the NVS bytes were the same before and after. On `004d5db0` the first
  scan's row ended `fenced-opens 1, fenced-writes 0` and the second
  `0, 0`: one refused open per boot, which fits the PHY store (082,
  "Keeping Wi-Fi out of NVS"). The first Wi-Fi start took 40 ms and the
  second 8-9 ms; before the fence the first took 93-98 ms (four boots).
- Phase 2 step B's spike of this design had a 22,504 B range: it used
  Arduino's WiFi class, which links lwIP and esp_netif (lwIP alone 4,397 B
  of it).

### Slice B: lwIP and esp_netif (flash record 77)

The join (docs/085) links esp_netif and lwIP. `WIFI_FILES` already named
both, so their `.data`/`.bss` went into the lent range: lwIP 3,146 B,
esp_netif 26 B, with small growth in the Wi-Fi libraries. Clean
`--health-report` build, compile evidence:

| | Before (record 76) | After |
| --- | ---: | ---: |
| Flash | 2,878,512 B | 3,025,200 B (+146,688 B) |
| Resident static RAM | 125,296 B | 125,296 B |
| Lent range | 22,528 B (pad 5,104 B) | 22,528 B (pad 1,856 B) |
| RTC | 300 B | 300 B |

The bench X3 (2026-10-29, USB power only; battery sleep and cold power-on
are not covered) ran this build (`a1aa1b74`) and alpha.1 (`fb1e4a03`)
through the journeys above:

| | alpha.1 | This build |
| --- | --- | --- |
| Largest free block at Home (5 boots) | 114,676 B | 114,676 B |
| Free heap at Home (DIAG) | 145,612 B | 146,292 B |
| Largest block while reading, rows 39 / 26 / 1 | 53,236 B each | 53,236 B each |
| `getMinFreeHeap`, rows 39 / 26 / 1 | 62,804 / 62,452 / 62,468 B | 63,528 / 63,148 / 63,156 B |
| Loop stack low, rows 39 / 26 / 1 | 6,404 / 6,404 / 6,980 B | 6,412 / 6,412 / 6,956 B |

- Every boot lent 22,528 B. Row 1's stack low is run-to-run spread:
  alpha.1 read row 1 three times today, at 6,980, 6,980 and 6,948 B.
- A bench-only image (this build plus a made-up network with a random
  32-character name the scan never heard, and a fake password) saved that
  network, then tested it: result "not found" (802.11 reason 201) after
  2.4 s, Wi-Fi off 6 ms later, no refused NVS open or write. Update mode's
  lowest free heap was 67,556 B. After the test, free heap stayed 6,316 B
  below its level before (lwIP's task and its memory, until the restart).
  Back restarted into a boot that lent 22,528 B and read row 1
  (`getMinFreeHeap` 63,156 B). The NVS bytes were the same before and
  after the test.
- The heap clear took 0.78 ms (238,724 B) in `heap_caps_init` and 0.04 ms
  (10,576 B) in `main_task`, at 160 MHz, every boot. First paint was
  1,976 ms against alpha.1's 1,979 ms.

Part 2 (2026-10-30, the same bench X3, USB power only) ran the same journeys
on the product image of the source merged with main `c77f0644`
(`0b232873`) and on alpha.1:

| | alpha.1 | Merged build |
| --- | --- | --- |
| Largest free block at Home (5 boots) | 114,676 B | 114,676 B |
| Free heap at Home (DIAG) | 145,612 B | 146,276 B |
| Largest block while reading, rows 39 / 26 / 1 | 53,236 B each | 53,236 B each |
| `getMinFreeHeap`, rows 39 / 26 / 1 | 62,796 / 62,452 / 62,468 B | 63,556 / 63,176 / 63,184 B |
| Loop stack low, rows 39 / 26 / 1 | 6,404 / 6,404 / 6,964 B | 6,412 / 6,412 / 6,956 B |

- Every boot of the merged build lent 22,528 B; first paint was 1,979 ms
  against alpha.1's 1,978 ms.
- Row 1's loop stack low is 8 B under alpha.1's 6,964 B. alpha.1 read row 1
  three times that day (6,964, 6,980 and 6,964 B) and three times the day
  before (6,980, 6,980 and 6,948 B); this build read 6,956 B both days.

### Phase 4: TLS and HTTP (flash record 79)

Update mode's GitHub check (docs/088) links `esp_http_client`, esp-tls and
mbedTLS's TLS layer. `WIFI_FILES` adds them and the DS peripheral code esp-tls
links, so their `.data`/`.bss` go into the lent range. Two DS locks in the
shared `esp_crypto_lock.c.obj` join it by name. The DS code's constructor, a
heap mutex, runs only as update mode starts. `heap_caps_malloc_prefer`, called
only by flash code, moves to flash. Clean `--health-report` build, compile
evidence only:

| | Before (record 78) | After |
| --- | ---: | ---: |
| Flash | 3,025,280 B | 3,228,624 B (+203,344 B) |
| Resident static RAM | 125,296 B | 125,296 B |
| Lent range | 22,528 B (pad 1,856 B) | 22,528 B (pad 80 B) |
| RTC | 300 B | 300 B |

The range is now 80 B from full: the next Wi-Fi feature with static state
needs a reviewed raise of the range's ceiling (`wifi_lent_bytes`).
