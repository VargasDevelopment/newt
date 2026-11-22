# Frosted Home and development USB bring-up

This candidate combines the approved Home B, 16px reading insets and bounded USB test bridge on the unchanged pinned FreeInk SDK. Component histories and the installed guarded reader are preserved. Build commands never access hardware. The exact compiled source, binary hashes, sizes and linked guard proof live in `artifacts/frosted-usb-verification.json`; use the development binary from its named archive, not an earlier Home or spacing image.

The first proposed development image was retired before any write after PR review found a manual-takeover race. Its corrected image is **1,211,936 bytes**, SHA-256 `4537ade50dca24bc900dc98ba1ba8147c00c28b3e1d59dfd21f98e96f1cce61b`, compiled from `8c37dfb`. The same 296-sector scope applies. The earlier exact image approval does not authorize this new hash.

## Development access to approve

`NEWT_DEV_USB=1` permits a local USB process to handshake, submit only existing Previous/Next/Activate/Back/Sleep button events, read status, and capture the complete existing monochrome frame. It grants no shell, arbitrary hardware/memory/file/flash/NVS/network commands. It is not authenticated. The binary remains development-capable across resets until replaced; session expiry does not remove this capability. Sessions end on observed disconnect, replacement handshake, physical takeover, stalled output or 60s inactivity. Unknown press completion must never be retried automatically. Ordinary builds omit the protocol. Gray Home capture is rejected; the remaining selector plane is not a valid full image. Render-return proves a blocking SDK call returned, not optical quality.

## Exact future write scope

The development candidate occupies 296 app sectors **[0x780000,0x8a8000)** on the previously verified cracked X3. No write has been authorized or executed for this candidate. Future approval must identify the exact receipt image SHA-256, this scope, and the above development capability. Only the old cracked unit is eligible. Leave the replacement/daily device untouched. Do not insert the prepared library card for this fixture-only candidate.

1. Recheck exact device identity, security state, 16MiB geometry, partition table and active app1. Stop on a mismatch or missing expected port; do not guess another port, recovery chord or layout.
2. Acquire a fresh complete 16MiB readback before any write. Preserve it privately; rederive and verify its 296-sector current-image restoration slice. Existing immutable original and guarded-reader slices covering the same range are also prepared privately. The old 161-sector slices are insufficient. No restoration write is implicit permission.
3. Verify the exact archive hash; write only the approved app1 payload, with tool hash verification and no erase-all, bootloader, table, OTA selection or NVS writes. Read all 16MiB back before boot: payload must match, erase padding must beFF, and every byte outside the 296-sector interval must equal the fresh pre-write backup.
4. Boot normally once; collect a bounded private startup log and confirm the preservation guard, expected panel, no BUSY/reset loop/storage fault. Verify startup NVS against the fresh baseline before any ordinary UI writes. Stop on a failure and report evidence; do not restore automatically.

## Bounded unattended checks after approval

Use the reviewed live client with the explicit already verified port; a serial open may itself reset this USB device, so establish a fresh boot/session and never infer persistence from that alone. Begin with HELLO and STATUS, verifying the expected build identity and Home screen. One action at a time, require ACCEPTED, CONSUMED and RENDER_RETURN/NO_VISUAL_CHANGE plus status. Abort on mismatch, timeout, damaged record, manual input or unknown completion; no retry loop.

At a verified cold Home with first card selected: Activate opens Pocket pause; capture its monochrome cards; Activate opens its note; capture; Back closes the note; Back returns Home. Home Back means the physical Read action: open the sample and capture its saved passage/footer, then Back returns Home and performs the ordinary scoped reading checkpoint. Bound this session to these six presses and read-only status/captures. These commands can produce normal Newt progress/statistics changes; they do not mutate installed app packages. Do not invoke Sleep while the owner is absent, since USB cannot wake deep sleep. Store logs/captures privately. This validates control and framebuffer bytes, not physical four-tone rendering.

## Owner optical and power acceptance

When present, inspect cold Home, all four focus rows, all routes and Read; four distinct tones, quiet borders, Inter legibility and static background; Home→reader→Home transitions; reading insets/footer/bar; app note; sleep/wake and held-power wake. Compare the actual screen with the truthful runtime preview, not the fictional approved fixture telemetry. Stop on stale image, BUSY failure or reset loop. Measure heap/refresh duration; compile figures are not physical measurements.

The SD spare has 61 verified copied EPUB payloads under `/Newt/Books`, but this candidate opens only the embedded sample. Real compressed EPUB/SD and Unicode coverage are active follow-up work. A saved reading coordinate remains a private migration candidate; no Newt position has been imported.
