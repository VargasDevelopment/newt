# Guarded reader installed on the cracked X3

October 1, 2026, verified by 22:11 UTC. The owner directly approved the exact guarded reader write in this execution conversation. This records the completed operation and its limits; it authorizes no additional firmware or recovery write.

## Exact image and preservation evidence

Compiled source `a4d881277e5715e76123dec096e869f6fb86c84e`, archived `.build/nvs-reader-a4d8812/firmware.bin`: **658,720 bytes**, SHA-256 `fd06a29aee2c2bbc63c31dc69553ca3e17bc7fff074f1f66aafc7c498274b582`. No source or image changes followed the passed clean build, host gate and independent guard/link review. FreeInk remains pinned at `cd6f5b831c8fad899723b6d406586f00c575f021`.

Fresh private chip identity/security, first 64 KiB and exact app-sector baseline checks passed. Esptool wrote app1 at `0x780000`, verified its payload hash and remained in download mode. The erase range was exactly 161 sectors, `[0x780000, 0x821000)`. The complete 16 MiB readback matched the baseline with only this payload and 736 bytes of erased padding substituted. Every byte outside that range matched, including bootloader, partition table, OTA selection, NVS, app0 and filesystems. Independent offline verification reconfirmed valid partition MD5 and OTA CRCs; sequence 6 still selects app1.

Full readback SHA-256: `aa8cb52d64823c41b604659fca57f638f4c45612ee2bea5e86816b6b86535c7e`. After first startup, another read-only first-64-KiB check confirmed shared NVS and other low flash remained byte-for-byte unchanged. The reader was then returned to normal boot. This establishes startup preservation for this healthy device state, not fault/power-loss durability of later Newt records.

Private flash images, original/baseline restoration slices, device identity and raw logs remain outside Git in the restricted preflight directory. They may contain credentials and must not be published. No SD/network operation, restoration, security change, OTA-selection change or replacement-device action occurred.

## Observed boot and limits

Both bounded captures (35 and 30 seconds) report:

| Observation | Actual result |
| --- | --- |
| Storage | `Newt shared NVS ready / preservation guard active` |
| Panel | VER `00 00 66`, verdict 1, UC8279, BUSY timeout 0 |
| Trusted builtin app | `pocket-pause / 0.1.0` loaded |
| Frame allocation | Shared logical 52,272 B plus FreeInk 52,272 B |
| Home startup heap | Free 169,664 B; largest block 114,676 B |
| Display power-on | SDK wait completed in 40 ms |
| Refresh completion | `8279_DRF` absent from both captures |
| Faults | No panic/watchdog/abort in the bounded captures |

The missing refresh log proves neither completion nor a hang. Independent source review confirmed that FreeInk's `waitRefreshComplete` can return without logging if no BUSY edge arrives within its grace period or the pulse completes before the interrupt is armed. Newt has no post-paint completion marker in this exact image. Physical screen/button response is the remaining evidence. No further reset or restoration was attempted to chase the log.

### Owner's first reader acceptance

The owner subsequently reported that the screen updated, read both chapters, exercised the menu/settings and calm included app, and passed the requested first-pass reader/button/exit-reopen/sleep-wake checks. The hardware photograph confirms a rendered Lexend reading page and footer on the cracked X3. This supplies physical acceptance beyond the ambiguous serial refresh logs. The calm app is a keeper candidate; this does not expand app scope.

The owner identified two visual fixes: prose needs a comfortable inset from the physical bezel/shadow, and chapter/page information needs separated footer groups matching the supplied CrossInk photograph. Reference Library IDs are `libfile_b23a713705208191bcf6431018368da8` (Newt) and `libfile_c832d38f67088191b3e4e29a451af192` (CrossInk). Those photos have been inspected; they are not exact pixel rulers.

Explicit cold-restart persistence was subsequently accepted in the separately agreed test below. No restart was performed while the owner used the device. MCU reader arena/heap peaks were not captured during this owner session. Repeated/held-wake durability, standby drain, interrupted writes and restoration writes also remain untested.

After the owner saved a changed setting, checkpointed a recognizable passage on exit to Home and said ready, one normal restart was performed. The 20-second capture reported active NVS preservation, no storage-read error and completed full refresh in 694 ms. Serial was closed before asking the owner to reopen the book. At 22:56:34 UTC, the owner confirmed both the recognizable passage and saved slim-bar setting survived this restart. This establishes one normal-restart test after Save/exit checkpoints, not arbitrary power-loss or interrupted-write durability. [Sanitized restart receipt](../artifacts/reader-persistence-restart-verification.json).

## Hands-on acceptance

Leave the development SD out. Check that Home has four choices, then press Next twice and OK to open **The Small Lantern**. Turn several pages forward and one backward; check readable text, correct orientation and quiet first/last-page bounds. A missing/blank/stuck screen should stop the physical session; do not restore or reflash automatically.

While reading, OK opens Return to Reading, Chapters, Reading Settings, Reading Statistics and Home. Check a chapter jump, Cancel/Back behavior and a saved orientation/slim-bar change. Settings write only on explicit Save. Progress checkpoints after changed active reading at 30 seconds and on exit/sleep/reflow; a visible `UNSAVED` warning means no durable-progress claim. Return Home and reopen the book to check the anchor. A later agreed reset can establish restart persistence; no deliberate interrupted-write test is part of this acceptance.

Briefly press and release Power to sleep, then wake and check the reading anchor with overlays closed. Sleeping USB disappearance is expected for this C3; wake the device before any further diagnostic request. Repeated/held-wake and standby consumption remain separate checks. Device information supplies actual battery/charging values without implying unplugged battery life.

[Sanitized executed receipt](../artifacts/guarded-reader-hardware-verification.json) · [Build/guard evidence](009-nvs-preserving-reader.md) · [Historical baseline hardware acceptance](007-first-hardware-boot.md).
