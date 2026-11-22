"""Session/day-clock protocol corpus, with independent selected-field checks.

No compilation or execution occurred while authoring this staged slice.
Numeric book values are synthetic, never behavior selectors.
"""
from datetime import datetime, timedelta, timezone
import random

U32 = (1 << 32) - 1
U64 = (1 << 64) - 1
FIELDS = ('sequence book spine character active pages sessions session reading open '
          'mark_ms mark_pages mark_sessions midnight expires midnight_utc day zone state '
          'folded_at lease folded_once imported end matched').split()


def utc(year, month, day, hour=0, minute=0, second=0):
    return int((datetime(year, month, day, hour, minute, second, tzinfo=timezone.utc)
                - datetime(2000, 1, 1, tzinfo=timezone.utc)).total_seconds())


def event(kind, now, *args):
    return ','.join(map(str, (kind, now & U32, *args)))


def trace(*events, initial=(7, 123, 0, 0, 0, 0, 0, 0)):
    return ' '.join(map(str, ('tracker', *initial, *events)))


def independent_cases():
    """Each expected map names output rows and fields; no translated state model."""
    cases = []
    def add(line, rows):
        cases.append((line, rows))

    add(trace(event('E', 0, 0, 0, 0), event('Q', 60000),
              event('N', 1000, 0, 0, 0, 0), event('N', 2000, 0, 1, 0, 0)),
        {1: {'active': 0, 'open': 60000}, 3: {'active': 2000, 'pages': 1}})
    for elapsed in (0, 1, 1999, 2000, 59999, 60000, 300000, 300001,
                    0x7fffffff, 0x80000000, U32):
        add(trace(event('E', 10, 0, 0, 0), event('N', 10 + elapsed, 0, 1, 0, 0)),
            {1: {'active': elapsed if elapsed <= 300000 else 0,
                 'pages': int(2000 <= elapsed < 0x80000000),
                 'sessions': int(60000 <= elapsed <= 300000),
                 'session': min(elapsed, 60000) if elapsed <= 300000 else 0}})
    add(trace(event('E', U32 - 999, 0, 0, 0), event('P', 1000)),
        {1: {'active': 2000, 'session': 2000, 'reading': 0, 'open': 0}})
    add(trace(event('E', 0, 0, 0, 0), event('P', 30000), event('E', 100000, 0, 0, 0),
              event('P', 130000), event('L', 140000), event('E', 140000, 0, 0, 0), event('P', 200000)),
        {3: {'sessions': 1, 'session': 60000}, 4: {'session': 0},
         6: {'active': 120000, 'sessions': 2}})
    add(trace(event('E', 0, 0, 0, 0), event('P', 30000), event('S', 30000, 19, 1, 1, 1),
              event('B', 30000), event('E', 40000, 0, 0, 0), event('P', 70000)),
        {3: {'session': 30000, 'folded_at': 0, 'lease': 0, 'folded_once': 0, 'imported': 0},
         5: {'active': 60000, 'sessions': 1}})
    add(trace(event('E', 0, 0, 0, 0), event('E', 60000, 1, 9, 1), event('P', 60000)),
        {1: {'character': 0, 'end': 0, 'active': 0, 'open': 60000},
         2: {'active': 60000, 'sessions': 1}})
    add(trace(event('N', 60000, 0, 9, 1, 0), event('P', 60000), event('R', 60000, 1, 2, 1),
              event('F', 60000), event('E', 60000, 1, 2, 1), event('N', 62000, 1, 3, 1, 0)),
        {0: {'active': 0, 'character': 0, 'end': 0}, 2: {'end': 1}, 5: {'end': 2}})
    add(trace(event('E', 0, 0, 0, 0), event('N', 2000, 0, 1, 0, 1),
              event('R', 3000, 2, 99, 0), event('N', 4000, 2, 100, 0, 0)),
        {1: {'pages': 0}, 3: {'pages': 1, 'active': 4000}})
    # Relocate resets accumulated dwell but does not restart the open interval.
    add(trace(event('E', 0, 0, 0, 0), event('N', 1500, 0, 0, 0, 0),
              event('R', 1600, 0, 7, 0), event('N', 2000, 0, 8, 0, 0)),
        {3: {'pages': 0, 'active': 2000}})
    add(trace(event('E', 0, 0, 0, 0), event('N', 60000, 0, 1, 0, 0),
              initial=(7, 123, 0, 0, U64 - 1, U32, U32, 59999)),
        {1: {'active': U64, 'pages': U32, 'sessions': U32, 'session': 60000}})
    add(trace(event('E', 0, 0, 0, 0), event('P', 1),
              initial=(7, 123, 0, 0, 0, 0, 17, U32)),
        {1: {'active': 1, 'session': U32, 'sessions': 17}})

    noon = utc(2026, 1, 1, 12)
    midnight = utc(2026, 1, 2)
    mid = 43200000
    expiry = 129600000
    add(trace(event('A', 0, noon, 0), event('M', 0, noon, 0), event('M', 0, noon + 10, 0),
              event('M', 0, noon + 11, 0), event('M', 0, noon, 1), event('M', 0, 0, 0),
              event('M', 999, noon + 1, 0)),
        {0: {'midnight': mid, 'expires': expiry, 'midnight_utc': midnight, 'day': 9497, 'state': 1},
         1: {'matched': 1}, 2: {'matched': 1}, 3: {'matched': 0}, 4: {'matched': 0},
         5: {'matched': 0}, 6: {'matched': 1}})
    add(trace(event('A', 0, noon, 0), event('E', mid - 30000, 0, 0, 0),
              event('N', mid + 30000, 0, 1, 0, 0), event('M', mid + 30000, midnight + 30, 0),
              event('O', expiry - 1, 0), event('M', expiry, midnight + 86400, 0), event('O', expiry, 0)),
        {2: {'mark_ms': 30000, 'mark_pages': 0, 'mark_sessions': 1, 'state': 2,
             'active': 60000, 'pages': 1, 'sessions': 1},
         3: {'matched': 1}, 4: {'state': 2, 'mark_ms': 30000}, 5: {'matched': 0}, 6: {'state': 3}})
    # Enter observes midnight while paused; an unfinished paused session stays attributed.
    add(trace(event('A', 0, noon, 0), event('E', mid - 31000, 0, 0, 0), event('P', mid - 1000),
              event('E', mid + 1000, 0, 1, 0)),
        {3: {'mark_ms': 30000, 'mark_sessions': 1, 'state': 2, 'active': 30000}})
    add(trace(event('A', U32 - 500, noon, 0), event('O', U32 - 500 + mid, 60000)),
        {0: {'midnight': (U32 - 500 + mid) & U32}, 1: {'state': 2, 'mark_ms': 60000, 'mark_sessions': 1}})
    add(trace(event('A', 0, noon, 0), event('O', mid - 1, 60000), event('O', mid, 60000),
              initial=(7, 123, 0, 0, U64 - 10, U32, U32, 59999)),
        {1: {'state': 1}, 2: {'state': 2, 'mark_ms': U64, 'mark_pages': U32, 'mark_sessions': U32}})
    add(trace(event('A', 0, noon, 0), event('E', mid - 300001, 0, 0, 0),
              event('N', mid, 0, 1, 0, 0)),
        {2: {'mark_ms': 0, 'mark_sessions': 0, 'state': 2, 'active': 0, 'pages': 1}})
    # Re-anchor clears a crossed mark, and untrusted local years clear it fully.
    add(trace(event('A', 0, noon, 0), event('O', mid, 1), event('A', mid, utc(2025, 12, 31), 0)),
        {2: {'state': 0, 'day': 0, 'mark_ms': 0, 'midnight': 0, 'expires': 0, 'midnight_utc': 0}})
    for start, hours in ((utc(2026, 3, 8, 5), 23), (utc(2026, 11, 1, 4), 25)):
        add(trace(event('A', 100, start, 7), event('O', 100 + hours * 3600000, 0)),
            {0: {'midnight': 100 + hours * 3600000, 'expires': 100 + (hours + 24) * 3600000,
                 'midnight_utc': start + hours * 3600, 'state': 1}, 1: {'state': 2}})
    return cases


def tracker_cases():
    rng = random.Random(0x54524143)
    cases = [line for line, _ in independent_cases()]
    # Heterogeneous sessions: forward/back, menu, repeated locator, jumps,
    # rebuild, wrap, stale/expired millis, end-forgetting, owner marks, anchors.
    for _ in range(180):
        now = rng.choice((0, 100, U32 - 1000, rng.getrandbits(32)))
        spine = character = 0
        events = []
        for _ in range(35):
            now = (now + rng.choice((0, 1, 999, 1000, 1999, 2000, 59999, 60000,
                                     300000, 300001, 0x7fffffff, 0x80000000))) & U32
            kind = rng.choice(('E', 'N', 'N', 'P', 'L', 'R', 'Q', 'B', 'F', 'S', 'A', 'M', 'O'))
            if kind in ('E', 'N', 'R'):
                spine = (spine + rng.randrange(2)) & U32
                character = (character + rng.randrange(3)) & U32
                args = [spine, character, rng.randrange(2)]
                if kind == 'N': args.append(rng.randrange(2))
            elif kind in ('A', 'M'):
                args = [utc(2026, rng.randrange(1, 13), rng.randrange(1, 29), rng.randrange(24),
                            rng.randrange(60), rng.randrange(60)), rng.randrange(15)]
            elif kind == 'O': args = [rng.choice((0, 1, 59999, 60000, 300000, U32))]
            elif kind == 'S': args = [rng.getrandbits(32), rng.randrange(2), rng.randrange(2), rng.randrange(2)]
            else: args = []
            events.append(event(kind, now, *args))
        cases.append(trace(*events, initial=(rng.getrandbits(32), rng.getrandbits(64), 0, 0,
                                            rng.choice((0, U64 - 1)), rng.choice((0, U32)),
                                            rng.choice((0, U32)), rng.choice((0, 1, 59999, 60000, U32)))))
    # All fixed zones, DST-season bounds, leap days, trusted-range edges and wraps.
    for zone in range(15):
        for y, m, d in ((2025, 12, 31), (2026, 1, 1), (2026, 3, 8), (2026, 3, 29),
                        (2026, 4, 5), (2026, 10, 4), (2026, 10, 25), (2026, 11, 1),
                        (2028, 2, 29), (2099, 12, 31), (2100, 1, 1)):
            for hour in (0, 12, 23):
                start = utc(y, m, d, hour, 59, 59)
                now = U32 - 1999
                events = [event('A', now, start, zone)]
                for delta in (0, 999, 1000, 3600000, 86400000, 176400000):
                    events.extend((event('M', now + delta, start + delta // 1000, zone),
                                   event('O', now + delta, 60000)))
                cases.append(trace(*events))
    # statsDate is total for u16; accepted C++ statsClock inputs normalize days.
    for day in (0, 1, 9496, 9497, 10000, 36524, 36525, 65534, 65535):
        cases.append(f'tracker_clock D {day}')
        for hour in (0, 23, 24, 255):
            cases.append(f'tracker_clock F {day} {hour}')
    for year in (0, 2025, 2026, 2028, 2099, 2100, 65535):
        for month in (0, 1, 2, 4, 12, 13, 255):
            for day in (0, 1, 28, 29, 30, 31, 32, 255):
                for hour in (0, 23, 24, 255):
                    cases.append(f'tracker_clock C {year} {month} {day} {hour} 255')
    return cases


def assert_tracker_expectations(lines, outputs):
    """Run against BOTH oracle and Rust output, independently of equality checks."""
    actual = dict(zip(lines, outputs, strict=True))
    for line, expected in independent_cases():
        rows = [dict(zip(FIELDS, map(int, row.split()), strict=True))
                for row in actual[line].split(' ; ')]
        for index, values in expected.items():
            for field, value in values.items():
                if rows[index][field] != value:
                    raise AssertionError(f'tracker independent row {index} {field}: '
                                         f'{rows[index][field]} != {value}: {line}')
    # Independent calendar checks for all direct-clock cases, using datetime,
    # not a second transcription of Hinnant's algorithm.
    epoch = datetime(2000, 1, 1)
    for line, output in zip(lines, outputs, strict=True):
        words = line.split()
        if words[0] != 'tracker_clock': continue
        if words[1] == 'D':
            date = epoch + timedelta(days=int(words[2]))
            expected = [date.year, date.month, date.day, 0, 0]
        elif words[1] == 'F':
            day, hour = map(int, words[2:])
            date = epoch + timedelta(days=day)
            expected = ([day, date.year * 12 + date.month - 1, date.day, hour, 1]
                        if 9497 <= day <= 36524 and hour <= 23 else [0] * 5)
        else:
            year, month, day, hour, _minute = map(int, words[2:])
            expected = [0] * 5
            if 2026 <= year <= 2099 and 1 <= month <= 12 and day >= 1 and hour <= 23:
                date = datetime(year, month, 1) + timedelta(days=day - 1)
                ordinal = (date - epoch).days
                if 9497 <= ordinal <= 36524:
                    expected = [ordinal, date.year * 12 + date.month - 1, date.day, hour, 1]
        if list(map(int, output.split())) != expected:
            raise AssertionError(f'independent tracker calendar mismatch: {line}: {output} != {expected}')
