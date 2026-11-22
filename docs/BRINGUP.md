# Cracked X3 bring-up: procedure and acceptance gates

This procedure does not authorize hardware operations. The separately approved October 1 baseline flash and owner acceptance are recorded in [the historical baseline receipt](007-first-hardware-boot.md). The guarded reader is now installed and has passed the owner's first reader/menu/settings/app/sleep checks; [its executed receipt](010-guarded-reader-hardware.md) separates actual evidence from remaining cold-restart/durability checks. The safe-inset candidate is built but not installed. Replacement X3 stays the daily device. Test only the confirmed cracked unit. This firmware has no implemented OTA/SD update recovery; [OTA is a separate product requirement](005-ota-firmware-updates.md).

## X3 display startup invariant

The display startup rationale lives here, rather than being shortened to fit
`firmware/main.cpp`'s source ceiling. Preserve the following original reasoning
word for word when changing the startup seam:

This is intentionally X3-only. No uncertain I2C fingerprint may silently
choose X4 geometry. The live display VER probe owns controller selection.
Canonical display bring-up order, on cold boot and deep-sleep reset alike:
select X3 geometry/profile, restore the shared SD bus rail, probe the panel,
then begin. setDisplayX3() MUST precede the probe: this pinned SDK's probe
promotes only ACTIVE.displayController, while setDisplayX3 resets the base
profile unless ACTIVE.board was explicitly changed to the UC8279 sibling.

Clock and battery I2C traffic do not select the board geometry. The startup
code points here; keep the profile exception and probe authority in this
canonical contract when adding another boot-time sensor.

## Keep the daily device and development card separate

The owner's plan is the **original SD in the replacement X3 for CrossInk daily reading**, and the **new SD in the cracked X3 for Newt development**. For the first baseline flash/boot, the new development card stayed in the Mac adapter; the baseline uses embedded content and does not require SD access. Neither card was written or erased during that bring-up.

For the daily-device migration, leave the current book to Home and cleanly power down before removing the original card. Preserve a whole-card private backup, including hidden `/.crosspoint`, and keep book paths unchanged: the [CrossInk cache documentation](https://crossink.dev/data-cache.html) identifies it as the reading progress/settings/bookmarks/statistics store. Firmware lives in the device's flash; moving the card does not by itself install CrossInk on the replacement. Do not migrate hardware/display NVS between units. New X3 hardware batches can differ; verify the compatible firmware release for that replacement.

CrossInk's official [Inky Flash Tools](https://inky.crossink.dev/#flash-tools) supports a compatible stock-X3 SD-root `update.bin` installation route. The separate authorized migration task verifies that image/version and its exact installation steps; this document neither supplies a guessed file nor initiates installation. USB is not the only supported CrossInk route. That stock update package is distinct from the Newt app-only binary and does not establish Newt recovery.

For Newt, keep the replacement/original card disconnected. Identify the cracked unit, preserve its current flash and the new card privately, and establish recovery before proposing any app write. Having a sacrificial unit does not substitute for a restorable backup.

## Morning checkpoint index

This historical index records the initial bring-up candidates. Later guarded-reader and safe-inset archives are documented separately. These archives, not current `.pio` output, identify candidates. Parentheses identify the compiled source commit where separately recorded; the other commit is the preserved evidence checkpoint. The initial session started with the baseline; any future write needs its separately approved exact image.

| Candidate / branch | Preserved commit (compiled source) | Archived binary / bytes | Evidence |
| --- | --- | --- | --- |
| Baseline / `spike/freeink-x3` | `44c454d` | `.build/baseline-44c454d/firmware.bin` / 417,072 | [Baseline receipt](../artifacts/verification.json) |
| Reader / `spike/offline-reader` | `4dd4252` (`2bb9968`) | `.build/offline-reader-2bb9968/firmware.bin` / 628,848 | [Reader receipt](../artifacts/reader-verification.json) |
| Agent loop / `spike/agent-app-loop` | `66015de` (`ba7b125`) | `.build/app-loop-ba7b125/firmware.bin` / 628,864 | [Agent-loop receipt](../artifacts/app-loop-verification.json) |
| Firmware loader / `spike/firmware-app-loader` | `bf7d0b3` (`b2c7abf`) | `.build/firmware-loader-b2c7abf/firmware.bin` / 636,864 | [Loader receipt](../artifacts/loader-verification.json) |
| Frosted reader / `spike/frosted-reader` | `c1b4cb3` (compiled source) | `.build/frosted-reader-c1b4cb3/firmware.bin` / 638,608 | [Frosted reader receipt](../artifacts/frosted-reader-verification.json) |
| Menu/settings / `spike/reader-menu-settings` | `f708ba5` (compiled source) | `.build/reader-menu-f708ba5/firmware.bin` / 644,736 | [Menu/settings receipt](../artifacts/reader-menu-verification.json) |
| Reading typography / `spike/lexend-reader` | `86d13c0` (compiled source) | `.build/typography-86d13c0/firmware.bin` / 657,456 | [Typography receipt](../artifacts/typography-verification.json) |

| Gate | Verified here | Still unverified |
| --- | --- | --- |
| Firmware | All seven C3 sources compile/link; exact archived images and metadata; baseline boots on identified old X3 | Physical display/button/sleep acceptance; later candidates on X3 |
| UI/buttons/sleep | Exact host pixels, action transitions, frame conversion, RTC validation | Panel geometry/refresh, physical input, actual sleep/wake/drain |
| Offline reader | Real fixture EPUB parsing/pagination/resume; host heap/fault tests | Real-device heap, user-book import, native persistence, daily-reader quality |
| App loop | Shared package integrity/decoder/routing; host generation/queue interruption tests | Native NVS durability; physical install or network transport |
| Recovery | This old unit's ROM read access/security state, duplicate full-flash backups and baseline full readback | Restore writes and reliable manual ROM recovery sequence |

From this project, these commands access source/artifacts only:

```sh
python3 tools/check.py
python3 tools/preview.py --screen home --actions next,next,next,activate,activate,sleep,wake
python3 tools/firmware-build.py
shasum -a 256 .build/baseline-44c454d/firmware.bin .build/offline-reader-2bb9968/firmware.bin .build/app-loop-ba7b125/firmware.bin .build/firmware-loader-b2c7abf/firmware.bin .build/frosted-reader-c1b4cb3/firmware.bin .build/reader-menu-f708ba5/firmware.bin .build/typography-86d13c0/firmware.bin
.toolchain/platformio/penv/bin/python -m esptool image-info .build/baseline-44c454d/firmware.bin
```

The build command compiles the **current checked-out branch**, not the preserved baseline. No command above connects to hardware. Before any hardware operation: The owner identifies the cracked unit and authorizes that session; replacement stays disconnected; establish the exact port and unrestricted ROM recovery; produce matching duplicate full-flash and private SD backups; inspect actual partition/boot selection; stop on unknown/locked recovery, mismatched backups or incompatibility. Only then prepare one exact app-only write and restore proposal for the owner's review. Detailed steps and acceptance observations follow below. Frosted is selected; its untested native grayscale/fonts are not a dependency for the preserved baseline proof.

## Stop before a first custom flash unless recovery is established

Some Xteink units are USB-locked. [CrossPoint's official warning](https://github.com/crosspoint-reader/crosspoint-reader#usb-locked-devices-xteink-unlocker) says custom firmware can re-lock USB and leave a unit stuck without OTA; its unlocker supports CrossPoint/CrossInk only. **If this specific unit is locked, requires an unlocker, or its recovery path is uncertain, do not install this spike.** Do not bypass locks, alter eFuses/security, or infer recoverability from another X3. App-level FreeInk [RecoveryBoot](https://github.com/Free-Ink/freeink-sdk/blob/cd6f5b831c8fad899723b6d406586f00c575f021/libs/hardware/RecoveryBoot/include/RecoveryBoot.h) cannot rescue firmware that fails before the recovery code executes.

The web flasher is not the diagnostic starting point here, because trying it may write flash. First establish purchase/source/firmware history with the owner, device identity, and the official ROM serial read/backup path. Confirm data cable and adequate charge. Do not apply an unverified internet button combination to force boot modes; confirm the procedure for this exact unit. Serial absence alone does not prove a factory lock.

## Preserve the unit before changing its state

1. With scoped permission for the hardware session, inspect serial enumeration and identify the **cracked X3's exact port**. Never use the first matching port or an automatic upload. Keep the replacement disconnected.
2. Use the approved project's pinned esptool executable for read-only `chip-id`, `flash-id`, and `get-security-info`; inspect the output privately and record chip/model, flash capacity, security/recovery state. Stop if ROM readback is unavailable or restricted. Do not use `--force` or security/eFuse commands. Esptool connects/resets hardware even for reads; these actions wait for that session's permission.
3. Read the full detected flash twice to separate local private backup files; for a confirmed 16 MiB unit the read range is `0x0` / `0x1000000`. Require identical SHA-256, exact size, and a second copy outside the working build directory. Backups include firmware, NVS, calibration and possibly credentials: keep private/ignored, never display raw contents or commit them.
4. Read/decode the actual partition table at `0x8000` from the **backup**, not sample PlatformIO output, and inspect OTA selection metadata from the recorded partition. Record current app slot, selected boot offset and app capacity. Stock X3 and reader firmware have different slot layouts; `0x10000`, `0x650000`, and `0x780000` are examples, not authorization to write them. [Deckle's install guide](https://getdeckle.web.app/install) also distinguishes stock X3 layouts and full-backup restoration.
5. Copy the entire SD card privately through an ordinary backup workflow after clean shutdown. **Do not erase `/.crosspoint`**: it contains progress/stats/settings/bookmarks and may contain credentials, not only regenerable cache. Future Newt data lives under its own namespace. A stats importer reads a copied backup, never the live reader tree.
6. Preserve the current firmware version/source/hash and a known recovery image. Confirm its restore procedure and that ROM connection works after a normal power cycle. Do not assume a full flash backup restores ROM/eFuse state; it does not.

Use [Espressif's esptool command reference](https://docs.espressif.com/projects/esptool/en/latest/esp32c3/esptool/basic-commands.html) and the acquired pinned version's `--help`. Command templates deliberately omit a device port and flash write offset until actual inspection provides them. A private bring-up receipt should contain hashes and selected addresses, never credential bytes.

## Compile and inspect before proposing a write

**The completed first bring-up used `spike/freeink-x3` at `44c454d`.** Its exact image is archived at `.build/baseline-44c454d/firmware.bin`. Later branches build different images; the guarded reader is currently installed. Never substitute a newer image for an approved candidate. [Decision 002](002-bounded-offline-reader.md) records the initial reader gate; [the guarded-reader receipt](010-guarded-reader-hardware.md) records its subsequent execution.

On the selected baseline source, run `python3 tools/check.py`, then `python3 tools/firmware-build.py` (or `--clean`). Both now pass on this Mac. The compiled app is 417,072 B; ELF/map, hashes, resource summary and image metadata are recorded in `artifacts/verification.json`. Recheck the specific binary proposed for the hardware session. Firmware buffer design alone does not prove runtime heap headroom.

Compare the built binary's app size and ESP32-C3/image metadata against the observed device/app partition. Check Arduino/IDF bootloader compatibility. Reuse existing bootloader/partition/NVS only if compatible; do not silently write a new table to solve a mismatch. Default PlatformIO's generated table is **not** a device table. The verified app header is DIO/80MHz/16MB; `board_upload.flash_size` fixes the reference sample's ignored `board_build.flash_size` setting. Actual flash capacity must be observed before flash. The precompiled IDF libraries still carry their upstream 4MB sdkconfig and baked `arduino-lib-builder` app-description/version/date; those are SDK defaults, not Newt's source identity or this build time. Use the Newt source commit and artifact hash receipt for identity, and verify the existing bootloader/profile before a write.

Prepare one exact **app-only observed-active-slot** write command for review, its erased-sector range, and an exact backup restoration proposal. This conservative first proof does not manipulate OTA selection metadata or install to an inactive slot without a separately implemented/tested boot-selection path. No `pio upload`, merged image, erase-all, bootloader write, partition rewrite or NVS overwrite. Any restore that writes low flash regions requires its own concrete confirmation.

## Hardware acceptance after a separately approved app write

| Step | Required observation |
| --- | --- |
| Cold boot | Serial banner, live panel VER/verdict and buffer/heap diagnostics; visible Newt Home within a bounded wait |
| Geometry | Portrait content upright, correct polarity, clean margins; photograph the physical display alongside exact host PNG |
| Buttons | Every physical input maps to expected action; selection/OK/Back/page directions; taps during refresh are queued |
| Reader proof | All three original story pages offline; first/last bounds hold; return Home and reopen position |
| Telemetry | Actual battery percent/voltage/charging only when SDK reports known; failures display unknown, never a made-up value |
| Sleep/wake | Power sleeps after release; sleep screen persists; power wakes by reset and restores selected screen/page; repeat at least 10 cycles, including a held wake button |
| Resource checks | Record runtime free/largest heap and queue-overflow logs. Retained RTC state survives deep sleep, not battery loss; no durable reading-progress claim |
| Recovery | Stop immediately on blank/incorrect display, reset loops, stuck button or sleep drain; use the already established ROM recovery route with separate exact restore confirmation |

Charging USB can affect power readings. Do not claim low-power correctness from a pretty sleep screen: measure actual unplugged standby consumption with appropriate equipment later. No network payload until this gate passes. No daily-firmware claim until real EPUB/font/progress/stats/offline-storage gates pass.

## Later reader experiment, after the baseline passes

Select the separate `spike/offline-reader` commit and verify `artifacts/reader-verification.json` against its binary. Review the app-only write again, including its Newt-only `newt_reader` NVS namespace writes; partition compatibility and private recovery backups still apply. No flash operation has been authorized by this document.

Open Home's third card, The Small Lantern. Confirm 7 pages per chapter, forward/back chapter crossing and end clamps; compare physical pages to `artifacts/epub*.png`. Record free/largest heap on entry, during repeated turns and after exit; verify workspace is released and reopen is stable. Test menu and deep-sleep anchor resume. For power-loss persistence, verify a checkpoint/exit save first, then use a separately agreed safe power-cycle procedure; an interrupted session may lose up to the 30-second checkpoint interval. Confirm unavailable storage or reader heap produces a usable Home and clear unavailable sample rather than a reboot loop. Active time excludes menus/sleep and caps idle time at five minutes; persistent counters have host evidence only until these device tests pass.

## Later loaded-app experiment

After baseline and reader acceptance, separately review the `spike/firmware-app-loader` app-only candidate using `artifacts/loader-verification.json`. Home's fourth card opens the verified app. Check local note, actual EPUB/device actions, Back, app selection/note restoration over repeated sleep/wake and safe Home on identity mismatch. Record free/largest heap; precompiled crypto/NVS provider runtime costs are unmeasured. Boot reads `newt_apps` without writing it; only confirmed empty storage falls back to the builtin artifact. No app install transport is exposed, no SD/FAT mutation is implemented, and host queue/sync cannot install on hardware. Native generation durability remains a future explicitly scoped test.

## Later menu/settings experiment

After the preserved baseline and reader pass, separately review `spike/reader-menu-settings` and its exact receipt. This candidate also writes only its own `newt_prefs` slots when Save is explicitly selected. Open the menu while reading: verify orientation and physical inputs, chapter jump/return, and scoped statistics without menu-time credit. Change orientation/bar, cancel once and verify no change; save once and verify semantic anchor/reflow, portrait Home and page resume after sleep. After a confirmed save, use an agreed safe power-cycle procedure to verify native persistence. Record actual free/largest heap and NVS diagnostics. Unknown/corrupt/future/ambiguous preference state must keep the draft or safe session defaults and block overwrite; reboot is currently required to retry preference loading. Do not manufacture hardware fault injection or erase NVS as a test.

## Later typography experiment

After baseline acceptance, separately review the exact `spike/lexend-reader` candidate and receipt. Use the cracked X3/new development SD, with the replacement/original daily card disconnected. No SD font or book access is implemented. Compare the real first/next sample pages against the archived typography PNGs at the same anchor, rather than older page numbers: the sample now has four pages per chapter. Test both reader orientations, optical stroke legibility, correct polarity, margins/footer separation, scaled headings and last-page bounds. Verify Return/Home/settings and repeated sleep/wake still work; record actual free/largest heap. Confirm saved character anchors reopen after reflow. These checks establish the monochrome font proof only; native AA/gray capability, waveform transitions and power remain separate hardware measurements.

## Future design refresh acceptance

These checks apply only after visual review and a later implementation of [the recorded preferences](design-requirements.md). The preserved firmware still uses its existing paint policy; recording this matrix changes no refresh behavior or hardware authorization.

| Check | Required observation |
| --- | --- |
| Five-page starting cadence | With explicitly documented count/reset semantics, exercise pages around the threshold, backward turns, chapter crossings, failed/clamped turns, repaints and reopen/wake. Record requested and actual SDK refresh modes; confirm the reading cadence is adjustable. |
| Gray/Frosted transitions | Exercise reading → overlay → dismissal and overlay → sleep → wake. Verify required SDK cleanup/rebase or stronger refresh occurs when needed, even before the five-page threshold. |
| Prior-image independence | Compare the same target after dense text, mostly white content and a cover. Intentional softness/texture comes from rendering; readability must not depend on uncontrolled prior-image residue. |
| Ghosting tolerance hypothesis | Inspect decorative areas alongside reading text, progress, crisp controls and spatial button labels. Residue may be aesthetically tolerable only if it does not obscure these; no claim that Frosted solves ghosting. |
| Responsiveness and power | Measure computation, upload and waveform separately; exercise input/cancellation at full-refresh and mode-transition boundaries. Record visible flashing, delayed actions and battery effects before accepting the design/cadence. |
