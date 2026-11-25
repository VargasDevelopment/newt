# Development USB test bridge

`NEWT_DEV_USB` defaults to zero in source, excluding the protocol and its storage; the one firmware build (regional DEV) sets it, so every current Newt image carries the bridge. It is not an installed capability or permission to connect/flash hardware.

The bridge accepts only a handshake, semantic button events, read-only status/boot diagnostics, bounded capture of the existing logical framebuffer, and one cancellable card write (below). Main remains the sole app/display owner; the physical polling task retains InputManager ownership. Synthetic and physical events share the queue. Responses distinguish accepted, consumed, and render-return completion. Render-return does not prove physical panel appearance or waveform quality.

An installed development image remains development-capable across resets until replaced. A handshake activates a session, which expires on observed disconnect, replacement handshake, physical interaction, stalled output, or 60 seconds without commands. Boot/session/request IDs reject stale input. This is a development test interface, not an authenticated authority boundary. A future build without it (for OTA or a release) would leave it out.

Physical interaction is the canonical manual-takeover boundary: main invalidates the session, pending ticket and capture BEFORE dispatching a physical event. The input task publishes observations and main checks each synthetic ticket under one short shared critical section; a physical observation after the loop's initial sample therefore still cancels accepted-but-unconsumed input. No serial, display or application work runs under that lock. A fresh HELLO is then required. Thus an old queued remote press cannot execute after blocking physical work, even if a USB reset/reconnect during that work went unobserved. This intentionally supersedes accepted but unconsumed remote input; already consumed actions are not rolled back. There is at most one accepted remote ticket, and main drains it before periodic work. There is no SDK event task or ISR connection-epoch hook: a transient USB reset/reconnect wholly between samples is not independently detected. Do not treat unplugging as a guaranteed withdrawal of a request already consumed, and never automatically retry a press with unknown completion.

A capture lease uses the existing frame, expires after 1.5 seconds without a valid chunk request or a hard 15 seconds total, and aborts on physical input. Tests account for the actual ten-millisecond loop cadence and 256-byte USB TX queue. Periodic redraw/checkpoint work is deferred only during that bounded lease. Physical events retain their ordinary ordering and invalidate remote work at takeover. USB never wakes deep sleep. Sleep uses the ordinary app path (including ordinary Newt progress saves), physical release check and GPIO wake. No shell, arbitrary memory, flash, NVS, network or credential commands exist; the one file command is the card write.

An active lease's original deadline cannot renew through FRAME. Expiry blocks reacquisition until main has given its normal periodic work an opportunity, closing capture-driven housekeeping starvation.

Before any separately approved deployment, disclose the development flag, exact image/source, these capabilities and session lifetime.

## Build

```sh
python3 tools/check.py
python3 tools/firmware-build.py --clean
```

The one firmware build includes the bridge and writes `.pio/regional-dev-usb/` output with the approved toolchain, SDK and preservation guard. `--dev-usb` is retired. Neither command uploads. There is no host simulation mode.

After a separately approved installation and explicit authorization to access the selected device, the live runner can use the already approved project-local pyserial 3.5:

```sh
.toolchain/platformio/penv/bin/python tools/dev-usb.py live \
  --port /dev/EXPLICIT_OWNER_SELECTED_PORT --expect-build 78c5b1fe6f37 \
  --timeout 8 --press next --output .build/dev-usb/live-capture
```

The expected build must match the actual approved image; a future combined image has its own identity. Live mode never discovers or guesses a port, uses URL/network transports, flashes/resets firmware, or retries a press with unknown completion. It constructs an unopened Serial instance. On Linux, it presets DTR true and RTS false, opens, then clears DTR. This avoids pySerial 3.5 clearing DTR while the CDC driver still has RTS asserted, the C3's reset combination. The Linux post-open clear tolerates only unsupported-modem ioctl errors (EINVAL/ENOTTY); other errors fail and close the port. Other platforms retain both presets false and perform no post-open DTR write. OS/adapter behavior can still transiently affect control lines; physical verification remains necessary. It clears only the host receive buffer, validates the exact build/capabilities, and uses the same bounded codec as simulation. Deadlines cover write and receive together; capture has a 14-second host total limit under the device's 15-second lease. At most 32 presses are accepted per invocation. Live sleep must be the last command and cannot be followed by capture; it reports requested entry, then closes, with physical wake required.

Fake-driver tests exercise the live transport against a protocol fixture, including fragmented reads, line states before open, wrong-build refusal, short writes, deadlines and no retries. These tests do not establish actual USB control-line transitions.

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

Host lines are at most 79 printable ASCII bytes, terminated by LF: `N1 HELLO`, then `N1 BOOT SESSION REQUEST STATUS`, `DIAG`, `PRESS previous|next|activate|back|sleep`, `FRAME`, `CHUNK GENERATION OFFSET` or `PUT` (the card write, whose lease lines are longer). Decimal numbers are canonical uint32 values; request IDs strictly increase within a session. Even five maximum uint32 fields in CHUNK occupy only 63 printable bytes. No other command is accepted. The parser rejects overflow, malformed numbers, extra tokens/control bytes and a partial line older than one second.

Replies begin `!N1 BOOT SESSION REQUEST`, end with ` *CRC32` and LF, and fit the 512-byte bound. Ordinary logs are outside the protocol. Damaged/interleaved replies must fail checksum and require a new handshake; neither a successful `write()` nor `flush()` proves host receipt. PRESS emits ACCEPTED only after queue insertion, CONSUMED after main accepts its ticket, then RENDER_RETURN after the actual blocking call returns or NO_VISUAL_CHANGE if no refresh happened. Disconnect observed before consumption cancels its ticket; already consumed semantic actions are not rolled back. Physical events can interleave in the ordinary batch, so completion describes the resulting batch frame.

FRAME identifies logical PBM1 dimensions, orientation, one-black polarity, generation, whole-frame CRC32, 160-byte chunk limit and last blocking display-call duration. CHUNK replies carry the same generation/offset and hex data, protected by record CRC. The host must validate every record, every offset/generation, the full byte count and whole-frame checksum. There is no framebuffer copy and no async-refresh path in this firmware. RENDER_RETURN measures the SDK call returning; BUSY wiring/controller behavior and physical appearance still require hardware validation.

The device adapter explicitly declares whether its retained frame is a complete monochrome image; unsupported formats fail FRAME. The combined firmware sets that capability false after an actual gray Home paint. FRAME fails with unsupported-format there; other complete monochrome screens remain capturable. A truthful multi-plane Home capture is future work. A last gray-selector plane must never be advertised as the complete image.

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
STATUS as usual. Keep the port open to observe idle work (the census runs
while the reader is idle) without an attachment's possible control-line glitch. A protocol
record while listening ends the session with an error. Listening after a final
`sleep` is rejected.

## Read-only wake diagnostics

New HELLO advertises `press,status,frame,diag2,put`. The client also accepts the earlier
`press,status,frame,diag2`, exact `press,status,frame,diag` schema and `press,status,frame` capabilities; the latter
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

DIAG calls only the diagnostics adapter. It does not request a framebuffer, enqueue
an event, save progress or renew the capture deadline. The dev-only snapshot has no
normal-firmware storage/symbols. Immutable boot facts and live reader fields must
be interpreted separately; host receipts identify the observation session.

## Card write

`put` writes a file to any validated path under the card root, including
`Newt/Books`. `Newt/Dev` remains useful for inert test files. This is a DEV
interface: staging a book, app, update or state record can change device behavior.
The code is `rust/core/src/dev_put.rs`; the firmware glue is
`rust/firmware/app_runtime/card_write.rs`. None of the scoped PUT receive path is included when `dev-usb` / `NEWT_DEV_USB` is off.

- **When:** only on Home with no book open, opening or due to reopen. Otherwise
  PUT answers `ERROR busy reason=reading|opening|not-home`. Update mode and a
  restart never write.
- **Paths:** card-relative UTF-8, with `/` between FAT long-name components.
  No absolute path, empty component, `..`, backslash, control byte, FAT forbidden
  character, DOS device name, or trailing dot/space. Each component fits 255
  UTF-16 units; the complete native path, including `/newt-sd/`, fits 511 bytes.
  Every existing parent is checked without following links. Missing directories
  are created. A final file may be replaced; a directory or other kind never is.
  The internal `NEWT.PUT`, `NEWT.OLD` and `NEWT.MRK` names and older
  `.newt-put`, `.newt-old`, `.newt-old-name` names are reserved.
  Before opening the port, the host refuses batch names that the pinned
  FatFs uppercase table treats as equal (including Unicode aliases such as
  `σ`/`ς`). Python's Unicode casing is different. The host also refuses `~`
  in every component, preventing a later batch destination from resolving to
  an earlier file's generated FAT short alias (for example `ABCDEF~1.TXT`).
  Existing files remain
  replaceable through any FatFs-equivalent spelling: FILE is an independent
  atomic replacement, and the device has no batch identity.
- **Size:** no PUT file-size cap or reserved free-space allowance. FILE checks
  the card's free bytes; insufficient space answers `ERROR card-full free=N`.
  An empty file is valid. File sizes and offsets use unsigned 64-bit counts;
  the card format's own file limits and later write errors still apply. Filling
  the card can prevent other Newt records from being saved.

### Lease

`N1 BOOT SESSION ID PUT` grants a lease, answered
`PUT line=4351 chunk=4096 path=hex data=raw`. With increasing request IDs:

1. `FILE PATH_HEX SIZE SHA256`, answered `FILE free=N`. PATH_HEX encodes the
   entire card-relative UTF-8 destination; SHA256 is 64 lowercase hex digits.
2. `DATA OFFSET LENGTH` plus LF, **immediately followed by exactly LENGTH raw
   bytes**, answered `DATA N` with the bytes written so far. Every block is
   4,096 bytes, except the final shorter block; an empty file has none.
   No delimiter follows the raw bytes. Newlines, NUL and `N1 HELLO` inside a
   block are ordinary data. There is one outstanding request, never a pipeline.
3. `DONE`, answered `DONE size=N sha256=HEX` after sync, complete card read-back,
   size/SHA-256 verification and commit. Incoming bytes alone are not the check.

The old 150-byte Base64 blocks came from HWCDC's default 256-byte RX queue:
its ISR drops bytes when that queue fills. That queue stays unchanged.
For a granted PUT only, an exclusive receive token masks the RX interrupt,
drains bytes already in the SDK queue first, then polls the C3 hardware FIFO.
An empty FIFO is polled for at most 250 microseconds before sleeping, so a
short gap within a USB burst does not impose a millisecond sleep per packet.
The FIFO applies USB backpressure while the lease fills its bounded buffer.
TX and the SDK's TX interrupt handling are unchanged. HELLO, DIAG, FRAME and
refused PUT requests keep their ordinary SDK receive path.

The token restores the previous RX enable bit on every lease exit, including
errors, disconnect, timeout and cancellation. It leaves pending FIFO bytes and
raw RX status for the SDK after restoration, so the next HELLO or PUT works.
Only the mask register read/modify/write uses a tiny critical section; no wait
or allocation occurs in it. Every input wait remains bounded by the lease's
3-second, non-renewing header/block timeout. A lease borrows one 4,543-byte heap block and frees
it on exit; no larger queue or receive buffer is held while PUT is unused.
The bridge's ordinary 80-byte command RX and 512-byte reply bounds do not change.

- Bytes go to the card root's reserved `NEWT.PUT`; the final name is untouched
  until DONE. A partial raw block is never written.
- Before its first rename, DONE syncs root `NEWT.MRK`, containing the
  complete relative target. A replaced file moves to root `NEWT.OLD`, the
  verified temp takes its name, and only then are backup and marker removed.
  A failed install rename attempts to restore the old file.
- Physical input, disconnect, 3 s without a complete header or raw block,
  a 1 s reply stall, or malformed/stale input abandons the lease and removes
  the temp. There is no total-duration deadline on an active transfer.
  Physical input and disconnect also stop read-back and the check immediately
  before the first rename. Once renaming starts, DONE finishes, restores or
  reports `recovery-needed`. HELLO at a header boundary cancels the lease;
  while receiving a block it is data, and during DONE it is read after DONE.
- A refused request ends the lease with `ERROR REASON` or
  `ERROR card step=STEP code=ERRNO`. No file or frame is automatically retried.
- `ERROR recovery-needed step=STEP code=ERRNO` means the commit cannot be
  proved complete or undone. The internal files stay; further writes refuse.
  A FAT rename cut short can leave two names or a hidden short alias on one
  cluster chain: deleting either can free the other's data. A marker therefore
  never triggers automatic deletion or recovery from mere presence checks.
  Old per-folder markers/backups also refuse writes to their target directory.
  The root scratch names use 8.3 form so no alternate long-name alias can
  bypass the destination checks.

To clear `recovery-needed`, repair the card's FAT filesystem on a computer,
confirm it has no cross-linked files, then remove the internal files. If repair
fails, copy off what is needed before reformatting. This exceptional recovery is
separate from staging or an ordinary interrupted transfer, which needs no card swap.

### Host

```sh
.toolchain/platformio/penv/bin/python tools/dev-usb.py put \
  --port /dev/EXPLICIT_OWNER_SELECTED_PORT --expect-build 0123456789ab \
  --folder Newt/Books --file local/generated.epub
```

`--folder` accepts a card-relative directory; `.` means the card root. The old
`apps`, `update` and `dev` spellings remain aliases for their `Newt/` directories.
`--name` changes the basename of a single `--file`. Paths and the batch count
(at most 64) are checked before opening the port. The client verifies the final
size and SHA-256, stops at the first failure and never retries an unknown DONE.
A missing DONE answer gives `"result": "unknown"`; establish a fresh handshake
and check the card before another write. Receipts include free bytes, total
seconds and bytes per second, including read-back and commit; DATA is summarized
once per file. The DONE deadline grows with file size.

DONE verifies the current temporary file before committing it; the protocol
has no command to read or hash an existing destination after a batch. Batch
name safety therefore relies on the host's restricted names and the pinned
FatFs case comparison, rather than a final verification of all destinations.

The client uses committed tables and needs no firmware toolchain. The gate
checks those tables against `libfatfs.a(ffunicode.c.obj)` in the pinned SDK;
`python3 tools/fatfs_case_generate.py --write` regenerates them after a reviewed
provider change. The source path and object SHA-256 are recorded in the table.

HWCDC ignores the `115200` baud argument: it uses the ESP32-C3's USB Serial/JTAG
full-speed USB link. Larger raw blocks remove the Base64 overhead and reduce
round trips and small card writes. End-to-end speed still depends on the host,
USB scheduling, SDK queue handling, card writes, sync and read-back. Virtual X3
rates measure its emulator/host stream, not the physical USB link or card.

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
There is no Home write at startup.

### PUT failure diagnostics and session recovery

Each file receipt keeps a bounded `protocol_progress`: the last sent request,
last acknowledged DATA offset and ID, elapsed/write time, cumulative DATA
frames/time, and the last valid response or transport error. It never stores
DATA payloads or an unbounded per-frame log. After a failed PUT, the client
keeps a read-only tail for at most 1.1 seconds to capture late lease statistics
before closing; it sends no further command. A successful write is still proved
only by the matching DONE response.

A granted DEV lease emits a number-only `PUT_STATS` row on exit, best effort
with a one-second output bound, including a temporarily disconnected channel.
No row is promised if the host remains gone or stops reading. Counters exist only on the active lease's
stack, with no idle heap. It reports the current request ID, block offset,
received/expected bytes, total bytes read, empty receive calls, pause count/time, DATA
receive time, card-write phase time, ACK enqueue time and commit phase time.
Times are milliseconds; phases include their ordinary code overhead, not just
USB or SD hardware time. ACK enqueue is not host receipt. No hardware overflow
bit is observed: incomplete reception or malformed framing narrows a failure
but does not by itself prove a hardware RX overflow.

Exit reason codes are 0 done, 1 HELLO, 2 physical input, 3 disconnected,
4 receive timeout, 5 invalid/overflowed line, 6 syntax, 7 session, 8 stale ID,
9 reported refusal, 10 output failure, 255 no recorded reason. The first reason
is retained even if its best-effort reply fails. The lease's existing receive
and output deadlines remain unchanged.

Before any action, a dirty or timed-out initial handshake triggers one bounded
resynchronization: remain silent for 4.1 seconds while draining old replies
(the three-second PUT receive bound plus its one-second statistics output
and cleanup margin), delimit any remaining partial line, then prove two fresh
HELLO sessions and a random-ID STATUS. Old responses can be discarded only
during that handshake recovery. Build checks and action identity checks stay
strict. Recovery never replays FILE, DATA, DONE or a press; persistent dirty
input fails closed before actions. Unknown commits still require inspection.
