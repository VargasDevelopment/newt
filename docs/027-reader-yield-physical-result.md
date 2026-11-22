# Reader cadence physical result

M1 is still blocked. The exact cadence candidate installed successfully and
preserved current progress, but the unchanged reading journey failed at the
sibling open. Ten stages validated; eleven were observed out of the planned 23.
No retry or further hardware action followed. The device remains awake on the
Library error screen with the reader closed and serial connection closed.

## Exact installed candidate

- Implementation: `1ad90ad5cb05b65f6ff99cccd25040808cf15163`
- Compiled source: `3b1041e2f901251a0956670b96f55b09de86f728`
- Tested evidence source: `9d80dbb21ef598f58216a26d83320c6eaeadb7eb`
- Build: `3b1041e2f901-regional-experiment`
- Image: **1,253,536 bytes**, SHA-256 `05a667f7a924ed5f8b95792cbda3ba90cec5ce3eeaed1eb883fd2d99679b49db`
- Write: **307 sectors, [0x780000, 0x8b3000)**, including **3936 FF padding bytes**.
- Full readback SHA-256: `79a35e00c637b344f6c78f3a728401037225d74fb7cc9e204bdf6fa51e14c675`

The coordinator approved this exact image and physical scope. Two final action
reviews were CLEAN after bounded corrections to the private esptool launcher.
Forty-five offline controls cover the actual pinned tool's write, connection,
reset, security and teardown failure paths. No toolchain files were modified.

Two independent fresh 16 MiB backups matched. Exact reconciliation reconstructed
every current NVS byte and rejected all 68 mutation controls. A third full read
matched before the single application write. Full readback then matched the
expected entire 16 MiB image, preserving every byte outside the approved span,
including current NVS, bootloader, partition table, OTA data and filesystems.
Both planned 35-second boots passed the expected panel/app, NVS guard and stable
Home checks; first-startup low 64 KiB remained identical. These are preservation
and startup results, not reading acceptance.

## Reading result

The unchanged journey used one connection and its original actions, 20-second
opening budget and 30-second terminal deadline. Its 96 operations map to 247
CRC-valid replies. No panic or reset after validated HELLO was captured. The
independent evidence audit is CLEAN and confirms the failure interpretation.

Primary paging passed: anchors **963 → 1257 → 1708 → 1257**, then exact reopen to
**1257**. This is the latest observed primary position. Post-journey durable NVS
was not recaptured; current device progress is authoritative. All prejourney
backups are historical and must not overwrite newer progress.

Sibling opening reached the budget at **20.094 seconds**, cancellation marker
**2**, then returned to the Library error screen with the reader closed. The
terminal reply arrived at **21.440 seconds**, within the original deadline.
The logged IoError precedes cancellation cleanup; closed-reader diagnostic
status 9 is the normal NotFound reset value and does not identify the cause.

| Opening | Selection interval | Reader interval | Traced total | Wire terminal |
|---|---:|---:|---:|---:|
| Primary | 2.169 s | 8.911 s | 12.608 s | 13.967 s |
| Primary reopen | untraced | untraced | untraced | 10.845 s |
| Sibling | 6.052 s | 12.734 s | 20.094 s | 21.440 s |

The phase counters are milliseconds / callbacks / sleeps / measured wait-ms:

| Open | Admission | Metadata | Layout |
|---|---|---|---|
| Primary | 8101 / 1630 / 441 / 142 | 585 / 40 / 20 / 4 | 220 / 23 / 5 / 1 |
| Reopen | 8120 / 1630 / 441 / 155 | 584 / 40 / 20 / 4 | 234 / 23 / 5 / 0 |
| Sibling | 12642 / 1182 / 561 / 142 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |

Sibling admission spent about **1.1%** of its measured time in waits. It timed
out before metadata or layout. This localizes the remaining cost to admission
but does not isolate SD reads, decompression or CRC work. The host suspension
result remains valid for its authored control; it did not establish device
performance or private-book readability. No validation or deadline was relaxed.

Sixty-eight stack observations showed a minimum **6616 bytes free of 16384**,
including a sample after primary reopen paint. The sibling reading path never
completed, so this does not certify the full fixed journey or all inputs.

## Next scope

Keep the exact installed image and raw evidence as a failed checkpoint, not a
known-good reading baseline. A bounded source investigation should distinguish
admission reads/seeks from inflate/CRC costs using authored controls while
preserving full source identity, every stream validation, cancellation, bounded
storage, reader typography/insets and both orientations. Any new physical image needs
its own exact candidate grant; no retry, restoration or extra input is authorized
by this result.

The remaining fixed recovery stages, settings, physical sleep/wake, optical
buttons, ghosting, cover preparation/transfer and owner acceptance remain
pending. The 3–5 day trial freeze and apps/SDK milestone follow M1 acceptance.
Library checkpoint 0993cd82133138fc7adfa4cd2aabe717b0104485 remains integrated;
optional NLC2 sidecars retain source size and two-second mtime binding. No card
or cover transfer occurred.

Sanitized measurements and evidence hashes are in
`artifacts/m1-reader-yield-physical.json`. Raw backups, private books, device
identities and serial captures remain local under ignored private directories.
