"""Unexecuted additions for rust/tests/corpus.py; only Python's standard library.

Call clock_cases() to append protocol inputs. Compare both executable responses
to clock_goldens() as well as to each other. Input calendar fields are always
valid UTC in 2000..2199; invalid enum bytes and invalid text are separate cases.
"""
import calendar
from datetime import datetime, timedelta


# Durable order, Settings text, telemetry ID, standard minutes, DST family.
ZONES = (
    ('UTC', 'utc', 0, None),
    ('Hawaii', 'hawaii', -600, None),
    ('Alaska', 'alaska', -540, 'us'),
    ('US Pacific', 'us-pacific', -480, 'us'),
    ('US Mountain', 'us-mountain', -420, 'us'),
    ('Arizona', 'arizona', -420, None),
    ('US Central', 'us-central', -360, 'us'),
    ('US Eastern', 'us-eastern', -300, 'us'),
    ('UK/Ireland', 'uk-ireland', 0, 'uk'),
    ('Central Europe', 'central-europe', 60, 'cet'),
    ('Eastern Europe', 'eastern-europe', 120, 'eet'),
    ('India', 'india', 330, None),
    ('China/Singapore', 'china-singapore', 480, None),
    ('Japan/Korea', 'japan-korea', 540, None),
    ('Australia Eastern', 'australia-eastern', 600, 'au'),
)

# Month, ordinal Sunday (-1 = last), local hour before the change.
RULES = {
    'us': ((3, 2, 2), (11, 1, 2)),
    'uk': ((3, -1, 1), (10, -1, 2)),
    'cet': ((3, -1, 2), (10, -1, 3)),
    'eet': ((3, -1, 3), (10, -1, 4)),
    'au': ((10, 1, 2), (4, 1, 3)),
}

INVALID_TEXT = (
    b'', b'--:--', b'24:00', b'99:59', b'12:60', b'12:99', b'1:05',
    b'12:05 ', b' 12:05', b'ab:cd', b'12-05', b'12:0', b'12:', b'12',
    b'1', b'00:0a', b'0a:00', b'12:05\n', b'12:05:00', b'\xff',
    b'12:05\xff', '１２:０５'.encode('utf-8'), b'1' * 64,
)


def _hx(text):
    return text.hex() if text else '_'


def _time(zone, utc):
    return f'clock_time {zone} {utc.year} {utc.month} {utc.day} {utc.hour} {utc.minute}'


def _transition(year, at, before_offset):
    month, ordinal, hour = at
    weeks = calendar.Calendar(firstweekday=calendar.MONDAY).monthdayscalendar(year, month)
    sundays = [week[calendar.SUNDAY] for week in weeks
               if week[calendar.SUNDAY]]
    day = sundays[-1] if ordinal == -1 else sundays[ordinal - 1]
    return datetime(year, month, day, hour) - timedelta(minutes=before_offset)


def clock_cases():
    cases = []
    for raw in range(256):
        cases.extend([f'zone_info {raw}', f'clock_format_info {raw}'])

    # Independent Gregorian calendar library finds Sundays in every supported
    # year, exercising last-Sunday months containing both four and five Sundays.
    for year in range(2000, 2200):
        for zone, (_, _, offset, family) in enumerate(ZONES):
            if family is None:
                continue
            for at, before_offset in zip(RULES[family], (offset, offset + 60)):
                transition = _transition(year, at, before_offset)
                for delta in (-1, 0, 1):
                    cases.append(_time(zone, transition + timedelta(minutes=delta)))

    # Cross local midnight for every month and zone, including both possible
    # offsets. These sample 2000/2004/2028 leap years and the non-leap 2100.
    for year in (2000, 2001, 2004, 2026, 2028, 2099, 2100, 2104, 2199):
        for month in range(1, 13):
            boundary = datetime(year, month, 1)
            for zone, (_, _, offset, family) in enumerate(ZONES):
                offsets = (offset,) if family is None else (offset, offset + 60)
                for local_offset in offsets:
                    utc = boundary - timedelta(minutes=local_offset)
                    for delta in (-1, 0, 1):
                        candidate = utc + timedelta(minutes=delta)
                        if 2000 <= candidate.year <= 2199:
                            cases.append(_time(zone, candidate))

    for raw in range(256):
        for utc in (datetime(2000, 1, 1), datetime(2199, 12, 31, 23, 59),
                    datetime(2026, 1, 15, 12), datetime(2026, 7, 15, 12)):
            cases.append(_time(raw, utc))

    for hour in range(24):
        for minute in range(60):
            text = f'{hour:02d}:{minute:02d}'.encode('ascii')
            for raw in (0, 1, 2, 255):
                cases.append(f'clock_text {raw} {_hx(text)}')
    for raw in (0, 1, 2, 255):
        cases.append(f'clock_text {raw} null')
        cases.extend(f'clock_text {raw} {_hx(text)}' for text in INVALID_TEXT)
    # Include independent sentinels even if the sampling families above change.
    cases.extend(clock_goldens())
    return list(dict.fromkeys(cases))


def _text_response(source, text):
    # The C++ writer touches only text and its terminator. The spare ninth byte
    # remains a5 when a one-digit hour produces seven displayed bytes.
    if source == 'output':
        output = text + b'\0' + b'\xa5' * (8 - len(text))
    else:
        output = b'\xa5' * 9
    encoded = 'null' if text is None else text.hex() or '-'
    return f'{source} {encoded} {output.hex()}'


def clock_goldens():
    """Independent semantic sentinels; not values learned from either binary."""
    expected = {}
    for raw in range(256):
        label, telemetry, _, _ = ZONES[raw if raw < len(ZONES) else 0]
        expected[f'zone_info {raw}'] = f'{int(raw < 15)} {label.encode().hex()} {telemetry.encode().hex()}'
        label, telemetry = ('12-hour', '12h') if raw == 1 else ('24-hour', '24h')
        expected[f'clock_format_info {raw}'] = f'{int(raw < 2)} {label.encode().hex()} {telemetry.encode().hex()}'
        if raw >= 15:
            expected[f'clock_time {raw} 2000 1 1 0 0'] = '2000 1 1 0 0'
            expected[f'clock_time {raw} 2199 12 31 23 59'] = '2199 12 31 23 59'

    # Inclusive start/exclusive end, opposite hemispheres, fixed and half-hour
    # zones, both supported-range edges, and the Gregorian century exception.
    expected.update({
        'clock_time 6 2026 3 8 7 59': '2026 3 8 1 59',
        'clock_time 6 2026 3 8 8 0': '2026 3 8 3 0',
        'clock_time 6 2026 11 1 6 59': '2026 11 1 1 59',
        'clock_time 6 2026 11 1 7 0': '2026 11 1 1 0',
        'clock_time 9 2026 3 29 0 59': '2026 3 29 1 59',
        'clock_time 9 2026 3 29 1 0': '2026 3 29 3 0',
        'clock_time 9 2026 10 25 0 59': '2026 10 25 2 59',
        'clock_time 9 2026 10 25 1 0': '2026 10 25 2 0',
        'clock_time 14 2026 4 4 15 59': '2026 4 5 2 59',
        'clock_time 14 2026 4 4 16 0': '2026 4 5 2 0',
        'clock_time 14 2026 10 3 15 59': '2026 10 4 1 59',
        'clock_time 14 2026 10 3 16 0': '2026 10 4 3 0',
        'clock_time 5 2026 7 1 12 0': '2026 7 1 5 0',
        'clock_time 4 2026 7 1 12 0': '2026 7 1 6 0',
        'clock_time 11 2000 2 28 18 30': '2000 2 29 0 0',
        'clock_time 11 2100 2 28 18 30': '2100 3 1 0 0',
        'clock_time 1 2000 1 1 0 0': '1999 12 31 14 0',
        'clock_time 14 2199 12 31 23 59': '2200 1 1 10 59',
    })

    for raw in (0, 1, 2, 255):
        expected[f'clock_text {raw} null'] = _text_response('null', None)
        for text in INVALID_TEXT:
            expected[f'clock_text {raw} {_hx(text)}'] = _text_response('input', text)
        for hour in range(24):
            for minute in range(60):
                text = f'{hour:02d}:{minute:02d}'.encode('ascii')
                if raw == 1:
                    shown = f'{(hour + 11) % 12 + 1}:{minute:02d} {"AM" if hour < 12 else "PM"}'.encode('ascii')
                    response = _text_response('output', shown)
                else:
                    response = _text_response('input', text)
                expected[f'clock_text {raw} {_hx(text)}'] = response
    return expected
