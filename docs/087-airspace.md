# 087: Airspace (#166, slices 1 and 2)

**Status:** slice 2 (host only). The data model, ranking and screens run in
host tests and the shell preview from invented fixtures. Slice 2 added the
spotlight, the silhouettes, the vertical rate, raw category, route fields and
the landscape setting. Nothing is wired into the firmware: Home's Apps still
opens Pocket Pause, and the firmware's flash, RAM, RTC and stacks are
unchanged. Issue:
[#166](https://github.com/VargasDevelopment/newt/issues/166).

Airspace lists the aircraft near a point the owner sets, ranked by how likely
each is to pass overhead.

## Model

`include/newt/airspace.h`. One `AirspaceSnapshot` is fixed size with no heap:
12 aircraft of 128 bytes each, 1,548 bytes in all (a `static_assert` in
`tests/airspace_test.cpp` holds the bound). Twelve fills two list pages and
keeps the radar readable; within 25 NM of a busy airport, rarely more than
that are in the air near enough to matter. A parser keeps the 12 it ranks
best.

Each `Aircraft` holds fixed-length text (ICAO address, callsign, registration,
ICAO type code, operator name, type name, squawk, the raw ADS-B emitter
category, and the route's origin and destination codes) and numbers (position
in micro-degrees, barometric altitude in ft, vertical rate in ft/min, ground
speed in kt, track in degrees, position age in s). The vertical rate is the
barometric rate, else the geometric one. The route fields are filled only by a
later, memory-only route lookup (adsb.lol's routeset), and stay empty when the
service marks the route implausible; a route shows only when both are set. A
`fields` bit marks each reported number, and `Ground` marks an aircraft
reported on the ground. An empty string or a missing bit is missing and shows
"Not reported"; nothing is ever filled in. `aircraftClass` derives the
silhouette class from the raw category: A1 light, A2 small, A3 and A4 large,
A5 heavy, A6 high performance, A7 helicopter, B1 glider, B2 lighter than air,
and Unknown for anything else or none. It is never guessed from the type code.
`alert` (7700, 7600, 7500) and `military` add a tag.

The snapshot carries one state (Live, Stale, Unavailable, Offline), whether a
fetch ever succeeded, and the data's age by the device's uptime.

## Ranking

`rankAirspace` (pure, in `src/airspace.cpp`):

1. Leave out aircraft on the ground and those with no position.
2. Predict the closest point of approach within 5 minutes, as if the aircraft
   flies straight on at its reported speed and track. The great-circle
   distance and bearing back to the point are taken in the aircraft's own
   frame, so the track keeps its meaning near a pole and across the date line,
   out to 250 NM, the providers' largest query. A missing speed or track counts
   as not moving.
3. Rank by the slant distance at that point (horizontal distance and reported
   altitude; a missing altitude counts as level with the point).
4. Positions older than 60 s rank after every newer one.
5. Ties break by ICAO address, so one snapshot always ranks the same.

Every page labels the result as a prediction: the spotlight's "If it flies
straight on", the detail's "Predicted, if it flies straight on: ...". The
words are plain: "Overhead in about 1 min" (within 93 m, about 300 ft),
"Closest in about 51 s, 1.7 NM away", "Closest now, 4.0 NM away", "Still
closing, 6.2 NM away in 5 min", or "Position 2 min old, listed last".

## Screens

Shared C++ (`src/airspace_render.cpp`, `src/airspace_spotlight.cpp`,
`src/airspace_draw.cpp`, `src/airspace_view.cpp`), in Newt's SystemPage style.
The Rust firmware does not ship Airspace, so the gate's Rust previews no
longer render it; the C++ preview (`tools/host.cpp --airspace`) still does
until the C++ sources are removed.

- **Spotlight** (the default): one ranked aircraft at a time, the best first.
  A large silhouette; the type name, else the ICAO type code, else "Type not
  reported"; the operator and registration; the route with an arrow, only
  when looked up; a grid of altitude ("On the ground" when so), speed,
  heading with its arrow, vertical rate ("Climbing", "Descending" or
  "Level"), distance and callsign, each "Not reported" when missing; the
  prediction with any alert or Military tag; a radar inset (north up, the
  point at the centre, the radius as the outer ring, every ranked aircraft a
  dot, the shown one marked with its track); "1 of 11 · 10 NM" on the
  title's line; and the status ("Live · updated 40 s ago"). The grid's first
  column holds a four-digit climb, so the columns stay put between aircraft;
  a wider fact (a five-digit climb) pushes the columns after it right rather
  than being cut short, and when the last would not fit, the gaps between
  the columns shrink instead. Military goes under an alert when one line cannot
  hold both. Prev and Next step through the aircraft, Select opens the
  details, Back (labelled List) shows the list.
- **List** (the alternate): the range ("1-5 of 11 · 10 NM") on the title's
  line, the status on its own line, **Check now**, the ranked
  aircraft five a page (a small silhouette, the name, distance and one tag;
  the type and altitude; the predicted pass in plain words), then **Airspace
  settings** and "N on the ground, not listed." Select on an aircraft
  spotlights it. The slice 1 radar-above-list layout is gone.
- **States**: Live, Stale, Unavailable (with or without older data), Offline,
  an empty sky, a check in progress (only Back shows), no point set, one
  aircraft only, and an aircraft with most fields missing. With no aircraft
  to show, the spotlight is the list.
- **Detail**: every field in two columns (who: callsign, operator, type,
  type code, registration, route, distance, address; how: category,
  altitude, vertical rate, speed, heading, squawk, position age), "Not
  reported" when missing, then the prediction, always whole: when the tags
  take a line of their own, the fields close up to leave its room. The tags
  share the status line when it holds both, else take the line under it.
  Back returns to the spotlight.
- **Airspace settings**: latitude and longitude (shown rounded to 2
  decimals), the radius (Select cycles 5, 10 and 25 NM) and the orientation
  (Select switches Portrait and Landscape). A coordinate is typed on
  FreeInk's keyboard (`src/key_entry.cpp`, the one Wi-Fi setup uses), opening
  on its digits; only text that can still become a coordinate types.
- **Apps**: Pocket Pause and Airspace (preview only), always portrait.

### Landscape

Landscape turns the panel as the reader's landscape does (`LandscapeCCW`: a
792 x 528 canvas in the same frame bytes). Each page has its own landscape
layout on a white card, with the title, clock and battery at its top and the
button rail on the right, each button level with the physical button it
names. The buttons keep their actions (no remap), so on the turned panel Next
sits above Prev. The spotlight puts the aircraft's details and silhouette on
the left and a larger radar with the prediction on the right, then the data's
state and its age, each on its own line; the range follows the title; the
list's rows put the type on the first line; the detail's columns sit side by
side.

Text that does not fit its box ends in "…", which hides what it cuts, and a
paragraph without room is left out, which leaves no mark at all.
`tests/airspace_render_test.cpp` draws every preview fixture in both
orientations on every page and row, again with every aircraft at its widest
tags, at its widest values (a five-digit climb or descent and a two-line
prediction), and with both. It fails on any "…", on any paragraph UiCanvas
could not draw whole (a host-only count, `NEWT_HOST_UI_PROBE`), or on any
ink past the card.

### Silhouettes

`src/airspace_silhouettes.cpp`: one original side view per class (nine),
drawn for Newt as integer outlines, quadratic curves, bars and discs in one
5:2 box (`assets/airspace/PROVENANCE.md`). The airliners share one stance,
seen a little from below: the near wing swept back under a slim root
fairing, and engines on short pylons. A drawing may also cut parts out of
its outline before its bars go on (the light aircraft's windows, which show
its strut). `drawSilhouette` fills a drawing at whatever width it is asked
for (64 px in a list row, 300 and 320 px in the spotlight) by an even-odd
scanline fill at pixel centres in exact integer units, then a 2 x 2 opening:
a pixel inks only inside a whole 2 x 2 block, so no size has one-pixel
slivers. Below 128 px a class may draw a simpler small form: the light
aircraft loses its windows, and the turboprop draws its wing and nacelle as
one block with no propeller. Tests check every class at every used size and
either side of 128 px.

Why vectors: compiled alone for the X3 (riscv32, `-Os`, one object, not
linked), the drawings are 2.8 KB of constant data and the rasterizer 5.1 KB
of code (part of it `std::sort`, which the firmware may already carry),
about 7.9 KB in all, and any new size is free. Stored 1-bit bitmaps
would be 208 bytes a class at 64 px, 4.5 KB at 300 px and 5.1 KB at 320 px:
about 88 KB for nine classes at the three sizes, and another set for each
new size. Run-length coding would shrink that but add a decoder. These are
estimates; the measured cost lands with the firmware slice's record.

The preview copy is plain: "Check now" (a rail button fits "Check", not
"Refresh"), "Checking...", "No aircraft in the air within 10 NM.", "No
Wi-Fi. Save a network in Settings, Wi-Fi, then check again."

## Fixture only

All aircraft, registrations and addresses are invented, around a point on
Seattle-Tacoma airport's own grounds (`tools/airspace_preview.h`). Routes
are invented too. Check now never fetches, and a saved setting, including the
orientation, lives only in the preview's memory.

`src/key_entry.cpp` is the keyboard's shared owner and needs its own
code-health adapter entry for `components/keyboard/keyboard.h`. Update mode
keeps its copy until a firmware slice moves it, with a measured flash record;
then one entry remains.

## Follow-ups

- The provider, the streamed parser and its failure fixtures (#166 phase 0
  and 2), with the provider credit its terms require and routes only when
  reported.
- A durable settings record for the point, radius and orientation (AGENTS.md's
  durable preference rules), and rounding the point before it is sent.
- The route lookup (adsb.lol's routeset), memory only, dropping routes the
  service marks implausible.
- Firmware wiring: the real Apps page (#138), Airspace opened from it, with
  measured flash and memory records; update mode moved onto `key_entry`.
- DEV FRAME privacy: pages that show the point must not leave the device
  unmasked.
- Watch mode (Airspace refreshing on its own while shown) needs the
  explicit memory handoff (#165) first, so Wi-Fi and the reader never hold
  the same RAM.
- Per-type art: silhouettes for common type codes, drawn the same way, only
  after the class art proves itself on the device; the class stays the
  fallback.
- Maybe: keep an emergency at the top regardless of rank.
