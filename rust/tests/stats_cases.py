"""Pure stats corpus. Independent assertions supplement full pinned-C++ equality.

Unexecuted staging: no compile, tests, gate or target run during authoring.
All digest bytes are synthetic data and never behavior selectors.
"""
import random
import struct
from datetime import date, timedelta

U16, U32, U64 = (1 << 16) - 1, (1 << 32) - 1, (1 << 64) - 1
FIRST_DAY, FIRST_MONTH = 9497, 24312
DEFAULT_FACTS = (0, 0, 0, 0, U16, 0)


def fnv(data):
    value = 2166136261
    for byte in data:
        value = ((value ^ byte) * 16777619) & U32
    return value


def seal(data, magic=None, generation=None, version=None):
    data = bytearray(data)
    if magic is not None: struct.pack_into('<I', data, 0, magic)
    if version is not None: struct.pack_into('<I', data, 4, version)
    if generation is not None: struct.pack_into('<I', data, 8, generation)
    struct.pack_into('<I', data, len(data) - 4, fnv(data[:-4]))
    return bytes(data)


def month_bytes(month=FIRST_MONTH, pages=0, buckets=(0, 0, 0, 0), days=None, generation=17):
    data = bytearray(104)
    struct.pack_into('<H', data, 12, month)
    struct.pack_into('<I4I31H', data, 16, pages, *buckets, *(days or [0] * 31))
    return seal(data, 0x4d54574e, generation, 1)


def book_bytes(digest=7, facts=DEFAULT_FACTS, generation=17):
    data = bytearray(64)
    data[12:44] = bytes([digest]) * 32
    struct.pack_into('<6H', data, 44, *facts)
    return seal(data, 0x4254574e, generation, 1)


def ledger_bytes(seconds=0, pages=0, sessions=0, finished=0, month=0, oldest=0, newest=0,
                 digest=0, watermark=(0, 0, 0), facts=DEFAULT_FACTS, days=None, buckets=(0, 0, 0, 0), month_pages=0):
    data = bytearray(180)
    struct.pack_into('<HHIIIHH', data, 12, oldest, newest, seconds, pages, sessions, finished, month)
    data[32:64] = bytes([digest]) * 32
    struct.pack_into('<QII6HI4I31H', data, 64, *watermark, *facts, month_pages, *buckets, *(days or [0] * 31))
    return seal(data, 0x5354574e, 17, 1)


def lease(digest=7, ms=0, pages=0, sessions=0, facts=DEFAULT_FACTS):
    return (digest, ms, pages, sessions, *facts)


def seed(ms=0, pages=0, sessions=0, finished=0, opened=1, held=None):
    return ','.join(map(str, (ms, pages, sessions, finished, opened, *(held or lease()))))


def fold(ms=0, pages=0, sessions=0, day=0, hour=0, end=0, mark=0, permille=U16, cross=0, target=None):
    return ','.join(map(str, ('F', ms, pages, sessions, day, hour, end, mark, permille, cross,
                             '-' if target is None else target.hex())))


def trace(*events, initial=None):
    return ' '.join(('stats_ledger', initial or seed(), *events))


def handover(held, final=None):
    return ','.join(map(str, ('H', *held))) + (',' + final if final else '')


def wire(kind, data, digest=7):
    return f'stats_wire {kind} {data.hex() or "-"} {digest}'


def cross(seconds=0, pages=0, sessions=0, finished=0, newest=0, longest=0, backs=()):
    bits = bytearray(92)
    for back in backs: bits[back // 8] |= 1 << (back % 8)
    return ','.join(map(str, (seconds, pages, sessions, finished, newest, longest, bits.hex())))


def summary(ledger, day, history='-', *archives, hour=0):
    return ' '.join(map(str, ('stats_summary', ledger.hex(), day, hour, history, *archives)))


def changed(data, offset, value, width=1):
    data = bytearray(data)
    data[offset:offset + width] = value.to_bytes(width, 'little')
    return seal(data)


def independent_cases():
    """Expected rows are explicit examples, not a second ledger/fold algorithm."""
    cases = []
    def add(line, expected): cases.append((line, expected))
    jan = month_bytes()
    add(trace(), {0: {'seconds': 0, 'digest': 7, 'watermark_ms': 0, 'first': 0, 'start': U16}})
    add(trace(fold(60000, 8, 1, FIRST_DAY, 5, target=jan), initial=seed(opened=0)),
        {1: {'needs': 0, 'seconds': 0, 'month': 0, 'digest': 0, 'watermark_ms': 0}})
    add(trace(initial=seed(U64, U64, U64, U32, held=lease(9, U64, U32, U32))),
        {0: {'seconds': U32, 'pages': U32, 'sessions': U32, 'finished': U16, 'first': 65534}})
    add(trace(initial=seed(opened=1, held=lease(0, 700, 8, 9, (1, 2, 3, 4, 5, 0)))),
        {0: {'digest': 0, 'watermark_ms': 0, 'first': 0, 'start': U16}})
    add(trace(fold(999, day=FIRST_DAY, target=jan), fold(1000, day=FIRST_DAY, target=jan),
              fold(1999, day=FIRST_DAY, target=jan), fold(2000, day=FIRST_DAY, target=jan)),
        {1: {'needs': 0, 'seconds': 0, 'watermark_ms': 0, 'first': 0},
         2: {'needs': 1, 'seconds': 1, 'watermark_ms': 1000, 'day1': 1, 'night': 1},
         3: {'seconds': 1, 'watermark_ms': 1000}, 4: {'seconds': 2, 'day1': 2}})
    for counters in ((9000, 4, 5), (10000, 3, 5), (10000, 4, 4)):
        add(trace(fold(*counters, day=FIRST_DAY, end=1, target=jan),
                  initial=seed(10000, 4, 5, held=lease(7, 10000, 4, 5))),
            {1: {'needs': 0, 'seconds': 10, 'pages': 4, 'sessions': 5,
                 'watermark_ms': counters[0], 'watermark_pages': counters[1], 'watermark_sessions': counters[2],
                 'end': FIRST_DAY, 'finished': 1, 'month': 0}})
    add(trace(fold(60000, 3, 1, FIRST_DAY, 5, permille=124),
              fold(61000, 4, 1, FIRST_DAY + 1, 17, target=jan)),
        {1: {'needs': 1, 'seconds': 60, 'month': 0, 'first': FIRST_DAY, 'last': FIRST_DAY, 'start': 124},
         2: {'seconds': 61, 'month': FIRST_MONTH, 'day2': 1, 'evening': 1, 'month_pages': 1, 'last': FIRST_DAY + 1}})
    add(trace(fold(60000, 3, 1, FIRST_DAY, target=month_bytes(FIRST_MONTH + 1))),
        {1: {'needs': 1, 'month': 0, 'seconds': 60, 'first': FIRST_DAY}})
    add(trace(fold(0, 1, 0, FIRST_DAY, 12, target=jan), fold(0, 1, 1, FIRST_DAY + 1, 21, target=jan)),
        {1: {'needs': 1, 'month': FIRST_MONTH, 'day1': 0, 'month_pages': 1},
         2: {'sessions': 1, 'day2': 0, 'last': FIRST_DAY + 1}})
    # Start one millisecond below the u64 maximum with saturated totals/days/buckets.
    saturated = month_bytes(pages=U32, buckets=(U32, 0, 0, 0), days=[U16] + [0] * 30)
    add(trace(fold(U64, U32, U32, FIRST_DAY, 5, target=saturated),
              initial=seed(U64, U64, U64, U32, held=lease(7, 0, 0, 0))),
        {1: {'seconds': U32, 'pages': U32, 'sessions': U32, 'finished': U16, 'day1': U16,
             'morning': U32, 'month_pages': U32, 'watermark_ms': U64 - U64 % 1000}})
    add(trace(fold(1000, 1, 1), initial=seed(U64, U64, U64)),
        {1: {'seconds': U32, 'pages': U32, 'sessions': U32}})
    add(trace(fold(mark=1), fold(day=FIRST_DAY, mark=1), fold(mark=2), fold(end=1), fold(mark=2)),
        {1: {'finished': 1, 'manual': 65534, 'flags': 1},
         2: {'finished': 1, 'manual': 65534}, 3: {'finished': 0, 'flags': 0, 'manual': 0, 'end': 0},
         4: {'finished': 1, 'end': 65534}, 5: {'finished': 0, 'end': 0}})
    add(trace(fold(end=1, mark=2)), {1: {'finished': 0, 'end': 0}})
    add(trace(fold(end=1), fold(cross=1), fold(mark=2), fold(end=1)),
        {1: {'finished': 1}, 2: {'finished': 0, 'flags': 2},
         3: {'finished': 0, 'end': 0, 'flags': 2}, 4: {'finished': 0, 'end': 65534}})
    unknown = (U16, U16, U16, 0, U16, 0)
    add(trace(fold(mark=2), initial=seed(finished=0, held=lease(facts=unknown))),
        {1: {'finished': 0, 'end': U16, 'first': U16}})
    add(trace(fold(mark=2), initial=seed(finished=0, held=lease(facts=(0, 0, FIRST_DAY, 0, 500, 0)))),
        {1: {'finished': 0, 'end': 0}})
    add(trace(handover(lease(9, 12000, 2, 1), fold(60000, 5, 1, FIRST_DAY, 12, end=1, target=jan))),
        {1: {'needs': 1, 'seconds': 60, 'pages': 5, 'sessions': 1, 'finished': 1, 'digest': 9,
             'watermark_ms': 12000, 'watermark_pages': 2, 'first': 65534, 'end': 0, 'day1': 60}})
    add(trace(handover(lease(0)), fold(60000, 1, 1)),
        {1: {'digest': 0, 'seconds': 0}, 2: {'seconds': 0, 'watermark_ms': 0}})
    # Independent exact encoders, including all fixed reserved bytes and FNV.
    for number in (FIRST_MONTH, 2028 * 12 + 1, 25199):
        days = [1, 59, 60, U16] + [0] * 27
        data = month_bytes(number, 83, (1, 20, 300, U32), days, U32)
        add(' '.join(map(str, ('stats_make', 'M', number, 83, 1, 20, 300, U32, *days, U32))), data.hex())
    facts = (FIRST_DAY, 65534, U16, FIRST_DAY + 3, 1000, 3)
    add(' '.join(map(str, ('stats_make', 'B', 37, *facts, U32))), book_bytes(37, facts, U32).hex())
    for kind, data, digest in [('L', ledger_bytes(digest=7), 7), ('M', jan, 7), ('B', book_bytes(), 7)]:
        state = 1 if kind == 'L' else 0
        add(wire(kind, data, digest), f'0 1 {state} {data.hex()}')
        invalid = []
        # Extents, checksum, magic and independently resealed unsupported/old versions.
        for length in (0, 1, 15, len(data) - 1, len(data) + 1):
            truncated = data[:length] if length <= len(data) else data + b'\0'
            invalid.append((truncated, 1))
        invalid.extend([(data[:-1] + bytes([data[-1] ^ 1]), 1),
                        (changed(data, 0, 0), 1), (seal(data, version=0), 1),
                        (seal(data, version=2), 2), (seal(data, version=U32), 2)])
        reserved = (174,) if kind == 'L' else (14, 15, 98, 99) if kind == 'M' else (56, 57, 58, 59)
        invalid.extend((changed(data, offset, 1), 0) for offset in reserved)
        for corrupt, classification in invalid:
            add(wire(kind, corrupt, digest), f'{classification} 0 {3 if kind == "L" else 0} -')
    for facts in ((FIRST_DAY, 65534, U16, FIRST_DAY + 1, 1000, 3),
                  (0, 0, 0, 0, 0, 0), (36524, 36524, 36524, 36524, U16, 1)):
        for kind, data in (('B', book_bytes(facts=facts)), ('L', ledger_bytes(digest=7, facts=facts))):
            add(wire(kind, data), f'0 1 {1 if kind == "L" else 0} {data.hex()}')
    leap_days = [0] * 31; leap_days[28] = 60
    leap = month_bytes(2028 * 12 + 1, days=leap_days)
    add(wire('M', leap), f'0 1 0 {leap.hex()}')
    # Invalid day/sentinels, fact flags/permille, missing lease consistency.
    for kind, data, base in [('B', book_bytes(), 44), ('L', ledger_bytes(digest=7), 80)]:
        for offset in (0, 2, 4, 6):
            for value in (1, FIRST_DAY - 1, 36525, 65533):
                add(wire(kind, changed(data, base + offset, value, 2)), f'0 0 {3 if kind == "L" else 0} -')
        for offset, value in ((8, 1001), (10, 4), (10, 65535)):
            add(wire(kind, changed(data, base + offset, value, 2)), f'0 0 {3 if kind == "L" else 0} -')
    for data in (ledger_bytes(watermark=(1, 0, 0)), ledger_bytes(facts=(FIRST_DAY, 0, 0, 0, U16, 0)),
                 ledger_bytes(month=FIRST_MONTH), ledger_bytes(month=FIRST_MONTH, oldest=FIRST_MONTH + 1, newest=FIRST_MONTH + 1),
                 ledger_bytes(month=FIRST_MONTH, oldest=FIRST_MONTH, newest=FIRST_MONTH - 1),
                 ledger_bytes(month=FIRST_MONTH - 1, oldest=FIRST_MONTH - 1, newest=FIRST_MONTH),
                 ledger_bytes(month_pages=1), ledger_bytes(days=[1] + [0] * 30)):
        add(wire('L', data), '0 0 3 -')
    for number in (0, FIRST_MONTH - 1, 25200, 65535): add(wire('M', month_bytes(number)), '0 0 0 -')
    for number, bad_index in ((FIRST_MONTH + 1, 28), (2028 * 12 + 1, 29), (FIRST_MONTH + 3, 30)):
        days = [0] * 31; days[bad_index] = 1
        add(wire('M', month_bytes(number, days=days)), '0 0 0 -')
    for digest in (0, 8): add(wire('B', book_bytes(), digest), '0 0 0 -')
    # Classification accepts a checksum-valid 16-byte header independently of codec extent.
    for version, classification in ((0, 1), (1, 0), (2, 2), (U32, 2)):
        header = seal(bytearray(16), 0x5354574e, 0, version)
        add(f'stats_wire C {header.hex()} {0x5354574e}', str(classification))
    # History bit 729 is readable; padding bits and bit 730 are outside the history.
    for newest, backs, probes, expected in (
            (0, (), (0, 1, U32), '1 0 0 0'), (0, (0,), (0, 1, U32), '0 1 0 0'),
            (10000, (0, 1, 729, 730, 735), (10000, 9999, 9271, 9270, 10001, 0), '0 1 1 1 0 0 0'),
            (U32, (0, 729), (U32, U32 - 729, U32 - 730, 0), '0 1 1 0 0')):
        bits = cross(newest=newest, backs=backs).split(',')[-1]
        add(' '.join(map(str, ('stats_make', 'H', newest, bits, *probes))), expected)
    # Bucket edges and holiday dates are independent calendar facts.
    for hour in (0, 4, 5, 11, 12, 16, 17, 20, 21, 23, 24, 255):
        bucket = 0 if 5 <= hour < 12 else 1 if 12 <= hour < 17 else 2 if 17 <= hour < 21 else 3
        for local_date, hats in ((date(2026, 10, 30), (0, 0)), (date(2026, 10, 31), (1, 0)),
                                  (date(2026, 12, 24), (0, 1)), (date(2026, 12, 25), (0, 1)),
                                  (date(2026, 12, 26), (0, 0))):
            day = (local_date - date(2000, 1, 1)).days
            hat = hats if hour <= 23 else (0, 0)
            add(f'stats_make C {day} {hour}', f'{bucket} {hat[0]} {hat[1]}')
    # Summary: today unread keeps yesterday's streak open; day 59 s is no streak.
    days = [0] * 31; days[0:9] = [60, 60, 0, 60, 60, 60, 60, 59, 0]
    l = ledger_bytes(seconds=479, pages=9, sessions=8, finished=2, month=FIRST_MONTH,
                     oldest=FIRST_MONTH, newest=FIRST_MONTH, days=days)
    add(summary(l, FIRST_DAY + 8), '479 8 9 2 1 0 0 299 0 4 171 | ')
    add(summary(l, FIRST_DAY + 7), '479 8 9 2 1 0 59 359 4 4 171 | ')
    add(summary(l, FIRST_DAY + 6), '479 8 9 2 1 0 60 360 4 4 171 | ')
    add(summary(l, FIRST_DAY + 5), '479 8 9 2 1 0 60 300 3 4 171 | ')
    days = [60, 60, 0] + [0] * 28
    l2 = ledger_bytes(month=FIRST_MONTH, oldest=FIRST_MONTH, newest=FIRST_MONTH, days=days)
    add(summary(l2, FIRST_DAY + 2), '0 0 0 0 1 0 0 120 2 2 171 | ')
    add(summary(l2, 0), '0 0 0 0 0 0 0 0 0 0 171 | ')
    add(summary(l2, FIRST_DAY + 6), '0 0 0 0 1 0 0 120 0 2 171 | ')
    # Imported totals add once, saturate, and do not create Newt reading seconds.
    add(summary(ledger_bytes(seconds=U32, pages=U32, sessions=U32, finished=U16), 0,
                cross(1, 1, 1, 1, longest=91)), f'{U32} {U32} {U32} {U16} 0 1 0 0 0 0 0 | ')
    add(summary(ledger_bytes(), FIRST_DAY, cross(7, 8, 9, 10, longest=91)), '7 9 8 10 1 1 0 0 0 91 0 | ')
    add(summary(l2, FIRST_DAY + 2, cross(newest=FIRST_DAY + 2, backs=(0,), longest=1)),
        '0 0 0 0 1 1 0 120 3 3 7 | ')
    # Full all-time scan, clock moved backward, gaps/failure/wrong archive, bounded callback trace.
    jan_days = [0] * 31; jan_days[28:31] = [60, 60, 60]
    feb_days = [60, 60, 0] + [0] * 28
    archive = month_bytes(days=jan_days)
    latest = ledger_bytes(month=FIRST_MONTH + 1, oldest=FIRST_MONTH, newest=FIRST_MONTH + 1, days=feb_days)
    for mode, expected in ((archive.hex(), '0 0 0 0 1 0 0 300 5 5 171 | 24312'),
                           ('M', '0 0 0 0 1 0 0 120 2 2 171 | 24312'),
                           ('U', '0 0 0 0 1 0 0 120 2 2 171 | 24312'),
                           (month_bytes(FIRST_MONTH + 2, days=jan_days).hex(), '0 0 0 0 1 0 0 120 2 2 171 | 24312')):
        add(summary(latest, FIRST_DAY + 33, '-', f'{FIRST_MONTH}:{mode}'), expected)
    add(summary(latest, FIRST_DAY + 30, '-', f'{FIRST_MONTH}:{archive.hex()}'),
        '0 0 0 0 1 0 60 180 3 5 171 | 24312')
    # Today after the ledger's range loads its one archive, no months beyond that range.
    add(summary(latest, FIRST_DAY + 60, '-', f'{FIRST_MONTH}:{archive.hex()}'),
        '0 0 0 0 1 0 0 0 0 5 171 | 24312')
    return cases


def stats_cases():
    cases = [line for line, _ in independent_cases()]
    rng = random.Random(14076)
    months = [(9497, FIRST_MONTH), (10227, 2028 * 12), (36524, 25199)]
    for _ in range(60):
        events = []
        ms, pages, sessions = rng.randrange(10000), rng.randrange(10), rng.randrange(3)
        for index in range(12):
            if index % 5 == 4: ms, pages, sessions = rng.randrange(1000), 0, 0
            else: ms += rng.choice((0, 1, 999, 1000, 59999, 60000, 90000)); pages += rng.randrange(4); sessions += rng.randrange(2)
            day, number = rng.choice(months)
            events.append(fold(ms, pages, sessions, day, rng.choice((0, 4, 5, 11, 12, 16, 17, 20, 21, 23)),
                               rng.randrange(2), rng.randrange(3), rng.choice((0, 500, 1000, U16)), rng.randrange(2),
                               rng.choice((None, month_bytes(number), month_bytes(FIRST_MONTH)))))
        cases.append(trace(*events))
    return cases


def ledger_fields(output):
    needs, encoded = output.split(':')
    data = bytes.fromhex(encoded)
    result = {'needs': int(needs), 'digest': data[32]}
    for field, offset, form in (
            ('oldest', 12, 'H'), ('newest', 14, 'H'), ('seconds', 16, 'I'), ('pages', 20, 'I'),
            ('sessions', 24, 'I'), ('finished', 28, 'H'), ('month', 30, 'H'),
            ('watermark_ms', 64, 'Q'), ('watermark_pages', 72, 'I'), ('watermark_sessions', 76, 'I'),
            ('first', 80, 'H'), ('last', 82, 'H'), ('end', 84, 'H'), ('manual', 86, 'H'),
            ('start', 88, 'H'), ('flags', 90, 'H'), ('month_pages', 92, 'I'),
            ('morning', 96, 'I'), ('afternoon', 100, 'I'), ('evening', 104, 'I'), ('night', 108, 'I'),
            ('day1', 112, 'H'), ('day2', 114, 'H')):
        result[field] = struct.unpack_from('<' + form, data, offset)[0]
    return result


def assert_stats_expectations(lines, outputs):
    actual = dict(zip(lines, outputs, strict=True))
    for line, expected in independent_cases():
        observed = actual[line]
        if isinstance(expected, str):
            # Empty callback traces may lose trailing whitespace in the host runner.
            if observed.rstrip() != expected.rstrip():
                raise AssertionError(f'independent stats output: {observed!r} != {expected!r}: {line}')
        else:
            rows = [ledger_fields(row) for row in observed.split(' ; ')]
            for index, fields in expected.items():
                for key, value in fields.items():
                    if rows[index][key] != value:
                        raise AssertionError(f'independent stats row {index} {key}: {rows[index][key]} != {value}: {line}')
