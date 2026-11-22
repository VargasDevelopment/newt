# Sequential reader candidate proposal

The physical M1 milestone remains blocked. The installed bounded-open image
completed ten validated stages of the fixed 23-stage journey, then returned to
Library when sibling opening reached its cooperative deadline. Command delivery,
primary page turns and exact saved-anchor reopening passed; sibling readability,
the remaining rejection/recovery/settings stages, optical buttons, battery wake
and owner acceptance remain incomplete. See `022-sequential-reader-source.md`
and the sanitized `artifacts/m1-bounded-open-journey.json` for those limits.

The isolated source correction avoids a redundant seek when the actual live
file cursor already matches a requested read offset. It adds no retained cursor
or buffer. Full SHA validation, catalog release, admission, parser reserves,
startup/NVS guards, cancellation, the 20-second opening budget and 30-second
command deadline remain intact. Its physical timing benefit is unverified.

## Frozen build

- Branch: `fix/m1-reader-sequential-io`
- Implementation: `82b5a9ca2b074af1bcd24408a887006a6f2bed18`
- Compiled source: `9489a63701d78e963da5d80abec3293c3bf596de`
- Build: `9489a63701d7-regional-experiment`
- Image: **1,252,752 bytes**
- Image SHA-256: `c1a5c214b57be03e2f86de680de58046cb41558c44db4d111673d4662b688cdf`
- ELF SHA-256: `732617b928776a82702b33730a46231d76cbeaf7f1202729b860151e145a678e`
- Partition table SHA-256: `334c9553e2799107ed33eb9057f8661c6dd757c662a661edac6c73bb99ae7d5e`
- Image: `.pio/regional-dev-usb/build/newt_x3/firmware.bin`
- Archive: `.build/m1-reader-sequential-9489a63701d7/regional-dev-usb/`
- Proposed application span: **306 sectors, [0x780000, 0x8b2000)**
- Image ends at `0x8b1d90`; sector padding is **624 FF bytes**.

No new candidate installation or hardware action is authorized. The earlier
grant applied to the exact installed `191c0795d401` image and its closed journey;
it cannot be reused for this image.

## Verification

The complete host gate passed with ASan/UBSan and exact preview checks, including
real compressed-reader admission, cancellation/rollback, full-digest mismatch
rejection, catalog lifetime and both orientations. All ten production/config/
test/tool scopes of the host-tested implementation equal the compiled freeze.
After two independent source and measured-cost reviews, only exact per-variant
flash costs and provenance were adopted. Policy tests passed: 21 structural,
39 resource and seven procedure-cache fixtures, plus the structural gate.

Four post-adoption clean native builds passed their current source/procedure/
output bindings, resource ceilings and linked NVS/sleep/owned-task checks:

| Variant | RAM bytes | Image bytes | RTC bytes |
|---|---:|---:|---:|
| Default | 133264 | 1237600 | 300 |
| DEV | 134080 | 1248896 | 300 |
| Regional | 133776 | 1241424 | 300 |
| Regional DEV | 134592 | 1252752 | 300 |

All 24 selected frames and the adapter's 32-byte frame are unchanged. Every
variant retains the owned 16 KiB loop task. Each archive contains all 334 bound
outputs, receipt, linked-guard report, compiled configuration and build log.
`artifacts/m1-reader-sequential-builds.json` binds these exact outputs and records
the separate physical acceptance status. Two independent final audits verified
all current bindings, all 334 outputs and archives per variant, actual native
measurements and linked guard/task arguments. Both are CLEAN. Final
maintainability and proposal review is also CLEAN. The proposal is ready for
coordinator action review; this does not authorize installation.

## Next coordinated hardware scope

After an explicit grant for this exact image, establish current known device
state through the approved observation/reset route and fresh target/layout
checks. Capture fresh matching full-flash backups and durable restoration copies
outside Git. Current progress is authoritative: explain actual reading changes
against historical evidence, preserve all current NVS bytes, and do not restore
an older checkpoint or waive unrelated differences.

Write only the proposed application span, with full-flash readback proving every
outside byte equals the fresh baseline. Verify the expected build/startup guard
through the two reviewed boot observations and the first-startup low-flash
preservation check. Then run one unchanged 23-stage reading journey on one
connection, retaining distinct accepted, consumed and terminal evidence and a
stack observation after actual reader paint. Stop and close on failure; do not
retry, extend either deadline, send arbitrary sleep/USB inputs or restore flash
automatically. No card/cover transfer is included in this proposal.

Library cover code supports source size plus POSIX modified time rounded down
to two seconds, exact 3412-byte sidecars and payload CRC. This remains decorative
and does not authorize opening. Actual card timestamps, covers, focus latency
and ghosting still require coordinated physical checks. The owner trial and the
second apps/SDK milestone wait for successful M1 reading acceptance.
