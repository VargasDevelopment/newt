# Newt: notes for coding agents

Newt is offline reading firmware for the Xteink X3 e-paper reader, written in
Rust on the pinned official FreeInk SDK. These rules protect readers' books,
settings and devices. Follow them even when a change looks too small to need
them.

## Start here

- Words: [CONTEXT.md](CONTEXT.md) holds the project's vocabulary. Use its
  terms in code, tests and copy.
- Map:
  - `rust/firmware`: the X3 image and its wiring. Native code under it only
    supplies the SDK and hardware ports.
  - `rust/salamander`: Salamander, the device layer (board, input, SD, NVS,
    heap) and the device's facts, such as its screen size. New natives go
    here; `rust-health` counts the ones still outside.
  - `rust/core`: shared logic with no allocation (books, durable records,
    updates, the slot installer).
  - `rust/system-ui`, `rust/stage-home`, `rust/library-current-ui`,
    `rust/reader-ui`, `rust/tiny-app`, `rust/ui-canvas`: screens, their state
    and drawing.
  - `rust/host/preview`: C fakes that run the real firmware on a computer for
    previews. `rust/tests`: Rust host test harnesses.
  - `tools/check.py`: the gate. `installer/`: the web install page.
- Apps: the built-in apps, with Pocket Pause as the working prototype, lead
  toward apps anyone can make and share. The owner designs that model: its
  package format, runtime, permissions and sharing. Don't build or propose
  your own. Fixes and polish to the existing apps are welcome.
- Larger changes: open an issue first. Say the problem, why it matters and
  your evidence. A new screen, setting, network feature or dependency needs
  the owner's agreement before any code.

## Check your work

| What you changed | Run |
|---|---|
| Anything, while you work | `python3 tools/check.py --suite quick` (or `--suite ui` for screens) |
| Anything, before a pull request | `python3 tools/check.py --profile linux` on Linux, or `python3 tools/check.py` on a Mac |
| Firmware sources | also `python3 tools/firmware-build.py --clean --health-report` |

- The gate prints `PASS` or names what failed. Fix the cause. Never edit an
  expected output, a golden hash, a reviewed frame or a resource ceiling just
  to make it pass. If an expected output is truly wrong, change it in its own
  commit and say why in the pull request.
- Use only the pinned toolchain ([README](README.md#build-from-source)). Don't
  install, upgrade or add dependencies. All Cargo dependencies are local paths.
  Keep tools and caches inside the checkout.
- Tests are focused: one behaviour, its failure cases, and a negative control
  that breaks the code on purpose and proves the test notices
  ([docs/CI.md](docs/CI.md#tiers)). Don't add broad smoke tests.

## Evidence

Host tests, the host preview, a firmware compile and a check on a real X3 are
four separate facts. A passing host test says nothing about the device. In
your pull request, say which of these you have. If you tested on an X3, name
its screen controller (UC8253 or UC8279d) and its power (USB or battery). On
battery, sleep is a full power-off, so it behaves differently from USB.

## Rules that keep Newt correct

1. **One copy of each screen.** The preview runs the real firmware, built for
   the host over C fakes in `rust/host/preview`. Host code owns only the fakes
   and the frame output. Never re-implement screen state, transitions or
   drawing outside the firmware; `tests/preview_owners_test.py` fails if you do.
2. **No special books.** A fix must work for books in general. Never
   special-case a book by title, file name, digest or any other identity. Test
   with several kinds of books and failures. Never put a copyrighted book, or
   its text, in a fixture, log or issue; use
   `tests/fixtures/the-small-lantern.epub` (CC0) or describe the structure.
3. **Reading works offline.** No feature may make reading need a network or a
   cloud service.
4. **Saved choices survive power loss.** A setting the reader chose must come
   back after a cold boot with all RAM lost.
   - Its authority is its durable record: the SD record, or for the four
     device-settings records in NVS (`newt_prefs`, `newt_pause`, `newt_apps`
     and the built-in sample's `newt_reader`), their A/B NVS record. RTC
     memory may only be a checked warm copy.
   - Commit and read back a choice before showing it as saved. Keep the old
     choice in use until then. Save an explicit "off" too, so an older "on"
     cannot come back.
   - A missing or corrupt record means the product default.
   - On SD, a read-back can return bytes that were never synced. If you
     cannot confirm a save, report it as unconfirmed, never as unsaved.
5. **Test power changes from a cold start.** A change to sleep, wake or
   startup needs host tests for:
   - a cold boot with all RTC and global state gone;
   - a saved choice overriding a stale RTC copy;
   - an explicit fallback choice;
   - a missing or corrupt record, and a failed save.

   Drive the real startup code, and include a control that fails when the
   durable restore is removed.
6. **Memory floors don't move.** Minimum free heap and stack margins protect
   stability. A pull request never lowers them. Flash, RAM or stack ceilings
   may grow only with a measured before/after record and a reason
   ([docs/CODE_HEALTH.md](docs/CODE_HEALTH.md#memory-policy)). A feature that
   is off costs the reader nothing.
7. **Keep provenance.** Code or data from elsewhere keeps its license and
   source in [docs/NOTICES.txt](docs/NOTICES.txt) and
   [docs/provenance.md](docs/provenance.md).

## Pull requests

- One change per pull request, with its tests.
- Commits are [Conventional Commits](https://www.conventionalcommits.org/)
  (`fix(reader): keep the chapter after a wake`). The type sets the version
  bump, and the gate rejects other subjects.
- The body says what changed, why, the gate result, and your evidence (above).
  Write `Closes #N` for the issue it fixes.
- Comments explain why, briefly. Keep them true when the code changes.
- Don't change release tooling, workflow permissions, tags or the version.
  Only the owner merges and releases.
