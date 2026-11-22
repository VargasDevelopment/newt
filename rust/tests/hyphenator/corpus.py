"""Independent synthetic FIBH fixtures; importing builds cases but executes nothing.
No language data. Expected outputs come from explicit boundary/weight reasoning,
not from either implementation or a duplicate binary-range matcher.
"""
from dataclasses import dataclass
import argparse
import struct
import subprocess


def table(patterns, maximum=None):
    entries = sorted((k.encode('utf-8') if isinstance(k, str) else k, bytes(v)) for k, v in patterns)
    assert len({k for k, _ in entries}) == len(entries)
    blob = bytearray()
    offsets = []
    for key, values in entries:
        assert 0 < len(key) <= 255 and len(values) == len(key) + 1
        offsets.append(len(blob))
        blob += bytes([len(key)]) + key + values
    maximum = max((len(k) for k, _ in entries), default=0) if maximum is None else maximum
    return b'FIBH\x01\x00' + bytes([maximum, 0]) + struct.pack('<I', len(entries)) + b''.join(struct.pack('<I', n) for n in offsets) + blob


def hx(data):
    return data.hex() or '-'


def breaks(points):
    return 'B ' + ' '.join(map(str, [len(points), *points]))


@dataclass
class Case:
    name: str
    commands: list
    expected: list
    native: bool = True


def matching(name, data, word, points, cap=255, native=True):
    word = word.encode() if isinstance(word, str) else word
    count = int.from_bytes(data[8:12], 'little')
    return Case(name, ['I ' + hx(data), f'B {cap} {hx(word)}'],
                [f'I 1 {int(count > 0)}', breaks(points)], native)


def cases():
    result = []
    basic = table([('abcdef', [0, 0, 1, 1, 0, 0, 0])])
    result.append(matching('basic', basic, 'abcdef', [2, 3]))
    result.append(matching('ascii-fold', basic, 'ABCDEF', [2, 3]))
    result.append(matching('cap-zero', basic, 'abcdef', [], 0))
    result.append(matching('cap-one', basic, 'abcdef', [2], 1))
    result.append(matching('miss', basic, 'ghijkl', []))
    result.append(matching('maxlen-cut', table([('abcdef', [0, 0, 1, 1, 0, 0, 0])], 5), 'abcdef', []))
    result.append(matching('maxlen-zero', table([('abcdef', [0, 0, 1, 1, 0, 0, 0])], 0), 'abcdef', []))
    result.append(matching('empty-table', table([]), 'abcdef', []))
    result.append(matching('sorted-range', table([('a', [0, 0]), ('aaa', [0]*4), ('abcdef', [0, 0, 1, 0, 0, 0, 0]), ('abcdeg', [0]*7), ('b', [0, 0]), ('z', [0, 0])]), 'abcdef', [2]))
    result.append(matching('maxmerge-even', table([('bc', [0, 1, 0]), ('abcd', [0, 0, 2, 3, 0])]), 'abcdef', [3]))
    result.append(matching('maxmerge-high-odd', table([('bc', [0, 9, 0]), ('abcd', [0, 0, 2, 0, 0])]), 'abcdef', [2]))
    result.append(matching('left-dot', table([('.ab', [0, 0, 0, 1])]), 'abcdef', [2]))
    result.append(matching('right-dot', table([('def.', [1, 0, 0, 0, 0])]), 'abcdef', [3]))
    for n in [0, 1, 4, 5, 6, 61, 62, 63, 64]:
        # Single byte key applies odd weights at every inter-byte boundary.
        expected = list(range(2, n - 2)) if 5 <= n <= 62 else []
        result.append(matching(f'word-bytes-{n}', table([('a', [1, 1])]), 'a'*n, expected))
    # Explicit independent source mapping families. Include every uppercase site.
    pairs = [(c, c + 32) for c in range(0x41, 0x5b)]
    pairs += [(c, c + 32) for c in range(0xc0, 0xdf) if c != 0xd7]
    pairs += [(c, c + 1) for c in range(0x100, 0x138, 2)]
    pairs += [(c, c + 1) for c in range(0x139, 0x149, 2)]
    pairs += [(c, c + 1) for c in range(0x14a, 0x178, 2)]
    pairs += [(0x178, 0xff)] + [(c, c + 1) for c in range(0x179, 0x17f, 2)]
    pairs += [(c, c + 32) for c in range(0x391, 0x3aa) if c != 0x3a2]
    pairs += [(c, c + 32) for c in range(0x410, 0x430)]
    pairs += [(c, c + 80) for c in range(0x400, 0x410)]
    pairs += [(c, c) for c in [0xd7, 0x3a2, 0x138, 0x149, 0x17f, 0x3ac, 0x450, 0x4ff, 0x4e00, 0xffff]]
    for upper, lower in pairs:
        word = chr(upper)*6
        key = (chr(lower)*6).encode()
        width = len(chr(upper).encode())
        result.append(matching(f'fold-{upper:04x}', table([(key, [1]*(len(key)+1))]), word, [2*width, 3*width]))
    # Odd values on continuation bytes must never become emitted offsets.
    key = 'é'*6
    result.append(matching('continuation-filter', table([(key, [1]*13)]), key, [4, 6]))
    result.append(matching('character-minima', table([('é', [1, 1, 1])]), 'é'*5, [4]))
    for n in [3, 4]:
        result.append(matching(f'few-characters-{n}', table([('é', [1, 1, 1])]), 'é'*n, []))
    result.append(matching('three-byte', table([('漢', [1]*4)]), '漢'*6, [6, 9]))
    # Invalid words NEVER go to C++: native may leave marked bytes uninitialized.
    for name, word in [('four-byte', '😀abcde'.encode()), ('truncated', b'abcde\xc3'),
                       ('continuation', b'ab\x80cde'), ('overlong', b'ab\xc0\xafcde'),
                       ('surrogate', b'ab\xed\xa0\x80cde'), ('bad-cont', b'ab\xe1xxcde')]:
        result.append(matching('reject-word-'+name, basic, word, [], native=False))
    # Only header failures with defined arithmetic enter native oracle.
    bad_headers = [None, b'', b'FIBH', b'NOPE'+basic[4:], basic[:4]+b'\x02\x00'+basic[6:]]
    bad_headers += [basic[:n] for n in range(12, 16)]
    for i, bad in enumerate(bad_headers):
        result.append(Case(f'failed-reinit-{i}', ['R', 'B 255 616263646566', 'I '+hx(basic), 'B 255 616263646566', 'I '+('null' if bad is None else hx(bad)), 'R', 'B 255 616263646566', 'I '+hx(basic), 'B 255 616263646566'],
                           ['R 0', 'B 0', 'I 1 1', 'B 2 2 3', 'I 0 0', 'R 0', 'B 0', 'I 1 1', 'B 2 2 3']))
    # Header succeeds on corrupt records: readiness is not record validity.
    corrupt = [basic[:n] for n in range(16, len(basic))]
    corrupt += [basic[:12]+b'\xff'*4+basic[16:], basic[:16]+b'\xff'+basic[17:],
                table([('a', [0, 1]), ('b', [0, 1])])[:12] + struct.pack('<II', 4, 0) + bytes([1, 97, 0, 1, 1, 98, 0, 1])]
    corrupt += [b'FIBH\x01\x00\x06\x00'+struct.pack('<I', 1)+struct.pack('<I', 0)+bytes([0, 1])]
    duplicate = table([('a', [0, 1]), ('b', [0, 1])])
    corrupt += [duplicate[:16]+struct.pack('<I', 0)+duplicate[20:]]
    for i, data in enumerate(corrupt):
        result.append(matching(f'reject-record-{i}', data, 'abcdef', [], native=False))
    overflow = b'FIBH\x01\x00\xff\x00'+b'\xff'*4
    result.append(Case('reject-count-overflow', ['I '+hx(overflow), 'R'], ['I 0 0', 'R 0'], False))
    result.append(Case('empty-reinit', ['I '+hx(basic), 'I '+hx(table([])), 'R', 'B 255 616263646566'], ['I 1 1', 'I 1 0', 'R 0', 'B 0']))
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--rust', required=True)
    parser.add_argument('--cpp', required=True)
    args = parser.parse_args()
    for case in cases():
        for name, executable in [('rust', args.rust), ('cpp', args.cpp)]:
            if name == 'cpp' and not case.native:
                continue
            run = subprocess.run([executable], input='\n'.join(case.commands)+'\n', text=True, capture_output=True, check=True)
            assert run.stdout.splitlines() == case.expected, (case.name, name, case.expected, run.stdout, run.stderr)
    print('all independently specified cases passed')


if __name__ == '__main__':
    main()
