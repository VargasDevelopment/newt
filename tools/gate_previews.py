#!/usr/bin/env python3
"""Host gate preview phase: CLI validation and the exact PBM/PNG previews.

The gate runs this as one phase after building the Rust newt-preview
(tools/rust_preview.py), so a parallel gate can buffer its output. It writes the artifacts/ previews and checks the
relations between them, in the same order as the serial gate always has.
"""
import struct
import sys
import zlib
from pathlib import Path

sys.dont_write_bytecode = True
from gate_runner import phase_run
from host_common import ROOT, export_preview, png_from_pbm


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def png_roundtrip(pbm: bytes, png: bytes) -> None:
    """Independently parse PNG chunks and reconstruct its canonical packed PBM."""
    require(png[:8] == b"\x89PNG\r\n\x1a\n", "PNG signature")
    offset, compressed = 8, bytearray()
    dimensions = None
    while offset < len(png):
        length = struct.unpack(">I", png[offset:offset + 4])[0]
        kind = png[offset + 4:offset + 8]
        data = png[offset + 8:offset + 8 + length]
        crc = struct.unpack(">I", png[offset + 8 + length:offset + 12 + length])[0]
        require(crc == zlib.crc32(kind + data), "PNG chunk checksum")
        if kind == b"IHDR":
            dimensions = struct.unpack(">IIBBBBB", data)
        if kind == b"IDAT":
            compressed.extend(data)
        offset += length + 12
    magic, geometry, pixels = pbm.split(b"\n", 2)
    width, height = map(int, geometry.split())
    require(magic == b"P4" and (width, height) in {(528, 792), (792, 528)},
            "PBM approved geometry")
    require(dimensions == (width, height, 1, 0, 0, 0, 0), "PNG monochrome geometry")
    raw = zlib.decompress(compressed)
    stride = width // 8 + 1
    require(len(raw) == stride * height, "PNG decoded byte count")
    packed = bytearray()
    for start in range(0, len(raw), stride):
        require(raw[start] == 0, "PNG row filter")
        packed.extend(byte ^ 255 for byte in raw[start + 1:start + stride])
    require(bytes(packed) == pixels, "PNG preserves exact framebuffer pixels")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("Usage: gate_previews.py NEWT_PREVIEW_BINARY")
    binary = Path(sys.argv[1])
    for arguments in (("--screen", "nonsense"), ("--telemetry", "live"),
                      ("--actions", "next,,wake"), ("--actions", "sleep,"),
                      ("--actions", "shell"), ("--screen",), ("--clock", "24:00"), ("--clock", "9:41"),
                      ("--clock", "12:00", "--date", "2026-11-31"), ("--date", "2026-11-26"),
                      ("--reader-orientation", "landscape"),
                      ("--reader-progress-bar", "yes"), ("--reader-orientation",), ("--stats", "busy"),
                      ("--other-firmware", "maybe"), ("--update", "maybe")):
        result = phase_run([str(binary), *arguments], capture_output=True)
        require(result.returncode == 2 and not result.stdout, "Host rejects invalid input")
    for arguments in (("--actions", "next,,wake"), ("--reader-orientation", "landscape"),
                      ("--reader-progress-bar", "yes")):
        result = phase_run([sys.executable, str(ROOT / "tools" / "preview.py"),
                                 *arguments], capture_output=True)
        require(result.returncode == 2, "Preview CLI rejects invalid flags")
    previews = [
        ("home", "home", "unknown", None),
        # Settings opens on its top row (the system page rule): Select on Time zone opens
        # the zone list and Select in it saves. On Time format below it, Select
        # switches the format and saves. Sleep screen is two rows down and
        # Diagnostics three.
        ("device-settings", "device", "unknown", None),
        ("device-settings-format", "device", "unknown", "next,activate"),
        ("device-settings-list", "device", "unknown", "activate"),
        ("device-settings-saved", "device", "unknown", "activate,next,activate"),
        ("device-unknown", "device", "unknown", "next,next,next,activate"),
        ("device-sample", "device", "sample", "next,next,next,activate"),
        ("sleeping", "sleeping", "unknown", None),
        ("epub", "epub", "unknown", None),
        ("epub-next", "epub", "unknown", "next"),
        ("epub-resumed", "epub", "unknown", "next,sleep,wake"),
        ("epub-reopened", "home", "unknown", "read,next,back,read"),
        ("loaded-app", "app", "unknown", None),
        ("loaded-app-note", "app", "unknown", "activate"),
        ("loaded-app-another", "app", "unknown", "next"),
        ("loaded-app-prompts", "app", "unknown", "activate,next"),
        ("loaded-app-resumed", "app", "unknown", "activate,sleep,wake"),
        ("loaded-app-home-entry", "home", "unknown", "next,activate"),
    ]
    for name, screen, telemetry, actions in previews:
        pbm, png = export_preview(binary, name, screen, telemetry, actions)
        png_roundtrip(pbm.read_bytes(), png.read_bytes())
    # A card app in /Newt/Apps (README, "Apps from your card"), with the
    # invented apps in tests/fixtures/apps (tools/preview_driver.py
    # --app-package): the offer, its outcomes and a cold boot after Install.
    card_apps = {"app-offer": ("offer", None),
                 "app-offer-declined": ("offer", "back"),
                 "app-installed": ("offer", "activate"),
                 "app-installed-open": ("offer", "activate,activate"),
                 "app-installed-restart": ("installed", None),
                 "app-refused-damaged": ("damaged", None),
                 "app-refused-newer": ("newer", None),
                 "app-refused-many": ("many", None),
                 "app-refused-changed": ("changed", "activate"),
                 "app-refused-unreadable": ("unreadable", None),
                 "app-storage-refused": ("storage", "activate"),
                 "app-unconfirmed": ("unconfirmed", "activate"),
                 "app-revert-pocket-pause": ("revert", "activate,activate"),
                 "app-offer-unavailable": ("unavailable", None),
                 "app-offer-long": ("long", None)}
    for name, (package, actions) in card_apps.items():
        pbm, png = export_preview(binary, name, "app", actions=actions, app_package=package)
        png_roundtrip(pbm.read_bytes(), png.read_bytes())
    app_page = {name: (ROOT / f"artifacts/{name}.pbm").read_bytes() for name in card_apps}
    require(app_page["app-offer-declined"] == app_page["app-revert-pocket-pause"]
            == (ROOT / "artifacts/loaded-app.pbm").read_bytes(), "Later and a reinstalled Pocket Pause open Pocket Pause")
    require(app_page["app-installed-restart"] == app_page["app-installed-open"],
            "a cold boot after Install opens the installed app")
    distinct = [name for name in card_apps if name not in ("app-offer-declined", "app-revert-pocket-pause",
                                                           "app-installed-restart")]
    require(len({app_page[name] for name in distinct}) == len(distinct), "each card app state draws its own screen")
    # Invented clock text: Home in the default 24-hour format, then after the
    # Time format row's Select.
    for name, screen, actions in (("home-clock-24h", "home", None),
                                  ("home-clock-12h", "device", "next,activate,back")):
        pbm, png = export_preview(binary, name, screen, actions=actions, clock="12:59")
        png_roundtrip(pbm.read_bytes(), png.read_bytes())
    # Reading statistics in each state, from reading the firmware counted on
    # fixture dates (tools/preview_driver.py): the page under Settings, and
    # the reader menu's panel for Library's first book, the one that was read.
    stats_states = ("ready", "counting", "undated", "unavailable", "newer")
    for state in stats_states:
        for name, screen, actions in ((f"device-stats-{state}", "device", "next,next,next,next,activate"),
                                      (f"reader-stats-{state}", "library",
                                       "activate,activate,next,next,next,activate")):
            # A set clock: without one the page has no today to date reading by.
            pbm, png = export_preview(binary, name, screen, actions=actions, stats=state, clock="21:30")
            png_roundtrip(pbm.read_bytes(), png.read_bytes())
    for screen in ("device", "reader"):
        pages = {(ROOT / f"artifacts/{screen}-stats-{state}.pbm").read_bytes() for state in stats_states}
        require(len(pages) == len(stats_states), "each statistics state draws its own screen")
    # Totals that include imported CrossInk history add one line to the page.
    for state in ("crossink", "crossink-undated"):
        pbm, png = export_preview(binary, f"device-stats-{state}", "device", actions="next,next,next,next,activate",
                                  stats=state, clock="21:30")
        png_roundtrip(pbm.read_bytes(), png.read_bytes())
    require(len({(ROOT / f"artifacts/device-stats-{state}.pbm").read_bytes()
                 for state in ("ready", "undated", "crossink", "crossink-undated")}) == 4,
            "the CrossInk line draws on the dated and undated page")
    pbm, png = export_preview(binary, "device-settings-stats-row", "device", actions="next,next,next,next")
    png_roundtrip(pbm.read_bytes(), png.read_bytes())
    # Sleep screen (#202): its three rows Off, then Last 7 days switched on.
    for name, actions in (("device-sleep-screen", "next,next,activate"),
                          ("device-sleep-screen-week-on", "next,next,activate,next,activate")):
        pbm, png = export_preview(binary, name, "device", actions=actions)
        png_roundtrip(pbm.read_bytes(), png.read_bytes())
    require((ROOT / "artifacts/device-sleep-screen.pbm").read_bytes() !=
            (ROOT / "artifacts/device-sleep-screen-week-on.pbm").read_bytes(), "Select switches Last 7 days on")
    # The sleeping newt's Thanksgiving outfit (#117) only on the fourth Thursday of November.
    sleep_days = {"sleeping-thanksgiving-eve": "2026-11-25", "sleeping-thanksgiving": "2026-11-26",
                  "sleeping-thanksgiving-after": "2026-11-27"}
    for name, date in sleep_days.items():
        pbm, png = export_preview(binary, name, "sleeping", clock="12:00", date=date)
        png_roundtrip(pbm.read_bytes(), png.read_bytes())
    sleep = {name: (ROOT / f"artifacts/{name}.pbm").read_bytes() for name in sleep_days}
    require(sleep["sleeping-thanksgiving-eve"] == sleep["sleeping-thanksgiving-after"]
            != sleep["sleeping-thanksgiving"], "the newt dresses for Thanksgiving on that day only")
    # About under Diagnostics, with the preview build's own identity (fixed
    # when it compiles); its What's new row and page show the committed notes.
    about = "next,next,next,activate,activate"
    for name, actions in (("device-about", about), ("device-about-whats-new-row", about + ",next"),
                          ("device-whats-new", about + ",next,activate")):
        pbm, png = export_preview(binary, name, "device", actions=actions)
        png_roundtrip(pbm.read_bytes(), png.read_bytes())
    require(len({(ROOT / f"artifacts/{name}.pbm").read_bytes() for name in (
        "device-about", "device-about-whats-new-row", "device-whats-new")}) == 3,
            "About moves focus and opens What's new")
    # Settings' last row opens the same About page.
    for name, actions in (("device-settings-about-row", ",".join(["next"] * 8)),
                          ("device-settings-about", ",".join(["next"] * 8 + ["activate"]))):
        pbm, png = export_preview(binary, name, "device", actions=actions)
        png_roundtrip(pbm.read_bytes(), png.read_bytes())
    require((ROOT / "artifacts/device-settings-about.pbm").read_bytes() ==
            (ROOT / "artifacts/device-about.pbm").read_bytes(), "Settings' About row opens About")
    # Restart into the other firmware, Settings' sixth row, with an invented
    # other slot: its row and confirm page for a known and an unknown
    # firmware, a switch under way, a failed check, an unconfirmed save or
    # selection, and a missing slot, whose row Select leaves Settings
    # unchanged. Back cancels the page.
    # A save only happens for a book read in this boot, so the unconfirmed-save
    # pages first turn a page of Library's first book, then go Home to Settings.
    # Updated to is the first Home after an install.
    def start(name, actions):
        if name in ("device-switch-save-unconfirmed", "device-update-not-started", "device-wifi-not-started"):
            return "library", "activate,next,back,next,next,activate," + actions
        return ("home" if name.startswith("device-updated") else "device"), actions

    row = "next,next,next,next,next"
    switch = {"device-switch-row": ("crosspoint", row),
              "device-switch-confirm": ("crosspoint", row + ",activate"),
              "device-switch-row-crossink": ("crossink", row),
              "device-switch-confirm-crossink": ("crossink", row + ",activate"),
              "device-switch-row-unknown": ("stock", row),
              "device-switch-confirm-unknown": ("stock", row + ",activate"),
              "device-switch-cancelled": ("crosspoint", row + ",activate,back"),
              "device-switch-switching": ("crosspoint", row + ",activate,activate"),
              "device-switch-failed": ("failing", row + ",activate,activate"),
              "device-switch-save-unconfirmed": ("unsaved", row + ",activate,activate"),
              "device-switch-switch-unconfirmed": ("unconfirmed", row + ",activate,activate"),
              "device-switch-missing": ("missing", row),
              "device-switch-missing-select": ("missing", row + ",activate")}
    for name, (slot, actions) in switch.items():
        screen, actions = start(name, actions)
        pbm, png = export_preview(binary, name, screen, actions=actions, other_firmware=slot)
        png_roundtrip(pbm.read_bytes(), png.read_bytes())
    page = {name: (ROOT / f"artifacts/{name}.pbm").read_bytes() for name in switch}
    distinct = [name for name in switch if name not in ("device-switch-cancelled", "device-switch-missing-select")]
    require(len({page[name] for name in distinct}) == len(distinct),
            "each Restart into the other firmware state and name draws its own screen")
    require(page["device-switch-cancelled"] == page["device-switch-row"], "Back cancels the switch")
    require(page["device-switch-missing-select"] == page["device-switch-missing"],
            "a missing other firmware keeps its row disabled")
    # Update mode (docs/UPDATES.md, "Card install"), reached from Settings' Update
    # row, with invented card fixtures against an invented running version
    # (rust/host/preview/update_fixtures.py): every list, confirm, progress and result state.
    # Settings' Wi-Fi row, the last, opens it on Wi-Fi networks (#103).
    entry = "next,next,next,next,next,next"
    networks = entry + ",next,activate"
    password, typed = networks + ",next,activate", ",activate" * 8
    update = {"device-update-row": (None, entry),
              "device-update-not-started": ("unsaved", entry + ",activate"),
              "device-wifi-row": (None, entry + ",next"),
              "device-wifi-not-started": ("unsaved", entry + ",next,activate"),
              "update-newer": ("newer", entry + ",activate"),
              # The same copied offer with a saved network: its check row reads as a download's does.
              "update-newer-saved": ("wifi-saved", entry + ",activate"),
              # Another firmware's .bin files (NEWT-71): a known name, an
              # unknown one by its file name, one that is not an app, a Newt
              # image under another name, and more files than the list.
              "update-firmware": ("firmware", entry + ",activate"),
              "update-firmware-known": ("firmware", entry + ",activate,next"),
              "update-firmware-unknown": ("firmware", entry + ",activate,next,next,next"),
              "update-firmware-invalid": ("firmware", entry + ",activate,next,next"),
              "update-firmware-invalid-select": ("firmware", entry + ",activate,next,next,activate"),
              "update-firmware-newt-file": ("firmware-more", entry + ",activate,next,next"),
              "update-confirm-firmware": ("firmware", entry + ",activate,next,activate"),
              "update-confirm-firmware-unknown": ("firmware", entry + ",activate,next,next,next,activate"),
              "update-confirm-firmware-replace": ("firmware-other-slot", entry + ",activate,next,activate"),
              "update-confirm-firmware-empty": ("firmware-empty-slot", entry + ",activate,next,activate"),
              "update-firmware-unconfirmed": ("firmware-unconfirmed", entry + ",activate,activate,activate"),
              "update-confirm-replace": ("other-slot", entry + ",activate,activate"),
              "update-confirm-reinstall": ("same", entry + ",activate,activate"),
              "update-older": ("older", entry + ",activate"),
              "update-unreadable": ("unreadable", entry + ",activate"),
              "update-rolled-back": ("rolled-back", entry + ",activate"),
              "update-none": ("none", entry + ",activate"),
              "update-no-card": ("no-card", entry + ",activate"),
              # The list's GitHub check (docs/UPDATES.md, "Updates over Wi-Fi"): its row, mid-download, and how it ends.
              "update-check-row": ("newer", entry + ",activate,next"),
              "update-check-ready-row": ("check-latest", entry + ",activate"),
              "update-check-downloading": ("check-downloading", entry + ",activate,activate"),
              "update-check-latest": ("check-latest", entry + ",activate,activate"),
              "update-check-no-release": ("check-no-release", entry + ",activate,activate"),
              "update-check-ready": ("check-ready", entry + ",activate,activate"),
              "update-writing": ("writing", entry + ",activate,activate,activate"),
              "update-installed": ("newer", entry + ",activate,activate,activate"),
              "update-failed-write": ("fail-write", entry + ",activate,activate,activate"),
              "update-failed-unsettled": ("fail-unsettled", entry + ",activate,activate,activate"),
              "update-unconfirmed": ("fail-unconfirmed", entry + ",activate,activate,activate"),
              "update-left": ("newer", entry + ",activate,back"),
              # The Wi-Fi networks page, with invented names. Back leaves update mode.
              "update-wifi": ("newer", networks),
              "update-wifi-scanning": ("wifi-scanning", networks),
              "update-wifi-none": ("wifi-none", networks),
              "update-wifi-failed": ("wifi-failed", networks),
              "update-wifi-back": ("newer", networks + ",back"),
              "update-after-wifi": ("newer", networks + ",back,next,next,activate," + entry + ",activate"),
              # Wi-Fi setup (docs/UPDATES.md, "Wi-Fi setup"): pick, the masked password, save or Forget.
              "update-wifi-pick": ("newer", networks + ",next"),
              "update-wifi-password": ("newer", password),
              "update-wifi-typed": ("newer", password + ",down,next,activate,activate,activate"),
              "update-wifi-password-back": ("newer", password + ",activate,back"),
              "update-wifi-saved": ("newer", password + typed + ",up,previous,activate"),
              "update-wifi-saved-list": ("newer", password + typed + ",up,previous,activate,back"),
              "update-wifi-not-saved": ("wifi-not-saved", password + typed + ",up,previous,activate"),
              "update-wifi-unconfirmed": ("wifi-unconfirmed", password + typed + ",up,previous,activate"),
              "update-wifi-forget": ("wifi-saved", networks + ",next" * 7),
              "update-wifi-forgotten": ("wifi-saved", networks + ",next" * 7 + ",activate"),
              # The saved network's one join (docs/UPDATES.md, "Wi-Fi setup") and how it ends.
              "update-wifi-connect": ("wifi-saved", networks + ",next" * 6),
              "update-wifi-joining": ("wifi-joining", networks + ",next" * 6 + ",activate"),
              "update-wifi-connected": ("wifi-saved", networks + ",next" * 6 + ",activate"),
              "update-wifi-wrong-password": ("wifi-wrong-password", networks + ",next" * 6 + ",activate"),
              "update-wifi-not-found": ("wifi-not-found", networks + ",next" * 6 + ",activate"),
              "update-wifi-connect-back": ("wifi-saved", networks + ",next" * 6 + ",activate,back"),
              # The first boot after an install: "Updated to" over Home, once.
              "device-updated": ("updated", None),
              # A test build's 23-character version takes its own title line.
              "device-updated-longest": ("updated-longest", None),
              "device-updated-back": ("updated", "back")}
    for name, (card, actions) in update.items():
        screen, actions = start(name, actions)
        pbm, png = export_preview(binary, name, screen, actions=actions, update=card)
        png_roundtrip(pbm.read_bytes(), png.read_bytes())
    page = {name: (ROOT / f"artifacts/{name}.pbm").read_bytes() for name in update}
    home = {"update-left", "update-wifi-back", "device-updated-back"}
    returns = {"update-wifi-password-back", "update-wifi-connect-back", "update-after-wifi", "update-check-ready",
               "update-firmware-invalid-select", "update-newer-saved"}
    require(len({page[name] for name in update if name not in home | returns})
            == len(update) - len(home) - len(returns), "each update state draws its own screen")
    require(page["update-wifi-connect-back"] == page["update-wifi-connect"], "Back from a join returns to its row")
    require(page["update-after-wifi"] == page["update-newer"], "Update opens on its list after a Wi-Fi visit")
    # With no saved network the check row sets up Wi-Fi instead (#365), so compare with a saved one.
    require(page["update-check-ready"] == page["update-newer-saved"], "a downloaded Newt shows as a copied offer does")
    require(page["update-newer-saved"] != page["update-newer"], "with no saved network the check row sets up Wi-Fi")
    require(page["update-firmware-invalid-select"] == page["update-firmware-invalid"],
            "a file that is not an app never installs")
    require(page["update-wifi-password-back"] == page["update-wifi-pick"],
            "Back from the password returns to its network, with nothing typed kept")
    require(all(page[name] == (ROOT / "artifacts/home.pbm").read_bytes() for name in home),
            "leaving update mode, Back on Wi-Fi networks too, restarts into Home, and Back from Updated to returns there")
    require(page["device-updated"] != (ROOT / "artifacts/device-whats-new.pbm").read_bytes(),
            "Updated to titles the notes What's new shows")
    for name, screen, actions, orientation, bar in (
            ("epub-landscape", "epub", None, "landscape-ccw", "off"),
            ("epub-landscape-bar", "epub", None, "landscape-ccw", "on"),
            ("epub-portrait", "epub", None, "portrait", "off"),
            ("epub-home-exit", "epub", "back", "landscape-ccw", "on"),
            ("home-reader-portrait", "home", None, "portrait", "on"),
            ("device-reader-landscape", "device", "next,next,next,activate", "landscape-ccw", "on")):
        pbm, png = export_preview(binary, name, screen, reader_orientation=orientation,
                                  reader_progress_bar=bar, actions=actions)
        png_roundtrip(pbm.read_bytes(), png.read_bytes())
        dimensions = pbm.read_bytes().split(b"\n", 2)[1]
        expected = b"792 528" if screen == "epub" and not actions and orientation == "landscape-ccw" else b"528 792"
        require(dimensions == expected, "Reader flags affect only book geometry")
    menu_previews = [
        ("reader-menu", "activate"),
        ("reader-chapters", "activate,next,activate"),
        ("reader-settings", "activate,next,next,activate"),
        ("reader-statistics", "activate,next,next,next,activate"),
        ("reader-chapter-two", "activate,next,activate,next,activate"),
        ("reader-settings-saved", "activate,next,next,activate,activate,next,next,activate,next,next,activate"),
        ("reader-settings-cancelled", "activate,next,next,activate,activate,back,back"),
        ("reader-menu-resumed", "activate,sleep,wake"),
    ]
    for name, actions in menu_previews:
        pbm, png = export_preview(binary, name, "epub", actions=actions)
        png_roundtrip(pbm.read_bytes(), png.read_bytes())
    require((ROOT / "artifacts/reader-settings-cancelled.pbm").read_bytes()
            == (ROOT / "artifacts/epub.pbm").read_bytes(), "Cancelled settings preserve page and geometry")
    require((ROOT / "artifacts/reader-menu-resumed.pbm").read_bytes()
            == (ROOT / "artifacts/epub.pbm").read_bytes(), "Sleep from menu resumes reading page")
    require((ROOT / "artifacts/reader-chapter-two.pbm").read_bytes()
            != (ROOT / "artifacts/epub.pbm").read_bytes(), "Real chapter navigation changes book content")
    # Settings rows: Orientation, Text size, Slim bar, Refresh, Save, Cancel.
    for save_actions in ("activate,next,next,activate,activate,next,next,activate,next,next,activate",
                         "activate,next,next,activate,next,activate,next,next,next,activate"):
        saved = phase_run([str(binary), "--screen", "epub", "--actions", save_actions],
                          check=True, capture_output=True)
        require(saved.stdout != (ROOT / "artifacts/epub.pbm").read_bytes(), "Saved reading settings change the page")
        for suffix in (",sleep,wake", ",back,read"):
            reopened = phase_run([str(binary), "--screen", "epub", "--actions", save_actions + suffix],
                                 check=True, capture_output=True)
            require(reopened.stdout == saved.stdout, "Verified preferences survive Home and a sleep's wake")
    # Quick Start's Skip keeps the upright book with its slim bar.
    upright = phase_run([str(binary), "--screen", "epub", "--reader-orientation", "portrait",
                         "--reader-progress-bar", "on"], check=True, capture_output=True)
    require((ROOT / "artifacts/epub.pbm").read_bytes() == upright.stdout,
            "Selected slim bar is the default book-reading design")
    require((ROOT / "artifacts/epub-landscape.pbm").read_bytes()
            != (ROOT / "artifacts/epub-landscape-bar.pbm").read_bytes(),
            "Configurable bar visibly changes reader chrome")
    # Clockwise is the same page turned half a turn on the panel, so its
    # logical frame matches counterclockwise; its buttons swap, so Left turns on.
    def book(orientation, actions=None):
        command = [str(binary), "--screen", "epub", "--reader-orientation", orientation]
        return phase_run(command + (["--actions", actions] if actions else []),
                         check=True, capture_output=True).stdout
    require(book("landscape-cw") == (ROOT / "artifacts/epub-landscape-bar.pbm").read_bytes(),
            "Clockwise landscape shows the counterclockwise page")
    forward = book("landscape-ccw", "next")
    require(forward != book("landscape-ccw") and book("landscape-cw", "previous") == forward
            and book("landscape-cw", "previous,next") == book("landscape-cw"),
            "Clockwise swaps the page buttons, so forward stays on the same side of the text")
    # Back then Next in one input drain: Home gets the second press before its
    # first repaint, while the clockwise book frame is still on the panel.
    require(book("landscape-cw", "back+next") == book("landscape-ccw", "back+next") != book("landscape-cw", "back"),
            "Presses queued after leaving a clockwise book use Home's buttons")
    require((ROOT / "artifacts/home-reader-portrait.pbm").read_bytes()
            == (ROOT / "artifacts/home.pbm").read_bytes(), "Reader flags preserve portrait Home")
    require((ROOT / "artifacts/device-reader-landscape.pbm").read_bytes()
            == (ROOT / "artifacts/device-unknown.pbm").read_bytes(), "Reader flags preserve portrait Device")
    require((ROOT / "artifacts/device-settings-list.pbm").read_bytes()
            != (ROOT / "artifacts/device-settings.pbm").read_bytes(), "Select on Time zone opens the list")
    require((ROOT / "artifacts/device-settings-saved.pbm").read_bytes()
            != (ROOT / "artifacts/device-settings.pbm").read_bytes(), "a pick shows the chosen zone")
    require((ROOT / "artifacts/device-settings-format.pbm").read_bytes()
            != (ROOT / "artifacts/device-settings.pbm").read_bytes(), "Select on Time format shows the new format")
    require((ROOT / "artifacts/home-clock-12h.pbm").read_bytes()
            != (ROOT / "artifacts/home-clock-24h.pbm").read_bytes(), "the Time format choice changes Home's clock")
    require((ROOT / "artifacts/epub-home-exit.pbm").read_bytes().split(b"\n", 2)[1] == b"528 792",
            "Leaving the reader restores portrait system geometry")
    reader_flags = ["--reader-orientation", "portrait", "--reader-progress-bar", "on"]
    book_actions = ["--screen", "epub", "--actions", "next,sleep,wake"]
    before = phase_run([str(binary), *reader_flags, *book_actions], check=True, capture_output=True)
    after = phase_run([str(binary), *book_actions, *reader_flags], check=True, capture_output=True)
    require(before.stdout == after.stdout and before.stderr == after.stderr,
            "Reader options are applied before opening regardless of argument order")
    reference = (ROOT / "artifacts/epub-next.pbm").read_bytes()
    require((ROOT / "artifacts/loaded-app.pbm").read_bytes()
            == (ROOT / "artifacts/loaded-app-home-entry.pbm").read_bytes(),
            "Home selects actual decoded builtin app in simulation")
    require((ROOT / "artifacts/home.pbm").read_bytes()
            == (ROOT / "artifacts/loaded-app-resumed.pbm").read_bytes(),
            "Pocket Pause wakes Home")
    require((ROOT / "artifacts/loaded-app-another.pbm").read_bytes()
            != (ROOT / "artifacts/loaded-app.pbm").read_bytes(), "Another pause avoids the immediately previous prompt")
    require((ROOT / "artifacts/loaded-app-prompts.pbm").read_bytes()
            != (ROOT / "artifacts/loaded-app-note.pbm").read_bytes(), "Pause settings show the selected prompt row")
    empty_actions = "activate,activate,next,activate,next,activate,next,activate"
    empty = phase_run([str(binary), "--screen", "app", "--actions", empty_actions],
                           check=True, capture_output=True)
    # Back keeps Home's Apps focus, so one Select reopens the app.
    reopened = phase_run([str(binary), "--screen", "app", "--actions", empty_actions + ",back,activate"],
                              check=True, capture_output=True)
    require(empty.stdout == reopened.stdout, "All-disabled pauses remain graceful after Home and re-entry")
    home = phase_run([str(binary), "--screen", "home"], check=True, capture_output=True)
    for draft in ("activate,activate", "activate,next,activate", "activate,previous"):
        resumed = phase_run([str(binary), "--screen", "app", "--actions", draft + ",sleep,wake"],
                                 check=True, capture_output=True)
        require(resumed.stdout == home.stdout, "Pause settings and drafts wake Home")
    require(reference != (ROOT / "artifacts/epub.pbm").read_bytes(),
            "Real EPUB next action changes visible text")
    for name in ("epub-resumed", "epub-reopened"):
        require(reference == (ROOT / f"artifacts/{name}.pbm").read_bytes(),
                "Real EPUB sleep/wake and menu re-entry preserve source anchor")
    for invalid in (b"P4\n1 1\n\0", b"P4\n264 1584\n" + bytes(52272),
                    b"P4\n792 528\n" + bytes(52271)):
        try:
            png_from_pbm(invalid)
        except ValueError:
            pass
        else:
            raise RuntimeError("PNG exporter accepted invalid geometry or frame size")


if __name__ == "__main__":
    main()
