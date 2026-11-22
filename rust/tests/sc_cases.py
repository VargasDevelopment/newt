"""Independent synthetic protocol expectations. NOT RUN during staging.
Later: python3 cases.py /absolute/oracle /absolute/sleep-crossink-host
No downloads, builds or source writes. Host subprocess transport only.
"""
import struct
import subprocess
import sys


def checksum(data):
    h = 2166136261
    for value in data:
        h = ((h ^ value) * 16777619) & 0xffffffff
    return struct.pack('<I', h)


def record(magic, size, body=b'', generation=0):
    b = bytearray(size)
    b[:12] = magic + struct.pack('<II', 1, generation)
    b[12:12 + len(body)] = body
    b[-4:] = checksum(b[:-4])
    return bytes(b)


def reseal(b):
    return bytes(b[:-4]) + checksum(b[:-4])


def vectors():
    empty_history = '00' * 92
    for version, size in [(1, 13), (2, 17), (3, 159)]:
        b = bytearray(size)
        b[0] = version
        struct.pack_into('<III', b, 1, 17, 3600, 91)
        if version >= 2:
            struct.pack_into('<I', b, 13, 3)
        if version == 3:
            struct.pack_into('<I', b, 61, 36524)
            b[65], b[156] = 5, 2
            struct.pack_into('<H', b, 157, 33)
        history = ('05' + '00' * 90 + '02') if version == 3 else empty_history
        expected = f'1 17 3600 91 {3 if version >= 2 else 0} {36524 if version == 3 else 0} {33 if version == 3 else 0} {history}'
        yield f'global {b.hex()}', expected
        for n in range(size):
            yield f'global {b[:n].hex() or "-"}', f'0 0 0 0 0 0 0 {empty_history}'
    for b in [bytes([4]), bytes(160)]:
        yield f'global {b.hex()}', f'2 0 0 0 0 0 0 {empty_history}'
    for version, size in [(1, 11), (2, 12), (3, 16), (4, 69), (5, 73)]:
        b = bytearray(size); b[0] = version
        yield f'book {b.hex()}', '1 0'
        if version >= 2:
            b[11] = 1
            yield f'book {b.hex()}', '1 1'
            b[11] = 2
            yield f'book {b.hex()}', '0 0'
        yield f'book {b[:-1].hex()}', '0 0'
    for name, expected in [(b'epub_1', '1'), (b'txt_007', '1'), (b'a_1', '1'), (b'Epub_1', '0'), (b'epub_', '0'), (b'_1', '0'), (b'a_1\0', '0')]:
        yield f'name {name.hex()}', expected
    streak = record(b'NWTK', 28, struct.pack('<HHIB', 9497, 3, 0x01020304, 14))
    cleared = record(b'NWTK', 28)
    for b in [streak, cleared]:
        yield f'streak {b.hex()}', b.hex()
        for n in range(len(b)):
            yield f'streak {b[:n].hex() or "-"}', '-'
    for padding in (21, 22, 23):
        bad = bytearray(streak); bad[padding] = 1
        yield f'streak {reseal(bad).hex()}', '-'
    corrupt = bytearray(streak); corrupt[20] ^= 2
    yield f'streak {reseal(corrupt).hex()}', '-'
    records = []
    global_body = bytearray(164); global_body[:3] = bytes([1, 3, 13]); global_body[4] = 1
    records.append(('global-record', record(b'NWCG', 180, global_body)))
    source_body = bytearray(144); source_body[:7] = b'epub_77'; source_body[32:36] = bytes([1, 2, 73, 1]); source_body[36] = 5; source_body[112] = 0xab
    records.append(('source-record', record(b'NWCB', 160, source_body)))
    records.append(('import-record', record(b'NWCI', 56, struct.pack('<I', 7) + bytes([0x42]) * 32)))
    for cmd, b in records:
        yield f'{cmd} {b.hex()}', b.hex()
        for n in range(len(b)):
            yield f'{cmd} {b[:n].hex() or "-"}', '-'
        for at in [0, 4, len(b)-1]:
            corrupt = bytearray(b); corrupt[at] ^= 1
            yield f'{cmd} {corrupt.hex()}', '-'
        future = bytearray(b); future[4] = 2
        yield f'{cmd} {reseal(future).hex()}', '-'
        generation = bytearray(b); generation[8] = 99
        yield f'{cmd} {reseal(generation).hex()}', b.hex()
    for cmd, original, offsets in [(records[0][0], records[0][1], [13, 15, 29, 175]), (records[1][0], records[1][1], [47, 121, 122, 123]), (records[2][0], records[2][1], [48, 49, 50, 51])]:
        for at in offsets:
            bad = bytearray(original); bad[at] = 9
            yield f'{cmd} {reseal(bad).hex()}', '-'
    # Independent NWTS wire construction: October 10 2026 is day 9779,
    # month 24321. Ten 60-second days; no lease/facts/buckets/pages.
    body = bytearray(164)
    struct.pack_into('<HHI', body, 0, 24321, 24321, 600)
    struct.pack_into('<H', body, 18, 24321)
    # Default BookFacts start_permille is unknown (0xffff).
    struct.pack_into('<H', body, 76, 0xffff)
    for i in range(10):
        struct.pack_into('<H', body, 100 + i*2, 60)
    ledger = record(b'NWTS', 180, body)
    cache = record(b'NWTK', 28, struct.pack('<HHIB', 9779, 9, 540, 255))
    yield f'sleep {ledger.hex()} - {cache.hex()} 9779 8 0 1 10 255', f'1 540 0 0 0 127 10 0 {cache.hex()}'
    yield f'sleep {ledger.hex()} - {cache.hex()} 9780 8 59 0 0 0', f'1 600 0 0 0 126 0 0 {cleared.hex()}'
    yield f'sleep {ledger.hex()} - {cache.hex()} 9780 8 60 0 0 0', f'1 600 0 0 0 127 11 0 {cleared.hex()}'


    from sc_boundary_cases import vectors as boundary_vectors
    yield from boundary_vectors()

def main():
    cases = list(vectors())
    payload = ''.join(command + '\n' for command, _ in cases)
    for executable in sys.argv[1:]:
        result = subprocess.run([executable], input=payload, text=True, capture_output=True, check=True)
        actual = result.stdout.splitlines()
        assert len(actual) == len(cases), (executable, len(actual), len(cases))
        for (command, expected), got in zip(cases, actual):
            assert got == expected, (executable, command, expected, got)
    assert len(sys.argv) == 3, 'provide pinned oracle and staged Rust host'
    print(f'{len(cases)} independent cases matched both hosts')


if __name__ == '__main__':
    main()


def corpus():
    return ['sc_' + command for command, _ in vectors()]

def assert_expectations(cases, replies):
    expected = {'sc_' + command: wanted for command, wanted in vectors()}
    for case, reply in zip(cases, replies):
        if case in expected and reply != expected[case]:
            raise AssertionError(f'Sleep/CrossInk independent expectation: {case}: {reply} != {expected[case]}')
