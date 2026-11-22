# System UI consistency (NEWT-42)

Owner notes (2026-10-24): Library has no clock. Settings opens on
Diagnostics, not its top row. Settings and Apps lack Home's buttons. The
newt / time / Offline / battery bar shows on some screens and not others.
The reader stays as it is.

## The rule

Every non-reader screen is a **system page** (`include/newt/system_page.h`).
Home is the reference, not a target: its look is unchanged (Stage redesign:
issue #29).

1. **Bar.** Rows 0-63 are Home's bar: the `newt` wordmark, the clock and
   `Offline · N%` (`--%` when the battery is unknown). The clock is HH:MM in
   the published zone and format, or `--:--` when it cannot be read. It follows
   Home's clock rule ([066](066-read-only-home-clock.md)): it is read when
   the page is composed and never ticked, so it may go stale while the screen
   waits. No screen adds a paint or refresh for the clock.
2. **Selector.** A list opens on its top row. A screen restores another
   position only for a reason stated in code:
   - Back from a child page returns to the row that opened it.
   - Library keeps your place on the shelf while it is open.
   - The time zone list opens on the published zone, the current value.
3. **Controls.** Controls are Home's four-button rail at Home's place (rows
   748+, cells centred at 95/201/331/437, in the Back, Confirm, Up, Down
   hardware order). Each label names what that press does on this screen, in
   at most 92 px of 28 px Inter (e.g. Back, Open, Edit, Save, Switch, Cancel,
   Other). A press that does nothing has no button. Up and Down hide at the
   ends of a list that stops there. List rows use Home's focus style (arrow,
   bold, rule) at one 43 px pitch, so a new row (e.g. NEWT-39's) needs only
   its label.

Content draws only inside the page's card. Text hints such as
`Back: Home` are replaced by the rail.

### Closing the class

`SystemPage` draws the shell paper, bar, card and rail. Its constructor takes
the bar and the buttons, so a page cannot be built without them. A screen that
is not a system page must be named in `SystemPageException`, with its reason.
`systemPageException()` switches over every `Screen` with no default, so a new
screen does not compile (`-Werror`) until it is placed. In firmware, only a
system page's own renderer reads the clock (`readSystemBar`), inside its own
call, so the bar never lives in the paint's frame.

`tests/system_ui_rule_test.py` checks the rule on the host previews. In every
state the shell and app previews reach (list ends, the empty, unavailable and
loading Library, Pocket Pause with zero, one or more prompts on, and generic
apps with one or three choices), it presses each shown button and requires a
change of the frame or app state. The first open's page is a static preview,
so its Cancel is covered by the firmware open tests instead.

Named exceptions: **Home** (the reference), **Reader** (the book owns its page,
menus and working frames, in any orientation), **Sleep** (the sleep image
covers the panel) and **FirstNewt** (the original drawing, kept as made).

## Inventory

| Screen | Today (base 4606a896) | Rule | Change |
|---|---|---|---|
| Home | Bar, cards, rail | Reference (exception) | None |
| Library: shelf, empty, unavailable, loading | Bar with `--:--`, rail | System page; keeps shelf place | Bar shows the clock; drawn by `SystemPage` |
| Settings | Wordmark only; "Home clock" block; `Back: Home` / `Select: open` text; opens on Diagnostics | System page; top row | Bar, rail (Back, Edit/Switch/Open, Up, Down); opens on Time zone; the Home clock block is gone (the bar shows it) |
| Time zone list | Frosted Settings; in-card `Back: cancel` / `Select: choose` | List over the frosted page; opens on the published zone | Crisp bar; rail Cancel, Save, Up, Down (Up hides at the top, Down at the bottom); shared row style |
| Diagnostics | Wordmark only; text hints | System page | Bar; rail Back, About |
| About | Wordmark only; text hints | System page; top row | Bar; Version and What's new rows ([081](081-about-build-whats-new.md)); rail Back, Down on Version (its Select only counts toward the first newt); Back, Open, Up on What's new |
| What's new | New ([081](081-about-build-whats-new.md)) | System page | Bar; rail Back, which returns to About's What's new row |
| First newt | Original drawing | Exception | None |
| Reading statistics | Wordmark only; text hint | System page | Bar; rail Back |
| Apps: Pocket Pause | White page; text hints | System page | Bar, card; rail Back, Edit, Other, Other (Other only when another enabled prompt can be picked) |
| Apps: Pocket Pause settings | White page; text hints | System page; top row | Bar, card; rail Cancel, Save/Switch, Up, Down |
| Apps: other tiny apps | White page; `Prev/Next`, `1 / 3`, `Select` text | System page | Bar, card; rail Back, Select, Up, Down; "Choice N of M" in the card |
| App unavailable notice | White page; text hint | System page | Bar, card; rail Back |
| Book opening (first open) | Bar with `--:--`; Cancel rail | System page | Bar shows the clock |
| Working frames (opening, saving, applying, loading) | Reader orientation | Reader (exception) | None |
| Sleep | Sleep image | Exception | None |
| Reader | Book page and menus | Exception | None |

System pages stay portrait whatever the reader's orientation.

## Cost

Paint and refresh behaviour is unchanged: each screen keeps its paints and
refresh modes, and no full refresh is added. The time zone list still moves by
repainting only its card; the bar keeps the time of its last full paint, as a
Home focus move does. A move that enters or leaves an end of the list also
redraws the rail and updates the whole panel Fast instead of the card's
window ([071](071-device-settings.md)). Firmware reads the clock once for each system page
composition, as Settings already did, so Library, Apps and first-open paints
now add that one register read (10 ms Wire timeout, no retries). Memory and
flash are in `docs/CODE_HEALTH.md` (flash record 62).

DEV USB: the CLOCK line's `reason` is `system` for these reads (it was
`settings`, [071](071-device-settings.md)). The line format is unchanged.

## Evidence limits

These are host previews and host tests only. Hardware paint timing and
pixels are separate checks.
