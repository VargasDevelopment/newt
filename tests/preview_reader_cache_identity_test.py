#!/usr/bin/env python3
"""Page cache identity in the host preview (#190): only what lays out a page
names its cache.

One card, several launches of the real firmware with the generated book S
(tools/page_turn_fixtures.py) at the product default settings:

1. Read two pages and idle, so the producer seals chapters 1 and 2 and the
   place is saved; then go Home.
2. Give the Pages files the names v0.8.0 gave them. Home must look the same,
   Read must land on the same page, and each chapter must rebuild once.
3. Turn the slim bar off, then change the refresh cadence: each is saved and
   rebuilds nothing.
4. Change the text size: both chapters rebuild under new names.

After each Save, the newt_prefs record the firmware would load (its decoder
and A/B rules, ported below) must hold exactly the settings the upgraded read
started from plus that Save's change. Controls rewrite a saved record to a
wrong valid cadence, downgrade the 34 px record to version 2 (which firmware
rejects, falling back to the older slot) and give both records one sequence
(which firmware treats as ambiguous); each must fail the same check.

Each launch's `Reader action` and producer `PC start`/`PC finish` rows must
match the expected sequence exactly; doctored copies of those logs check that
an extra, missing or repeated build fails. The text size launch shows the test
sees a rebuild when one is due. Restoring the v0.8.0 identity itself is
controlled in rust/tests/reader_short_turn/upgrade.rs: the preview links the
reader as a prebuilt library, which its controls can't edit. The world
reports a known battery (`--telemetry sample`) so the producer runs.
"""
from pathlib import Path
import re
import sys
import tempfile
import zlib

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'tools'), str(ROOT / 'rust/host/preview')]
import page_turn_fixtures  # noqa: E402
import preview_driver  # noqa: E402
import rust_preview  # noqa: E402
from preview_driver import BACK, CONFIRM, LEFT, RIGHT, WAIT_MINUTE  # noqa: E402

ACTION = re.compile(r'^Reader action: schema=\d+ total=\d+ residual=\d+ flags=\d+ '
                    r'actions=(\d+) route=(\d+) miss=(\d+)$')
START = re.compile(r'^PC start: at=\d+ from=(\d+) next=(\d+)$')
FINISH = re.compile(r'^PC finish: at=\d+ status=(\d+) interrupted=(\d+)$')
ROWS = {'Reader action: ': ('turn', ACTION), 'PC start: ': ('build', START), 'PC finish: ': ('sealed', FINISH)}
CHARACTER = re.compile(r'\bactual_character=(\d+)')

# This build's names for chapters 1 and 2, and the names v0.8.0 (cache
# revision 5) gave the same files: pinned from a v0.8.0 build of this world.
# The renamed files keep this build's bytes, so a firmware that still looked
# them up would accept them and skip the rebuild this test expects.
V080_NAMES = {
    '7ee0c0106bb0ce5937d725f9e6c0b8757bddaf30a3bcb9e538ea18f1cdf41e3f':
        '234dd21bc87c6a76db0ce88283284c41562343e90b7fa87271dc24ec65280f5d',
    'c5c401db5ade130949d47b6c0738953c209b735a1b9c3140afeb949b111a08a7':
        '8f245c8b851af27665821c9e411dc4c8f350448cf3fc0f827913b83c8b7d3ead',
}

# From a reading page: the menu, then Reading settings. Rows: Orientation,
# Text size, Slim bar, Refresh, Save, Cancel.
SETTINGS = (CONFIRM, RIGHT, RIGHT, CONFIRM)
BAR_OFF = (*SETTINGS, RIGHT, RIGHT, CONFIRM, RIGHT, RIGHT, CONFIRM, RIGHT, LEFT)
NEXT_REFRESH = (*SETTINGS, RIGHT, RIGHT, RIGHT, CONFIRM, RIGHT, CONFIRM, RIGHT, LEFT)
NEXT_SIZE = (*SETTINGS, RIGHT, CONFIRM, RIGHT, RIGHT, RIGHT, CONFIRM, RIGHT, LEFT)


def turn(route, miss=0):
    """One press's action row. Route bits (ReaderOpenRoute): 1 probe, 2 hit, 4 laid out."""
    return ('turn', 1, route, miss)


HIT, MENU, LAID_OUT = turn(3), turn(0), turn(5, miss=2)
CHAPTERS_1_AND_2 = (('build', 0, 0), ('sealed', 0, 0), ('build', 0, 1), ('sealed', 0, 0))
# The resumed page, nine menu presses, Save's redraw, then next and back.
SAVED_CACHED = (HIT, *[MENU] * 9, HIT, HIT, HIT)
EXPECTED = {
    'seed': (MENU, LAID_OUT, *CHAPTERS_1_AND_2, HIT, HIT),
    'home': (HIT, MENU),
    'upgraded home': (),
    'upgraded read': (LAID_OUT, *CHAPTERS_1_AND_2),
    'bar off': SAVED_CACHED,
    'refresh': SAVED_CACHED,
    'text size': (HIT, *[MENU] * 9, LAID_OUT, *CHAPTERS_1_AND_2, HIT, HIT),
}
# Each Save's loaded settings after the orientation: (slim bar, refresh pages, text size byte).
SAVED = {'bar off': (0, 5, 0), 'refresh': (0, 10, 0), 'text size': (0, 10, 34)}
# The firmware's newt_prefs NVS keys (SLOT_KEYS, rust/core/src/durable.rs), and
# the settings it keeps when both are missing (ViewSettings::default).
SLOTS = ('slot0', 'slot1')
DEFAULTS = (1, 1, 5, 0)


def events(log):
    """Every action and producer run row, in order; a row that doesn't parse breaks the sequence."""
    found = []
    for line in log.splitlines():
        for prefix, (kind, pattern) in ROWS.items():
            if line.startswith(prefix):
                match = pattern.match(line)
                found.append((kind, *map(int, match.groups())) if match else ('unreadable', line))
    return found


def mismatch(expected, log):
    found = events(log)
    for index, (want, got) in enumerate(zip(expected, found)):
        if want != got:
            return f'event {index}: expected {want}, got {got}'
    if len(found) != len(expected):
        return f'{len(found)} events, expected {len(expected)}'
    return None


def u32(record, at):
    return int.from_bytes(record[at:at + 4], 'little')


def decode(record):
    """Preferences::decode (rust/core/src/preferences.rs): (sequence,
    (orientation, slim bar, refresh pages, text size byte)), or 'corrupt' or
    'unsupported'. There is no host decoder to reuse, so this follows it rule
    for rule, in its order."""
    if len(record) < 8 or record[:4] != b'NWTP':
        return 'corrupt'
    version = u32(record, 4)
    if not 1 <= version <= 4:
        return 'unsupported'
    if len(record) < 16 or record[12] > 2 or (record[12] == 2 and version < 3) \
            or not (record[15] == 0 or (version == 4 and record[15] in (22, 24, 27, 34))):
        return 'corrupt'
    if len(record) != 24 or record[13] > 1 or u32(record, 20) != zlib.crc32(record[:20]) \
            or (version == 1 and record[14] != 0) or (version > 1 and record[14] not in (1, 5, 10)) \
            or any(record[16:20]):
        return 'corrupt'
    return u32(record, 8), (record[12], record[13], 5 if version == 1 else record[14], record[15])


def select(slots):
    """PreferencesStore::read_and_select (rust/core/src/preferences_store.rs)
    over [slot0, slot1], None for a missing key: the active slot's index, None
    when both are missing, or why firmware loads neither."""
    read = ['missing' if slot is None else 'failed' if len(slot) > 24 else decode(slot) for slot in slots]
    for error in ('failed', 'unsupported'):
        if error in read:
            return f'newt_prefs {error}'
    valid = [index for index, value in enumerate(read) if not isinstance(value, str)]
    if len(valid) == 2:
        delta = (read[0][0] - read[1][0]) % 2**32
        if delta == 2**31 or (delta == 0 and slots[0] != slots[1]):
            return 'newt_prefs ambiguous'
        return int(delta >= 2**31)
    if valid:
        return valid[0]
    return None if read == ['missing', 'missing'] else 'newt_prefs corrupt'


def loaded(slots):
    """The settings firmware loads from these slots, or why it loads none."""
    index = select(slots)
    if isinstance(index, str):
        return index
    return DEFAULTS if index is None else decode(slots[index])[1]


def save_problem(slots):
    """None if each Save's loaded settings are the upgraded read's orientation
    plus exactly that Save's change, else the first that isn't."""
    before = loaded(slots['upgraded read'])
    if isinstance(before, str):
        return f'upgraded read: {before}'
    for name, change in SAVED.items():
        expected = (before[0], *change)
        saved = loaded(slots[name])
        if saved != expected:
            return f'{name}: saved {saved}, expected {expected}'
    return None


class Card:
    """One preview world whose card, NVS and retained state carry across launches."""

    def __init__(self, binary, directory):
        self.binary, self.directory, self.logs, self.slots = binary, directory, {}, {}
        preview_driver.world(binary, directory, preview_driver.parse(
            ['--telemetry', 'sample', '--library-fixture', 'empty']))
        books = directory / 'card/Newt/Books'
        books.mkdir(parents=True, exist_ok=True)
        (books / 'book-s.epub').write_bytes(page_turn_fixtures.build_book('s'))
        self.pages = directory / 'card/Newt/Cache/Pages'

    def launch(self, name, keys):
        """(final frame, saved character, whether the prefs record changed), after checking the rows."""
        preview_driver.fresh(self.directory)
        prefs = self.prefs()
        frames, log = rust_preview.run(self.binary, self.directory, keys)
        self.logs[name], self.slots[name] = log, self.prefs()
        problem = mismatch(EXPECTED[name], log)
        assert problem is None, f'{name}: {problem}'
        diag = [line for line in (self.directory / 'receipt.txt').read_text().splitlines()
                if line.startswith('DIAG ')]
        return frames[-1], int(CHARACTER.search(diag[-1])[1]), self.prefs() != prefs

    def prefs(self):
        """[slot0, slot1] bytes, None for a missing key, so a gap can't shift the other slot."""
        paths = [self.directory / 'nvs/newt_prefs' / key for key in SLOTS]
        return [path.read_bytes() if path.exists() else None for path in paths]

    def files(self):
        """{name: bytes} of the Pages folder, without the book's revision prefix."""
        return {path.stem.split('-')[1]: path.read_bytes() for path in self.pages.glob('*.fibp')}

    def rename_to_v080(self):
        for path in list(self.pages.glob('*.fibp')):
            book, name = path.stem.split('-')
            path.rename(self.pages / f'{book}-{V080_NAMES[name]}.fibp')


def journey(binary, directory):
    card = Card(binary, directory)
    page, saved, _ = card.launch('seed', (CONFIRM, CONFIRM, RIGHT, RIGHT, WAIT_MINUTE))
    assert saved > 0, 'the seed never left the first page'
    assert set(card.files()) == set(V080_NAMES), f'Pages names changed: {sorted(card.files())}'
    home, _, _ = card.launch('home', (BACK,))

    card.rename_to_v080()
    upgraded = card.files()
    shown, _, _ = card.launch('upgraded home', ())
    assert shown == home, 'Home looks different after the upgrade'
    shown, character, _ = card.launch('upgraded read', (BACK,))
    assert (shown, character) == (page, saved), f'Read landed on character {character}, not {saved}'
    rebuilt = card.files()
    assert {name: rebuilt.get(name) for name in upgraded} == upgraded, 'v0.8.0 Pages files changed'
    assert set(rebuilt) == set(upgraded) | set(V080_NAMES), f'unexpected Pages files: {sorted(rebuilt)}'

    bar_off, character, saved_choice = card.launch('bar off', BAR_OFF)
    assert saved_choice and bar_off != page and character == saved, 'the slim bar did not turn off in place'
    shown, character, saved_choice = card.launch('refresh', NEXT_REFRESH)
    assert saved_choice and shown == bar_off and character == saved, 'the refresh change was not saved in place'
    assert card.files() == rebuilt, 'a presentation setting changed the Pages files'

    _, _, saved_choice = card.launch('text size', NEXT_SIZE)
    # Each Save stores exactly its one change: bar off, refresh 5 -> 10, then 30 -> 34 px.
    problem = save_problem(card.slots)
    assert problem is None, problem
    resized = card.files()
    assert saved_choice and len(resized) == len(rebuilt) + 2 and set(rebuilt) < set(resized), \
        f'the text size change did not add two chapters: {sorted(resized)}'
    return card


def doctored(log, change):
    lines = log.splitlines()
    change(lines)
    return '\n'.join(lines)


def first(lines, pattern):
    return next(i for i, line in enumerate(lines) if pattern.match(line))


def drop_builds(lines):
    lines[:] = [line for line in lines if not (START.match(line) or FINISH.match(line))]


def build_again(lines):
    at = first(lines, FINISH) + 1
    lines[at:at] = ['PC start: at=0 from=0 next=0', 'PC finish: at=0 status=0 interrupted=0']


def build_after_save(lines):
    lines.append('PC start: at=0 from=0 next=0')


# Each a slack the sequence check must refuse: (launch, change, its exact failure).
LOG_CONTROLS = {
    'v0.8.0 Pages accepted instead of rebuilt': ('upgraded read', drop_builds, '1 events, expected 5'),
    'chapter 1 rebuilt twice after the upgrade': ('upgraded read', build_again,
                                                  "event 3: expected ('build', 0, 1), got ('build', 0, 0)"),
    'the slim bar change rebuilds': ('bar off', build_after_save, '14 events, expected 13'),
    'the refresh change rebuilds': ('refresh', build_after_save, '14 events, expected 13'),
}


def rewritten(slots, index, at, value):
    """slots with slot index's bytes from at replaced by value, its CRC recomputed."""
    slots = [bytearray(slot) for slot in slots]
    record = slots[index]
    record[at:at + len(value)] = value
    record[20:] = zlib.crc32(record[:20]).to_bytes(4, 'little')
    return [bytes(slot) for slot in slots]


# Each a saved record the Save check must refuse: (launch, rewrite of its
# slots given the active index, the exact failure under orientation o).
SAVE_CONTROLS = {
    'a valid but wrong cadence': (
        'refresh', lambda slots, at: rewritten(slots, at, 14, b'\x01'),
        'refresh: saved ({o}, 0, 1, 0), expected ({o}, 0, 10, 0)'),
    'the 34 px record downgraded to version 2': (
        'text size', lambda slots, at: rewritten(slots, at, 4, (2).to_bytes(4, 'little')),
        'text size: saved ({o}, 0, 10, 0), expected ({o}, 0, 10, 34)'),
    'both records on one sequence': (
        'text size', lambda slots, at: rewritten(slots, 1 - at, 8, slots[at][8:12]),
        'text size: saved newt_prefs ambiguous, expected ({o}, 0, 10, 34)'),
}


def main():
    binary = rust_preview.FIRMWARE if len(sys.argv) == 2 else rust_preview.build()
    with tempfile.TemporaryDirectory(prefix='newt-cache-identity-') as scratch:
        card = journey(binary, Path(scratch))
    logs = card.logs
    for label, (name, change, failure) in LOG_CONTROLS.items():
        problem = mismatch(EXPECTED[name], doctored(logs[name], change))
        assert problem == failure, f'log control {label}: {problem!r}'
    orientation = loaded(card.slots['upgraded read'])[0]
    for label, (name, change, failure) in SAVE_CONTROLS.items():
        problem = save_problem({**card.slots, name: change(card.slots[name], select(card.slots[name]))})
        assert problem == failure.format(o=orientation), f'save control {label}: {problem!r}'
    print('PASS: v0.8.0 Pages rebuild once with Home and the page unchanged; the slim bar and refresh '
          f'keep the cache; a text size change rebuilds; each Save stores exactly its setting; '
          f'{len(LOG_CONTROLS)} log controls and {len(SAVE_CONTROLS)} saved-record controls fail')


if __name__ == '__main__':
    main()
