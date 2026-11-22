"""Deterministic, heterogeneous inputs; expected behavior comes from pinned C++.

Wire fixture builders use Python struct/zlib independently of both codecs.
No book identity controls behavior. Synthetic numeric identities are test data.
"""
import random
import struct
import zlib


def hx(bytes_):
    return bytes_.hex() if bytes_ else '_'


def progress(sequence=1, book=1, spine=0, character=0, active=0, pages=0, sessions=0):
    body = struct.pack('<4sIIQIIQIII', b'NWTR', 1, sequence, book, spine, character,
                       active, pages, sessions, 0)
    return body + struct.pack('<I', zlib.crc32(body))


def prefs(sequence=1, orientation=1, bar=1, cadence=5, version=2):
    body = struct.pack('<4sIIBBB5x', b'NWTP', version, sequence, orientation, bar, cadence)
    return body + struct.pack('<I', zlib.crc32(body))


def scalar(kind='zone', value=1, sequence=1):
    body = struct.pack('<4sIII', {'clock': b'NWTC', 'zone': b'NWTZ', 'sleep': b'NWTP'}[kind],
                       1, sequence, value)
    checksum = 2166136261
    for byte in body:
        checksum = ((checksum ^ byte) * 16777619) & 0xffffffff
    return body + struct.pack('<I', checksum)


def colliding_progress():
    """Two valid, different checkpoints with the same CRC, via GF(2) solving.

    These are format fixtures, not special library identities. The spine word
    is unconstrained by the progress codec; its content is solved algebraically.
    """
    first = progress(sequence=10)
    body = bytearray(progress(sequence=11, book=2)[:-4])
    base = zlib.crc32(body)
    basis = {}
    for bit in range(32):
        changed = bytearray(body); changed[20 + bit // 8] ^= 1 << (bit % 8)
        column, combination = zlib.crc32(changed) ^ base, 1 << bit
        while column:
            pivot = column.bit_length() - 1
            if pivot not in basis:
                basis[pivot] = (column, combination)
                break
            column ^= basis[pivot][0]; combination ^= basis[pivot][1]
    target, patch = struct.unpack('<I', first[-4:])[0] ^ base, 0
    while target:
        column, combination = basis[target.bit_length() - 1]
        target ^= column; patch ^= combination
    body[20:24] = struct.pack('<I', patch)
    if zlib.crc32(body) != struct.unpack('<I', first[-4:])[0]:
        raise RuntimeError('Invalid independently generated CRC collision fixture')
    return first, bytes(body) + first[-4:]


def sd_envelope(tag, payload, generation):
    record = bytearray(256)
    record[:4] = tag; record[4:9] = b'slot0'
    struct.pack_into('<II', record, 12, len(payload), generation)
    record[20:20 + len(payload)] = payload
    checksum = 0xffffffff
    for byte in record[:-4]:
        checksum ^= byte
        for _ in range(8):
            checksum = (checksum >> 1) ^ (0x82f63b78 if checksum & 1 else 0)
    struct.pack_into('<I', record, 252, checksum ^ 0xffffffff)
    return bytes(record)


from identity_corpus import corpus as identity_cases
from catalog_cases import catalog_corpus
from admission_cases import corpus as admission_cases
from sc_cases import corpus as sleep_crossink_cases
from book_graph_cases import corpus as book_graph_cases
from persistence_cases import corpus as persistence_cases
from stats_cases import stats_cases
from tracker_cases import tracker_cases
from clock_cases import clock_cases
from frame_cases import corpus as frame_cases

def corpus():
    rng = random.Random(0x4e455754)
    cases = []

    def add(*fields):
        cases.append(' '.join(map(str, fields)))

    # Full-width integers, zero identity, RAM-only session credit, fixed extent.
    for _ in range(220):
        values = [rng.getrandbits(32), rng.getrandbits(64), rng.getrandbits(32),
                  rng.getrandbits(32), rng.getrandbits(64), rng.getrandbits(32),
                  rng.getrandbits(32), rng.getrandbits(32)]
        add('progress_encode', *values)
        add('progress_decode', hx(progress(*values[:7])))
    for book in [0, 1, (1 << 64) - 1]:
        for sequence in [0, 1, 0x7fffffff, 0x80000000, 0xffffffff]:
            add('progress_encode', sequence, book, 0xffffffff, 0xffffffff,
                (1 << 64) - 1, 0xffffffff, 0xffffffff, 60000)
    golden = progress()
    for size in range(56):
        add('progress_decode', hx((golden + b'\0' * 4)[:size]))
    for index in range(52):
        for bit in [1, 0x80]:
            altered = bytearray(golden); altered[index] ^= bit
            add('progress_decode', hx(altered))
    for index in [0, 4, 12, 44]:
        altered = bytearray(golden); altered[index] ^= 1
        altered[-4:] = struct.pack('<I', zlib.crc32(altered[:-4]))
        add('progress_decode', hx(altered))

    sequences = [0, 1, 2, 0x7fffffff, 0x80000000, 0x80000001, 0xfffffffe, 0xffffffff]
    for a in sequences:
        for b in sequences:
            for different in [False, True]:
                first, second = progress(a), progress(b, 2 if different else 1)
                add('progress_select', hx(first), hx(second))
                add('progress_plan', hx(first), hx(second), 99, 3, 4, 5, 6, 7, 8, 9)
    for _ in range(180):
        first, second = progress(rng.getrandbits(32)), progress(rng.getrandbits(32), 2)
        for slot in [0, 1]:
            pair = [first, second]
            pair[slot] = rng.randbytes(rng.randrange(65))
            add('progress_select', hx(pair[0]), hx(pair[1]))
            add('progress_plan', hx(pair[0]), hx(pair[1]), 99, 0 if slot == 0 else 3, 4, 5, 6, 7, 8, 9)

    for orientation in [0, 1, 2, 255]:
        for bar in [0, 1]:
            for cadence in [0, 1, 2, 5, 9, 10, 255]:
                for sequence in [0, 0x78563412, 0xffffffff]:
                    add('prefs_encode', sequence, orientation, bar, cadence)
    for orientation in [0, 1]:
        add('geometry', orientation)
        for bar in [0, 1]:
            for cadence in [1, 5, 10]:
                add('prefs_decode', hx(prefs(orientation=orientation, bar=bar, cadence=cadence)))
    add('prefs_decode', hx(prefs(version=1, cadence=0)))
    golden = prefs()
    for size in range(28):
        add('prefs_decode', hx((golden + b'\0' * 4)[:size]))
    for index in range(24):
        altered = bytearray(golden); altered[index] ^= 0x80
        add('prefs_decode', hx(altered))
    for version in [0, 3, 0xffffffff]:
        for size in [0, 4, 7, 8, 12, 24, 25]:
            add('prefs_decode', hx((prefs(version=version) + b'\0')[:size]))
    for index in range(12, 20):
        for value in [2, 3, 255]:
            altered = bytearray(golden); altered[index] = value
            altered[-4:] = struct.pack('<I', zlib.crc32(altered[:-4]))
            add('prefs_decode', hx(altered))
    for interval in [0, 1, 5, 10, 255, 256, 0xffffffff]:
        for actions in ['q', 'f' + 'cpq' * 14, 'f' + 'c' * 300 + 'q', 'p' * 9 + 'rq',
                        'f' + 'p' * 300 + 'q', 'fpcprqfqq']:
            add('refresh', interval, actions)
        for _ in range(20):
            add('refresh', interval, ''.join(rng.choice('cfprq') for _ in range(90)))

    for kind, maximum in [('clock', 1), ('zone', 14), ('sleep', 7)]:
        for value in range(maximum + 1):
            for sequence in [0, 1, 0xffffffff]:
                record = scalar(kind, value, sequence)
                add('scalar_encode', kind, value, sequence)
                add('scalar_decode', kind, hx(record))
        if kind != 'sleep':
            for value in [maximum + 1, 255]:
                add('scalar_encode', kind, value, 1)
        record = scalar(kind)
        for size in range(24):
            add('scalar_decode', kind, hx((record + b'\0' * 4)[:size]))
        for index in range(20):
            altered = bytearray(record); altered[index] ^= 1
            add('scalar_decode', kind, hx(altered))
        add('scalar_decode', kind, hx(scalar(kind, maximum + 1)))

    for _ in range(100):
        record = rng.randbytes(256)
        name = rng.choice([b'', b'a', b'slot0', b'1234567', b'12345678', b'\0x', b'ab\0cd'])
        size = rng.choice([0, 1, 19, 32, 231, 232, 233, 0xffffffff])
        tag = rng.choice([b'NRS2', b'NWTR', b'\0\xff\0\x80'])
        add('sd_seal', hx(tag), hx(name), size, rng.getrandbits(32), hx(record))
        add('sd_intact', hx(tag), hx(record))
    for name in [b'', b'newt_reader', b'x' * 31, b'x' * 32, b'x' * 33, b'x\0ignored']:
        add('sd_header', hx(name), hx(rng.randbytes(256)))
    for tag in [b'NRS2', b'NWTR', b'\0\xff\0\x80']:
        for size in [0, 1, 20, 32, 231, 232]:
            sealed = sd_envelope(tag, rng.randbytes(size), rng.getrandbits(32))
            add('sd_intact', hx(tag), hx(sealed))
            add('sd_intact', hx(b'FAIL'), hx(sealed))
            for index in [12, 16, 20, 251, 252, 255]:
                changed = bytearray(sealed); changed[index] ^= 1
                add('sd_intact', hx(tag), hx(changed))
    sealed = sd_envelope(b'NRS2', bytes(range(232)), 0xffffffff)
    for index in range(256):
        changed = bytearray(sealed); changed[index] ^= 0x80
        add('sd_intact', hx(b'NRS2'), hx(changed))

    torn = bytearray(scalar(value=2, sequence=2)); torn[12] ^= 1
    bad_schema = bytearray(scalar()); bad_schema[4] = 2
    records = ['-', '_', hx(scalar()), hx(scalar(value=2, sequence=2)),
               hx(scalar(value=2)), hx(scalar(sequence=0x80000001)),
               hx(scalar(sequence=0xffffffff)), hx(scalar(sequence=0)),
               hx(torn), hx(bad_schema), hx(scalar()[:-1]), hx(scalar() + b'\0'),
               hx(scalar(value=255))]
    # Explicit boundary and failure combinations, then a reproducible mixed corpus.
    for allow in [0, 1]:
        for first in records:
            for second in records:
                for operation in ['load', 'save', 'protected', 'matching']:
                    add('store', 'zone', allow, 0, 'g', first, second, operation, 2, 1)
    for mask in [0, 1, 2, 4, 8, 16, 32, 64, 128, 144, 160, 192]:
        for script in ['g', 'a', 'b', 'bg', 'bb', 'ba', 't']:
            for operation in ['load', 'save', 'protected', 'publish', 'restore', 'admitted']:
                add('store', 'zone', 1, mask, script, hx(scalar()), '-', operation, 2, 1)
    # Explicit fallback values replace an older opt-in. Reboot state consists
    # only of the model's durable arrays; no old live choice is consulted.
    for script in ['g', 'a', 'bg', 'bb']:
        add('store', 'zone', 1, 0, script, hx(scalar(value=5)), '-', 'publish', 0, 5)
    # A third value forces a real protected rewrite: retrying the newest value
    # can deduplicate before the target-selection branch is reached.
    for swapped in [False, True]:
        pair = [hx(scalar(value=1, sequence=10)), hx(scalar(value=2, sequence=11))]
        if swapped:
            pair.reverse()
        for value in [0, 3, 14]:
            for protected in [1, 2]:
                for script in ['g', 'a', 'bg', 'bb']:
                    add('store', 'zone', 1, 0, script, *pair, 'protected', value, protected)
    for _ in range(500):
        add('store', rng.choice(['zone', 'note']), rng.randrange(2),
            rng.choice([0, 4, 8, 16, 32, 64, 128]), rng.choice(['g', 'a', 'bg', 'bb', 't']),
            rng.choice(records), rng.choice(records),
            rng.choice(['load', 'save', 'protected', 'matching', 'admitted']),
            rng.choice([0, 1, 2, 3, 14, 15]), rng.choice([0, 1, 2, 3]))

    corrupt = bytearray(progress(2)); corrupt[20] ^= 1
    unknown = bytearray(progress(2)); unknown[4] = 2
    reserved = bytearray(progress(2)); reserved[44] = 1
    reserved[-4:] = struct.pack('<I', zlib.crc32(reserved[:-4]))
    collision = colliding_progress()
    variants = ['-', '_', hx(progress()), hx(progress(2, character=3)), hx(progress(1, book=2)),
                hx(progress(0x80000001)), hx(progress(0xffffffff)), hx(progress(0)), hx(corrupt),
                hx(unknown), hx(reserved), hx(progress()[:-1]), hx(progress() + b'\0')]
    proposed = [0, 1, 0, 0, 0, 0, 0, 59999]
    for first in variants:
        for second in variants:
            for operation in ['load', 'save', 'save_peers', 'peers']:
                for strict in [0, 1]:
                    add('progress_store', 0, 'g', first, second, operation, '-', strict, *proposed)
    for mask in [0, 1, 2, 4, 8, 16, 32, 64, 128, 144, 160, 192]:
        for script in ['g', 'a', 'bg', 'bb', 'ba', 't']:
            for operation in ['save', 'save_peers', 'load', 'checkpoint', 'peers']:
                for book in [0, 1, 2]:
                    add('progress_store', mask, script, hx(progress()), '-', operation,
                        struct.unpack('<I', progress()[-4:])[0], 1,
                        0, book, 4, 5, 1000, 2, 1, 59999)
    for pair in [(progress(10), progress(11, character=3)), collision]:
        for swapped in [False, True]:
            first, second = pair[::-1] if swapped else pair
            for checkpoint in [0, struct.unpack('<I', pair[0][-4:])[0], struct.unpack('<I', pair[1][-4:])[0]]:
                for operation in ['checkpoint', 'save', 'save_peers']:
                    for script in ['g', 'a', 'bg', 'bb']:
                        add('progress_store', 0, script, hx(first), hx(second), operation, checkpoint,
                            1, 0, 1, 8, 9, 1000, 3, 2, 60000)
    from preferences_cases import preferences_cases
    cases.extend(preferences_cases(prefs, hx))
    cases.extend(identity_cases())
    cases.extend(catalog_corpus())
    cases.extend(admission_cases())
    cases.extend(stats_cases())
    cases.extend(persistence_cases())
    cases.extend(book_graph_cases())
    cases.extend(sleep_crossink_cases())
    cases.extend(tracker_cases())
    cases.extend(clock_cases())
    cases.extend(frame_cases())
    from prefix_cases import corpus as prefix_corpus
    cases.extend(prefix_corpus())
    from external_progress_cases import corpus as external_progress_corpus
    cases.extend(external_progress_corpus())
    from stats_service_cases import corpus as service_corpus
    cases.extend(service_corpus())
    return cases
