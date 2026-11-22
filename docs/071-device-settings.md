# Device Settings (NEWT-32)

The third Home section is named **Settings** (formerly Your X3). It opens the
Settings page, a portrait system page like the other device pages. The name
fits the Frosted, Stage and Stage quiet rows and the Stage dock at their
existing type sizes. Frosted Home no longer draws a route note beside it: the
old "(Settings)" note would repeat the title.

The Settings page shows the Home clock, then these rows:

- **Time zone: \<zone\>**. Select opens the zone list (below). While focus is
  on this row the note reads "Whole device. Sets the Home clock.", or the
  outcome of the last pick.
- **Time format: \<24-hour or 12-hour\>** (NEWT-47). Select switches the
  format and saves it ([074](074-clock-format.md)).
- **Sleep screen** (#202). Select opens a page with three On/Off rows, all
  Off by default: Book progress, Last 7 days and When you read. Each row's
  note says what it shows. Select switches the focused row and saves all
  three in one `newt_sleep` record, published only once verified, like the
  time format. An unconfirmed save keeps the shown choices and replaces that
  row's note until a verified save or leaving Settings. What the sleep screen
  reads for them is in [076](076-reading-stats.md) section 10.
- **Diagnostics**. Select opens the existing device details page. Back
  returns to Settings, and Back again returns Home. This row has no note.
- **Reading statistics** (NEWT-40, [076](076-reading-stats.md)).
- **Restart into another firmware** (NEWT-39, NEWT-71). The row names the
  firmware in the other slot ("Restart into CrossInk", or "the other
  firmware" when Newt does not know its name). Select opens one confirm page;
  when the other slot holds no other firmware the row is disabled and says so
  ([077](077-return-to-crossink.md)).

Focus opens on Diagnostics, so from a focused Settings row on Home,
Diagnostics is two presses (Select, Select). About sits below Diagnostics: it
shows the exact build and a What's new row
([081](081-about-build-whats-new.md)), and the five-press first newt on its
Version row is unchanged. There is no Wi-Fi row or Set Time screen.

## Zone list

Select on the Time zone row opens the 15 zones as a list over the Settings
page. Everything behind the list is frosted with the existing Sleep frost
(`frostSleepBackdrop`, monochrome), so only the list is crisp. Focus starts
on the published zone. Next and Previous move it and clamp at both ends
(UTC, Australia Eastern), like the Settings rows. Nine rows show at once, and
the list scrolls to keep focus in view; a thin bar at the right shows the
position.

- **Select** saves the focused zone through the existing `newt_zone` record
  and `commitTimeZone` ([068](068-home-clock-time-zone.md)) and closes the
  list. Picking the zone already shown still commits: after an unreadable boot
  or an unconfirmed commit, the card may hold another zone.
- **Back** closes the list. Nothing is written or published.

A verified commit publishes the zone, and the note reads "Saved. Home shows
this time." An unconfirmed commit keeps the old zone shown and reads "Could
not confirm the save. Home keeps \<old zone\>; after a restart it may
change." That notice stays on the Time zone row through focus moves and
Diagnostics until a verified pick or leaving Settings, because the card may
already hold the new zone.

## Design

The list is one shared component, `ListOverlay`
(`include/newt/list_overlay.h`, `src/list_overlay.cpp`): a title and a fixed
array of labels. `dispatchListOverlay` moves a caller-owned focus and returns
`Moved`, `Clamped`, `Picked`, `Cancelled` or `Unhandled` (Sleep, Read).
`renderListOverlay` frosts a caller-lent backdrop into the frame and draws the
card. Another settings list needs only a title and its labels.

`AppState` holds the open flag and the focused zone id in existing padding
(`sizeof(AppState)` stays 24, and a default state stays all zero). Plain
`dispatch` leaves an open list alone, except that Sleep closes it.
`dispatchZoneList` (`firmware/device_settings.h`) owns the list, because it
holds the published zone: it opens the list, moves focus and commits a pick
before any other dispatch sees the press. The host preview calls the same
function against an in-memory record.

The frost holds no memory while the list is closed. An open list draws the
Settings page into a borrowed scratch frame for one paint and frosts it into
the logical frame. Firmware lends the display's native buffer, as Sleep does;
`toNativeFrame` overwrites it right after. The frost's one-column stack array
is the existing Sleep cost.

Settings reads the clock once per paint through the existing reader, so its
time matches Home. The Device screen already repaints each minute. DEV USB
emits a CLOCK line with `reason=settings` for that read (`reason=system` since
[078](078-system-ui-consistency.md)); the line format is unchanged.

### Moves repaint only the card

Opening the list frosts the whole screen. A move inside it does not: the
device measured about 1.23 s per move at `53f443f`, against 0.70 s for a
Settings row move, because each move re-rendered the page and frosted it
again. `listOverlayCard` gives the card's rectangle, and `renderListOverlay`
with a null backdrop redraws only the card. Its panel clears the whole card
first, so the result equals a full paint.

`loop()` keeps list moves (`zoneListMove`) apart from every other change. A
pass whose only changes are moves calls `paint` with its existing partial flag
(on Home it still means the status only). If `zoneListCardOnly` holds,
`paintZoneListMove` redraws just the card into the logical frame, with no page
render, clock read or frost, and `displayListCard` requests the card's native
window, Fast: x 144, y 40, 504 × 448 (the card is native x 150–640, y 40–487,
widened to whole bytes; [078](078-system-ui-consistency.md) moved its hints to
the rail, so it is shorter). The pinned X3 drivers have no window of their own;
FreeInk falls back to a whole-panel Fast update, where only the card's pixels
differ. A move that enters or leaves an end (UTC, Australia Eastern) also
changes the rail, whose Up or Down shows only where focus can move
([078](078-system-ui-consistency.md)). `paintZoneListMove` reads the rail's
shown buttons from the logical frame, redraws the rail and requests the whole
panel, Fast, with no window: the rail lies at the far native edge, so a window
over it and the card would be most of the panel. Interior moves keep the card
window. The tenth UI update is still the full cleanup of the whole screen
(`UiRefreshPolicy`, unchanged). `paint` and `paintNonHomeScreen` keep their
signatures and selected stack entries; the move path is its own function.

The logical frame holds the frost at the start of such a pass, without any
retained record: firmware paints each change in the pass that makes it, every
other paint while the list is open frosts the page again (opening, the
minute or battery repaint, a pass with any other press, the first paint after
a boot), and no frame lender runs on Settings. The page under the frost only
changes by its clock while the list is open. Every clock text is checked: none
changes a frosted pixel outside the card. The display receipt must also show
a valid portrait binary frame. Otherwise the paint frosts again. A move does
not refresh the frosted clock; the next frosting paint does.

DEV builds report each move in `STATUS refresh_ms`, from render start to the
display's return, as before.

## Tests

- `tests/list_overlay_test.cpp`: clamped navigation, pick, cancel and other
  buttons, and the frosted backdrop around the card, for the zone list and a
  second, three-row list. Every move in both lists redraws only the card,
  changes no pixel outside it and equals the full paint. All 1,440 clock
  texts leave the frost outside the card unchanged.
- `tests/wake_evidence_test.cpp` runs the actual `paint`, `paintNonHomeScreen`,
  consumed-action statements and pass paint from `firmware/main.cpp`. Twenty
  moves leave and enter both ends. Interior moves keep marks outside the card,
  change the native image only inside the requested window and request it; a
  move at an end updates the whole panel Fast with the rail of a full paint;
  every tenth update is the full cleanup. A repaint, a pass that
  also closed the list and a gray receipt frost again. At `53f443f` it fails:
  "a list move changes no pixel outside the card" (and, with that check off,
  "a move requests only the card's native window").
- `tests/app_test.cpp`: press counts for Home → Settings → Diagnostics →
  Back → Settings → Back → Home, that plain dispatch leaves an open list
  alone, and which notices clear.
- `tests/time_zone_firmware_test.py`: the NEWT-30 power-lifecycle matrix
  through the zone list statement in `firmware/main.cpp`: open on the
  published zone, a pick saves through the verified path, Back writes
  nothing, clamping, and unconfirmed commits. Negative controls include a
  pick published before its commit and a Back that saves (see 068).
- Reviewed frames: `home` and `device` (Settings) in
  `artifacts/library-ui-review.json`. Gate previews add `device-settings`,
  `device-settings-list` and `device-settings-saved`.

## Device checks (pending)

Host results only. The Lead must check: Home → Settings, open the zone list,
pick a zone, and the Home clock follows; Back closes the list without a
change; Diagnostics is reachable in two presses; a reset keeps the zone. The
move latency on the panel (target near the 0.70 s row move) and any ghosting
around the card before the tenth-update cleanup are unmeasured.
