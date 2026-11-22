# First Newt boot on the cracked X3

October 1, 2026, verified by 20:57 UTC. The owner separately approved the concrete baseline app-only write after the private recovery preflight. This records executed evidence, not permission for another firmware write or restoration.

## Exact candidate and preserved device state

The flashed candidate is baseline source `44c454df42d1ddaba566d18b3eaa1e88475ca45b`, archived as `.build/baseline-44c454d/firmware.bin`: 417,072 bytes, SHA-256 `f7f8c8191499f3ac85e250c751f51aaaf8464eb626653b46159ff5fe5f27f337`. No later reader, app-loader, menu or typography candidate was flashed. The current source checkout remains `spike/lexend-reader`; rebuilding it produces a different candidate.

The old cracked unit was identified as an ESP32-C3, 16 MiB flash, with unrestricted ROM reads and no secure boot or flash encryption. Two complete private flash backups matched in both length and SHA-256. Offline partition and OTA-record checks identified the active `app1` at `0x780000`, capacity `0x770000`. Live low-flash readback and device identity were reconfirmed before writing.

The approved write touched exactly 102 sectors, `[0x780000, 0x7e6000)`. Esptool verified the written payload hash and left the device in download mode. A complete 16 MiB readback then matched the expected original image with only the Newt payload and 720 bytes of erased padding substituted. Every byte outside those sectors matched the original backup, including bootloader, partition table, OTA selection, NVS, the other app slot and filesystems. An independent offline review passed the same comparison and reconfirmed valid partition MD5/OTA CRCs and unchanged active-slot selection.

Private backups, logs and exact receipts are outside the Git project in the restricted `task-14/x3-preflight-20261001-_u17gdhg` directory. Do not publish them: device backups may contain credentials. No SD writes, security changes, boot-selection changes or restoration were performed. The new development SD remained in the Mac adapter; the baseline needs only embedded UI/story data. The replacement and original daily card were not targets.

## Observed on real hardware

After verification, one normal reset used the existing esptool USB hard-reset helper, followed by a 35-second serial capture. No retry resets were performed.

| Observation | Actual result |
| --- | --- |
| App startup | `Newt X3 bring-up / hardware validation pending` banner |
| Panel detection | VER `00 00 66`, verdict 1, UC8279 selected |
| Panel timeout | BUSY timeout 0 |
| Frame allocation | Portrait 52,272 bytes plus FreeInk 52,272 bytes |
| Runtime heap | Free 172,232 bytes; largest block 114,676 bytes |
| First display power-on | SDK wait completed in 39 ms |
| First full refresh | SDK wait completed in 694 ms |
| Faults | No panic, watchdog, abort or reset loop in the bounded capture |

At that initial serial-only checkpoint, SDK refresh completion was **not physical visual acceptance**. The later owner report below supplies the first hands-on pass. A single boot does not establish standby drain, repeated sleep/wake durability or recovery writing.

### Owner's first hands-on pass

At 21:11 UTC on October 1, the owner reported that the supplied first-pass selection/buttons, three-page story, telemetry and sleep/wake checks all worked. Parent photo review confirmed an intact device-info layout with battery 100%, voltage 4,363 mV, charging Yes, X3/UC8279 detected and memory 171,980 bytes free. The photographed WAKE field was `cold / reset boot`, apparently before the separate sleep test; the photograph does not prove deep-sleep wake. The owner's report supplies that first-pass behavior evidence. Ten-cycle/held-wake testing and standby consumption measurement remain separate gates.

## Next hands-on checks

Leave the new SD out for the baseline check. Confirm the cute Newt Home appears with correct orientation/polarity and take a photograph. Exercise selection, OK, Back and page directions; then open the embedded story and device information. Record the actual battery/charging display without inferring battery health from USB-powered readings.

After the inputs are confirmed, test a deliberate power-button sleep and wake, checking the sleep screen and retained selection/page. A sleeping C3 can disappear from USB; disappearance alone is not a lock or failed flash. Repeat the sleep/wake matrix in [the bring-up procedure](BRINGUP.md#hardware-acceptance-after-a-separately-approved-app-write) before accepting the baseline. Stop on a blank/stuck panel, reset loop or unexpected input.

Do not flash a later candidate or restore automatically. Any next write needs its exact candidate/hash, sector scope and separate authorization. The read-backup path is proven; restoration writes and a reliable manual ROM recovery sequence remain unexercised. No daily-firmware, native font, app-install, SD-update or OTA claim follows from this baseline boot.
