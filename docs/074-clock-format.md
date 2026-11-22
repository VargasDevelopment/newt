# Clock format: 12-hour or 24-hour (NEWT-47)

Settings has a **Time format** row between Time zone and Diagnostics. Select
on it switches between 24-hour and 12-hour and saves the new format at once.
The product default is 24-hour, so nothing changes until someone chooses.

Every place Newt shows the time uses the published format: the Home clock in
all three Home themes (Frosted, Stage and Stage quiet) and the Home clock line
on the Settings page. No other screen shows the time. The reader chrome has
no clock, and Sleep draws Home without a clock reading, so its frosted
backdrop keeps `--:--` in both formats.

## Format

- 24-hour: `HH:MM` as before (`09:41`, `21:41`).
- 12-hour: hours without a leading zero, then AM or PM (`9:41 AM`,
  `9:41 PM`). Midnight is `12:xx AM` and noon `12:xx PM`.
- An invalid reading still shows `--:--`.

The marker fits everywhere it appears. Home draws the clock in R24, centred
in a 112 px box. The widest 12-hour time is `10:44 AM` at 106.2 px; `12:59
PM` is 102.5 px. At its widest, `Offline · 100%`, the battery text still
starts 46.7 px to the right of the clock. Settings has 428 px.

`include/newt/clock_format.h` holds the format and `formatClockText`, a pure
function shared by host preview and firmware. The clock source still reads
`HH:MM` in the zone (`homeClock.text`, unchanged in size and signature); the
renderers apply the format when they draw it. Home gets the format through a
`HomeData` flag (`kHomeTwelveHour`), which firmware sets from the published
format at each Home paint. A changed flag repaints Home, like any other drawn
flag. The format string uses a buffer that each renderer already has, so no
renderer frame grows for it.

## Durable choice

The format is an acknowledged persistent preference with its own SD record,
`newt_clock`, beside the zone's `newt_zone` ([068](068-home-clock-time-zone.md)).
It uses the same verified A/B `DurableRecordStore` and the same SD routing
(`kReadingStateNamespaces`). `NWTC` version 1 is 20 bytes: magic, version,
sequence, format (0 = 24-hour, 1 = 12-hour) and FNV checksum. It is
independent of the zone, progress, resume intent and reader settings.

- `beginHomeClock` restores the format, after the zone and before the boot
  clock read and first paint. The record is the only authority for every
  reset class. There is no RTC copy, so no stale RTC or warm value can
  override it, and RTC use is unchanged.
- A missing, damaged or unknown record, or an unreadable card, gives 24-hour.
  Nothing repairs or rewrites the record at boot.
- Each Select writes the opposite of the shown format. Choosing 24-hour writes
  an explicit 24-hour record, so an older 12-hour choice cannot return. After
  an unreadable boot the card may already hold the new value; the write still
  commits it explicitly.
- The previous format stays live until the write is verified. A lost
  acknowledgement is reconciled by one read-only load. Otherwise Settings
  keeps the old format and says "Could not confirm the save. Home keeps
  \<format\>; after a restart it may change." The commit is unconfirmed, not
  unsaved: a reboot may restore the complete old or new format.

The published format is one bit in `HomeClockSource`, sharing the byte of its
paint latch, so it adds no RAM. `restoreClockFormat` and `commitClockFormat`
are in `firmware/clock_format_preference.h`; `dispatchClockFormat` in
`firmware/device_settings.h` owns the row's Select, as `dispatchZoneList`
owns the zone list.

## Settings notes

Each row shows its own note. On Time format the default note reads "12-hour:
9:41 PM. 24-hour: 21:41." A verified save reads "Saved. Home shows this
time.", and the page's clock line already shows the new format. An
unconfirmed save stays on the Time format row through focus moves until a
verified save or leaving Settings. One notice is kept at a time: a later save
on the other row replaces it.

## Telemetry

DEV USB CLOCK lines add `format=24h` or `format=12h` before `local`. `local`
stays the `HH:MM` that Home formats.

## Tests

- `tests/clock_format_test.cpp`: midnight, noon and hour edges; every minute
  of the day against an independent 12-hour oracle; every UTC hour in every
  zone in winter and summer; invalid text passes through; and the widest
  12-hour time fits the 112 px Home box.
- `tests/clock_format_firmware_test.py` compiles the actual boot clock start
  and the Settings zone list and Time format statements from
  `firmware/main.cpp` with an in-memory record. It checks first boot,
  a 12-hour commit, boot with every global discarded, explicit 24-hour over an
  older 12-hour record, a durable record beating a stale warm value in both
  directions, missing/short/corrupt records, an unknown format, unavailable
  SD, rejected writes, landed but unverified writes (a reboot restores the
  new value), Select after an unreadable boot, lost acknowledgements, and that
  moves, Back, other rows and an open zone list never write. Controls that
  remove, `#if 0` or comment out the restore, move it after the boot read,
  publish before the commit, or drop or short-circuit the Time format
  dispatch must fail.
- `tests/home_clock_test.py` paints each Home theme through the actual
  `paintHomeScreen` in 12-hour form (`12:59 PM`, `10:44 AM`, `12:05 AM`,
  `9:41 PM`) and checks that only the clock region changes. A control that
  removes the format publication from `paintHomeScreen` must fail.
- `tests/list_overlay_test.cpp` checks every clock text in both formats
  leaves the zone list's frost unchanged outside the card.
- `tests/app_test.cpp` covers the three rows' navigation and notices.
- Reviewed frames: `device` in `artifacts/library-ui-review.json`. Gate
  previews add `device-settings-format`, `home-clock-24h` and
  `home-clock-12h`. `tools/preview.py --clock HH:MM` shows invented clock text
  in Home and Settings previews.

## Device checks (pending)

These are host results only. The Lead must:

1. Home → Settings, Previous to Time format, Select. Confirm the page clock
   and the Home clock show 12-hour with AM/PM, and that the CLOCK line reads
   `format=12h` with `local` matching real time in the zone.
2. Check that neither the Home clock nor the battery text clips or overlaps,
   in each Home theme, ideally near 10:xx or 12:xx.
3. Confirm 12-hour survives USB sleep/wake, unplugged battery sleep/wake and
   cold power-on, as separate checks, recording each reset reason.
4. Select again for 24-hour, cold boot, and confirm 24-hour stays.
