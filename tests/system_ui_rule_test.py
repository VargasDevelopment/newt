"""The system UI rule (docs/078) on the shipped preview renderers: every
non-reader system page shares the system bar, opens its list on the top row and
uses the shared button rail, where every shown button does something. Host pixels
of the real firmware built for the host (tools/rust_preview.py) only. Stage Home
has its own separately tested composition. The first open's page and installed
tiny apps are not reachable in this preview yet (issue #357)."""
from pathlib import Path
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from host_common import png_from_pbm
from rust_preview import build, cli

WIDTH, HEIGHT, BAR, RAIL = 528, 792, 64, 748
CENTERS = (95, 201, 331, 437)  # Home's four button cells, in hardware order
PRESSES = ("back", "activate", "previous", "next")  # the same order, as preview actions
SETTINGS = "next,next,activate"
ZONE_LIST = SETTINGS + ",activate"  # opens on the published zone, the list's top
POCKET = "next,activate"
# Your pauses, from Pocket Pause: Select toggles the focused prompt (three rows,
# then Save), and Save returns to the experience with the new prompts.
EDIT = POCKET + ",activate"
# Update mode (docs/083), reached through Settings' Update row, or on Wi-Fi
# networks through the Wi-Fi row below it.
UPDATE = SETTINGS + ",next" * 6 + ",activate"
WIFI = SETTINGS + ",next" * 7 + ",activate"
# Every system page state the shell preview reaches, by the presses that reach it.
SCREENS = {
    "library": "activate",
    "library-second": "activate,next",
    "library-last": "activate,next,next,next",
    "library-empty": "activate",
    "library-unavailable": "activate",
    "settings": SETTINGS,
    "settings-time-format": SETTINGS + ",next",
    "settings-time-format-saved": SETTINGS + ",next,activate",
    "settings-sleep-screen-row": SETTINGS + ",next,next",
    "settings-diagnostics-row": SETTINGS + ",next,next,next",
    "settings-last-row": SETTINGS + ",next" * 9,
    "firmware-switch-confirm": SETTINGS + ",next" * 5 + ",activate",
    "zone-list-top": ZONE_LIST,
    "zone-list-middle": ZONE_LIST + ",next" * 7,
    "zone-list-bottom": ZONE_LIST + ",next" * 14,
    # Sleep screen (#202): three rows, all Off; Select toggles the focused row.
    "sleep-screen": SETTINGS + ",next,next,activate",
    "sleep-screen-last-row": SETTINGS + ",next,next,activate,next,next",
    "sleep-screen-week-on": SETTINGS + ",next,next,activate,next,activate",
    "diagnostics": SETTINGS + ",next,next,next,activate",
    "about": SETTINGS + ",next,next,next,activate,activate",
    "about-whats-new-row": SETTINGS + ",next,next,next,activate,activate,next",
    "whats-new": SETTINGS + ",next,next,next,activate,activate,next,activate",
    "reading-statistics": SETTINGS + ",next" * 4 + ",activate",
    "pocket-pause": POCKET,
    "pocket-pause-two-enabled": EDIT + ",activate,next,next,next,activate",
    "pocket-pause-one-enabled": EDIT + ",activate,next,activate,next,next,activate",
    "pocket-pause-none-enabled": EDIT + ",activate,next,activate,next,activate,next,activate",
    "pocket-pause-settings": EDIT,
    "pocket-pause-settings-save-row": EDIT + ",previous",
    "update-list": UPDATE,
    "update-list-firmware": UPDATE,
    "update-list-firmware-file": UPDATE + ",next",
    "update-list-firmware-more": UPDATE + ",next",
    "update-confirm-firmware": UPDATE + ",next,activate",
    "update-confirm-firmware-other-slot": UPDATE + ",next,activate",
    "update-confirm-other-slot": UPDATE + ",activate",
    "update-firmware-unconfirmed": UPDATE + ",activate",
    "update-confirm-same": UPDATE + ",activate",
    "update-older": UPDATE,
    "update-none": UPDATE,
    "update-no-card": UPDATE,
    "update-rolled-back": UPDATE,
    "update-unreadable": UPDATE,
    # The other slot holds CrossInk, so an install first asks to replace it.
    "update-result-write": UPDATE + ",activate,activate",
    "update-result-unsettled": UPDATE + ",activate,activate",
    "update-result-unconfirmed": UPDATE + ",activate,activate",
    # The list's GitHub check (docs/088).
    "update-check-row": UPDATE + ",next",
    "update-check-ready-row": UPDATE,
    "update-check-latest": UPDATE + ",activate",
    # The Wi-Fi networks page (invented names).
    "update-wifi": WIFI,
    "update-wifi-none": WIFI,
    "update-wifi-failed": WIFI,
    # Wi-Fi setup (docs/085): a network picked, its password, a result, Forget.
    "update-wifi-pick": WIFI + ",next",
    "update-wifi-password": WIFI + ",next,activate",
    "update-wifi-saved": WIFI + ",next,activate" + ",activate" * 8 + ",up,previous,activate",
    "update-wifi-forget": WIFI + ",next" * 7,
    "update-wifi-connected": WIFI + ",next" * 6 + ",activate",
    # The first boot after an install: "Updated to" over Home, once (docs/083).
    "updated": "",
}
# The card and outcome each update state runs with (rust/host/preview/update_fixtures.py).
UPDATES = {"update-list": "newer", "update-list-firmware": "firmware",
           "update-list-firmware-file": "firmware", "update-list-firmware-more": "firmware-more",
           "update-confirm-firmware": "firmware", "update-confirm-firmware-other-slot": "firmware-other-slot",
           "update-confirm-other-slot": "other-slot", "update-firmware-unconfirmed": "firmware-unconfirmed", "update-confirm-same": "same", "update-older": "older",
           "update-none": "none", "update-no-card": "no-card",
           "update-rolled-back": "rolled-back", "update-unreadable": "unreadable",
           "update-result-write": "fail-write", "update-result-unsettled": "fail-unsettled",
           "update-result-unconfirmed": "fail-unconfirmed", "updated": "updated",
           "update-check-row": "newer", "update-check-ready-row": "check-latest",
           "update-check-latest": "check-latest",
           "update-wifi": "newer",
           "update-wifi-none": "wifi-none", "update-wifi-failed": "wifi-failed",
           "update-wifi-pick": "newer", "update-wifi-password": "newer", "update-wifi-saved": "newer",
           "update-wifi-forget": "wifi-saved", "update-wifi-connected": "wifi-saved"}
FIXTURES = {"library": "four", "library-second": "four", "library-last": "four", "library-empty": "empty",
            "library-unavailable": "unavailable"}
# As the firmware switch's, an unconfirmed save keeps Select: it tries
# again, and goes on once a later reader save is verified. A save only
# happens for a book read in this boot, so Library's first book turns a page.
READ = "activate,activate,next,back,"
# Library's scan and Wi-Fi's scan are busy pages where the preview ends,
# so presses there are not replayed.
STATIC = {"update-not-started": ["--update", "unsaved", "--actions", READ + UPDATE],
          "wifi-not-started": ["--update", "unsaved", "--actions", READ + WIFI],
          "library-loading": ["--library-fixture", "loading", "--actions", "activate"],
          "update-wifi-scanning": ["--update", "wifi-scanning", "--actions", WIFI]}
# A system page with the same bar where the clock changed: a saved 12-hour
# format, and the GitHub check, which sets the clock from GitHub's Date.
BAR_AFTER = {"settings-time-format-saved": (SCREENS["settings-time-format-saved"] + ",back,activate",),
             "update-check-latest": (SCREENS["update-check-latest"] + ",back", "--update", "check-latest")}
# Back on an unconfirmed install restarts, and the other slot may start: the
# preview ends there, as it draws no other firmware.
RESTARTS = {("update-result-unconfirmed", "back")}


def ink(data, x, y, width=WIDTH):
    return bool(data[(y * width + x) // 8] & (0x80 >> (x % 8)))


def pixels(pbm):
    png_from_pbm(pbm)  # exact geometry and byte count
    header, size, data = pbm.split(b"\n", 2)
    assert header == b"P4" and size == f"{WIDTH} {HEIGHT}".encode()
    return data


def shown(data, center):
    return any(ink(data, center + dx, y) for dx in range(-49, 50) for y in range(RAIL, HEIGHT))


def joined(*actions):
    return ",".join(action for action in actions if action)


class SystemUiRuleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.binary = Path(sys.argv[1]) if len(sys.argv) > 1 else cli(build())
        cls.cache = {}
        cls.system = cls.frame(SETTINGS + ",next")

    @classmethod
    def frame(cls, actions="", *arguments):
        return pixels(cls.output(actions, *arguments))

    @classmethod
    def output(cls, actions="", *arguments):
        """The preview's whole output, which may be a landscape book page."""
        command = [str(cls.binary), "--screen", "home", "--telemetry", "sample", "--clock", "21:41",
                   *arguments, *(("--actions", actions) if actions else ())]
        key = tuple(command)
        if key not in cls.cache:
            cls.cache[key] = subprocess.run(command, check=True, capture_output=True).stdout
        return cls.cache[key]

    @classmethod
    def screen(cls, name, press=""):
        """The output of a named state, after one more press."""
        if name in STATIC:
            return cls.output("", *STATIC[name])
        fixture = ("--library-fixture", FIXTURES[name]) if name in FIXTURES else ()
        card = ("--update", UPDATES[name]) if name in UPDATES else ()
        return cls.output(joined(SCREENS[name], press), *fixture, *card)

    def test_every_system_screen_shares_the_system_bar(self):
        for name in [*SCREENS, *STATIC]:
            with self.subTest(name):
                reference = self.frame(*BAR_AFTER[name]) if name in BAR_AFTER else self.system
                self.assertEqual(pixels(self.screen(name))[:BAR * WIDTH // 8], reference[:BAR * WIDTH // 8],
                                 "the shared wordmark, clock and Offline · battery")

    def test_settings_list_opens_on_its_top_row(self):
        entry = self.frame(SETTINGS)
        self.assertEqual(self.frame(SETTINGS + ",previous"), entry, "nothing above the entry row")
        self.assertNotEqual(self.frame(SETTINGS + ",next"), entry, "the rows lie below it")
        self.assertEqual(self.frame(SETTINGS + ",next,back,activate"), entry,
                         "a new visit opens on the top row again")
        # Stated exception: Back from a child page returns to the row that opened it.
        self.assertEqual(self.frame(SETTINGS + ",next,next,activate,back"), self.frame(SETTINGS + ",next,next"))

    def test_every_control_uses_the_shared_button_frame(self):
        # Home's button frame: the pixels every Home button inks, by offset
        # from its cell's center, outside the label's inner area.
        offsets = [(dx, y) for dx in range(-49, 50) for y in range(RAIL, HEIGHT)]
        label = lambda dx, y: -46 <= dx < 46 and RAIL + 4 <= y < HEIGHT - 6
        button = {(dx, y) for dx, y in offsets
                  if not label(dx, y) and all(ink(self.system, c + dx, y) for c in CENTERS)}
        self.assertTrue(button, "the middle Settings row shows all four button frames")
        cells = {c + dx for c in CENTERS for dx in range(-55, 56)}
        frames = {name: pixels(self.screen(name)) for name in [*SCREENS, *STATIC]}
        for name, data in frames.items():
            with self.subTest(name):
                self.assertFalse([x for x in range(WIDTH) if x not in cells
                                  for y in range(RAIL, HEIGHT) if ink(data, x, y)],
                                 "the rail is white paper outside Home's button cells")
                visible = [c for c in CENTERS if shown(data, c)]
                for c in visible:
                    self.assertTrue(all(ink(data, c + dx, y) for dx, y in button),
                                    f"the control at {c} has Home's button frame")
                self.assertTrue(visible, "every screen shows at least Back")

    def test_the_named_ends_stop(self):
        # The states below are real ends, so their hidden buttons are checked
        # where a press does nothing.
        for name, press in (("zone-list-top", "previous"), ("zone-list-bottom", "next"),
                            ("library", "previous"), ("library-last", "next"),
                            ("settings", "previous"), ("settings-last-row", "next"),
                            ("sleep-screen", "previous"), ("sleep-screen-last-row", "next"),
                            ("pocket-pause-one-enabled", "next"), ("pocket-pause-one-enabled", "previous"),
                            ("pocket-pause-none-enabled", "next"),
                            # About's Version Select only counts toward the first newt (docs/081).
                            ("about", "previous"), ("about", "activate"), ("about-whats-new-row", "next"),
                            ("whats-new", "previous"), ("whats-new", "next"), ("whats-new", "activate"),
                            ("updated", "previous"), ("updated", "next"), ("updated", "activate")):
            with self.subTest(name, press=press):
                self.assertEqual(self.screen(name, press), self.screen(name))
        # The update list's offer tops it and its check row ends it; an older
        # offer is refused. Scan tops the networks; with none heard and none
        # saved it is the only row.
        for name, press in (("update-check-row", "next"),
                            ("update-list", "previous"), ("update-older", "activate"),
                            ("update-wifi-none", "next"), ("update-wifi", "previous"),
                            ("update-wifi-forget", "next")):
            with self.subTest(name, press=press):
                self.assertEqual(self.screen(name, press), self.screen(name))
        # With no saved network that row sets up Wi-Fi: Select opens the networks page (#365).
        self.assertEqual(self.screen("update-check-row", "activate"), self.screen("update-wifi"))
        # Back from What's new returns to About with focus on its row.
        self.assertEqual(self.screen("whats-new", "back"), self.screen("about-whats-new-row"))
        self.assertNotEqual(self.screen("zone-list-middle", "next"), self.screen("zone-list-middle"))

    def test_update_writing_waits(self):
        # While writing, checking GitHub and on the success page (which
        # restarts) no button shows; the bar is still the shared one. The
        # preview ends at a busy page, so presses there are not replayed.
        writing = pixels(self.output(UPDATE + ",activate,activate", "--update", "writing"))
        self.assertEqual(writing[:BAR * WIDTH // 8], self.system[:BAR * WIDTH // 8])
        self.assertFalse([c for c in CENTERS if shown(writing, c)], "no button while writing")
        running = pixels(self.output(UPDATE + ",activate", "--update", "check-downloading"))
        self.assertFalse([c for c in CENTERS if shown(running, c)], "no button while checking")
        done = pixels(self.output(UPDATE + ",activate,activate", "--update", "newer"))
        self.assertFalse([c for c in CENTERS if shown(done, c)], "no button on the restarting page")
        self.assertNotEqual(done, writing)

    def test_every_shown_button_does_something(self):
        # A press that changes nothing has no button: at list ends, with no
        # other pause to pick, and in every other reachable state.
        for name in SCREENS:
            before = self.screen(name)
            for center, press in zip(CENTERS, PRESSES):
                if shown(pixels(before), center) and (name, press) not in RESTARTS:
                    with self.subTest(name, press=press):
                        self.assertNotEqual(self.screen(name, press), before,
                                            f"{press} is shown, so it changes the screen")


if __name__ == "__main__":
    unittest.main(argv=sys.argv[:1])
