# Next reader candidate: recovery and NVS gates

October 1, 2026. The reader write remains preparation only; no additional flash or restoration. The owner finished the first baseline tests, then separately authorized read-only recovery reconnect/readback. That result is recorded below. The original character is preserved separately as [the first Newt](../assets/first-boot-newt/README.md); no deployed artwork was changed.

The historical-image blocker below is resolved by the separately compiled/reviewed [NVS-preserving reader](009-nvs-preserving-reader.md). The final candidate keeps the same 161-sector scope; preserve the historical archive and distinguish proposed/executed hardware receipts.

## Coordinated read-only recovery check

Once the owner finishes, reconfirm the old cracked X3 and its exact port against private original identity, with the replacement/original daily card excluded. The prepared private proposal uses the existing pinned esptool with one connection attempt: one USB download reset for identity, then security and full 16 MiB flash read with no further resets. Logs/readback stay private and use a new output path. The trusted esptool read stub runs in RAM; no flash, SD or security writes are requested.

Check identity/security, partition MD5, both OTA CRCs, unchanged active `app1` at `0x780000`, and unchanged bootloader/table/other app. Compare the entire post-test baseline image against the already verified readback, especially NVS. Preserve and investigate differences; never automatically erase/restore. A sleeping C3 may be absent from USB; stop and coordinate a wake rather than retry resets. One deliberate normal reset with bounded logging returns to baseline after reading; it is a cold/reset boot, so it can return Home rather than retain the story session.

At **21:15 UTC**, the authorized ROM reconnect succeeded with the same private chip identity and unchanged security state. Complete 16 MiB readback SHA-256 remained `0addb4552592d9f830eaa2d2c2d7c0613c781badb0a41370b77aad78f9eae7d9`: every byte matched the pre-first-boot verified baseline, **including all NVS bytes after the hands-on and sleep/wake tests**. The 161-sector baseline restoration slice matched the live readback. No flash/erase/restore/security/SD writes ran. The private `post-newt-recovery-verification.json` records this executed check.

One subsequent normal reset returned to baseline Newt. The bounded 20-second capture observed its banner, UC8279 verdict 1/BUSY timeout 0, 172,232 bytes free heap/largest block 114,676, and full refresh completion in 693 ms, without a fault or reset loop. The device was left running the original baseline; no reader upgrade was performed.

## Historical reader image and restoration scope

The proposed next useful screen is the preserved Lexend reader at source `86d13c072998d5a5885659903764b5b4d08e55f3`, `.build/typography-86d13c0/firmware.bin`, 657,456 bytes, SHA-256 `fe584741e6bfa764efb2bf01c056a1b13eaad76a25cfa3646f37b573dda16779`. It cleanly compiled and passed host gates; native reader allocation, input, menu and persistence remain untested. Its 69,968-byte workspace and Expat transient heap need real measurements. This is an embedded sample EPUB, printable-ASCII monochrome reading font, menu/settings and trusted builtin app; no SD import, networking, updater or native grayscale support.

The image would erase **161 sectors / 659,456 bytes**, `[0x780000, 0x821000)`, within the observed `app1` capacity `0x770000`. Its payload ends at `0x820830`, followed by 2,000 bytes of erased padding. Private 161-sector restoration slices were prepared from the original CrossInk backup and verified baseline readback; the baseline slice must match/regenerate from the post-test live snapshot before a write. The old 102-sector restoration slice cannot fully undo this larger candidate. App restoration does not restore NVS; automatic OTA rollback is not implemented. No restoration is authorized by preparing a file.

**The historical image is not ready for a write approval:** the upstream NVS initialization issue below needs a tested project-owned guard, a new compiled source/image identity and a fresh receipt. Keep this archive intact rather than relabel it as guarded firmware.

## Exact app-owned NVS writes

The observed shared NVS partition is `0x9000` / `0x5000` bytes. Namespaces limit ownership of keys, not physical page mutations: commits and garbage collection can change shared NVS pages. Actual available capacity and fault behavior are not yet measured.

| Namespace | Candidate behavior |
| --- | --- |
| `newt_reader`, `slot0/slot1` | Two 52-byte progress/stat records. Changed state saves on a 30-second active-reader checkpoint, reader exit/sleep, and checkpoint before settings reflow. Commit and exact read-back verification; same-value records avoid rewriting. |
| `newt_prefs`, `slot0/slot1` | Two 24-byte preference records. Only explicit settings Save writes the inactive slot, commits and verifies; identical preferences avoid a write. Unknown/future/ambiguous state blocks saves. |
| `newt_apps`, `slot0/slot1` | Read only in this firmware. Confirmed absence chooses the builtin package; no install transport or `PackageStore.save` call is exposed. |

Reading may lose up to the checkpoint interval if an active session is interrupted. Later persistence testing needs a confirmed checkpoint and coordinated power cycle. No NVS erasure, existing-reader-key writes or migration is intended; storage-full/read/save failure must remain visible without formatting storage.

## Upstream startup can format shared NVS

Local inspection of the already acquired Arduino core found `cores/esp32/esp32-hal-misc.c:328–335`: before Newt's `setup()`, `initArduino()` calls `nvs_flash_init()`. If it returns `ESP_ERR_NVS_NO_FREE_PAGES` or `ESP_ERR_NVS_NEW_VERSION_FOUND`, Arduino erases the **entire NVS partition** through `esp_partition_erase_range()` and retries initialization. Newt's fail-safe store code does not prevent that earlier branch.

This startup code also exists in the already deployed baseline. The initial full readback verified preservation **before the first Newt boot**; the separate 21:15 UTC post-test full read subsequently confirmed that all NVS bytes remained unchanged through baseline boot and the reported hands-on/sleep tests. That establishes preservation during this session, not safety if either upstream initialization error occurs later.

Do not silently accept whole-NVS formatting as part of scoped Newt progress/settings writes. Before the next candidate, add a bounded project-owned guard against this startup erasure, test the two error cases and ordinary initialization, prove it is linked into the C3 image, and recompile/archive with a new exact hash. Preserve the toolchain/upstream pin. If initialization cannot safely succeed, keep Home/diagnostics usable and Newt persistence unavailable; do not clear somebody else's reader data.

The private `x3-preflight-20261001-_u17gdhg/next-recovery-and-reader-proposal.json` records exact argument arrays, conditions, restoration hashes and the executed read-only recovery result. Its reader write/restoration arguments remain unexecuted and unauthorized. An independent bounded review validated addresses/hashes, app-owned NVS effects and the upstream startup-erasure risk. The next owner decision, after the guard and fresh build gates, is the final exact reader image plus its declared runtime NVS writes.
