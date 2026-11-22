# Home clock time zone (NEWT-30)

The DS3231 keeps UTC. Newt stores one named zone and Home shows local time,
24-hour by default; NEWT-47 adds a 12-hour choice
([074](074-clock-format.md)). There is no Set Time screen. Since #261,
Update's Check sets the chip, in UTC, from GitHub's Date
([066](066-read-only-home-clock.md#setting-the-clock-261)); the zone only
changes how Home shows it. The phase-1 `--:--` rules are unchanged: the
zone converts only a reading Home would already show.

The zone is set on Home's Settings page ([071](071-device-settings.md)).
NEWT-30 first placed it as a row in Reading settings; NEWT-32 moved it and
restored that dialog. Select on the zone row opens a list of the zones, and
Select in the list commits the focused one. The product default is UTC.

`src/time_zone.cpp` is the whole table: a short label, a telemetry id, the
standard offset and one of five DST rules in POSIX TZ `Mm.w.d/h` form (US, UK,
Central Europe, Eastern Europe, Australia). DST is always one hour.
`localClockTime()` is a pure function of the UTC reading and the zone, shared
by host preview and firmware. It never uses newlib `setenv`/`tzset`. The UTC
year selects the transitions; every listed change is months away from New
Year, so the local year names the same ones. Zones are append-only durable
ids. Arizona and the fixed-offset zones have no DST.

## Durable choice

The zone is an acknowledged persistent preference with its own SD record,
`newt_zone`, which uses the same verified A/B `DurableRecordStore` as the Home
theme and selected book. `NWTZ` version 1 is 20 bytes: magic, version,
sequence, zone and FNV checksum. It is independent of progress, resume intent
and reader settings. Production routes `newt_zone` to SD with the other
reading state.

- Boot restores it before the boot clock read and the first paint.
  `beginHomeClock` (`firmware/time_zone_preference.h`) restores, then reads
  the clock; `setup()` paints afterwards. The record is the only authority for
  every reset class. There is no RTC copy, so no
  stale RTC value can override it, and RTC use is unchanged.
- A missing, damaged or unknown record, or an unreadable card, gives UTC.
  Nothing repairs or rewrites the record at boot.
- A pick in the zone list always commits, even if it equals the shown zone:
  after an unreadable boot or an unconfirmed commit, the card may hold another
  one. Choosing UTC writes an explicit UTC record, so an older zone cannot
  return.
- The previous zone stays live until the write is verified. A lost
  acknowledgement is reconciled by one read-only load. Otherwise Settings
  keeps the old zone and says it could not confirm the save. The commit is
  unconfirmed, not unsaved: a reboot may restore the complete old or new zone.
- Back from the list, or leaving Settings, writes nothing. If the card was unreadable at
  boot, Home shows UTC and the card's stored zone returns on the next boot.

A changed zone shows on the next Home paint. Home does not repaint by itself.

## Telemetry and tests

DEV USB CLOCK lines replace phase 1's `text` field with `zone=<id>
format=<24h|12h> local=HH:MM`. `local` is the time Home shows, before its
format is applied, or `--:--` when the reading is invalid; the existing `raw` field keeps the chip's UTC time. Production emits
no clock telemetry.

`homeClock.zone` in `firmware/home_clock.h` is the firmware's published zone,
held in existing padding. Restore and verified commits write it; the zone
list's focus is a zone id in `AppState` padding.
`readHomeClock` and `plausibleClock` keep their measured signatures and
frames; the conversion runs in a separate `localClockText`.

- `tests/time_zone_test.cpp` checks a conversion table: DST start and end
  minute edges for US Central, Central Europe and Australia Eastern, Arizona
  against Mountain, UTC, and day, month, year, leap-day and 2100 rollover.
- `tests/time_zone_firmware_test.py` compiles the actual production boot
  clock start and Settings zone list statements from `firmware/main.cpp` with an
  in-memory record and the real headers. It checks that the boot clock read
  sees the restored zone, boot with every global discarded, explicit UTC over an
  older zone, missing/short/corrupt records, a checksummed record naming an
  unknown zone, unavailable SD, rejected writes, landed writes that cannot be
  verified (a reboot restores the new value), choosing the shown zone after
  an unreadable boot or an unconfirmed commit, lost acknowledgements, and
  that Back and browsing write nothing. Controls that remove, `#if 0` or
  comment out the restore, move it after the boot read, start the clock
  without it, publish a pick before its commit, save on Back, or drop the
  zone list dispatch must fail.
- `tests/home_clock_test.py` renders each Home theme with a local time and
  fails if the conversion is removed.

## Device checks (pending)

These are host results only. The Lead must:

1. Set US Central, return Home, and compare Home and the CLOCK `raw` (UTC)
   and `local` fields with real time.
2. Check that the Settings rows, zone list and notes do not clip.
3. Confirm the zone survives USB sleep/wake, unplugged battery sleep/wake and
   cold power-on, as separate checks, recording each reset reason.
4. Choose UTC again, cold boot, and confirm UTC stays.
