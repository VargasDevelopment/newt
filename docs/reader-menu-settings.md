# Decision 004: reading menu and saved reader choices

October 1, 2026. `spike/reader-menu-settings` builds directly on the preserved Frosted geometry checkpoint. It keeps the same real embedded EPUB, portrait system, original provisional bitmap type and monochrome framebuffer. No font imports, grayscale integration, SD/device access, new dependencies or SDK changes.

Confirm while reading opens Return to Reading, Chapters, Reading Settings, Reading Statistics and Home. Chapters are actual bounded EPUB reading-order entries, labeled honestly as Chapter 1/2; this is not an EPUB navigation-document or title extractor. Back dismisses submenus, then the menu. Ordinary reading keeps its existing Back-to-Home action. Physical mapping/spatial hints still need the X3 check.

Overlay entry pauses the existing reading tracker. Returning to the page starts a fresh qualifying session; menu/repaint/sleep time and chapter jumps do not earn forward-page credit. Statistics show this book's active minutes, qualifying forward-page turns and qualified sessions, from existing persisted counters. They are not library history or calendar streaks. A parser/allocation failure requests an unavailable-state repaint; first/last page clamps remain quiet.

Settings expose only reader orientation and slim bar. Changes are a draft until Save. Cancel/Back preserve the current page/settings. Save writes the inactive 24-byte record in `newt_prefs` (`slot0/slot1`), commits and checks exact decoded read-back before applying the settings. The previous slot stays intact; same-value saves avoid writes. Records use explicit version/sequence/flags/reserved bytes and CRC32. Unknown reads, future versions or ambiguous generation order block saving. Confirmed empty storage selects defaults; a valid record can recover beside known corrupt inactive data. A lost acknowledgement after a completed commit may make that value newest after restart, so failure says **Could not verify save. Draft kept.**

A verified choice checkpoints the paused semantic character anchor, releases the old reader workspace and reopens it in the selected geometry. If subsequent book allocation/reflow fails, the saved preference remains confirmed and the book becomes unavailable; it does not claim a successful reading session. Reading-progress persistence uses its separate existing record and can fail independently. Unknown preference storage permits safe session defaults but does not overwrite the unknown record; preference recovery is retried on reboot, not when reopening the book in the same session. Orientation applies only to reading and its overlays; Home remains portrait. Sleep resumes the reading page, never an open overlay or unsaved draft.

The host uses the same storage policy with a process-local memory backend. Home/reopen and simulated sleep/wake reload it within that process; a new preview process starts fresh. Native NVS durability, physical orientation/buttons, sleep and runtime heap are **not hardware tested**. No additional framebuffer or rendering heap is introduced; the reader workspace remains 69,968 bytes on C3 / 70,088 on host. The preference store is 72 bytes on C3; its isolated compile probe reports an 80-byte save stack frame, not a peak call-chain or runtime bound.

```sh
python3 tools/check.py
python3 tools/firmware-build.py --clean
python3 tools/preview.py --screen epub --actions activate
python3 tools/preview.py --screen epub --actions activate,next,activate
python3 tools/preview.py --screen epub --actions activate,next,next,activate
python3 tools/preview.py --screen epub --actions activate,next,next,next,activate
```

[Menu preview](../artifacts/reader-menu.png) · [Chapters](../artifacts/reader-chapters.png) · [Settings](../artifacts/reader-settings.png) · [Statistics](../artifacts/reader-statistics.png). These are exact shared-source host pixels, not photographs or native Frosted rendering. First hardware bring-up remains the preserved 417,072-byte baseline, following [BRINGUP](BRINGUP.md).

Compiled source **`f708ba5f9249d4932b5d44e8c04b0edf975fb43c`**; archived image `.build/reader-menu-f708ba5/firmware.bin`: **644,736 bytes**, SHA-256 `1ce05141ad2e4f320b7ff5866fe47b464b081f16ae6c2d91d554d50da62d85b6`. PlatformIO static RAM is 71,372 bytes; IDF linked DRAM is 131,460 bytes, with 189,836 bytes not linked. These are link-time evidence, not runtime available heap. [Exact evidence receipt](../artifacts/reader-menu-verification.json) includes binary/ELF/map, host/C3 logs, stack probe, previews and all five prior image hashes. Both bounded reviews approved after the failure-repaint fix.

## First boot orientation (NEWT-81)

New installs read upright. When `newt_prefs` is confirmed empty at boot, Newt
asks whether the card already ran Newt (`cardHistory`,
`firmware/reader_preferences_store.h`). The card is new only when it is mounted
and answers that it has no `Newt/State` folder, and no legacy NVS copy of the
seen-build, selection or resume record exists. The folder holds every record
Newt writes, damaged ones too, which read as missing.

- A fresh card reads in portrait. Anything else keeps landscape: history that
  is present, damaged or unreadable, or no card yet.
- The answer is saved as an ordinary preferences record, verified before the
  reader uses it, so it is decided once and later records can't turn a new
  install sideways. Saved preferences always win and are never re-decided.
- If the card can't be read at boot, the session reads in landscape and
  decides at its first Home, before Newt notes this build on the card
  (`finishFirstPaint`, `firmware/update_seen.h`), so the note can't change the
  answer. No card by then decides nothing and saves nothing.
- A valid record, a corrupt one, or an unknown future one is never overwritten.
- If the save is not confirmed, the session keeps landscape, Reading shows
  that preferences are unavailable, and the note is still written. The record
  may still hold the new value, which a later boot loads.
- Reader preferences stay in their per-device NVS store, as since
  2026-10-01. A card moved to another X3 with no saved preferences there is
  decided by the rule above, which keeps landscape for a card that ran Newt.
- alpha.13 reads either record and saves beside it.

`tests/reader_orientation_firmware_test.py` runs main.cpp's own setup slice
from `loadReaderPreferences()` to the boot's reader settings, and the actual
first-note settle, against these cases. It covers cold boots, a rollback to
alpha.13 and negative controls. `tests/reading_state_backend_test.cpp` checks
the folder rule on real files, including a damaged record.
