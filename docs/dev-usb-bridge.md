# Development USB test bridge

The isolated USB component was based on `808e908`; its Frosted bring-up is historical. Stage is now the only Home. `NEWT_DEV_USB` defaults to zero in source, excluding the protocol and its storage; the one firmware build (regional DEV, owner decision 2026-10-26) sets it, so every current Newt image carries the bridge. It is not an installed capability or permission to connect/flash hardware.

The bridge accepts only a handshake, semantic button events, read-only status/boot diagnostics, and bounded capture of the existing logical framebuffer. Main remains the sole app/display owner; the physical polling task retains InputManager ownership. Synthetic and physical events share the queue. Responses distinguish accepted, consumed, and render-return completion. Render-return does not prove physical panel appearance or waveform quality.

An installed development image remains development-capable across resets until replaced. A handshake activates a session, which expires on observed disconnect, replacement handshake, physical interaction, stalled output, or 60 seconds without commands. Boot/session/request IDs reject stale input. This is a development test interface, not an authenticated authority boundary. A future build without it (for OTA or a release) would leave it out.

Physical interaction is the canonical manual-takeover boundary: main invalidates the session, pending ticket and capture BEFORE dispatching a physical event. The input task publishes observations and main checks each synthetic ticket under one short shared critical section; a physical observation after the loop's initial sample therefore still cancels accepted-but-unconsumed input. No serial, display or application work runs under that lock. A fresh HELLO is then required. Thus an old queued remote press cannot execute after blocking physical work, even if a USB reset/reconnect during that work went unobserved. This intentionally supersedes accepted but unconsumed remote input; already consumed actions are not rolled back. There is at most one accepted remote ticket, and main drains it before periodic work. There is no SDK event task or ISR connection-epoch hook: a transient USB reset/reconnect wholly between samples is not independently detected. Do not treat unplugging as a guaranteed withdrawal of a request already consumed, and never automatically retry a press with unknown completion.

A capture lease uses the existing frame, expires after 1.5 seconds without a valid chunk request or a hard 15 seconds total, and aborts on physical input. Tests account for the actual ten-millisecond loop cadence and 256-byte USB TX queue. Periodic redraw/checkpoint work is deferred only during that bounded lease. Physical events retain their ordinary ordering and invalidate remote work at takeover. USB never wakes deep sleep. Sleep uses the ordinary app path (including ordinary Newt progress saves), physical release check and GPIO wake. No shell, arbitrary memory, files, flash, NVS, network or credential commands exist.

An active lease's original deadline cannot renew through FRAME. Expiry blocks reacquisition until main has given its normal periodic work an opportunity, closing capture-driven housekeeping starvation.

Before any separately approved deployment, disclose the development flag, exact image/source, these capabilities and session lifetime. The installed M1 image is `f6c6792bf8e4-regional-experiment`, with DIAG enabled. Earlier owner-controlled reading sleep/wake passed with USB connected; a later battery-only cycle returned Home. Resume reliability remains open; additional device access remains on hold.

## Build

```sh
python3 tools/check.py
python3 tools/firmware-build.py --clean
```

The one firmware build includes the bridge and writes `.pio/regional-dev-usb/` output with the approved toolchain, SDK and preservation guard. `--dev-usb` is retired. Neither command uploads. The C++ host simulation (`simulate`) was deleted with the C++ Newt in NEWT-90; a Rust host fixture is a follow-up.

After a separately approved installation and explicit authorization to access the selected device, the live runner can use the already approved project-local pyserial 3.5:

```sh
.toolchain/platformio/penv/bin/python tools/dev-usb.py live \
  --port /dev/EXPLICIT_OWNER_SELECTED_PORT --expect-build 78c5b1fe6f37 \
  --timeout 8 --press next --output .build/dev-usb/live-capture
```

The expected build must match the actual approved image; a future combined image has its own identity. Live mode never discovers or guesses a port, uses URL/network transports, flashes/resets firmware, or retries a press with unknown completion. It constructs an unopened Serial instance and sets DTR/RTS false BEFORE opening. OS/adapter behavior can still transiently affect control lines; physical verification remains necessary. It clears only the host receive buffer, validates the exact build/capabilities, and uses the same bounded codec as simulation. Deadlines cover write and receive together; capture has a 14-second host total limit under the device's 15-second lease. At most 32 presses are accepted per invocation. Live sleep must be the last command and cannot be followed by capture; it reports requested entry, then closes, with physical wake required.

Fake-driver tests exercise the live transport against a protocol fixture, including fragmented reads, line states before open, wrong-build refusal, short writes, deadlines and no retries. These tests do not establish actual USB control-line transitions. A previous authorized diagnostic attachment coincided with a visible return to Home; its exact cause remains unproven. No device was accessed while implementing the host freshness correction.

The client verifies two HELLO replies with the same boot/build/capabilities and
exactly advancing sessions, then proves a current response using read-only STATUS
with a randomly selected upper-half uint32 request ID before optional actions.
Earlier firmware without DIAG remains supported. Requests continue increasing,
with at least 65,536 reserved IDs. Old complete low-ID transcripts fail before
PRESS, FRAME or DIAG; this is freshness validation, not authentication.

JSON receipts include bounded request/response traces with boot, session, request
ID, UTC and monotonic timestamps, plus opening/closing lifecycle events. Each
record's `body` binds the exact command/arguments or decoded response; request
bodies strip only N1 and the separately recorded identities. Raw DIAG identities
and captured private book pages remain in ignored local receipts.
Operational failures retain their receipts. Output follows connection closure;
a failed close reports `closed=false`. Live receipts explicitly report
`opening_may_reset_device=true`: read-only commands and verified response freshness
do not make attachment non-disruptive. Raw startup logs are omitted.

## Wire boundary

Host lines are at most 79 printable ASCII bytes, terminated by LF: `N1 HELLO`, then `N1 BOOT SESSION REQUEST STATUS`, `DIAG`, `PRESS previous|next|activate|back|sleep`, `FRAME`, or `CHUNK GENERATION OFFSET`. Decimal numbers are canonical uint32 values; request IDs strictly increase within a session. Even five maximum uint32 fields in CHUNK occupy only 63 printable bytes. No other command is accepted. The parser rejects overflow, malformed numbers, extra tokens/control bytes and a partial line older than one second.

Replies begin `!N1 BOOT SESSION REQUEST`, end with ` *CRC32` and LF, and fit the 512-byte bound. Ordinary logs are outside the protocol. Damaged/interleaved replies must fail checksum and require a new handshake; neither a successful `write()` nor `flush()` proves host receipt. PRESS emits ACCEPTED only after queue insertion, CONSUMED after main accepts its ticket, then RENDER_RETURN after the actual blocking call returns or NO_VISUAL_CHANGE if no refresh happened. Disconnect observed before consumption cancels its ticket; already consumed semantic actions are not rolled back. Physical events can interleave in the ordinary batch, so completion describes the resulting batch frame.

FRAME identifies logical PBM1 dimensions, orientation, one-black polarity, generation, whole-frame CRC32, 160-byte chunk limit and last blocking display-call duration. CHUNK replies carry the same generation/offset and hex data, protected by record CRC. The host must validate every record, every offset/generation, the full byte count and whole-frame checksum. There is no framebuffer copy and no async-refresh path in this firmware. RENDER_RETURN measures the SDK call returning; BUSY wiring/controller behavior and physical appearance still require hardware validation.

The device adapter explicitly declares whether its retained frame is a complete monochrome image; unsupported formats fail FRAME. The combined firmware sets that capability false after an actual gray Home paint. FRAME fails with unsupported-format there; other complete monochrome screens remain capturable. A truthful multi-plane Home capture is future work. A last gray-selector plane must never be advertised as the complete image.

## Standalone USB component evidence

These measurements are historical for the isolated USB component. The combined Home candidate has its own `artifacts/frosted-usb-verification.json` receipt and larger image.

Compiled source checkpoint: `78c5b1fe6f371e63b2653bed2ba0c81667095014`. Both ordinary and explicitly enabled development images compile with the pinned approved C3 tools and pass the actual linked NVS preservation guard. Development HELLO carries the clean source identity `78c5b1fe6f37`. [Verification receipt](../artifacts/dev-usb-verification.json) records exact binary/ELF/map hashes and private local archive paths.

Ordinary image: **658,864 bytes**; development image: **664,048 bytes**. Linked data RAM increases **624 bytes** and the existing queue's payload storage increases **224 bytes**, for **848 bytes** of persistent state/storage. Linked IRAM text increases **48 bytes**. There is no additional task, framebuffer or authored protocol heap-allocation call. Existing CDC queues, SDK allocations and existing-task stack are separate. Compiler stack estimates include **592 bytes** for command processing and **48 bytes** for polling; they are not physical stack-high-water measurements.

Three fresh bounded correctness review rounds and a separate strict maintainability review finished clean. Earlier concrete findings were closed by timing-faithful transfer tests, live connection/expiry consumption gating, the canonical manual-takeover boundary and nonrenewing capture/housekeeping gating. The complete host ASan/UBSan gate, exact chunk reconstruction, damaged-record framing, queue/full/session/manual-takeover tests and default-symbol exclusion pass. [Host capture](../artifacts/dev-usb-host-capture.png) is the shared UI rendered through the real protocol fixture, with zero physical refresh timing. No USB device, firmware flash, SD, credentials, network deployment or publication was accessed.

The source checkpoint is ready for a separate deployment review. The live host transport is implemented and fake-driver tested; actual host USB behavior, throughput/backpressure, button debounce, controller BUSY behavior, panel appearance, held-power handling, sleep/wake and actual heap/stack/power measurements remain hardware work. Live commands require explicit port/build selection and were never executed against hardware. The development binary's unauthenticated local USB capabilities and persistence described above must be disclosed before choosing to install it.

Live client source checkpoint: `7b357cf2d228b5eb20b9c014e9b87746930e4b92`; firmware is unchanged from the compiled checkpoint above. Two focused transport review rounds finished clean after fixing pyserial's size-fill timeout behavior and strengthening the fake driver to reproduce it. The client now waits for one byte and drains only bytes already available, avoiding a fill timeout for every short frame reply. The complete host gate, including the fake live driver, passed at this source checkpoint. No actual serial port was opened.

## Firmware rows and listening

The client keeps four number-only firmware rows from the serial stream and drops
every other unframed line, as before. A row is kept only when the whole line
matches its exact format with canonical unsigned numbers (at most 20 digits),
so a row carrying any other text, such as a name, is dropped:

- `Runtime:` the loop task's stack size and lowest free stack, and the boot's
  lowest free heap (`kind` is one of the fixed observation names).
- `PC census:` the page cache census: result, per-kind name counts in the cache
  folder (unknown, page, semantic, progress, identity), bytes, time and units.
- `PC admission:` and `PC target:` the idle producer's decision and target.

The report lists them under `firmware_rows`, at most 256 per session; extra rows
are counted in `firmware_rows_dropped`. `--listen SECONDS` (0 to 600) keeps the
already-open port after the last press and reads only these rows, then sends
STATUS as usual. Opening the port resets the X3, so idle work (the census runs
while the reader is idle) is only visible within one open session. A protocol
record while listening ends the session with an error. Listening after a final
`sleep` is rejected.

## Read-only wake diagnostics

New HELLO advertises `press,status,frame,diag2`. The client also accepts the earlier
exact `press,status,frame,diag` schema and `press,status,frame` capabilities; the latter
reports diagnostics unavailable without sending DIAG. `live --diagnostics` requests DIAG after STATUS and performs no press
or capture unless those options were explicitly supplied. Sleep plus diagnostics
is rejected before opening the port. The same exact-build/deadline/CRC rules apply.

DIAG reports boot reset/cause, RTC record status and requested destination, restore
and checkpoint-save outcomes, validated requested book/spine/character, live reader
status/open/progress-ready/actual locator, and first consumed physical action. The
boot target is immutable even after navigating or closing the reader; live fields
are sampled separately. Numeric values are canonical unsigned integers.
DIAG2 adds `version=2`, live `current_book`, `heap`, `largest`, `opening`,
`library_count`, `library_selected` and `device`.
`current_book` identifies the live reader workspace and is zero when no workspace
exists; the older `book` remains the immutable boot target. Heap and largest block
are current unsigned byte counts; `opening` reports the current open transaction.
The bridge polls outside synchronous opens, so DIAG cannot observe progress while
an open blocks. A failed Library candidate closes its reader and restores only
the previous selection/progress, reporting `current_book=0` and `reader_open=0`.
Other reader failures can retain an allocated workspace with a nonzero identity
and `reader_open=0`; readiness requires the open/status/locator facts together.
Library count is at most 128 and selection is a zero-based index less than count,
or zero when count is zero. `device` is a stable opaque fingerprint expressed as
exactly 16 lowercase hexadecimal characters, distinct from the boot nonce; it
contains no raw device identifier. Firmware supplies these observations separately
from the bridge; the client validates their types and bounds.
The negotiated schema is strict: DIAG2 requires its version and all new fields,
while legacy DIAG rejects them. Unavailable DIAG2 is exactly
`DIAG version=2 available=0`; legacy unavailable DIAG remains `DIAG available=0`.
Physical action 255 means none consumed. The maximum DIAG2 body is 459 bytes;
maximum identities plus checksum produce a 507-byte record. Preserving every
legacy and new field requires a development-only wire bound of 512 bytes,
64 persistent TX bytes above the former 448-byte bound. Reducing the RX buffer
from 96 to 80 bytes preserves every canonical command and recovers 16 bytes.
The local command body
grows from 380 to 464 bytes. No additional command or multipart diagnostic is used.
The firmware's separate aggregate RAM assertion also counts the eight-byte device
fingerprint and keeps its existing 1 KiB bound; the core bridge/queue subtotal is
not a complete firmware RAM measurement.
On the combined reader/Library C3 checkpoint the named firmware aggregate is
1,016 bytes. Its scope excludes the SDK's additional 28-byte `ESP_EFUSE_MAC`
descriptor. Complete linked DEV-minus-normal SRAM is 816 bytes; with the
separately modeled 224-byte runtime queue increase that is 1,040 bytes before
allocator/queue metadata and task peaks. The existing named 1 KiB assertion
remains unchanged; it does not promise complete runtime DEV cost below 1 KiB.
Record status codes followed `SleepRecordStatus` in the C++ `firmware/retained_state.h` (deleted in NEWT-90; see git history).

DIAG calls only the diagnostics adapter. It does not request a framebuffer, enqueue
an event, save progress or renew the capture deadline. The dev-only snapshot has no
normal-firmware storage/symbols. It is evidence for the next owner-controlled physical
sleep/wake. Immutable boot facts and live reader fields must be interpreted
separately; current host receipts identify the observation session. The owner's
independent visible sleep/wake acceptance does not supply an instrumented anchor.

## One Home build

Home always uses Stage, including Sleep's Home backdrop. `home-stage` and
`home-frosted` are no longer accepted presses. The bridge, `purge-cache` and
existing diagnostics remain. New HELLO build IDs are plain 12-hex revisions;
archived regional candidates keep their suffixed IDs in their own receipts.

Newt's active NVS footprint is fixed: `newt_apps` (app), `newt_prefs` (reader
settings), `newt_pause` (Pocket Pause preferences) and `newt_reader` (built-in
sample progress). Selected SD books, per-book progress/statistics, resume intent,
Home clock zone and format use SD records. The retired `newt_home` namespace
and `/Newt/State/newt-home.nrs`, if present, are ignored and left intact. They
are Newt historical data, never a foreign-storage error or an erase target.
There is no new Home write at startup. Removing its RTC mirror reduces the
expected aggregate RTC allocation from 304 to 300 B; native measurement and
separate physical power checks remain required.
