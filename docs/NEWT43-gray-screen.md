# NEWT-43: rare gray screen investigation

This candidate builds DEV-only telemetry on main 817073e0, including NEWT-41
working frames and credited destination paints. Phase 1 supplied the diagnosis. No waveform, persistence,
refresh budget or panel power policy changes. Neither photograph establishes a
controller identity or a cause. The supplied before/after photos were inspected:
white areas have widespread gray speckling before navigation, while text stays
sharp; the later photo has clean white areas. The selected Home section also
changes, so the pair is evidence of recovery after an update, not an identical
frame comparison. No hardware was used by this worker. Device verification and
the Lead's independent review remain pending.

## Frame paths and state assumptions

References below describe this branch's Newt sources and the pinned SDK paths
under `vendor/freeink-sdk/libs/display/FreeInkDisplay/`. Regional builds also
compose `patches/freeink-regional-gray.patch`; that patch must be considered
alongside the pin, rather than treating the pin as the regional binary.

- Cold boot and ESP deep-sleep wake execute `setup`: boot evidence is snapped
  before restoration (`firmware/main.cpp::setup`), the SD rail is released before
  controller detection, and `display.begin()` resets and initializes the panel
  (`firmware/main.cpp::setup`). `DisplayReceipt` is ordinary, non-RTC state, initially invalid
  (`firmware/display_receipt.h::DisplayReceipt`). NEWT-41 can show a Full working frame, then
  a Fast first reader page: `paint(!pressAnswered())` (`firmware/main.cpp::setup`) and
  `settlePress()` (`firmware/press_acknowledgement_runtime.h::settlePress`) credit the
  completed working cleanup. A Fast first *reader page* is therefore expected;
  it does not imply that no earlier cleanup established its baseline.
- Every slow press can show one working frame before its work:
  `acknowledgePress` (`firmware/press_acknowledgement_runtime.h::acknowledgePress`) calls
  `paintWorkingFrame` (`firmware/book_open_display.h`). `FirstOpen` keeps the
  portrait explanatory screen; Opening, Saving, Applying and Loading use the
  reader orientation. Valid binary predecessors permit Fast in either
  orientation. Direct gray or an invalid receipt selects Full. Leaving Overlay
  writes an inverse destination into both controller planes
  (`firmware/overlay_exit.h::driveFromInverse`) and requests Fast, driving every pixel. The
  five `working-*` reasons, plus `-inverse` where applicable, distinguish these
  requests. A working receipt is not a completed reading page or wake restore.
  `settlePress` changes refresh-budget bookkeeping, not the panel: the next
  actual reader/UI/Home request has its own trace. A changed-source reimport
  continues the same press, rather than issuing a second acknowledgement
  (`firmware/source_verification_runtime.h::serviceChangedSource`).
- Binary reader/UI paints (`firmware/main.cpp::paintNonHomeScreen`) request Full for an invalid
  receipt, gray predecessor, orientation change, explicit force or a spent
  refresh budget. Other pages request Fast. Overlay exit uses the inverse
  rebase above via `firmware/binary_display.h::displayBinaryFrame`. `toNativeFrame` only rotates/inverts host bytes; it neither
  activates the panel nor supplies the previous controller frame
  (`firmware/frame_adapter.h::toNativeFrame`).
- Return to Home reaches `paintHomeScreen` (`firmware/main.cpp::paintHomeScreen`). Default
  Frosted and default Stage Home upload complete Direct selectors, then issue
  one gray activation (`firmware/home_display.h::paintFrostedHome`,
  `firmware/home_display.h::paintStageHome`). Direct has no
  separate binary base and does not use a binary old-frame diff. Unsupported
  gray falls back to Full in those functions.
- Regional Home normally uses Overlay: a binary base/settle with Half fallback
  (`firmware/home_display.h::paintOverlayHomeImpl`), then complete overlay
  masks and the gray nudge in that function. Repeated Home compositions rebase both planes to the inverse
  destination; a due cleanup requests resync. Eligible focus updates use
  guarded binary islands/regions (`firmware/home_display.h::paintFrostedHome`,
  `firmware/home_display.h::paintStageHomeFocus`); rejected requests fall back
  to the complete composition. These operations power off analog rails by
  their SDK default, but are not deep sleep. Battery-only Direct regions
  (`firmware/home_display.h::updateFrostedHomeBattery`) are compiled only in the alternative non-Overlay regional setup.
- Sleep paints a complete Direct scene, or Full monochrome on unsupported
  gray (`firmware/sleep_display.h::paintSleepScreen`). The caller waits for that
  blocking operation before closing the book, sending panel deep sleep and
  removing board rails (`firmware/main.cpp::enterSleep`). There is no same-process
  hibernate/resume or general panel power-off API used by Newt. Wake goes
  through the fresh initialization above. Zone-list focus asks for a Fast
  window (`firmware/device_settings.h::displayListCard`); pinned X3 drivers fall back to
  whole-panel Fast. Zone cleanup is Full
  (`firmware/device_settings.h::paintZoneListMove`).

For the SDK, `src/FreeInkDisplay.cpp:598` drains pending async work before a
blocking binary update. `:805` validates complete non-Overlay gray planes before
activation; `:1036` drains pending work before panel deep sleep. Newt uses
blocking calls and one UI owner, so the polling button task cannot initiate a
second display operation. A receipt advances only after the blocking return;
it does not independently prove the physical waveform completed.

Mode meanings differ by controller:

- UC8253 Fast uses the retained DTM1 binary previous frame; finish copies the
  destination into DTM1 (`src/driver/Uc8253X3Driver.cpp:186`, `:302`). Half drives
  the destination independently of DTM1. Full seeds DTM1 white, uses the strong
  bank, then conditions/settles. Boot invalidates the baseline and forces an
  initial full sync (`:159`, `:207`). A rails-down request with `turnOff=false`
  is promoted to Half (`:197`), with invalid state still taking full sync.
- UC8279 Fast uses retained DTM1 and the DU bank; Half and Full both select GC.
  Invalid baseline, initial boot budget or resync also selects GC
  (`src/driver/Uc8279Driver.cpp:125`, `:143`). Finish rewrites DTM1 inside the
  full PTL window (`:203`). GC is still transition-dependent; its name does
  not mean any arbitrary old RAM is safe. First boot seeds old white; Direct
  gray exit first paints the destination from its inverse (`:146`).
- Direct supplies two absolute selector planes and loads the Direct LUT at
  activation (UC8253 `:484`; UC8279 `:318`). Both invalidate the binary baseline
  afterward. The next binary update recovers explicitly (UC8253 `:188`,
  UC8279 `:130`). Overlay needs a calibrated binary base, then uses masks and
  a nudge LUT. Its old/new planes cannot be treated as a displayed BW frame
  without cleanup/rebase. SDK Absolute uses a separate binary base followed
  by complete absolute planes; Newt requests Direct or Overlay instead. The
  regional patch adds guarded plane-preserving
  windows and rejects invalid baselines; it also resets its added state at
  begin/deep sleep and invalidates regional eligibility on full BW writes.

## Hypotheses, ranked for the next bench capture

This is a code-informed investigation order, not measured probabilities.

1. **BUSY acknowledgement or physical completion mismatch.** Both drivers
   wait for PON and refresh, then publish validity. But the bus permits missed
   BUSY assertions and bounded waits: `src/bus/EpdBus.cpp`, `waitBusy`'s
   `X3TwoPhase` branch and `waitRefreshComplete`, return without an error result
   after a missed edge or timeout. The driver cannot distinguish that from a
   successful waveform before writing old RAM and publishing validity. This
   is a concrete observability gap, not proof it happened. Capture missing
   PON/DRF waits, unusually short passes or roughly 30-second waits around a
   bad frame. A harmless completed pulse before observation is also possible.
2. **Gray drive's physical starting state / supply or settling sensitivity.**
   Direct bypasses binary conditioning and relies on its absolute LUT to
   establish the result (UC8253 `beginGrayscale:329`, `displayGray:484`;
   UC8279 `beginGrayscale:350`, `displayGray:318`). It appears on Home and sleep,
   matching two reported contexts. Code loads panel voltage/booster settings
   at init and waits for PON; it supplies no measurement of rail settling or
   optical response. The panel and power condition are unknown in the photos.
   Keep this separate from a software old-frame validity defect.
3. **Silent controller RAM loss while software remains alive.** Warm Fast and
   regional focus updates trust retained controller planes. Unexpected rail
   loss could violate that assumption. However, ordinary Newt deep-sleep wake
   resets the controller, invalidates its baseline and forces the first binary
   paint; regular full-frame calls default to keeping analog rails on. Regional
   focus's POF is not deep sleep and the driver intentionally retains RAM.
   There is no code evidence that normal POF loses that RAM. This weakens the
   originally proposed fast-after-wake explanation substantially.
4. **Stale gray LUT or omitted wake LUT reload.** Each binary path reloads its
   bank (UC8253 `:222`, UC8279 `:182`); gray activation loads its bank too.
   Controller begin reloads voltage/booster/panel registers. Direct exits have
   explicit recovery; Overlay exits rebase. No Newt custom-LUT calls exist.
   These custom-bank paths do not perform a Newt temperature measurement or
   temperature-specific LUT selection. Temperature sensitivity remains a
   hardware question, but there is no demonstrated omitted wake-only reload.
5. **Sleep interrupts a refresh or another software update races it.** Blocking
   paint, single-owner rendering, SDK pending-work drains and paint-before-
   deep-sleep ordering argue against it. A missed BUSY wait remains hypothesis
   1 rather than evidence of a second Newt display writer.
6. **Accumulated Fast ghosting.** Reader and UI policies bound Fast sequences;
   Home composition and sleep normally use gray, and first binary boot paint
   is Full. A rare broad gray failure immediately after opening/waking is a
   weaker fit than gradual ghosting after many turns. Still record page/focus
   counts and cleanup settings to test this independently.

No generic validity fix is justified yet. Re-sending both buffers is already
used for Overlay exit and costs two RAM transfers without a GC flash. It does
not reconstruct an unknown physical grayscale state: writing the same new
frame into both planes makes a differential update drive little or nothing.
Writing an inverse destination can force all transitions, but its optical
result depends on the actual panel/LUT. Adding that or a Full flash to every
wake/Home would add latency without establishing a cause. The pinned driver is
left unchanged.

## Landing cost

Allocated flash record 61 and protected update 38 follow main's NEWT-51
records 60/37. The 31-entry ordinary-RAM ring is exactly 256 B, including its
sequence and indices. It records continuously in DEV builds and compiles out
of release builds. Its memory is never held while the feature is off, so
`idle_owner_approval` is null; no owner approval is claimed.
Four clean measurements on merged source `d619a042` give:

| Lane | Flash before → after (B) | Static RAM before → after (B) | RTC before → after (B) |
| --- | ---: | ---: | ---: |
| default | 2,232,480 → 2,232,480 (+0) | 133,232 → 133,232 | 300 → 300 |
| dev-usb | 2,383,296 → 2,386,496 (+3,200) | 134,064 → 134,320 | 304 → 304 |
| regional | 2,239,136 → 2,239,136 (+0) | 133,728 → 133,728 | 300 → 300 |
| regional-dev-usb | 2,392,000 → 2,395,712 (+3,712) | 134,560 → 134,816 | 304 → 304 |

Release flash, RAM, RTC and selected stack ceilings are unchanged against
main's firmware. DEV static RAM increases by exactly the 256-byte ring.
Protected update 38 records the exact measured DEV paint-stack increases:
Home/non-Home/sleep are 112/96/144 B in DEV. Regional DEV raises non-Home
80 → 96 B, sleep 128 → 144 B, Overlay 64 → 80 B, Stage-context 80 → 96 B and
Stage's upload lambda 16 → 32 B. Its Home (80 B), Frosted (80 B), Stage
(112 B) and Stage focus (48 B) remain unchanged.
The record and receipt retain exact before/after for every changed selected
frame; all existing selected keys and previous ceilings are preserved. The
unselected Bridge command frame grows 832 → 880 B for bounded encoding.
Floors stay fixed. Cross-family review r2 of `739c22a8` is clean; the Lead
verifies r4 directly. Compiler frames do not establish runtime margins or
physical behavior.

## Telemetry and current bench tools

DEV firmware records panel API requests from boot in one fixed ordinary-RAM
ring, independently of USB connection, handshake or synthetic PRESS. It covers
reset/initialization, boot-or-wake working frames, physical buttons, idle Home
updates, binary paints, gray-base preparation, gray activation and regional
focus APIs. A cleanup credit is bookkeeping and creates no panel entry; its
destination's actual request does. No live Panel/ack/Wait diagnostic collection
remains in the client. The historical latency-classification literals remain
separate, with their original PRESS attribution and bounds. Acceptance projects
only its defined connection/protocol fields and retains its strict schema.

`tools/dev-usb.py panel-ring` reads one bounded PANEL reply after a fresh
handshake. It sends no PRESS or repaint. Each decoded entry has a boot-local
32-bit sequence, milliseconds since boot, API-call duration, mode, reason and
outcome. IDs and strings are fixed tables; no book text or arbitrary logs are
stored. The 31 most recent requests survive disconnect and handshake changes.
Sequences are derived from the ring's next sequence and chronological count;
the first retained sequence exposes overwritten history. Compare modulo 2^32
within one boot; the protocol boot identity separates resets. Milliseconds also
wrap modulo 2^32. Duration saturates at 65,535 ms with an explicit flag.

A bool API records accepted or rejected from its return value. A void API is
accepted when it returns; this cannot establish internal driver acceptance.
The monochrome Home/sleep recovery call is marked fallback (or rejected if a
bool API refuses it). Eligibility checks and compositions that never invoke a
panel API do not create entries. An unfinished call is pending. Sleep-entry
itself is omitted: ESP deep sleep discards this RAM before it can be read. The
new wake boot starts a fresh ring. Shutdown, reset, crash or power loss loses
older history. A hung UI task may prevent readout. Missing overwritten or
previous-boot requests cannot rule out a hypothesis.

The following commands are for the Lead's separately authorized bench session;
none was run on a device by this worker. Use the approved project interpreter
and follow [bench-flash.md](bench-flash.md) for the candidate manifest, app1-only
install, backup, shared lease and STOP handling. These receipts alone are not a
flash-ready candidate manifest.

```sh
mkdir -p /var/tmp/newt43-bench
# Lead only: reviewed archived candidate and its exact manifest hash.
.toolchain/platformio/penv/bin/python -B tools/bench_flash.py install CANDIDATE \
  --manifest-sha256 REVIEWED_MANIFEST_SHA256 --session .build/bench/newt43

# After gray appears during physical-button use, stop input and record optics.
# Reconnect if needed; collect the ring before any deliberate recovery action.
.toolchain/platformio/penv/bin/python -B tools/dev-usb.py panel-ring \
  --port "$bench_port" --expect-build "$build_id" --timeout 30 \
  > /var/tmp/newt43-bench/gray-01-ring.json
.toolchain/platformio/penv/bin/python -B tools/dev-usb.py live \
  --port "$bench_port" --expect-build "$build_id" --timeout 30 \
  --diagnostics --output /var/tmp/newt43-bench/gray-01 \
  > /var/tmp/newt43-bench/gray-01.json

# Optional single synthetic action, chosen from the actual current screen.
.toolchain/platformio/penv/bin/python -B tools/dev-usb.py live \
  --port "$bench_port" --expect-build "$build_id" --timeout 30 \
  --press activate --diagnostics --output /var/tmp/newt43-bench/action-01 \
  > /var/tmp/newt43-bench/action-01.json
# Read panel-ring again to collect that action's calls.

# Sleep is final, with no capture/diagnostics. Wake is always physical.
.toolchain/platformio/penv/bin/python -B tools/dev-usb.py live \
  --port "$bench_port" --expect-build "$build_id" --timeout 30 \
  --press sleep > /var/tmp/newt43-bench/sleep-01.json
# Wake physically, then run panel-ring and the no-PRESS DIAG/frame command above.
```

Get the explicit by-id `bench_port` from the authorized installation record and
`build_id` (including regional suffix) from the reviewed candidate/HELLO record.
Close each client before the next: install/live/panel-ring share one unit lease.
Preserve error JSON and partial evidence. An unknown PRESS completion is never
replayed automatically. A later reconnect changes the power condition; record
that change. Idle battery paint can still append an entry during readout;
the single UI owner encodes one consistent snapshot into its bounded reply.

## Reproduction matrix and interpretation

1. Record build/variant, controller verdict (detected/assumed/inconclusive),
   reset reason/wake cause, battery/charging, temperature, Home style, orientation
   and refresh-page setting. Film optics. Read the ring after physical actions
   and after wake. Record sequence ranges, rejected calls and fallback outcomes.
   Boot ID, firmware milliseconds and host monotonic time are separate clocks.
2. On USB power, run 30 Home and 30 reading sleep/wake trials. Leave asleep for
   30 seconds; alternate immediate navigation and 10 seconds untouched. Video
   starts before sleep and ends 10 seconds after wake. Require deep-sleep reset
   evidence. Full working-opening/first-open followed by Fast reader is the
   intended NEWT-41 cleanup-credit sequence; verify both requests and outcomes.
3. Exercise 30 opens/returns across at least three heterogeneous ordinary books,
   warm and naturally cold, both orientations, chapter changes, slow settings
   save/apply/loading, 30 Home focus moves and reading beyond cleanup interval.
   Do not delete user caches to manufacture a trial. Collect ring plus protocol.
4. Repeat both 30-cycle wake groups unplugged on battery, with no USB during
   trials. Use video/on-device evidence; reconnect after a failure while that
   boot is alive to collect its ring/DIAG/frame. Previous sleep-entry and older
   boots are gone. USB success cannot satisfy battery acceptance.
5. Separately do 10 genuine cold power-ons by the approved procedure. Require
   POWERON reset evidence and record supply. Inspect working and first reader
   frames. Sleep and software reboot do not count as cold power-on.
6. On gray, stop input for 10 seconds and photograph/video the unchanged optics.
   Read ring, DIAG and logical frame without PRESS, then make one deliberate
   recovery navigation and save its before/after evidence. Preserve all failed,
   partial and no-reproduction trials. Measure latency and minimum loop-stack
   margin independently before owner acceptance.

A clean logical PBM with gray optics supports a display-drive problem. Accepted
Full/Fast or gray requests prove only the API path and return. A rejected base
or regional Fast request is explicit, followed by any attempted fallback; a
void driver's internal promotion/rejection may remain invisible. API durations
near 30 seconds are suspicious for the driver's bounded timeout, but do not
prove BUSY timing or successful physical completion. Driver wait lines are not
retained by this ring; separately authorized serial/BUSY capture is needed for
that evidence. The ring cannot prove LUT bytes, temperature, rail settling,
controller identity, physical waveform or clean white pixels. An accepted
Full working frame preceding a Fast reader page disproves a skipped software
cleanup in that retained sequence, subject to the API/physical completion gap.
Compare power conditions and video before changing panel policy. A recovered
frame does not erase the original fault. No waveform fix is asserted here.
