# Linux bench app installs

`tools/bench_flash.py` installs or restores a reviewed candidate into the one
app slot a tool may write. `tools/x3_slots.py` decides which slot that is from
the device on each run (each slot's app descriptor and Newt's mark, plus the
OTA selection records; docs/077-return-to-crossink.md,
docs/082-ota-updates.md section 1):

- **One slot holds another firmware** (any valid app without Newt's mark:
  stock, CrossPoint, CrossInk or another): Newt's slot is the other one. On
  the bench X3 that is app1 (`0x780000`); on the main X3 app0 (`0x10000`).
  When that Newt is the one running, the install rewrites it in place (below).
- **No slot holds another firmware** (after SD installs, docs/083): the slot that boots must hold
  Newt, be what the highest selection record selects, and be confirmed (state
  valid, or undefined). The tool writes only the other slot, so the running
  Newt stays the fallback.

| Layout (Newt in app0 or app1) | Slot written | Also needs |
| --- | --- | --- |
| Newt + another firmware | Newt's slot | nothing |
| Newt + Newt | the one that does not boot | a new session after each install |
| Newt + Empty | the empty one | nothing |
| Newt + Invalid | the invalid one | `--repair-invalid` (below) |

The tools never replace another firmware: put one in the other slot, or
replace it, with Newt's card install (docs/083), or on the bench with
`install-other` ([below](#another-firmware-into-the-spare-slot)). The tool refuses when both
slots hold another firmware; when no slot holds another firmware and
the slot that boots does not hold Newt; when a newer selection is still pending
("let the X3 boot once (slots does) and retry"); when the running selection is
not confirmed yet (state new); when a slot is unreadable, except
the repair below; or when no selection record is valid. It never writes
the other firmware's slot (except `restore-other` over the image
`install-other` wrote, below), the Newt that boots when there is none, the
bootloader, partition table, NVS or another region. It writes OTA selection
sectors only to make the written slot boot and to keep its fallback (below).
Restore supplies the previous
candidate and its reviewed manifest hash to the same command. Keep that
candidate and the full backup for recovery; the backup is not an automatic
whole-device restore.

Agents may flash the bench through this tool and test it through
`tools/dev-usb.py`, with one lease holder at a time for both: the Lead or a worker
whose brief grants the lease. Every other device, including the main X3, needs
explicit owner approval each time. A process lock enforces tool exclusion; it
does not grant that authorization. Host tests never access connected devices.

`tools/dev-usb.py live` is Linux-only; simulation and host fixtures are portable.
Flash only from the canonical clone or its worktrees.

## Another firmware into the spare slot

`install-other <image> --image-sha256 <pin>` puts another firmware (for
example the stock Xteink image, for the install-from-stock check) into the
slot that does not boot. `restore-other --from <install session>` puts that
slot's earlier bytes back. Each takes a new `--session`; `install-other`
refuses an existing directory, even a previous restore session. Neither selects
anything: `boot-other` and `boot-newt` do, as above.

- **Image, before the unit is opened:** its SHA-256 equals the pin; it is one
  whole ESP32-C3 app (header, 1 to 16 placed segments, checksum and appended
  SHA-256, `x3_slots.image_intact`), DIO/80MHz/16MB as the X3's own, inside
  the usable slot and without Newt's mark (a Newt build installs with
  `install` and its manifest).
- **Unit:** the slot that boots holds a selected, confirmed Newt
  (`x3_slots.running_newt`, as without another firmware above), and its whole
  image must pass the integrity check from the full backup. The other
  slot holds an older Newt or nothing; another firmware, an unreadable image,
  a pending or unconfirmed selection all refuse before any write.
- **Write:** only that slot's span, through `Device.write_spare`, which refuses
  every other offset. The same session backup and on-chip MD5 proofs as an
  install: everything outside the span, selection records included, before
  and after the write, then the span itself. Then one reset into the running
  Newt and the 30-second window.
- **Restore:** the install session's `other.json` records the slot, span and
  the image's MD5 and write phase. The payload is that span from the install session's backup
  (its SHA-256 must match the session receipt; it must hold Newt or nothing).
  It writes only while the same slot does not boot and still holds exactly the
  image `install-other` wrote. If that session records a started but unverified
  write, it also authorizes restoring its own backup over a partially written
  spare. The shared STOP still requires the recovery review below before access;
  no tool clears it automatically. Without that unfinished-write record,
  nonmatching spare bytes still refuse. The install session is kept as that backup.

### Plan stock runs around an owner power press

On the bench, X3 stock V6.5.5 powered itself off about one second after a
reset that was not a power-button press, making USB disappear. Its first boot
after `install-other` kept USB up; the reset after the next `slots` did not.
After `boot-other` into stock, expect to need an owner power press before the
next command, and plan stock runs around it. To recover, the owner presses
power once: USB returns while stock runs. Then `boot-newt` selects Newt again.

## A failed first boot

`install` and `restore` always select the written slot with a fresh *new*
record (`x3_slots.install_selection`), even when that slot already boots. Its
first boot is then pending until the image confirms itself at its first
finished screen, whichever it is (docs/083); a reset or power loss before that
makes the bootloader abort the record and boot the fallback:

| Layout | Slot written | A failed first boot returns to |
| --- | --- | --- |
| bench: CrossInk app0, Newt app1 running | app1, in place | CrossInk, app0 |
| main: Newt app0 running, CrossInk app1 | app0, in place | CrossInk, app1 |
| another firmware running (after `boot-other`) | Newt's slot | that firmware, the running slot |
| no other firmware | the slot that does not boot | the running, confirmed Newt |

In place, the running Newt's own record cannot be the fallback: it would
return to the new bytes. So before any app byte changes, the install writes a
settled (valid) record selecting the other firmware into the record the next
boot does not follow (`select-other-fallback`) and proves the selection area
on chip (`fallback-md5`); an interrupted install from then on boots it. After
`written-range-md5`, Newt's fresh record goes into the other record. Every
install and repair first runs step `fallback-image`: the firmware's image
check (`appImageIntact`, ported in `x3_slots.image_intact`) on the fallback's
bytes in the session backup. It refuses before any write if that check fails
or if the fallback would be the written slot. The receipt records
`first_boot_fallback`. A normal install still ends with one reset and HELLO
from the new build, which answers only after confirming itself.

## Damaged slots and the repair

A power loss during an SD install can leave the slot that does not boot
damaged: a cut while the installer writes its first sector last leaves the
head erased after the cut, and an interrupted erase of the first block leaves
it erased. An erased head reads Empty: an ordinary install writes it. Any other
damaged head reads Invalid and needs the repair. The installer selects a new
image only after it verifies, so the records still boot the running Newt.

`install` or `restore` with `--repair-invalid` writes only the Invalid slot,
and only when the other slot holds Newt that boots, is selected and is
confirmed, and its whole image still verifies: step `fallback-image` (above)
checks its bytes in the session backup, and `before-outside-md5` proves the device
still holds exactly those bytes (its full slot lies outside the written span).
Each failed condition refuses before any write: the damaged slot boots, a
newer selection is pending, the survivor's selection is not confirmed, the
image check fails, or an MD5 differs. The flag also refuses when the other slot is not
Invalid ("nothing to repair"). The repair never writes the survivor and selects
the repaired slot only after `written-range-md5` proves the candidate; then one
reset and HELLO, as for any install. The recovery authority below still
applies.

## Owner-controlled unit identity

By default identity is the owner's root-owned
`/etc/udev/rules.d/99-newt-x3.rules` (the bench). For a separately approved
main-unit session, pass `--identity-rule /etc/udev/rules.d/99-newt-x3-main.rules`
on **every** command; [080](080-main-device-switch.md) gives the owner-only
rule setup. Selection never edits the bench rule or discovers another unit. The tool requires a regular,
root-owned rule file without group or other write permission, containing exactly
one active rule line with one Espressif vendor (`303a`) and one MAC-shaped USB
serial. Missing, writable, malformed or ambiguous rules fail before any device
call. The by-id port is derived from that serial. There is no config, environment
identity override or serial-device enumeration. An explicitly selected rule has
the same ownership, permissions and content checks; failure never falls back
to the bench. The receipt records the rule path, MAC and derived port. Old `.build/bench/config.json`
files are ignored and should be removed from each old worktree.

Before opening the selected port, the tool checks its actual USB descriptor
(serial, vendor and USB-Serial/JTAG product). It then checks the decoded esptool
MAC, disabled secure boot/encryption, and 16 MiB flash. The authoritative rule is
only read; changing host rules or permissions is outside this workflow.

Use the existing approved project toolchain (esptool 5.1.2 and pyserial 3.5).
This workflow does not install dependencies.

```sh
.toolchain/platformio/penv/bin/python -B tools/bench_flash.py slots --session .build/bench/SESSION
.toolchain/platformio/penv/bin/python -B tools/bench_flash.py install CANDIDATE \
  --manifest-sha256 REVIEWED_SHA256 --session .build/bench/SESSION
.toolchain/platformio/penv/bin/python -B tools/bench_flash.py restore PREVIOUS_CANDIDATE \
  --manifest-sha256 PREVIOUS_REVIEWED_SHA256 --session .build/bench/SESSION
.toolchain/platformio/penv/bin/python -B tools/bench_flash.py boot-newt CANDIDATE \
  --manifest-sha256 REVIEWED_SHA256 --session .build/bench/SESSION
.toolchain/platformio/penv/bin/python -B tools/bench_flash.py boot-other CANDIDATE \
  --manifest-sha256 REVIEWED_SHA256 --session .build/bench/SESSION
```

`slots` only reads: it prints both slots' descriptors (with Newt's version and
build from its identity block, when present; reported only, never trusted for
a decision), the selection records and the decision or refusal, then resets the
unit. `boot-newt` writes only the selection sector, after an on-chip MD5 proves
Newt's slot already holds exactly this candidate; it writes nothing when Newt is
already selected. It is the USB way back from another firmware (docs/077).
Without another firmware both slots may hold Newt: `boot-newt` selects the
slot whose on-chip MD5 matches the candidate, checking the slot that boots
first (then nothing is written), and never selects before that MD5.
`boot-other` needs a slot that holds another firmware and refuses before any
backup otherwise.

`boot-other CANDIDATE --manifest-sha256 HASH --session SESSION` is the
USB recovery when a confirmed Newt cannot reach Settings. It uses the same
identity, lease, backup and STOP rules, proves the other firmware's descriptor
and full slot MD5 against that session's backup, and only selects it. Neither
app slot is written. Both boot-only commands protect the current Newt bytes
too. Another firmware has no DEV HELLO: require its later VALID selection
readback.

A candidate is the one firmware build (regional DEV). It contains
`manifest.json`, `firmware.bin`, `partitions.bin`,
`firmware-identity-regional-dev-usb.json`, `gate.log`, `gate-receipt.json`, and
`code-health-regional-dev-usb.json`. The externally reviewed manifest SHA-256 pins
all artifact hashes. Checks cover the clean revision/variant/image identity,
successful gate receipt and log hash, the resource total, ESP32-C3 app header,
segment bounds, XOR checksum, appended SHA-256, usable slot capacity, and
partition-table checksum/layout. The image embeds its build ID
(`<12-hex revision>`; historical candidates retain `-regional-experiment`): it must match the clean revision and
contain no `-dirty` suffix, and HELLO must report it after every Newt boot. A
retired variant's identity receipt or an image without that build ID is
refused. The identity receipt's revision, variant and `binary_sha256` must also
match the image. The archived
candidate's clean source identity is authoritative; the tool's worktree may be a later
revision. The manifest is a local review receipt, not a signature or independent
source review. Legacy manifest `scripts` attestations remain hash-covered; this
tool does not execute those archived scripts.

## Per-unit lease and failure state

Both live tools use `tools/bench_lease.py`. State lives at
`$(git rev-parse --git-common-dir)/newt-bench/<mac12>/`, outside checkout files,
sessions and temporary directories. It follows the physical USB serial across
worktrees. The `lock` file is never unlinked or replaced by either tool. Never
include this Git metadata directory in worktree or temporary-file cleanup.

The flash tool takes that lock before any device call. Live `dev-usb.py` reads
only its explicitly selected port's USB descriptor and takes the same lock,
keyed by that serial, before constructing the serial client. Contention fails
non-blocking with `bench is leased`. The flash tool keeps the lease through its
own DEV USB handshake and closure. Direct protocol fixtures and simulation are
unchanged. Other serial programs do not participate; lease holders must keep
them closed. The reading-readiness journey also holds this lease around its
USB client, through receipt recording and closure.

A shared `STOP` blocks both tools before opening a port. Selecting a new session,
changing worktrees, deleting session files or cleaning `/var/tmp` cannot bypass
it. Failures before the first erase/write call print FAIL and create no STOP:
this attempt has left flash unchanged.

Immediately before issuing the first write, a durable `write.json` records the
run directory, session directory and last good backup path. After all flash,
boot and receipt checks succeed, it records `verified`. If the process dies
while the attempt is unconfirmed, the next lease refuses device access and
records STOP. A caught failure after writing may have started records STOP
immediately. Its record preserves the same recovery paths and outside-flash
proof. A stale or malformed shared journal also fails closed. Successful runs
never create STOP; neither tool ever clears one.

Recovery authority is explicit: **the Lead** decides recovery for a failed app
write whose post-write outside MD5 checks are intact; **the owner** decides in
all other cases, including changed or unknown outside flash. Stop and report any
surprise. Preserve the backup and logs; never blindly retry a failed write or
reset automatically. Both esptool block and whole-write retries are disabled. The final ESP32-C3
reset also runs its pinned HardReset sequence only once: esptool's reset
wrapper would otherwise close/reopen and retry on an I/O error.
After the required decision, that authority must archive the shared STOP and
unconfirmed write record (including any `write.pending`) while holding the same
unit lock and with all serial clients closed. Removing only STOP leaves the
unconfirmed journal blocking access. Never remove or recreate `lock`. There is
no tool force, erase, address override or automatic recovery command.

After the Lead/owner decision above, set `unit` to the directory containing the
STOP path printed in the refusal. With all serial clients closed, that authority
can run this exact manual archive command. It preserves all three records in a
new private directory and keeps the existing lock inode:

```sh
unit='/absolute/unit/directory/from/the/STOP/message'
test -f "$unit/lock"
flock -n "$unit/lock" sh -eu -c '
  cd "$1"
  archive=$(mktemp -d recovery.XXXXXXXX)
  for record in STOP write.json write.pending; do
    if [ -e "$record" ]; then
      mv -- "$record" "$archive/"
    fi
  done
' recovery "$unit"
```

This only archives the approved recovery latch; it does not restore firmware or
verify a recovery image. Keep the last good backup and run logs.

## Session backup and byte preservation

A session is one named private backup directory for one unit and host boot,
lasting at most four hours. Session files are backup data, not the authority for
identity, locking or failure recovery. The first invocation reads all 16 MiB
once, confirms a device-side MD5, fsyncs `backup.bin`, and publishes its SHA-256
receipt. Missing backup or receipt forces a fresh capture and preserves any
orphan. A stale `session.pending` refuses before opening the device or taking a
backup. Corrupt backup or mismatched/expired receipt fails closed. Full dumps
may contain personal device data: keep them private and out of Git.

The device table must equal the complete candidate table sector; generated
3 KiB tables have erased padding. The highest valid OTA record selects the
booting slot, as the bootloader decides. `install` and `restore` always end
with the same sequence (`x3_slots.install_selection`), whether or not the
written slot already boots:

1. Only when the written slot is the one that boots (Newt beside another
   firmware, rewritten in place): select that firmware with a settled record
   (`select-other-fallback`) and prove the selection sectors
   (`fallback-md5`), before any app byte changes.
2. Write the app range and prove it.
3. Select the written slot with a fresh record, the next sequence, state new,
   in the other record's sector, as ESP-IDF does (`select-newt`).
4. Check the selection sectors against the expected bytes (`selection-md5`).

`boot-newt` and `boot-other` write only step 3's selection, for their
target, or nothing when that slot is already selected.

Esptool's RAM stub keeps the app stopped. Immediately before and after every
write, on-chip MD5 checks compare every region outside the written span and
the selection sectors against the original session backup (the other
firmware's or the running Newt's whole slot included), including the bytes at the written slot
minus one and exactly at the write end. The entire written span, including erased padding, is checked separately.
MD5 is official esptool's accidental-corruption check, not an authenticity
claim. Checked candidate bytes stay in memory through the write.

This proves the installed candidate bytes independently of HELLO:
`bench_device.Device.write` calls pinned esptool's `write_flash`, and
`bench_flash.install` independently requires `written-range-md5` against the
exact candidate plus erased padding before selecting it. `boot-newt` requires
`newt-slot-md5` against the candidate before selection. `slots` reports each
app descriptor's `elf_sha256`; compare Newt's value with the candidate's
descriptor (decoded by `x3_slots.classify`). That is the descriptor's ELF
SHA-256, not `firmware.bin`'s SHA-256 or a Git build ID, and a matching header
alone does not prove the entire installed image. The full-range MD5 supplies
the byte comparison; HELLO supplies the running build ID.

The first candidate fixes the sector-aligned session span and the written
slot. Smaller candidates are padded to that span; larger candidates need a new
session. Without another firmware, each confirmed install moves the writable slot to
the other one, so the next install, restore or `boot-newt` refuses with "The
writable slot differs from the session; start a new session directory". An
A-to-B-to-A check in that layout uses a new session for each step. A new session is
allowed only when the shared per-unit state permits access. Outside flash
changes between invocations also fail closed before writing. The original
backup remains the recovery image for the session.

After verified writing, the tool resets the unit and checks the exact DEV build
via two USB HELLO responses and a fresh STATUS challenge, recording DIAG when
available. Newt confirms a new selection only at its first finished screen
(docs/083), and HELLO answers only after that: an answered HELLO
means the new build painted and confirmed itself. After an install, `slots`
should show the new record `valid`, the device's own evidence of that
confirmation. The handshake runs on esptool's port, still open from the reset:
opening a port resets the X3, and a second boot before Newt confirms a new
selection makes the rollback bootloader abort it and start the other slot. It
resends HELLO for up to 20 s while Newt boots (6.5 s measured), then requires
the strict pair. After a switch from Newt, wait at least 30 s before opening any
port, so the new app can confirm its selection first.

Every reset now keeps esptool's port and the lease for at least 30 seconds
before closing it, including `slots`, `boot-newt`, DEV handshake failures and
reset errors. `boot-other` (no HELLO) prints `PASS confirm-window` before closure.
This protects the confirmation window even if closing toggles reset; it does
not prove that Newt confirmed or that closure never resets. Ctrl-C and SIGTERM
after the reset are deferred through the hold and port
closure, with the lease still held, then re-raised as cancellation. The run is
unconfirmed and follows STOP recovery if a write started, rather than reporting
PASS; read-only cancellations create no write STOP. SIGKILL and
unplug cannot be deferred; never use them during the window. A `boot-other`
receipt explicitly leaves running identity unconfirmed: wait for the command to return, then use `slots`
to require a VALID selection of the other firmware. The hardware close behavior remains pending
the [080 rehearsal](080-main-device-switch.md#bench-rehearsal-for-the-lead).
Terminal output gives timed PASS/FAIL steps; private per-run logs and receipts
remain in the session. This is a USB-powered reset/install check, not a battery
sleep/wake or cold-power check.

## Host verification and handoff

```sh
TMPDIR=/var/tmp/bench-tests python3 -B tests/bench_flash_test.py --negative-controls
TMPDIR=/var/tmp/bench-tests python3 -B tests/bench_flash_layouts_test.py --negative-controls
```

Create the scratch directory first. Tests inject an owner-rule path, isolated
shared-state root, USB descriptors, byte-addressed flash and host boot identity.
They never use a real serial port or shared production lock. Fixture and
simulation coverage remains in the existing USB suites. Guard-removal controls
cover authority selection, shared STOP across sessions/worktrees/cleanup,
interruption, the cross-tool lease, pre-write failure, both boundary bytes before
and during writes, the mutant that skips a sector after the span, slot identity,
the refusal of two other firmwares, boot-newt's slot proof and the
transport's other-firmware and selection-sector refusals.
`bench_flash_layouts_test.py` covers every layout an SD install can leave,
with Newt running from app0 and from app1: Newt+Newt, Newt+Empty,
Newt+Invalid after a torn header or an interrupted first erase, and another
firmware installed into Newt's old slot. Its guard removals cover each repair
refusal, the repair's proof before selection, `boot-newt`'s MD5 choice
without another firmware and the transport's refusal of the
Newt that boots. `tests/bench_install_other_cases.py` (run by the same file)
covers `install-other` and `restore-other`: the round trip with Newt running
from either slot, each image and layout refusal, the outside and written-range
proofs, restore's own-image and backup checks and `write_spare`'s refusals. Session span and fast-path table checks
have separate regressions; the fake device follows the rollback bootloader, so
a second reset before Newt confirms a new selection fails the install.
`tests/bench_handshake_test.py` covers the held-port readiness handshake and
`tests/x3_slots_test.py` the rule itself. The suite belongs to
both quick and complete gates and can run on the canonical Mac host.

The Lead performs the hardware A-to-B-to-A install/restore check after host
handoff, with the bench lease and two approved candidates. If B needs a larger
span, use a fresh session for B and then restore A in that session. Confirm both
outside MD5 regions and the written span. Require the expected HELLO
identity after each reset, recording screen behavior separately. Record power conditions and timings separately from
host-test evidence.
