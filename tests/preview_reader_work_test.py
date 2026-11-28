#!/usr/bin/env python3
"""Reader work invariants in the host preview: once the background producer
has sealed a chapter, every turn in it comes from the page cache.

The real firmware opens a generated book (tools/page_turn_fixtures.py, S),
turns once so the producer is admitted, idles two minutes so it seals the
current and next chapter, then turns across the chapter edge and back. Each
action's `Reader action` row (rust/core/src/reader_action_profile/trace.rs)
must then show a cache hit, no ordinary page and no miss, and no producer run
may restart for a sealed chapter. Setting A is the product default
(landscape, Px30, slim bar on); B turns the bar off.

The world reports a known battery (`--telemetry sample`): with an unknown
battery the producer is denied power and never runs.
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

BACK, CONFIRM, LEFT, RIGHT, UP, DOWN, POWER, WAIT_MINUTE, QUEUED = range(9)
ACTION = re.compile(r'^Reader action: schema=\d+ total=\d+ residual=\d+ flags=\d+ actions=\d+ route=(\d+) miss=(\d+)$')
START = re.compile(r'^PC start: at=\d+ from=(\d+) next=(\d+)$')
FINISH = re.compile(r'^PC finish: at=\d+ status=(\d+) interrupted=(\d+)$')
SPINE = re.compile(r'\bactual_spine=(\d+)')
HIT, ORDINARY = 2, 4  # ReaderOpenRoute bits: CacheHit, OrdinaryPage.

FORWARD = (CONFIRM, CONFIRM, RIGHT, WAIT_MINUTE, WAIT_MINUTE, *[RIGHT] * 13)
ROUND_TRIP = (*FORWARD, LEFT, LEFT)
SETTINGS = {'A': 'on', 'B': 'off'}


def play(binary, bar, keys):
    """(serial log, final actual_spine) of one fresh world playing `keys`."""
    with tempfile.TemporaryDirectory(prefix='newt-reader-work-') as scratch:
        directory = Path(scratch)
        options = preview_driver.parse(['--telemetry', 'sample', '--library-fixture', 'empty',
                                        '--reader-progress-bar', bar])
        preview_driver.world(binary, directory, options)
        books = directory / 'card/Newt/Books'
        books.mkdir(parents=True, exist_ok=True)
        (books / 'book-s.epub').write_bytes(page_turn_fixtures.build_book('s'))
        preview_driver.fresh(directory)
        _, log = rust_preview.run(binary, directory, keys)
        diag = [line for line in (directory / 'receipt.txt').read_text().splitlines() if line.startswith('DIAG ')]
        spine = SPINE.search(diag[-1]) if diag else None
        return log, int(spine.group(1)) if spine else None


def sealed_turns(log):
    """The problem with the turns after chapters 1 and 2 are sealed, or None."""
    lines = log.splitlines()
    sealed_at, start = None, None
    for index, line in enumerate(lines):
        if (match := START.match(line)):
            start = int(match.group(1)) + int(match.group(2))
        elif (match := FINISH.match(line)) and start == 1:
            if match.groups() != ('0', '0'):
                return f'the producer run for chapter 2 ended status={match[1]} interrupted={match[2]}'
            sealed_at = index
            break
    if sealed_at is None:
        return 'the producer never sealed chapters 1 and 2'
    turns = 0
    for line in lines[sealed_at + 1:]:
        if (match := START.match(line)) and int(match.group(1)) + int(match.group(2)) <= 1:
            return f'the producer restarted for sealed chapter {int(match[1]) + int(match[2]) + 1}'
        if (match := ACTION.match(line)):
            route, miss = int(match[1]), int(match[2])
            if not route & HIT or route & ORDINARY or miss:
                return f'turn {turns + 1} after sealing laid out again: route={route} miss={miss}'
            turns += 1
    if turns < 14:
        return f'only {turns} turns after sealing'
    return None


def journey(binary, bar):
    _, spine = play(binary, bar, FORWARD)
    if spine != 1:
        return f'the forward turns ended in spine {spine}, not across the chapter edge'
    log, spine = play(binary, bar, ROUND_TRIP)
    if spine != 0:
        return f'the turns back ended in spine {spine}, not in chapter 1'
    return sealed_turns(log)


# A firmware edit that keeps the producer from ever running must fail.
CONTROL = ('reader_image_producer.rs', 'Reason::None => ProducerEntry::Ready,',
           'Reason::None => ProducerEntry::Blocked,')


def main():
    binary = rust_preview.FIRMWARE if len(sys.argv) == 2 else rust_preview.build()
    for name, bar in SETTINGS.items():
        problem = journey(binary, bar)
        assert problem is None, f'setting {name} (bar {bar}): {problem}'
    if not negative_controls(__file__):
        print(f'PASS: {len(SETTINGS)} reader work journeys hit the sealed cache (controls not in this tier)')
        return
    control = rust_preview.control([CONTROL], optimize=False)
    problem = journey(control, SETTINGS['A'])
    assert problem == 'the producer never sealed chapters 1 and 2', f'control: unexpected result {problem!r}'
    print(f'PASS: {len(SETTINGS)} reader work journeys hit the sealed cache; the control fails')


if __name__ == '__main__':
    main()
