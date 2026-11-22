# About: the exact build and What's new (#143)

About's "Version: 0.1.0-alpha.0" only moves on a Commitizen release
([RELEASING](RELEASING.md)). Owner updates are unreleased main builds, so the
version alone cannot tell the owner which build the X3 runs. About now shows
the exact build, and a What's new page says what changed. Version bumps are
not part of this change.

## About

Settings → Diagnostics → Select opens About, a system page
([078](078-system-ui-consistency.md)), and so does Settings' last row, About
(NEWT-81). It has two rows:

- **Version: 0.1.0-alpha.0**, focused when About opens. Under it, not a row,
  the build line: **Build c83f64b673ec · Oct 27, 2026**.
- **What's new**. Select opens the What's new page.

The FreeInk note stays below the rows.

### The build line

The ID is the DEV build ID that HELLO reports: the 12-hex Git revision
`tools/firmware-build.py` captures before compiling. The date is that
revision's commit date (`git show -s --format=%cs`), from the same capture,
never the compile clock. Both reach the firmware as generated build flags
(`NEWT_DEV_BUILD_ID`, `NEWT_DEV_BUILD_DATE`), which `require_build_scope`
accepts only in their exact formats.

- A dirty build shows **Build c83f64b673ec-dirty**, with no date: its source
  is not that commit.
- A build without an exact identity (host builds, plain compiles) shows no
  build line rather than a wrong one. `formatBuildLine` refuses anything that
  is not 12 lowercase hex digits (optionally `-dirty`) and a valid
  `YYYY-MM-DD`.

### Focus and the first newt

Down moves focus to What's new and Up back to Version; both clamp. Select on
What's new opens its page, and Back there returns to About with focus on What's
new. Back on About returns to where it opened: Diagnostics, or Settings on
its About row.

The Version row keeps the five-press first newt
([design requirements](design-requirements.md)): five consecutive Selects on
Version open it, and Back returns to About on Version. One Select on Version
does nothing visible. Any other press (a focus move, Read, a What's new visit)
restarts the count, so a half count never completes later.

The rail follows 078: on Version, Back and Down (Select only counts toward the
first newt, so it has no button); on What's new, Back, Open and Up. The What's
new page shows Back only.

## What's new

The notes live in `rust/device-ui/whats_new_notes.h`, the one file the Lead edits
before each owner update ([RELEASING](RELEASING.md)). Each update has its date
as shown ("Oct 27, 2026") and up to six changes in plain words, newest first.
The page draws each date in bold and its changes as wrapped bullets.

`drawWhatsNew` measures each update before drawing it. An update that would
not fit whole is left off with every older one, so no line is ever clipped.
`tests/about_test.cpp` requires the committed notes to fit, so the gate fails
before an update is silently dropped; then drop the oldest update.

## Memory

- `AppState` stays 24 bytes: a one-byte `AboutView` replaces the `aboutOpen`
  flag. RTC is unchanged.
- The notes and the build ID and date are constant text in flash. The page
  draws them only while it is painted; nothing is held in RAM while reading,
  and there is no new heap.
- Flash record 69 measured the one build at 2,395,712 → 2,398,048 B
  (+2,336 B: the notes, build-line formatting, the What's new page and
  About's rows). SRAM stays 133,440 B and RTC 300 B. Every selected stack
  ceiling is unchanged, so there is no protected record; the new frames
  (`renderAbout` 128 B, `drawWhatsNew` 96 B, `formatBuildLine` 112 B) run
  only while About or What's new is painted.

## Tests and evidence

- `tests/about_test.cpp`: the build line from a fixed identity, none without
  one, dirty marked, the widest line fits; focus moves, What's new and Back;
  five Selects and Back; a partial count across a focus move; the committed
  notes fit and older updates drop off whole.
- `tests/build_scope_test.py`: the generated date flag is accepted only once
  and only as `YYYY-MM-DD`.
- `tests/system_ui_rule_test.py`: every shown About and What's new button does
  something, and the hidden ones do nothing.
- Host previews (host pixels, not hardware): `artifacts/device-about.png`,
  `device-about-no-build.png`, `device-about-whats-new-row.png` and
  `device-whats-new.png` (the build identity is the one compiled into the
  preview firmware; `--build-id`/`--build-date` are not ported, issue #357).
