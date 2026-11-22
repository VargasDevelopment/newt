"""Inflater protocol validation and independently authored corpus."""
from pathlib import Path
import re
import zlib

if not __debug__:
    raise RuntimeError('strict protocol validation requires Python assertions')

ROOT = Path(__file__).resolve().parent
NAT = r'(?:0|[1-9][0-9]*)'
HEX = r'(?:-|(?:[0-9a-f]{2})+)'
REQUEST = re.compile(rf'CALL ({NAT}) ({NAT}) ({NAT}) ({HEX})\Z')
RESULT = re.compile(rf'RESULT (-4|-3|-2|-1|0|1|2) ({NAT}) ({NAT}) ({NAT}) ({HEX})\Z')


def parse_request(raw):
    assert isinstance(raw, bytes) and raw.endswith(b'\n') and b'\r' not in raw
    lines = [line + '\n' for line in raw.decode('ascii').split('\n')[:-1]]
    assert all(line.endswith('\n') and line.count('\n') == 1 for line in lines)
    assert re.fullmatch(rf'TINFL1 (32|64) ({NAT})\n', lines[0])
    calls = []
    for line in lines[1:]:
        if line == 'RESET\n':
            continue
        m = REQUEST.fullmatch(line[:-1]); assert m, line
        flags, offset, capacity = map(int, m.group(1, 2, 3))
        data = b'' if m[4] == '-' else bytes.fromhex(m[4])
        calls.append((flags, offset, capacity, data))
    return calls


def parse_result(raw, calls):
    # Validate the COMPLETE raw-LF grammar and all local counters before any
    # semantic comparison; splitlines alone would silently accept CRLF/VT/FF.
    assert raw.endswith(b'\n') and b'\r' not in raw
    lines = raw.decode('ascii').split('\n'); assert lines.pop() == ''
    assert len(lines) == len(calls)
    parsed = []
    for line, (_, _, capacity, data) in zip(lines, calls):
        m = RESULT.fullmatch(line); assert m, repr(line)
        status, consumed, produced, adler = map(int, m.group(1, 2, 3, 4))
        value = b'' if m[5] == '-' else bytes.fromhex(m[5])
        assert consumed <= len(data) and produced <= capacity
        assert produced == len(value) and adler <= 0xffffffff
        parsed.append((status, consumed, produced, adler, value))
    return parsed


def compressed(data, level, strategy, wrapped):
    c = zlib.compressobj(level, zlib.DEFLATED, 15 if wrapped else -15, 8, strategy)
    return c.compress(data) + c.flush()


def fixtures():
    yield 'hand-empty-fixed', bytes.fromhex('0300'), b'', False
    yield 'hand-empty-fixed-with-tail', bytes.fromhex('0300deadbeef'), b'', False
    yield 'hand-empty-zlib', bytes.fromhex('789c030000000001'), b'', True
    yield 'hand-empty-zlib-bad-checksum', bytes.fromhex('789c030000000002'), None, True
    yield 'hand-stored-abc', bytes.fromhex('010300fcff616263'), b'abc', False
    sources = {
        'overlap-distance-one': b'a' * 70000,
        'all-byte-values': bytes(range(256)) * 300,
        'heterogeneous-markup': (b'<p lang="en">reader &amp; text</p>\n' * 700 + bytes(range(251)) * 70),
        'long-code-distribution': b''.join(bytes([i]) * (1 + i % 23) for i in range(256)) * 19,
    }
    for wrapped in (False, True):
        c = zlib.compressobj(6, zlib.DEFLATED, 15 if wrapped else -15)
        data = b'<first>multi block</first>' * 300 + bytes(range(256)) * 200
        cut = 1000
        packed = c.compress(data[:cut]) + c.flush(zlib.Z_SYNC_FLUSH) + c.compress(data[cut:]) + c.flush()
        yield f'multi-block-sync-{wrapped}', packed, data, wrapped
    for name, data in sources.items():
        for kind, level, strategy in [('stored',0,zlib.Z_DEFAULT_STRATEGY), ('fixed',6,zlib.Z_FIXED), ('dynamic',6,zlib.Z_DEFAULT_STRATEGY)]:
            for wrapped in (False,True):
                packed = compressed(data,level,strategy,wrapped)
                yield f'{name}-{kind}-{wrapped}', packed, data, wrapped
                yield f'{name}-{kind}-{wrapped}-truncated', packed[:-1], None, wrapped
                if wrapped:
                    yield f'{name}-{kind}-checksum', packed[:-1] + bytes([packed[-1]^1]), None, True
    for name, packed, wrapped in [('reserved-block',b'\x07',False),('stored-complement',bytes.fromhex('010100ffff41'),False),('header-method',bytes.fromhex('789d030000000001'),True),('header-dictionary',bytes.fromhex('7820030000000001'),True)]:
        yield name, packed, None, wrapped
