#!/usr/bin/env python3
"""newt-preview's command line: the real firmware, built for the host by
tools/rust_preview.py, driven through one preview world.

A world is a folder the C fakes read (rust/host/preview/fakes): the card,
NVS, the clock, the app slots and fixture.conf. Every flag becomes data in
that world or presses on its buttons; the firmware decides every screen.
A preview runs in two parts. Setup boots a new card and answers Quick Start
(and any reader preference) with real presses, then seeds statistics by
reading on fixture dates. The preview then cold-boots that card, reaches
--screen and plays --actions. stdout is the last frame as PBM; stderr is the
firmware's own DEV USB replies (HELLO, DIAG, STATUS, FRAME).

Exit codes: 2 invalid input, 3 a book did not open.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
from xml.etree import ElementTree as ET
import zipfile
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'tools'), str(ROOT / 'rust/host/preview')]
import rust_preview  # noqa: E402
from rust_preview import WAKE  # noqa: E402
import update_fixtures  # noqa: E402

BACK, CONFIRM, LEFT, RIGHT, UP, DOWN, POWER, WAIT_MINUTE = range(8)
# action_for_button: Back is Read on Home, so "read" and "back" are one button.
ACTIONS = {'previous': LEFT, 'up': UP, 'next': RIGHT, 'down': DOWN, 'activate': CONFIRM,
           'back': BACK, 'read': BACK, 'sleep': POWER, 'wake': WAKE}
# From Home, whose Books section has focus after a cold boot.
SCREENS = {'home': [], 'library': [CONFIRM], 'app': [RIGHT, CONFIRM], 'device': [RIGHT, RIGHT, CONFIRM],
           'epub': [BACK], 'sleeping': [POWER]}
# Quick Start from a new card: skip it, or Start and answer its four steps
# (reading orientation, then skip the time zone and Wi-Fi, then Next, Done).
SKIP_QUICK_START = [RIGHT, CONFIRM]
QUICK_START = {'portrait': [CONFIRM, CONFIRM, DOWN, CONFIRM, DOWN, CONFIRM, CONFIRM, CONFIRM],
               'landscape-ccw': [CONFIRM, DOWN, CONFIRM, DOWN, CONFIRM, DOWN, CONFIRM, CONFIRM, CONFIRM]}
# From Home: the sample's Reading settings, its slim bar switched off, saved.
BAR_OFF = [BACK, CONFIRM, RIGHT, RIGHT, CONFIRM, RIGHT, CONFIRM, RIGHT, RIGHT, CONFIRM, BACK]
TELEMETRY = ('unknown', 'sample')
STATS = ('ready', 'counting', 'undated', 'unavailable', 'newer', 'crossink', 'crossink-undated')
LIBRARIES = ('healthy', 'four', 'no-covers', 'cover-fault', 'shelf', 'sparse', 'long-title', 'loading',
             'unavailable', 'empty')
SAMPLE = ROOT / 'tests/fixtures/the-small-lantern.epub'
COVER_ART = ROOT / 'tests/fixtures/library-cover-images'
LONG_TITLE = 'A very long title about the quiet orchard and the letters we carried home from the shore'
# Statistics' fixture dates: three evenings of reading, the last on "today".
TODAY = (2026, 11, 3)
READING = [((2026, 10, 30, 20, 0), 40), ((2026, 11, 1, 8, 0), 25), ((2026, 11, 3, 22, 0), 12)]


class Invalid(ValueError):
    """Input the old preview rejected with exit 2."""


def clock_registers(year, month, day, hour, minute):
    """DS3231 registers (BCD, UTC): what the board fake's clock reads."""
    bcd = lambda value: (value // 10) << 4 | value % 10
    weekday = __import__('datetime').date(year, month, day).isoweekday()
    return bytes([0, bcd(minute), bcd(hour), weekday, bcd(day), bcd(month), bcd(year % 100)]) + bytes(9)


def clock_value(text):
    hours, colon, minutes = text.partition(':')
    if not (len(text) == 5 and colon and hours.isdigit() and minutes.isdigit()
            and int(hours) < 24 and int(minutes) < 60):
        raise Invalid(f'--clock needs HH:MM, not {text!r}')
    return int(hours), int(minutes)


def action_codes(text):
    names = text.split(',')
    if len(names) > 1024 or any(name not in ACTIONS for name in names):
        raise Invalid(f'--actions: {text!r}')
    return [ACTIONS[name] for name in names]


def choice(flag, value, allowed):
    if value not in allowed:
        raise Invalid(f'{flag}: {value!r}')
    return value


def parse(arguments):
    """The old CLI's flags, validated before anything boots."""
    if len(arguments) % 2:
        raise Invalid('missing argument value')
    flags = {}
    for flag, value in zip(arguments[::2], arguments[1::2]):
        flags.setdefault(flag, []).append(value)
    def last(flag, default=None):
        return flags.get(flag, [default])[-1]
    known = {'--screen', '--telemetry', '--actions', '--clock', '--reader-orientation', '--reader-progress-bar',
             '--stats', '--other-firmware', '--update', '--library-fixture', '--build-id', '--build-date',
             '--working-preview', '--opening-preview', '--quick-start', '--quick-start-facts', '--radio',
             '--app-package'}
    unknown = set(flags) - known
    if unknown:
        raise Invalid(f'unknown flags {sorted(unknown)}')
    keys = []
    for screen in flags.get('--screen', []):
        keys += SCREENS[choice('--screen', screen, SCREENS)]
    for actions in flags.get('--actions', []):
        keys += action_codes(actions)
    other = last('--other-firmware', 'crossink')
    if other not in update_fixtures.OTHERS:
        raise Invalid(f'--other-firmware: {other!r}')
    update = last('--update')
    if update is not None:
        try:
            update_fixtures.update_world(update)
        except ValueError as error:
            raise Invalid(str(error)) from None
    for flag in ('--build-id', '--build-date', '--working-preview', '--opening-preview', '--quick-start',
                 '--quick-start-facts', '--radio', '--app-package'):
        if flag in flags:
            # The firmware's build identity is fixed at compile time, and these
            # states have no fixture in the real firmware yet (issue #357).
            raise Invalid(f'{flag} is not available in the firmware preview')
    return dict(
        keys=keys,
        telemetry=choice('--telemetry', last('--telemetry', 'unknown'), TELEMETRY),
        clock=clock_value(last('--clock')) if '--clock' in flags else None,
        orientation=choice('--reader-orientation', last('--reader-orientation'), (None, 'portrait', 'landscape-ccw')),
        bar=choice('--reader-progress-bar', last('--reader-progress-bar', 'on'), ('on', 'off')),
        stats=choice('--stats', last('--stats'), (None, *STATS)),
        library=choice('--library-fixture', last('--library-fixture', 'healthy'), LIBRARIES),
        other=other, update=update)


def epub(path, title, identifier, cover=None):
    """The sample EPUB under another title and identifier, so each book's
    bytes differ; `cover` (0-3) adds one of the authored cover images."""
    opf, dc = 'http://www.idpf.org/2007/opf', 'http://purl.org/dc/elements/1.1/'
    ET.register_namespace('', opf)
    ET.register_namespace('dc', dc)
    with zipfile.ZipFile(SAMPLE) as source:
        package = ET.fromstring(source.read('OEBPS/content.opf'))
        package.find(f'.//{{{dc}}}title').text = title
        package.find(f'.//{{{dc}}}identifier').text = f'urn:newt-preview:{identifier}'
        if cover is not None:
            ET.SubElement(package.find(f'{{{opf}}}manifest'), f'{{{opf}}}item',
                          {'id': 'newt-cover', 'href': 'cover.png', 'media-type': 'image/png',
                           'properties': 'cover-image'})
        with zipfile.ZipFile(path, 'w', compression=zipfile.ZIP_STORED) as archive:
            for entry in source.infolist():
                data = ET.tostring(package) if entry.filename == 'OEBPS/content.opf' else source.read(entry)
                archive.writestr(entry, data)
            if cover is not None:
                archive.writestr('OEBPS/cover.png', (COVER_ART / f'style-{cover}.png').read_bytes())
    os.utime(path, (1790812800, 1790812800))


def prepare_covers(books, covers):
    """Offline-prepared covers, as tools/prepare-library-covers.py makes them:
    the firmware's producer cannot decode these PNG covers itself."""
    import importlib.util
    spec = importlib.util.spec_from_file_location('prepare_library_covers', ROOT / 'tools/prepare-library-covers.py')
    tool = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(tool)
    with tempfile.TemporaryDirectory(prefix='newt-preview-covers-') as work:
        tool.prepare(books, covers, [sys.executable, '-I', '-B', str(ROOT / 'tools/library-cover-fixture.py')],
                     Path(work))


def library(name, card):
    """The --library-fixture card: Newt/Books and its prepared covers. Built
    once per name under .build and copied with its file times."""
    cached = rust_preview.WORK / 'libraries' / name
    if not cached.exists():
        cached.parent.mkdir(parents=True, exist_ok=True)
        scratch = Path(tempfile.mkdtemp(prefix=name + '.', dir=cached.parent))
        books = scratch / 'Newt/Books'
        books.mkdir(parents=True)
        authored = ('A Map of Morning', 'Letters from the Shore', 'The Quiet Orchard', 'The Small Lantern')
        titles = {'healthy': [('The Small Lantern', None), ('The Small Lantern - copy', None)],
                  'four': [(title, style) for style, title in enumerate(authored)],
                  'cover-fault': [(title, style) for style, title in enumerate(authored)],
                  'no-covers': [(title, None) for title in authored],
                  'loading': [(f'Lantern stories {index:02}', None) for index in range(1, 71)],
                  'shelf': [(f'Lantern stories {index:02}', index % 4) for index in range(1, 62)],
                  'sparse': [(f'Lantern stories {index:02}', index % 4) for index in range(1, 6)],
                  'long-title': [(LONG_TITLE, None)]}.get(name, [])
        for title, cover in titles:
            epub(books / f'{title}.epub', title, title, cover)
        if any(cover is not None for _, cover in titles):
            prepare_covers(books, scratch / 'Newt/Covers')
        if name == 'cover-fault':
            # A damaged prepared cover: each file ends a third of the way in.
            for cover in (scratch / 'Newt/Covers').iterdir():
                stat = cover.stat()
                cover.write_bytes(cover.read_bytes()[:stat.st_size // 3])
                os.utime(cover, (stat.st_atime, stat.st_mtime))
        try:
            scratch.rename(cached)
        except OSError:  # Another preview built it first.
            shutil.rmtree(scratch)
    shutil.copytree(cached, card, dirs_exist_ok=True)
    return {'loading': {'freeze': 'library.scan'},
            'unavailable': {'store.refuse': 'card-list:/newt-sd'}}.get(name, {})


def write_conf(directory, extra):
    """fixture.conf: what update_fixtures wrote, with this preview's keys."""
    path = directory / 'fixture.conf'
    lines = [line for line in (path.read_text().splitlines() if path.exists() else []) if line]
    conf = dict(line.split('=', 1) for line in lines)
    conf.update(extra)
    path.write_text(''.join(f'{key}={value}\n' for key, value in conf.items()))


def fresh(directory):
    """Forget a part's run files; the card, NVS, retained words and slots stay."""
    for name in ('panel.bin', 'frame.pbm', 'receipt.txt', 'serial.log', 'exit.txt', 'script.txt', 'clock.bin'):
        (directory / name).unlink(missing_ok=True)


def play(firmware, directory, keys):
    try:
        return rust_preview.run(firmware, directory, keys)
    except rust_preview.Unplayable as error:
        raise Invalid(str(error)) from None


def seed_statistics(firmware, directory, state):
    """Statistics the firmware itself counted: Library's first book (the
    embedded sample counts nothing) read on each fixture evening, with a
    page turn every four minutes so no interval idles out."""
    if state == 'counting':
        # The ledger is not yet written: setup's seed is gone, and the card
        # refuses the seed's next write.
        (directory / 'card/Newt/Stats/newt-stats.nrs').unlink()
        return {'store.refuse': 'card-write:/newt-sd/Newt/Stats/newt-stats.nrs'}
    dated = state not in ('undated', 'crossink-undated')
    for (year, month, day, hour, minute), minutes in READING:
        fresh(directory)
        if dated:
            (directory / 'clock.bin').write_bytes(clock_registers(year, month, day, hour, minute))
        turns = []
        for index in range(minutes // 4):
            turns += [WAIT_MINUTE] * 4 + [RIGHT if index % 2 == 0 else LEFT]
        play(firmware, directory, [CONFIRM, CONFIRM, *turns, *[WAIT_MINUTE] * (minutes % 4), BACK])
    if state.startswith('crossink'):
        # CrossInk's own global statistics file (version 2): sessions,
        # seconds, pages and finished books it counted before Newt.
        folder = directory / 'card/.crosspoint'
        folder.mkdir(parents=True, exist_ok=True)
        (folder / 'global_stats.bin').write_bytes(
            bytes([2]) + b''.join(value.to_bytes(4, 'little') for value in (41, 98400, 1830, 2)))
    if state == 'newer':
        grow_records(directory / 'card/Newt/Stats/newt-stats.nrs')
    return {'unavailable': {'store.refuse': 'card-read:/newt-sd/Newt/Stats/'}}.get(state, {})


def crc32c(data):
    crc = 0xffffffff
    for byte in data:
        crc ^= byte
        for _ in range(8):
            crc = crc >> 1 ^ (0x82f63b78 if crc & 1 else 0)
    return crc ^ 0xffffffff


def reseal_records(path, edit):
    """Apply `edit` to each sealed key record of a core sd_record file
    (256-byte records, an NRSK tag on each key's record, size at 12, the
    value from 20, CRC32C last) and seal it again."""
    data = bytearray(path.read_bytes())
    for at in range(256, len(data), 256):
        record = data[at:at + 256]
        if record[:4] != b'NRSK' or int.from_bytes(record[252:], 'little') != crc32c(record[:252]):
            continue
        edit(record)
        record[252:] = crc32c(record[:252]).to_bytes(4, 'little')
        data[at:at + 256] = record
    path.write_bytes(bytes(data))


def grow_records(path):
    """A newer firmware's ledger: each record four bytes longer, so this
    firmware reads it as a newer layout."""
    def grow(record):
        record[12:16] = (int.from_bytes(record[12:16], 'little') + 4).to_bytes(4, 'little')
    reseal_records(path, grow)


def older_seen_build(path):
    """The last Newt that reached Home (core update_seen: NWTS, version at 12,
    FNV-1a at 52) becomes an older one, so this boot is the first after an
    update."""
    def older(record):
        value = record[20:76]
        if value[:4] != b'NWTS':
            return
        value[12:36] = update_fixtures.OLDER.encode().ljust(24, b'\0')
        hash = 2166136261
        for byte in value[:52]:
            hash = (hash ^ byte) * 16777619 & 0xffffffff
        value[52:56] = hash.to_bytes(4, 'little')
        record[20:76] = value
    reseal_records(path, older)


def first_boots(firmware, directory, keys):
    """A new card's first boots (Quick Start, then any bar choice). They are
    the same for every preview of one build, so their finished world is kept
    per build and copied; a new build (size or time) boots them again."""
    stat = Path(firmware).stat()
    key = '%08x' % zlib.crc32(json.dumps([str(firmware), stat.st_size, stat.st_mtime_ns, keys]).encode())
    kept = rust_preview.WORK / 'first-boots' / key
    if not kept.exists():
        kept.parent.mkdir(parents=True, exist_ok=True)
        scratch = Path(tempfile.mkdtemp(prefix='new-', dir=kept.parent))
        for part in keys:
            play(firmware, scratch, part)
        try:
            scratch.rename(kept)  # Parallel previews publish one complete copy.
        except OSError:
            shutil.rmtree(scratch)
    shutil.copytree(kept, directory, dirs_exist_ok=True)


def world(firmware, directory, options):
    """Setup: a new card's first boots, then this preview's fixture data."""
    orientation = options['orientation']
    first_boots(firmware, directory, [QUICK_START[orientation] if orientation else SKIP_QUICK_START,
                                      *([BAR_OFF] if options['bar'] == 'off' else [])])
    conf = library(options['library'], directory / 'card')
    if options['stats']:
        conf.update(seed_statistics(firmware, directory, options['stats']))
    fresh(directory)
    update_fixtures.write(directory, update=options['update'], other_firmware=options['other'],
                          run=lambda keys: (play(firmware, directory, keys), fresh(directory)))
    if options['update'] == 'updated':
        older_seen_build(directory / 'card/Newt/State/newt-seen.nrs')
    if options['telemetry'] == 'sample':
        conf['telemetry'] = 'sample'
    write_conf(directory, conf)
    fresh(directory)
    if options['clock']:
        (directory / 'clock.bin').write_bytes(clock_registers(*TODAY, *options['clock']))


def preview(firmware, arguments):
    options = parse(arguments)
    with tempfile.TemporaryDirectory(prefix='newt-preview-') as scratch:
        directory = Path(scratch)
        world(firmware, directory, options)
        play(firmware, directory, options['keys'])
        if os.environ.get('NEWT_PREVIEW_KEEP'):
            # Debugging aid: keep the finished world (card, NVS, serial.log).
            shutil.copytree(directory, os.environ['NEWT_PREVIEW_KEEP'], dirs_exist_ok=True)
        frame = directory / 'frame.pbm'
        if not frame.exists():
            raise RuntimeError('the firmware never refreshed the panel')
        receipt = directory / 'receipt.txt'
        return frame.read_bytes(), receipt.read_text() if receipt.exists() else ''


def prepared_library(firmware, arguments):
    """`prepared-library CARD COVERS SELECTED [damaged]`: an authored card
    and its prepared covers in Library, SELECTED focused; `damaged` opens a
    zeroed copy of that book, so the firmware draws its own open error."""
    if (len(arguments) not in (3, 4) or not arguments[2].isdigit()
            or arguments[3:] not in ([], ['damaged'])):
        raise Invalid('prepared-library CARD COVERS SELECTED [damaged]')
    card, covers, selected = Path(arguments[0]), Path(arguments[1]), int(arguments[2])
    with tempfile.TemporaryDirectory(prefix='newt-preview-') as scratch:
        directory = Path(scratch)
        first_boots(firmware, directory, [SKIP_QUICK_START])
        shutil.copytree(card, directory / 'card', dirs_exist_ok=True)
        shutil.copytree(covers, directory / 'card/Newt/Covers', dirs_exist_ok=True)
        keys = [CONFIRM, *[RIGHT] * selected]
        if len(arguments) == 4:
            books = sorted((directory / 'card/Newt/Books').glob('*.epub'))
            damaged = books[selected]
            stat = damaged.stat()
            # The same size and time, so its prepared cover still matches.
            damaged.write_bytes(bytes(stat.st_size))
            os.utime(damaged, (stat.st_atime, stat.st_mtime))
            keys.append(CONFIRM)
        fresh(directory)
        play(firmware, directory, keys)
        return (directory / 'frame.pbm').read_bytes(), (directory / 'receipt.txt').read_text()


def main():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--firmware', type=Path, required=True)
    known, arguments = parser.parse_known_args()
    try:
        if arguments[:1] == ['prepared-library']:
            frame, receipt = prepared_library(known.firmware, arguments[1:])
        else:
            frame, receipt = preview(known.firmware, arguments)
    except Invalid as error:
        print(f'newt-preview: {error}', file=sys.stderr)
        return 2
    sys.stderr.write(receipt)
    sys.stdout.buffer.write(frame)
    return 0


if __name__ == '__main__':
    sys.exit(main())
