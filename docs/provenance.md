# Source provenance

Newt's own code is under the MIT License ([LICENSE](../LICENSE)). Every
third-party input keeps its own license, listed here. Sources were inspected
2026-10-01; licensing was rechecked 2026-11-21 against the Rust firmware
image. Source behavior is pinned; README summaries may lag it.

## Notices

- [NOTICES.txt](NOTICES.txt) is Newt's third-party notice file: every
  third-party component linked into `newt.bin`, with its license and notice
  text, copied from the pinned sources. Every GitHub Release ships it beside
  `newt.bin` ([RELEASING](RELEASING.md#publish-a-github-release)).
- The install page bundles other code (esptool-js and its dependencies), so
  its build ships that code's notices from
  [installer/NOTICES.txt](../installer/NOTICES.txt), next to the firmware's
  `NOTICES.txt`.

## Name and logo

The Newt name and these images are not covered by the MIT license
([README](../README.md#name-and-logo)). The list comes from the tools that
read the brand source and from the Rust firmware build's linker map (commit
`cd032e179043`, `v0.1.0-alpha.21`, `firmware.bin` SHA-256 `5c93ecd3…e719`).

| File | What it is | Made by |
| --- | --- | --- |
| `assets/frosted-home/source/assets/newt-brand-reference.png` | the source art: the Newt mascot and wordmark; every file below comes from it | owner-approved original |
| `docs/newt-wordmark-light.png`, `docs/newt-wordmark-dark.png` | the README's wordmark | `tools/make-install-wordmark.py` |
| `installer/src/newt-wordmark.png` | the install page's wordmark | `tools/make-install-wordmark.py` |
| `rust/device-ui/src/assets.rs` (`kSleepBrand`, `kSleepBrandMaskRuns`) | the sleep screen's brand stamp | pinned literal from the brand reference (converted from the retired `src/sleep_brand_data.inc`) |
| `assets/frosted-home/stage-backdrop.bin`, `rust/stage-home/assets/frosted-home/stage-backdrop.bin` | Home's backdrop: the blurred mascot with the wordmark (the copy under `rust/stage-home/` is linked into `newt.bin`) | `tools/make-stage-backdrop.py` |
| `rust/system-ui/src/data.rs` (`BASE_MAP`, `BASE_TILES`), from `assets/frosted-home/resources.json.zlib` | Frosted Home's base raster, with the wordmark baked in | pinned literal (converted from the retired `src/frosted_home_data.inc`) |

A firmware build from this source links the last three into `newt.bin` and
shows the sleep stamp and the Home backdrop, so whoever distributes a
modified build must replace them. Screenshots under `artifacts/` and the
design mockups in `assets/frosted-home/source/screens/` also show these
images.

## Where each input ends up

"Repo" means files tracked in this repository. "Not linked" was checked
against the current Rust build's linker map: the code is discarded at link
time or never referenced.

| Input | License | Repo | `newt.bin` | Install page |
| --- | --- | --- | --- | --- |
| Newt's own code, tools, docs and tests (including `rust/tiny-app/src/pocket-pause.app`, built from `apps/pocket-pause.json`) | MIT | yes | yes | yes |
| Newt brand artwork (listed under Name and logo above, including `rust/stage-home/assets/frosted-home/stage-backdrop.bin`, `kSleepBrand` in `rust/device-ui/src/assets.rs` and `BASE_MAP`/`BASE_TILES` in `rust/system-ui/src/data.rs`) | Not MIT (see Name and logo) | yes | yes | wordmark only |
| `rust/firmware/sample.epub` (embedded built-in sample, identical to `tests/fixtures/the-small-lantern.epub`) | CC0-1.0 | yes | yes | no |
| `tests/fixtures/the-small-lantern.epub` and the library cover fixtures | CC0-1.0 | yes | no | no |
| [FreeInk SDK](https://github.com/Free-Ink/freeink-sdk/tree/cd6f5b831c8fad899723b6d406586f00c575f021) `cd6f5b8`, including code derived from the OpenX4 E-Paper Community SDK | MIT | git submodule; `patches/` holds Newt's changes; `rust/THIRD_PARTY.md` lists Newt's Rust translations (`rust/core/`, `rust/device-ui/src/update/keyboard.rs`) | yes | no |
| Expat (in FreeInkBook) | MIT | submodule | yes (C objects) | no |
| miniz (in FreeInkBook; Rust port `rust/core/src/inflate.rs`) | MIT | submodule and `rust/core/src/inflate.rs` | yes (Rust `newt_core::inflate`; ROM `mz_*`/`tinfl_*`) | no |
| libunibreak (in FreeInkBook) | zlib-style | submodule | yes (C object) | no |
| TJpgDec (in FreeInkBook; Rust port `rust/core/src/jpeg/`) | ChaN's license | submodule and `rust/core/src/jpeg/` | yes (Rust `newt_core::jpeg`) | no |
| stb_truetype (in FreeInkFont) | MIT or public domain | submodule | not linked | no |
| FreeType (in FreeInkFont) | FTL | submodule; `rust/firmware/fonts/native/native.c` adapter | not linked | no |
| Unicode character data | Unicode License v3 | submodule (`libunibreak`), `rust/core/src/chapter_layout/arabic.rs`, and Rust `core` | yes | no |
| pngle (in FreeInkBook; Rust port `rust/core/src/pngle.rs`) | MIT | submodule (`patches/freeink-cover-decoder.patch` is not applied by the firmware build) and `rust/core/src/pngle.rs` (host tests) | not linked | no |
| en-US hyphenation patterns (in FreeInkBook) | notice-preserving permission | submodule | not linked | no |
| Lexend Deca | OFL-1.1 | TTF and OFL text (`assets/frosted-home/source/assets/`, `assets/fonts/OFL.txt`); derived atlas data (`assets/fonts/`, `rust/reader-atlas/src/data.rs`) | NewtReadingAtlas bitmaps | no |
| Noto Sans, Noto Sans Symbols, Symbols 2, Math | OFL-1.1 | OFL texts and derived atlas data (`assets/fonts/`, `rust/reader-atlas/src/data.rs`); TTFs stay outside the repo | NewtReadingAtlas bitmaps | no |
| Inter | OFL-1.1 | TTFs and OFL text (`assets/frosted-home/source/assets/`); derived atlas data (`rust/ui-canvas/src/data.rs`, `rust/system-ui/src/data.rs`) | NewtHomeAtlas bitmaps | no |
| [CrossInk](https://github.com/uxjulia/CrossInk/blob/b25beb13761d5851f98e7eada09aa5e7d430df48/LICENSE) `b25beb1` generated `lexenddeca_14_regular.h`, Lexend-only copy (see Typography) | Lexend Deca glyphs, OFL-1.1; generated in CrossInk's MIT repository (Copyright (c) 2025 Dave Allie) | `assets/frosted-home/source/assets/` (frozen Home handoff; read only by a host test) | its 341 original regular glyphs, in NewtReadingAtlas | no |
| Arduino-ESP32 core 3.3.7 (pioarduino) | LGPL-2.1-or-later; Espressif's files Apache-2.0 | no (locked toolchain download) | yes | no |
| ESP-IDF 5.5 prebuilt libraries, with FreeRTOS, lwIP, FatFs, TLSF, Mbed TLS, wpa_supplicant and Espressif's Wi-Fi, PHY, Bluetooth and coexistence libraries | Apache-2.0; MIT, BSD, ChaN's license, BSD-3-Clause, Apache-2.0 (Mbed TLS also offers GPL-2.0-or-later), BSD | no (locked toolchain download) | yes | no |
| Newlib (libc, libm) | various free licenses | no (toolchain) | yes (`libc.a`) | no |
| GCC runtime (libgcc, libstdc++) | GPL-3.0 with the GCC Runtime Library Exception | no (toolchain) | yes | no |
| Rust 1.93.1 `core`, `alloc` and `compiler_builtins` 0.1.160 | MIT OR Apache-2.0 (`compiler_builtins`: MIT AND Apache-2.0 WITH LLVM-exception) | no (locked toolchain, `rust/toolchain.json`) | yes (`core`, `alloc` in `libnewt_app_image.a`) | no |
| Mozilla CA list, from certifi 2026.7.22 | MPL-2.0 | no (`tools/crt_bundle.py` pins its SHA-256) | yes | no |
| esptool-js 0.7.1 | Apache-2.0 | no (`installer/pnpm-lock.yaml` pins it) | no | yes |
| Flasher stubs inside esptool-js (from esp-flasher-stub v1.2.2) | Apache-2.0 or MIT | no | no | yes |
| pako 2.2.0, with zlib's notice | MIT, zlib | no | no | yes |
| atob-lite 2.0.0 | MIT | no | no | yes |

`NOTICES.txt` and `installer/NOTICES.txt` carry each shipped component's
full license and notice text.

## Linked archives and objects in `newt.bin`

Derived from the linker map of the clean Rust firmware build at commit
`cd032e179043f53ed9f0c636a1e31fbc89c518e5` (`build_id` `cd032e179043`,
`v0.1.0-alpha.21`, `python3 tools/firmware-build.py --clean --health-report`,
`firmware.bin` SHA-256
`5c93ecd3467ed3fed559ca91d16c1aebd1586164aba9cd427923d543d078e719`): every
input section with a nonzero size and load address in an allocated output
section (not `.debug*`, `.comment` or `.riscv.attributes`), grouped by archive
or object directory. String-merge sections were checked separately and add no
archive. "§" is the `NOTICES.txt` section that covers it.

| Archive | Component | License | Covered |
| --- | --- | --- | --- |
| `libnewt_app_image.a` | Newt Rust crates (`rust/`), Rust 1.93.1 `core` and `alloc`, Rust translations of FreeInkBook/FreeInkFont/FreeInkUI/miniz/TJpgDec/Arabic shaping, embedded font atlases (`NewtReadingAtlas`, `NewtHomeAtlas`), brand art (`rust/stage-home/assets/frosted-home/stage-backdrop.bin`, `kSleepBrand`, `BASE_MAP`/`BASE_TILES`), `rust/firmware/sample.epub` and `rust/tiny-app/src/pocket-pause.app` | MIT; Rust `core`/`alloc` MIT OR Apache-2.0; OFL-1.1; ChaN's license; Unicode-3.0; CC0-1.0; brand art outside MIT | LICENSE; §1, §3, §5-§7, §13, §16 |
| `libFrameworkArduino.a` | Arduino-ESP32 core 3.3.7 | LGPL-2.1-or-later; Espressif's files Apache-2.0 | §8, §13, §14 |
| `libfreertos.a` | ESP-IDF FreeRTOS | MIT (kernel), Apache-2.0 (port) | §9, §13 |
| `liblwip.a` | ESP-IDF lwIP | BSD-3-Clause (lwIP), Apache-2.0 (port) | §9, §13 |
| `libfatfs.a` | ESP-IDF FatFs | ChaN's license, Apache-2.0 (port) | §9, §13 |
| `libheap.a` | ESP-IDF heap, with TLSF | Apache-2.0, BSD-3-Clause (`tlsf.c`) | §9, §13 |
| `libmbedcrypto.a`, `libmbedtls.a`, `libmbedtls_2.a`, `libmbedx509.a` | Mbed TLS, with Espressif's port | Apache-2.0 (Mbed TLS also offers GPL-2.0-or-later) | §9, §13 |
| `libwpa_supplicant.a` | ESP-IDF wpa_supplicant | BSD (hostap), Apache-2.0 (Espressif files) | §9, §13 |
| `libhttp_parser.a` | ESP-IDF http_parser | MIT (Joyent and Node contributors) | §9 |
| `libnet80211.a`, `libpp.a`, `libcore.a`, `libmesh.a`, `libespnow.a` | esp32-wifi-lib | Apache-2.0 | §9, §13 |
| `libphy.a` | esp-phy-lib | Apache-2.0 | §9, §13 |
| `libcoexist.a` | esp-coex-lib | Apache-2.0 | §9, §13 |
| `libc.a` | Newlib | various free licenses | §10 |
| `libgcc.a`, `libstdc++.a` | GCC runtime | GPL-3.0 with the Runtime Library Exception | §11 |
| `libapp_update.a`, `libbootloader_support.a`, `libbt.a` (only `bt.c`; no Bluetooth blob), `libcxx.a`, `libefuse.a`, `libesp-tls.a`, `libesp_adc.a`, `libesp_app_format.a`, `libesp_coex.a`, `libesp_common.a`, `libesp_driver_gpio.a`, `libesp_driver_i2c.a`, `libesp_driver_uart.a`, `libesp_driver_usb_serial_jtag.a`, `libesp_event.a`, `libesp_http_client.a`, `libesp_hw_support.a`, `libesp_mm.a`, `libesp_netif.a`, `libesp_partition.a`, `libesp_phy.a`, `libesp_pm.a`, `libesp_ringbuf.a`, `libesp_rom.a`, `libesp_security.a`, `libesp_system.a`, `libesp_timer.a`, `libesp_vfs_console.a`, `libesp_wifi.a`, `libespcoredump.a`, `libespressif__esp_diagnostics.a`, `libhal.a`, `liblog.a`, `libnewlib.a`, `libnvs_flash.a`, `libnvs_sec_provider.a`, `libpthread.a`, `libriscv.a`, `libsoc.a`, `libspi_flash.a`, `libtcp_transport.a`, `libvfs.a` | ESP-IDF components (`esp_diagnostics` is Espressif's managed component); any lwIP-derived glue in `esp_netif` is under lwIP's terms | Apache-2.0 | §9, §13 |

| Objects | Component | License | Covered |
| --- | --- | --- | --- |
| `rust/firmware/native/`, `rust/firmware/nvs/`, `rust/firmware/serial_console/`, `rust/salamander/`, `rust/target-expat/` | Newt native C/C++ wiring, with the embedded Mozilla CA list (`update_https.cpp.o`) | MIT; MPL-2.0 | LICENSE; §12, §15 |
| FreeInkDisplay, BatteryMonitor, InputManager, PowerManager, XteinkDetect (and header-only BoardConfig) | FreeInk SDK | MIT | §1 |
| FreeInkBook `vendor/` | Expat (`expat_xmlparse.c.o`, `expat_xmlrole.c.o`, `expat_xmltok.c.o`), libunibreak (`unibreak_impl.c.o`, with Unicode data); FreeInkBook's C++ objects, miniz, TJpgDec and pngle add 0 bytes | MIT, zlib-style and Unicode-3.0 | §2, §4, §6 |
| FS, SD, SPI, Wire libraries | Arduino-ESP32 3.3.7 | LGPL-2.1-or-later (`FS.cpp`, `SPI.cpp`, `Wire.cpp`), Apache-2.0 (`vfs_api.cpp`, `SD.cpp`, `sd_diskio.cpp`, `sd_diskio_crc.c`) | §8, §13, §14 |

FreeInkFont (including stb_truetype and FreeType) and FreeInkUI C++ objects are
loaded by PlatformIO's library build but add 0 allocated bytes (only empty
`.eh_frame` entries). Neither stb_truetype nor FreeType is linked or ported to
Rust in `newt.bin`, so `NOTICES.txt` omits stb_truetype; `NOTICES.txt` still
lists Espressif's Bluetooth library in §9 conservatively (only
`libbt.a(bt.c.obj)` is linked, with no Bluetooth blob). `libm.a` is passed to
the linker and adds 0 allocated bytes.

## Typography

`assets/fonts/NewtReadingAtlas.json` is a modified 341-codepoint font subset
under SIL OFL 1.1, with its full copyright and license in
`assets/fonts/OFL.txt`; later blocks extend all four faces to 2,041
codepoints. Newt's own tools render it from pinned Lexend Deca and Noto TTFs
with FreeType; no downloaded script is run or copied, and no mixed-font
fallback face or CrossInk code is imported. The original 95 printable-ASCII
glyphs, advances and 899 pairs remain unchanged.
[Asset provenance and source pins](../assets/fonts/README.md) records the
exact ranges, hashes and reproduction commands. Independent host proofs
compare every added raster, all 341 advances and 116,281 pairs with that
source. Static data avoids a runtime rasterizer.

**Closed check: CrossInk's generated typography.** The repository now holds
only a Lexend Deca copy of CrossInk's generated header, as the owner chose.

- The original header, `lexenddeca_14_regular.h` at CrossInk commit
  `b25beb13761d5851f98e7eada09aa5e7d430df48` (213,607 bytes, SHA-256
  `14e0fcd4fda8857e54184088c026c21da276a97c3c90aa56812169b86c8de107`), was
  generated from four fonts: Lexend Deca (OFL), ChareInk7, Noto Sans Symbols
  (OFL) and Noto Sans CJK SC (OFL). ChareInk7's license is not established:
  CrossInk's `ChareInk7` font folder has no license file.
- The tracked copy, `assets/frosted-home/source/assets/lexenddeca_14_regular.h`
  (155,387 bytes, SHA-256
  `c4f576ff651e1866974afae902edd6183be95225ca06054eadba8f5656c94770`), drops
  the 116 glyphs that `fontconvert.py` could take from a fallback font: every
  `--font-include-intervals` range in the header's own command (λ, 34
  Cyrillic letters, ℓ, 7 music signs and 73 CJK ideographs). They were four
  whole compression groups, so every kept byte is unchanged; only array
  lengths, offsets, interval bases and counts were recomputed, and the
  fallback fonts' kerning entries were removed.
- The rest is Lexend Deca: `fontconvert.py` tries Lexend Deca first for every
  codepoint and allows a fallback font only inside its include ranges, and
  every kept codepoint is in the pinned Lexend Deca TTF's character map or is
  one of `fontconvert.py`'s generated blanks (control, zero-width and bidi
  marks).
- For all 658 kept codepoints, the copy and the original have the same glyph
  records, pixels, ligatures and all 432,964 kerning pairs (originally checked
  by the retired C++ test `tests/reader_font_unicode_proof_test.py`).
- No firmware input reads the header or `assets/frosted-home/source/`. The C++
  generators that previously verified those files (`tools/make-reader-font.py`
  and `tools/make-frosted-home.py`) were retired when the C++ firmware was
  removed in NEWT-90; the Rust atlases (`rust/reader-atlas/src/data.rs`,
  `rust/ui-canvas/src/data.rs` and `rust/system-ui/src/data.rs`) are pinned
  literals converted from those verified outputs (`tools/pack-reader-atlas.py --check`
  verifies the packed reading atlas against `rust/reader-atlas/source/reader_font_data.inc`).
- NewtReadingAtlas's original 341 regular glyphs (printable ASCII, Latin-1,
  Latin Extended-A and common punctuation) were imported from the original
  header and are Lexend Deca. `NOTICES.txt` carries CrossInk's MIT notice for
  them.
- CrossInk's `fontconvert.py` came from epdiy (LGPL-3.0 on its main branch).
  Newt neither runs nor copies it; `assets/fonts/sources.json` pins it by
  SHA-256 as a reference only.

NewtHomeAtlas is rendered from the unmodified Inter fonts
([Home resources](../assets/frosted-home/PROVENANCE.md)). Derived font names
avoid every Reserved Font Name.

## Other inputs and references

| Input | Role | License |
| --- | --- | --- |
| [FreeInk PlatformIO sample](https://github.com/Free-Ink/freeink-sdk/blob/cd6f5b831c8fad899723b6d406586f00c575f021/platformio.sample.ini) | Reference board, compiler and framework configuration, pared to X3 dependencies | MIT |
| [XteinkDetect source](https://github.com/Free-Ink/freeink-sdk/blob/cd6f5b831c8fad899723b6d406586f00c575f021/libs/hardware/XteinkDetect/include/XteinkDetect.h) | Authoritative live panel probe; the OEM NVS read is diagnostic, not controller selection | MIT |
| Earlier Newt/Wayfinder research | Read-only R&D reference; no code copied | Its own terms; no relicensing implied |
| [CrossInk source](https://github.com/uxjulia/CrossInk/blob/b25beb13761d5851f98e7eada09aa5e7d430df48/src/CrossPointSettings.h) `b25beb1` | Reader, stats and recovery research; besides the typography above, no code or assets imported | MIT |
| [Standard Ebooks](https://standardebooks.org/about/standard-ebooks-and-the-public-domain) editions of [Pride and Prejudice](https://standardebooks.org/ebooks/jane-austen/pride-and-prejudice), [Frankenstein](https://standardebooks.org/ebooks/mary-shelley/frankenstein), [Moby Dick](https://standardebooks.org/ebooks/herman-melville/moby-dick) and [Alice's Adventures in Wonderland](https://standardebooks.org/ebooks/lewis-carroll/alices-adventures-in-wonderland/john-tenniel) | Books on the preview card for the README screens (`docs/screen-*.png`); the books are not in the repository or `newt.bin` | Editions CC0; texts and cover art U.S. public domain |
| Product photos (`docs/photo-*.jpg`) | The README's photos of Newt on an X3, taken by the project's owner; the books, covers and page text shown belong to their publishers | Not covered by the MIT license; all rights reserved |

The FreeInk SDK keeps its OpenX4/CidVonHighwind display-waveform attribution.
Its nested Lucide icon submodule is not initialized or used. Newt's first
bitmap font, its mascot and the reading sample are original to this project.

Design references stay separate from firmware inputs.
[Deckle](https://getdeckle.web.app/) is an independent X3/FreeInk product with
illustrated sleep screens and preview-driven creation; its source was not open
as of this research, and no code or assets were reused. CrossPoint and
CrossInk serve as reader UX, stats and recovery references, not Newt's
firmware base. The recovery reference was CrossPoint develop
`099e89bcb56cbb4e47cc57c1234a6db270ab3f4a` (2026-09-30), [pinned README
warning](https://github.com/crosspoint-reader/crosspoint-reader/blob/099e89bcb56cbb4e47cc57c1234a6db270ab3f4a/README.md#usb-locked-devices-xteink-unlocker).

## Toolchain

The project-local toolchain is pinned. `docs/toolchain-lock.json` and
`docs/toolchain-linux-lock.json` record every archive's and wheel's publisher
URL, size and SHA-256, `docs/toolchain-python.lock` and
`docs/toolchain-python-linux.lock` fix the Python helpers, and
`rust/toolchain.json` pins Rust 1.93.1. Firmware compiles against the exact
FreeInk pin. Build tools are not shipped; their licenses stay in their
installed distributions. Espressif GCC is GPL with runtime library exceptions;
Rust is MIT OR Apache-2.0; PlatformIO Core and pioarduino are Apache-2.0;
SCons is MIT; esptool is GPL-2.0-or-later. Package labels can differ from
component details: keep the actual archive notices rather than treating one
manifest label as a universal license.

## Offline-reader inputs

The same SDK pin supplies
[FreeInkBook](https://github.com/Free-Ink/freeink-sdk/tree/cd6f5b831c8fad899723b6d406586f00c575f021/libs/book/FreeInkBook)
and its font interfaces. Preserve all vendored notices under the SDK's
`third_party/`, even when unused code is removed at link time.

The same pin supplies
[FreeInkUI](https://github.com/Free-Ink/freeink-sdk/tree/cd6f5b831c8fad899723b6d406586f00c575f021/libs/ui/FreeInkUI)
(MIT): Newt uses a Rust translation of its keyboard layout and navigator
(`rust/device-ui/src/update/keyboard.rs`) for update mode's Wi-Fi password
page ([UPDATES.md](UPDATES.md#wi-fi-setup)) and draws the keys itself.

`tests/fixtures/the-small-lantern.epub` and the identical embedded built-in
sample `rust/firmware/sample.epub` (embedded by `rust/firmware/sample_epub.rs`)
are original Newt text generated by `tools/make-reader-fixture.py`, dedicated
to CC0-1.0 in `tests/fixtures/README.md`. Size 6,931 B; SHA-256
`5e4c9375286ce1cbfcd7e8a0706dce29868420c2203bcbc806e63f77f25e22a5`. They
contain no third-party book content or credentials.

Airspace's aircraft silhouettes were original Newt vector drawings in code
(retired with the C++ firmware in NEWT-90), with no image source or
third-party art; see `assets/airspace/PROVENANCE.md`.

Pocket Pause's source (`apps/pocket-pause.json`), note text, package encoder
and decoder, embedded binary (`rust/tiny-app/src/pocket-pause.app`, built by
`tools/make-builtin-app.py`) and delivery simulator are original Newt work.
Its Rust renderer (`rust/tiny-app/`) reuses `rust/ui-canvas/`.

## Package integrity providers

The firmware loader uses the SDK's Mbed TLS `mbedtls_sha256` API (Apache-2.0
OR GPL-2.0-or-later; Newt uses it under Apache-2.0) via
`rust/salamander/board/native/image.cpp` and
`rust/firmware/reader_hash_layout.cpp`. No library source was copied or
changed. Host tests use CommonCrypto on macOS and the pinned OpenSSL on Linux
(`rust/tests/include/newt/host_sha256.h`); neither is redistributed. Integrity
checks are not a signing or authentication claim.

## Frosted reader reference

The first reader geometry slice followed an earlier, unpublished design
reference. No reference
code, text, images, TTFs or generated font headers were imported. The owner
later chose its slim progress bar and the reading menu.

## Derived C3 linker script

`tools/wifi-sections.py --rust-app` writes `.build/rust-app/ld/sections.ld`
from the pinned ESP-IDF 5.5.2 `esp32c3/ld/sections.ld` in
framework-arduinoespressif32-libs 5.5.0+sha.87912cd291 (Espressif,
Apache-2.0). It refuses any other script or package by SHA-256 and makes three
changes: a Wi-Fi data range, libphy/libesp_ringbuf and selected IRAM functions
in flash, and deferred Wi-Fi constructors ([UPDATES.md](UPDATES.md#wi-fi-memory)). The
derived file is generated, never checked in; it keeps Espressif's SPDX notice
and a header naming Newt's change.
