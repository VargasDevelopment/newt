#!/usr/bin/env python3
"""Reader work invariants in the host preview: once the background producer
has sealed a chapter, every turn in it comes from the page cache.

The real firmware opens a generated book (tools/page_turn_fixtures.py, S),
turns across the chapter edge and back. The preview presses only after a
quiet spell, so the producer seals chapters 1 and 2 before the first turn.
The whole log, from boot, must then hold exactly the expected sequence of
`Reader action` rows (rust/core/src/reader_action_profile/trace.rs) and
producer `PC start`/`PC finish` rows: one build per chapter, and a single
cache hit per turn. A missing turn, an extra build or a laid-out page all
break the sequence. Setting A is the product default (landscape, Px30, slim
bar on); B turns the bar off.

The world reports a known battery (`--telemetry sample`): with an unknown
battery the producer is denied power and never runs (#222).
"""
from pathlib import Path
import re
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'tools'), str(ROOT / 'rust/host/preview')]
import page_turn_fixtures  # noqa: E402
import preview_driver  # noqa: E402
import rust_preview  # noqa: E402
from gate_tier import negative_controls  # noqa: E402
from preview_driver import CONFIRM, LEFT, RIGHT  # noqa: E402

ACTION = re.compile(r'^Reader action: schema=\d+ total=\d+ residual=\d+ flags=\d+ '
                    r'actions=(\d+) route=(\d+) miss=(\d+)$')
START = re.compile(r'^PC start: at=\d+ from=(\d+) next=(\d+)$')
FINISH = re.compile(r'^PC finish: at=\d+ status=(\d+) interrupted=(\d+)$')
SPINE = re.compile(r'\bactual_spine=(\d+)')

KEYS = (CONFIRM, CONFIRM, *[RIGHT] * 14, LEFT, LEFT)
SETTINGS = {'A': 'on', 'B': 'off'}


def turn(route, miss=0):
    """One press's action row. Route bits (ReaderOpenRoute): 1 probe, 2 hit, 4 laid out."""
    return ('turn', 1, route, miss)


def build(chapter):
    """One producer run for `chapter` (1-based) that sealed it."""
    return [('build', chapter), ('sealed', 0, 0)]


HIT = turn(3)
EXPECTED = (
    turn(0),          # Home opens the Books list.
    turn(5, miss=2),  # The book opens on a fresh card: probed, no cache to attach, laid out.
    *build(1), *build(2),
    *[HIT] * 13,      # The 13th turn enters chapter 2.
    *build(3),
    *[HIT] * 3,       # The last turn forward, then back across the edge.
)


def events(log):
    """Every action and producer run row in the log, in order."""
    found = []
    for line in log.splitlines():
        if (match := ACTION.match(line)):
            found.append(('turn', *map(int, match.groups())))
        elif (match := START.match(line)):
            found.append(('build', int(match[1]) + int(match[2]) + 1))
        elif (match := FINISH.match(line)):
            found.append(('sealed', *map(int, match.groups())))
    return found


def mismatch(log):
    """Where the log leaves the expected sequence, or None."""
    found = events(log)
    for index, (want, got) in enumerate(zip(EXPECTED, found)):
        if want != got:
            return f'event {index}: expected {want}, got {got}'
    if len(found) != len(EXPECTED):
        return f'{len(found)} events, expected {len(EXPECTED)}'
    return None


def play(binary, bar):
    """(serial log, final actual_spine) of one fresh world playing KEYS."""
    with tempfile.TemporaryDirectory(prefix='newt-reader-work-') as scratch:
        directory = Path(scratch)
        options = preview_driver.parse(['--telemetry', 'sample', '--library-fixture', 'empty',
                                        '--reader-progress-bar', bar])
        preview_driver.world(binary, directory, options)
        books = directory / 'card/Newt/Books'
        books.mkdir(parents=True, exist_ok=True)
        (books / 'book-s.epub').write_bytes(page_turn_fixtures.build_book('s'))
        preview_driver.fresh(directory)
        _, log = rust_preview.run(binary, directory, KEYS)
        diag = [line for line in (directory / 'receipt.txt').read_text().splitlines() if line.startswith('DIAG ')]
        spine = SPINE.search(diag[-1]) if diag else None
        return log, int(spine.group(1)) if spine else None


def journey(binary, bar):
    log, spine = play(binary, bar)
    if spine != 0:
        return log, f'the turns back ended in spine {spine}, not in chapter 1'
    return log, mismatch(log)


def doctored(log, change):
    """`log` with `change(lines)` applied: a control for the sequence check itself."""
    lines = log.splitlines()
    change(lines)
    return '\n'.join(lines)


def first(lines, pattern, after=0):
    return next(i for i, line in enumerate(lines) if i >= after and pattern.match(line))


def lay_out_first_hit(lines):
    at = first(lines, ACTION, after=first(lines, START))
    lines[at] = lines[at].replace('route=3 miss=0', 'route=5 miss=2')


def drop_first_hit(lines):
    del lines[first(lines, ACTION, after=first(lines, START))]


def rebuild_chapter_1(lines):
    at = first(lines, FINISH) + 1
    lines[at:at] = ['PC start: at=0 from=0 next=0', 'PC finish: at=0 status=0 interrupted=0']


# Log-level controls: each slack the sequence check must refuse, and the
# event where it must notice.
LOG_CONTROLS = {
    'a sealed turn laid out again': (lay_out_first_hit, 6),
    'a turn row went missing': (drop_first_hit, 18),
    'chapter 1 built again between seals': (rebuild_chapter_1, 4),
}
# A firmware edit that keeps the producer from ever running must fail.
CONTROL = ('reader_image_producer.rs', 'Reason::None => ProducerEntry::Ready,',
           'Reason::None => ProducerEntry::Blocked,')


def main():
    binary = rust_preview.FIRMWARE if len(sys.argv) == 2 else rust_preview.build()
    for name, bar in SETTINGS.items():
        log, problem = journey(binary, bar)
        assert problem is None, f'setting {name} (bar {bar}): {problem}'
        if name == 'A':
            for label, (change, at) in LOG_CONTROLS.items():
                problem = mismatch(doctored(log, change))
                assert problem and problem.startswith(f'event {at}:'), f'log control {label}: {problem!r}'
    if not negative_controls(__file__):
        print(f'PASS: {len(SETTINGS)} reader work journeys match the sealed-cache sequence; '
              f'{len(LOG_CONTROLS)} log controls fail (firmware control not in this tier)')
        return
    control = rust_preview.control([CONTROL], optimize=False)
    _, problem = journey(control, SETTINGS['A'])
    assert problem == "event 2: expected ('build', 1), got ('turn', 1, 5, 2)", f'control: unexpected result {problem!r}'
    print(f'PASS: {len(SETTINGS)} reader work journeys match the sealed-cache sequence; '
          f'{len(LOG_CONTROLS)} log controls and the firmware control fail')


if __name__ == '__main__':
    main()
