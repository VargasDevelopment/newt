# Read-only Home clock (NEWT-26)

Read the X3 DS3231 at the pinned sensor-bus address, after BatteryMonitor has
initialized the shared Wire bus. Only the UI task reads either device. Do not
use the SDK Rtc::begin(): it writes the control register. A read sends only a
register pointer, then reads the 16-byte time/status snapshot; it never writes
register data or acknowledges OSF. Only Update's Check writes the chip
([Setting the clock](#setting-the-clock-261)). Each bus transaction uses a 10 ms
Wire timeout; restore the gauge's previous timeout on every result. No retries.

Home displays 24-hour HH:MM only if the snapshot completed, OSF is clear, BCD
and time fields are valid, and the day exists in its Gregorian month/year.
Decode the chip's 12-hour register mode and century bit without changing them.
Otherwise display --:--. This phase shows the chip's stored civil time as-is;
UTC storage and a local-time display offset belong to phase 2. No network,
settings, clock writes or persistence changes are introduced.

Read at boot (even if reading resumes), and at an existing full Home composition.
The firmware owns six text bytes; Home borrows them for its blocking paint and
clears the pointer before returning. A fast focus move or battery-only status
pass keeps the displayed text without reading the chip. Only after a focus
attempt is rejected does its full fallback read; nested full fallbacks share
one read. A minute rollover cannot turn a fast focus move into a full repaint.
The smaller conservative refresh cache funds this source within existing RAM.
There is no reader clock and the sleep backdrop keeps the placeholder.

Skip a standalone five-minute tick. Default Home uses whole-screen Direct
composition. Regional Home uses Overlay composition and supports only proven
binary focus regions; the existing battery rectangle does not cover the clock.
No existing small-region clock update works across these paths. A whole-screen
gray refresh would delay input/sleep and add unnecessary display work. Time can
therefore remain stale while Home stays awake without other paints.

DEV USB emits one CLOCK line at boot and for every attempted read. `reason`
is boot or home (NEWT-32 adds settings, renamed system by [078](078-system-ui-consistency.md)); `read` reports a complete snapshot,
`osf` is -1 when unreadable, and `valid` is the display decision. `raw` is the unmodified BCD year-month-date
and hour:minute:second in hexadecimal; the month century and hour mode/PM bits
remain visible. `weekday` and `status` are raw hex too. Phase 2 replaced
`text` with `zone` and `local` ([068](068-home-clock-time-zone.md)).
Production emits no clock telemetry.

Host tests compile the verbatim firmware Home paint and startup read, exercise
valid/OSF/impossible-calendar/bad-BCD/missing/short/error results, verify timeout
restoration and a pointer-only transaction, and render each Home style with
valid and fallback text. Mutation controls remove OSF/plausibility/fallback,
Home publication and the boot read. Real Home-pipeline tests cover minute
rollover during fast focus, rejected focus/Overlay fallbacks, status passes,
explicit full refresh and due cleanup; an eager-read mutation must fail. Frames
must differ only in the clock slot.

Hardware acceptance remains pending (phase 1). The Lead must compare the raw time and
Home text against a reference, record the reset reason, and test USB sleep,
unplugged battery power-off for at least 30 minutes, and cold power-on
separately. Check that the chip advances correctly across each condition,
battery status still works, Home focus and reader input stay responsive, the
reader has no clock, and failed/unset clock readings leave --:--. Do not set
the clock or clear OSF in this phase. If the chip holds UTC, the phase-1 display
shows UTC until phase 2 adds the local-time offset.

The exact four-lane resource audit, raw reports, protected-frame guards and
evidence limits are recorded in [CODE_HEALTH.md](CODE_HEALTH.md#newt-26-read-only-home-clock-record-48-2026-10-21).
The [X3 startup contract](BRINGUP.md#x3-display-startup-invariant) preserves the
original display-probe authority and UC8279 profile exception independently of
this boot-time I2C read. Lead hardware checks must include DEV runtime
`stack-min-free` as well as the clock/reference-time comparison.

## Setting the clock (#261)

A newcomer whose earlier firmware never set the chip would see `--:--` and
undated statistics forever. Newt now sets the chip from the network, using the
one internet request it already makes: Update's Check
([088](088-ota-wifi-install.md)). Wi-Fi Test only joins, and nothing else in
Newt reaches the internet, so there is no other source and no extra request.

**Convention.** The chip holds UTC, 24-hour, century bit clear (2000-2099),
weekday 1-7 from Monday. CrossPoint and CrossInk write the same through the
FreeInk SDK's `Rtc::set` (both keep UTC and add the zone when they show it),
and the stock firmware reads 24-hour BCD plus 2000 and applies its time zone
offset. A UTC write therefore keeps each firmware right under its own zone
setting. Their weekday orders differ and none shows it. Newt still never
calls `Rtc::begin()`, whose control register write was only outside phase 1's
read-only rule; Newt leaves the control register and alarms as they are.

**Source.** The `Date` header of the first response the check opens, which
is GitHub's API (`firmware/update_https.h`). If it has no readable Date, the
check sets no time; a later response never stands in. The response arrived
over TLS with the server checked against the certificate bundle, and its status and
headers came whole. It has whole seconds. The setter adds the time since it
arrived, to the nearest second, so it lands within a second or two of the
server's clock. Plain NTP was not used: it is unauthenticated and would be a
new service to contact.

**Rules** (`newt/network_clock.h`, `firmware/network_clock.h`):

- Only the IMF-fixdate form is read, and its weekday must match its date.
  Anything else counts as no Date.
- The time must fall from the day before this build's commit date (About's
  date; 2026-01-01 without one) through 20 years after it, and before 2100.
- The setter runs once per check, after Wi-Fi is off and before the result
  shows, on the task that owns the bus. A check that restarts first (Wi-Fi
  not proven off, or TLS kept part of its memory) leaves the clock alone. It reads the chip first and writes
  only if the chip is invalid (OSF set, implausible or century-marked fields)
  or more than 2 s away. A chip that does not answer is not written.
- The write marks the time unfinished first: month 0 with the new year,
  then seconds to date, then the month. One readback after all three must
  hold 24-hour mode and the written time (at most a second later). Only then, if OSF was set, the status register is written
  with OSF clear, ones in A1F and A2F (which leaves them as they are; they
  can only be written 0) and its other bits as read. A second readback must
  show OSF clear and the same time.
- A failure changes nothing more: no Date, an untrusted date, an unread chip,
  a refused write, a readback mismatch or an OSF that stays set. A write that
  stops partway leaves month 0, so Home shows `--:--` and statistics have no
  dates until the next check sets the chip; the chip changes the month only
  at a month's end. A write refused before its first byte leaves the chip as
  it was. OSF stays set until the whole time reads back. No retries follow.

The system bar shows the new time on the result page, Home at its next boot.
DEV USB prints one line per check, `CLOCK reason=network result=<no-date|
untrusted|unread|close|written|not-written|mismatch|osf-kept> utc=<unix>
valid=<0|1> drift=<chip minus network, s> osf=<-1|0|1>`; production prints
none. No RAM is held outside a check.

Memory (flash record 1XX, number assigned at merge; measured clean at
`4b8ab919`): flash 3,308,096 → 3,311,280 B (+3,184); resident SRAM 125,312,
lent range 22,528 and RTC 300 B unchanged; no heap. No selected stack ceiling
changes: `checkUpdateOnline` (not selected) grows 448 → 464 B, and the new
`setClockFromNetwork` (112 B) and `setNetworkClock` (96 B) frames run on
update mode's setup task after Wi-Fi is off.

Tests: `tests/network_clock_test.py` runs the parser, bounds, encoding and the
actual setter against a register-level chip with its faults (absent, refused
writes, a write that stops after each byte, stuck fields, a stuck OSF, an
alarm that fires during the check, a time that changes while OSF clears,
12-hour and century-marked values); each control removes one rule and must
fail. `tests/wifi_setup_test.py` runs the actual update-mode wiring: the
setter gets the first verified response's Date only after Wi-Fi is off, no
time from a response that failed to open, and none when the first verified
response had no Date.
Hardware: one Check on the bench, with the chip's time and OSF before and
after.
