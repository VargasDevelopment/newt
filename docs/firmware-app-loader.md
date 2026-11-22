# One portable app, loaded by firmware

2026-10-01. `spike/firmware-app-loader` starts from preserved app-loop checkpoint `66015de`. Earlier baseline/reader/app-loop branches and archived binaries remain unchanged. The smaller baseline `44c454d` remains the first hardware candidate.

The C3 firmware now selects **Your little app** on Home. It loads the exact 615-byte portable Pocket pause artifact through the same bounded integrity wrapper and decoder used by the host. A selected card can show its local note, open the actual embedded EPUB reader, or open device information. One shared semantic-routing function serves firmware and host simulation. Platform code owns drawing, buttons, reading and device access. Apps contain data and these known actions; no executable code, filesystem paths or network access.


Current M1 behavior supersedes the historical checkpoint below: Morning Page
and Apps open Pocket Pause v0.2.0 directly to an enabled prompt, with shared
settings/explicit Save and private `newt_pause` preferences. Generic package
routing remains unchanged. The built-in 615-byte artifact SHA-256 is
`a0fc8ee651cd3e50cf9649e16b014ecf3cf76b78e7a71a43cb66e13ecca580ab`.
RTC v4 adds a checksummed sleep-rejection tag and permits explicit absent
selection only for the verified Pocket package. See [current M1 evidence](design-requirements.md).

## Reproduce the checkpoint

```sh
python3 tools/make-builtin-app.py --check
python3 tools/newt.py build apps/pocket-pause.json
python3 tools/check.py
python3 tools/firmware-build.py --clean
python3 tools/loader-c3-limits.py
python3 tools/preview.py --screen app --actions activate,sleep,wake
python3 tools/preview.py --screen app --actions next,activate
```

The complete host gate also works when invoked by absolute path from another directory. Newt's agent build captures test output and sets the test working directory explicitly. The generated header embeds portable bytes and a separate expected SHA-256; the preflight checks both against the agent-authored source before firmware compilation. The app remains `NEWTAPP1`, 615 bytes, SHA-256 `0bb2c995109525d062dce831f66f13dcebcd5314697e2c27c0d09de99fecf8ef`. Recipient intent is absent from both portable bytes and the builtin header.

`loadTinyAppPackage` checks exact size, explicit unsupported version, expected SHA-256 and the existing strict field decoder. Failed operations preserve output. It uses the installed SDK's mbedTLS on C3 and Apple's installed CommonCrypto for host tests. SHA-256 establishes byte integrity against supplied metadata, not authorship or authorization. No dependencies were added.

## Private storage generations

Firmware opens only Newt's `newt_apps` NVS namespace and two bounded keys, `slot0`/`slot1`. Each private 691-byte record contains a version, local generation, package digest, unchanged portable payload and whole-record digest. This envelope belongs to device storage. Its generation is independent of app version and the host fixture's recipient desired revision.

Both slots are checked before selection. A known damaged inactive record can recover through the previous valid generation. Unknown native read/open failures, oversized records, unsupported versions, and equal/half-range generation conflicts fail closed. Only confirmed namespace/record absence is Empty. Boot uses the builtin artifact only for Empty; other failures show an unavailable screen. **Boot never installs or writes an app record.** The existing reader still owns its separate progress writes.

The save seam validates before opening writable storage, preserves the sole valid prior slot, writes only the inactive slot, commits and verifies exact readback. Identical package identity skips writes. Host fault tests cover torn/corrupt writes, failed commit/readback, restart, wraparound, ambiguity and unsupported state. A complete committed write with lost readback may become the newest valid generation after restart; the caller receives failure until verified. This is recoverable-generation logic, not a physical flash power-loss guarantee. Native NVS commit/durability behavior remains untested on X3. There is no FAT/SD adapter or claim of FAT rename atomicity.

There is deliberately no exposed install command yet. Host `queue`/`sync` remain the separate serialized file-backed fixture described in [the earlier app loop](app-loop-experiment.md); they do not reach this NVS seam. A future bounded delivery adapter can supply expected bytes/identity without changing the portable format. Network, accounts, generalized schemas and signing architecture remain outside this checkpoint.

## Sleep and evidence limits

RTC v3 retains raw bounded app selection/note state plus package identity. Checksummed/range-checked state resumes the tiny app only when the loaded package digest matches; a changed/unavailable app returns safely to Home. RTC retention does not survive battery loss. The same tests exercise native frame rotation, retained-state corruption and identity mismatch, alongside all earlier reader tests.

See [verification](../artifacts/loader-verification.json), [static C3 reports](../artifacts/loader-c3-limits.txt), and exact previews `artifacts/loaded-app*.png`. Source-owned definition/state/store/snapshot are 607/8/1,400/644 bytes on C3. The trusted renderer reuses the existing 52,272-byte portrait frame. Compiler stack reports are per-function static estimates, excluding call-chain peaks and precompiled crypto/NVS/runtime allocation. MCU heap headroom, panel/buttons/sleep, native NVS and flash recovery have never been hardware-tested.

The meaningful next step is the supervised cracked-X3 bring-up in [BRINGUP](BRINGUP.md), starting with the preserved baseline. After baseline and reader gates pass, separately review this app-only candidate and exercise app note → reader/device, sleep/identity restoration and measured heap. Replacement stays disconnected and remains the daily device.
