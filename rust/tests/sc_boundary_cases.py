"""Independent boundary vectors; wire fixtures use explicit contract offsets."""
from datetime import date
import struct
from sc_cases import record, reseal


def day(month, dom):
    return (date(2026, month, dom) - date(2000, 1, 1)).days


def ledger(month, oldest, days, buckets=(0, 0, 0, 0), total=0):
    b = bytearray(record(b'NWTS', 180))
    struct.pack_into('<HHI', b, 12, oldest, month, total)
    struct.pack_into('<H', b, 30, month)
    struct.pack_into('<H', b, 88, 65535)  # Default unknown start-permille.
    struct.pack_into('<4I', b, 96, *buckets)
    for dom, seconds in days.items():
        struct.pack_into('<H', b, 112 + (dom - 1) * 2, seconds)
    return reseal(b)


def archive(month, days, buckets=(0, 0, 0, 0)):
    b = bytearray(record(b'NWTM', 104))
    struct.pack_into('<H', b, 12, month)
    struct.pack_into('<4I', b, 20, *buckets)
    for dom, seconds in days.items():
        struct.pack_into('<H', b, 36 + (dom - 1) * 2, seconds)
    return reseal(b)


def sleep_vectors():
    oct_month, nov_month = 24321, 24322
    cleared = record(b'NWTK', 28).hex()
    def request(l, a, c, month, dom, hour, unsaved, expected):
        return (f'sleep {l.hex()} {a.hex() if a else "-"} {c.hex() if c else "-"} '
                f'{day(month, dom)} {hour} {unsaved} 0 0 0', f'{expected} {cleared}')
    nov = ledger(nov_month, oct_month, {i: 60 for i in range(1, 5)}, (0, 1800, 0, 0), 1140)
    oct_archive = archive(oct_month, {i: 180 for i in range(27, 32)}, (0, 0, 0, 1800))
    # Archive available/missing, earlier bucket wins a tie, then current bucket wins.
    yield request(nov, oct_archive, None, 11, 4, 23, 0, '1 1080 24321 24321 0 127 0 2')
    yield request(nov, oct_archive, None, 11, 4, 23, 1, '1 1080 24321 24321 0 127 0 4')
    yield request(nov, None, None, 11, 4, 23, 0, '1 1080 24321 24321 0 15 0 0')
    # Clock setback into archived history; then before the complete known range.
    yield request(nov, oct_archive, None, 10, 31, 8, 9000, '1 720 0 0 1 31 0 0')
    yield request(nov, oct_archive, None, 9, 30, 8, 0, '1 0 0 0 0 0 0 0')
    yield request(nov, None, None, 9, 30, 8, 0, '0 0 0 0 0 0 0 0')
    # Day seven stops the weekly archive hint while habit still needs that month.
    yield request(nov, oct_archive, None, 11, 7, 23, 0, '1 1140 0 24321 0 120 0 2')
    # A missing third month is not a known empty month.
    gap = ledger(nov_month, oct_month - 1, {i: 60 for i in range(1, 5)}, total=1140)
    yield request(gap, oct_archive, None, 9, 30, 8, 0, '0 0 0 0 1 0 0 0')
    ten = ledger(oct_month, oct_month, {i: 60 for i in range(1, 11)}, total=600)
    def cache(dom, run, before):
        return record(b'NWTK', 28, struct.pack('<HHIB', day(10, dom), run, before, 255))
    # Valid stamp, mismatching proof, future stamp and wrapping unsaved seconds.
    yield request(ten, None, cache(10, 9, 540), 10, 10, 8, 0, '1 540 0 0 0 127 10 0')
    yield request(ten, None, cache(10, 9, 539), 10, 10, 8, 0, '1 540 0 0 0 127 0 0')
    yield request(ten, None, cache(11, 9, 540), 10, 10, 8, 0, '1 540 0 0 0 127 0 0')
    yield request(ten, None, cache(10, 9, 540), 10, 10, 8, 0xffffffff, '1 540 0 0 0 127 10 1')
    yield request(ten, None, None, 10, 10, 8, 0xffffffff, '1 540 0 0 0 126 0 1')
    yield request(ten, None, cache(10, 65535, 540), 10, 10, 8, 0, '1 540 0 0 0 127 0 0')
    seven = ledger(oct_month, oct_month, {i: 60 for i in range(11, 18)}, total=420)
    yield request(seven, None, cache(10, 9, 0), 10, 16, 8, 0, '1 300 0 0 0 127 16 0')
    yield request(seven, None, cache(10, 9, 0), 10, 17, 8, 0, '1 360 0 0 0 127 0 0')
    empty = ledger(0, 0, {})
    yield request(empty, None, None, 11, 4, 23, 1799, '1 0 0 0 0 1 0 0')
    yield request(empty, None, None, 11, 4, 23, 1800, '1 0 0 0 0 1 0 4')
    saturated = ledger(nov_month, nov_month, {}, (0xffffffff, 0xffffffff, 0, 0))
    yield request(saturated, None, None, 11, 4, 13, 0xffffffff, '1 0 0 0 0 1 0 1')


def frozen_vectors():
    # Canonical independent status/candidate/binding matrices, not encoder output.
    raw_global = bytes([1]) + bytes(12)
    for status, candidate, valid in [(2, 0, True), (3, 1, True), (3, 3, True),
                                     (2, 1, False), (3, 0, False), (1, 2, False),
                                     (1, 5, False), (1, 3, True)]:
        b = bytearray(record(b'NWCG', 180))
        b[12:15] = bytes((status, candidate, len(raw_global) if status == 1 else 0))
        if status == 1:
            b[16:29] = raw_global
        b = reseal(b)
        yield f'global-record {b.hex()}', b.hex() if valid else '-'
    for status, candidate, version, binding, has_digest, valid in [
        (2, 0, 0, 0, False, True), (2, 5, 0, 0, False, False),
        (3, 1, 0, 0, False, False), (1, 5, 1, 0, False, True),
        (1, 5, 1, 1, True, True), (1, 5, 1, 2, True, True),
        (1, 5, 1, 1, False, False), (1, 5, 1, 0, True, False),
        (1, 2, 1, 0, False, False), (1, 2, 5, 0, False, True),
        (1, 4, 1, 0, False, True), (1, 1, 1, 0, False, True),
        (1, 3, 1, 0, False, True), (1, 0, 1, 0, False, False)]:
        size = {0: 0, 1: 11, 5: 73}[version]
        b = bytearray(record(b'NWCB', 160))
        b[12:19] = b'epub_77'
        b[44:48] = bytes((status, candidate, size, binding))
        if size:
            b[48] = version
        if has_digest:
            b[124] = 0xab
        b = reseal(b)
        yield f'source-record {b.hex()}', b.hex() if valid else '-'


def vectors():
    yield from sleep_vectors()
    yield from frozen_vectors()
