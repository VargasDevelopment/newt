"""Staged differential inputs; expected replies always come from pinned C++.

Independent struct/FNV fixture construction, standard library only. Synthetic
names and hashes exercise structure; no application behavior keys on them.
"""
import random
import struct


def hx(value):
    return value.hex() if value else '_'


def fnv(value, initial=2166136261):
    result = initial
    for byte in value:
        result = ((result ^ byte) * 16777619) & 0xffffffff
    return result


def sha(prefix=1, tail=1):
    return struct.pack('<Q', prefix) + bytes(23) + bytes([tail])


def sd_identity(name=b'Alpha.epub', digest=None, version=1, magic=0x53444e31):
    digest = sha() if digest is None else digest
    filename = (name + bytes(96))[:96]
    checksum = fnv(digest + filename, ((2166136261 ^ version) * 16777619) & 0xffffffff)
    return struct.pack('<II', magic, version) + digest + filename + struct.pack('<I', checksum)


def selected(name=b'Alpha.epub', digest=None, sequence=1):
    digest = sha() if digest is None else digest
    body = struct.pack('<4sII', b'NWLS', 1, sequence) + digest + (name + bytes(96))[:96]
    return body + struct.pack('<I', fnv(body))


def sleep(book=1, checkpoint=0x12345678, settings=0x507, sequence=1):
    body = struct.pack('<4sIIQII', b'NWBR', 1, sequence, book, checkpoint, settings)
    return body + struct.pack('<I', fnv(body))


def rechecksum(record):
    return record[:-4] + struct.pack('<I', fnv(record[:-4]))


def corpus():
    rng = random.Random(0x4944454e)
    cases = []

    def add(*fields):
        cases.append(' '.join(map(str, fields)))

    names = [b'', b'\0', b'a.epub', b'a.epub\0', b'a.EpUb\0', b'.epub\0', b'.a.epub\0',
             b'../a.epub\0', b'a\\b.epub\0', b'a.epub\0ignored\\tail',
             b':Shelf:A.epub\0', b':Newt:..:A.epub\0', b'::A.epub\0',
             b'a' * 90 + b'.epub\0', b'a' * 91 + b'.epub\0', b'a' * 96,
             'Лист.epub\0'.encode(), '紙.epub\0'.encode(), '😀.epub\0'.encode(),
             '\u00a0\ufeff\U0010ffff.epub\0'.encode()]
    # Invalid leads, truncation, continuation, overlong, surrogate, upper bound;
    # valid noncharacters remain scalar UTF-8 and are not silently blacklisted.
    scalars = [b'\x80', b'\xc0\xaf', b'\xc1\xbf', b'\xc2', b'\xc2A', b'\xe0\x80\xaf',
               b'\xed\xa0\x80', b'\xf0\x80\x80\xaf', b'\xf4\x90\x80\x80', b'\xf5\x80\x80\x80',
               b'\xfe', b'\xff', '\ufffe'.encode(), '\U0010ffff'.encode()]
    names += [value + b'.epub\0' for value in scalars]
    names += [chr(cp).encode() + b'.epub\0' for cp in [1, 9, 31, 32, 126, 127, 128, 159, 160]]
    for name in names:
        add('name_valid', hx(name))
        for digest in [bytes(32), sha(0, 1), sha((1 << 64) - 1, 255)]:
            add('sd_make', hx(name), hx(digest))

    raw = sd_identity()
    for index in range(140):
        bad = bytearray(raw); bad[index] ^= 0x80
        add('sd_validate', hx(bad))
        add('selected_encode', hx(bad), 1)
    for version in [0, 1, 2, 0x100, 0xffffffff]:
        add('sd_validate', hx(sd_identity(version=version)))
    # Retained record validates nonzero post-NUL bytes if checksum agrees;
    # durable selected encoding canonicalizes them before persistence.
    padded = bytearray(raw); padded[135] = 0x80
    initial = ((2166136261 ^ 1) * 16777619) & 0xffffffff
    padded[-4:] = struct.pack('<I', fnv(padded[8:136], initial))
    add('sd_validate', hx(padded))
    add('selected_encode', hx(padded), 7)

    for name in [b'Alpha.epub', b':Shelf:Long title.EPUB', '紙😀.epub'.encode(), b'a' * 90 + b'.epub']:
        for digest in [sha(), sha(0, 1), sha(0x8877665544332211, 255), bytes(32)]:
            for sequence in [0, 1, 0x80000000, 0xffffffff]:
                add('selected_encode', hx(sd_identity(name, digest)), sequence)
                add('selected_decode', hx(selected(name, digest, sequence)))
    golden = selected()
    for size in range(148):
        add('selected_decode', hx((golden + bytes(4))[:size]))
    for index in range(144):
        bad = bytearray(golden); bad[index] ^= 0x80
        add('selected_decode', hx(bad))
    for bad_name in [b'../Bad.epub', b'.Bad.epub', b'NoExt', b'\xed\xa0\x80.epub', b'x' * 96,
                     b'Valid.epub\0garbage', b'a\x1f.epub']:
        add('selected_decode', hx(selected(bad_name)))
    for at in [0, 4]:
        bad = bytearray(golden); bad[at] ^= 1
        add('selected_decode', hx(rechecksum(bad)))

    for book in [0, 1, 0x100000000, (1 << 64) - 1]:
        for checkpoint in [0, 1, 0xffffffff]:
            for cadence in [0, 1, 2, 5, 10, 255, 256]:
                for bits in range(16):
                    settings = (cadence << 8) | bits
                    add('sleep_encode', book, checkpoint, settings, 0xffffffff)
                    add('sleep_decode', hx(sleep(book, checkpoint, settings)))
    for settings in [0x514, 0x524, 0x544, 0x584, 0x1000504, 0xffffffff]:
        add('sleep_encode', 1, 0, settings, 1)
        add('sleep_decode', hx(sleep(settings=settings)))
    golden_sleep = sleep()
    for size in range(36):
        add('sleep_decode', hx((golden_sleep + bytes(4))[:size]))
    for index in range(32):
        bad = bytearray(golden_sleep); bad[index] ^= 0x80
        add('sleep_decode', hx(bad))
    for at in [0, 4]:
        bad = bytearray(golden_sleep); bad[at] ^= 1
        add('sleep_decode', hx(rechecksum(bad)))
    add('sleep_checkpoint', 1, 0, 0, 0, 0, 0, 0, 60000)
    for _ in range(40):
        add('sleep_checkpoint', rng.getrandbits(32), rng.getrandbits(64), rng.getrandbits(32),
            rng.getrandbits(32), rng.getrandbits(64), rng.getrandbits(32), rng.getrandbits(32),
            rng.getrandbits(32))

    a = selected(sequence=10)
    b = selected(b'Beta.epub', sha(2, 2), sequence=11)
    collision = selected(b'Collision.epub', sha(1, 2), sequence=11)
    renamed = selected(b'Renamed.epub', sha(1, 1), sequence=11)
    torn = bytearray(b); torn[60] ^= 1
    unknown = bytearray(b); unknown[4] = 2
    variants = ['-', '_', hx(a), hx(b), hx(collision), hx(renamed), hx(torn), hx(unknown),
                hx(b[:-1]), hx(b + b'\0'), hx(selected(sequence=0x8000000a)),
                hx(selected(b'Beta.epub', sha(2, 2), sequence=10)),
                hx(selected(sequence=0xffffffff)), hx(selected(sequence=0))]
    proposed = hx(sd_identity(b'Gamma.epub', sha(3, 3)))
    for first in variants:
        for second in variants:
            for operation in ['load', 'book', 'save', 'protected']:
                for protected in [0, 1, 2, 99]:
                    add('selected_store', 0, 'g', first, second, operation, proposed, protected)
    # A proposed colliding short ID must not replace a different full SHA peer.
    for proposed_raw in [sd_identity(b'Collision.epub', sha(1, 2)), sd_identity(b'Renamed.epub')]:
        add('selected_store', 0, 'g', hx(a), hx(b), 'protected', hx(proposed_raw), 1)
    for mask in [0, 1, 2, 4, 8, 16, 32, 64, 128, 144, 160, 192]:
        for script in ['g', 'a', 'b', 'bg', 'bb', 'ba', 't']:
            for operation in ['load', 'book', 'save', 'protected']:
                add('selected_store', mask, script, hx(a), hx(b), operation, proposed, 1)
    # Three values and both orders expose protected target selection.
    for first, second in [(a, b), (b, a)]:
        for protected in [1, 2]:
            add('selected_store', 0, 'g', hx(first), hx(second), 'protected', proposed, protected)
    for invalid in [sd_identity(digest=bytes(32)), sd_identity(version=2),
                    sd_identity(b'../Bad.epub'), sd_identity(b'a' * 96)]:
        add('selected_store', 0, 'g', hx(a), '-', 'save', hx(invalid), 0)

    reading = sleep(sequence=7)
    home = sleep(0, 0, 0, sequence=8)
    torn_home = bytearray(home); torn_home[28] ^= 1
    unknown = bytearray(home); unknown[4] = 2
    variants = ['-', '_', hx(reading), hx(home), hx(torn_home), hx(unknown),
                hx(home[:-1]), hx(home + b'\0'), hx(sleep(sequence=0x80000007)),
                hx(sleep(0, 0, 0, sequence=7)), hx(sleep(sequence=0xffffffff)), hx(sleep(sequence=0))]
    for first in variants:
        for second in variants:
            for operation in ['load', 'save']:
                for value in [(0, 0, 0), (1, 0x12345678, 0x507), (2, 0, 0xa0c)]:
                    add('sleep_store', 0, 'g', first, second, operation, *value)
    for mask in [0, 1, 2, 4, 8, 16, 32, 64, 128, 144, 160, 192]:
        for script in ['g', 'a', 'b', 'bg', 'bb', 'ba', 't']:
            for first, second in [(reading, home), (home, reading), (reading, torn_home), (torn_home, reading)]:
                for value in [(0, 0, 0), (1, 0x12345678, 0x507), (2, 0, 0xa0c)]:
                    add('sleep_store', mask, script, hx(first), hx(second), 'save', *value)
    for value in [(0, 1, 0), (0, 0, 0x507), (1, 1, 0), (1, 1, 0x514)]:
        add('sleep_store', 0, 'g', hx(reading), '-', 'save', *value)
    return cases


def cold_restarts(cases, responses):
    """Build fresh store reads from oracle durable arrays, dropping dirty bytes."""
    restarts = []
    for case, response in zip(cases, responses):
        fields = case.split()
        if (fields[0] in ('selected_store', 'sleep_store')
                and fields[5] in ('save', 'protected') and '_' not in fields[3:5]):
            # Existing state output maps both missing and present-empty to '-'.
            # Don't claim presence-preserving reboot coverage for empty blobs.
            # The protocol omits trailing payloads for failed loads, but always
            # has the same four visible/persisted array fields at the end.
            first, second = response.split()[-2:]
            if fields[0] == 'selected_store':
                tail = ['load', fields[6], fields[7]]
            else:
                tail = ['load', '0', '0', '0']
            restarts.append(' '.join([fields[0], '0', 'g', first, second, *tail]))
    return restarts


# Exact mutations are proposals for check.py CONTROLS after integration.
# Each must compile, then produce a behavioral mismatch; compile failure is not
# proof. Parent must run count(before)==1 and the complete merged corpus.
CONTROLS = (
    ('selected canonical padding', 'core/src/selected_book.rs',
     'if name[length + 1..].iter().any(|&byte| byte != 0)',
     'if false && name[length + 1..].iter().any(|&byte| byte != 0)'),
    ('selected full digest peer identity', 'core/src/selected_book.rs',
     '|a, b| a.digest == b.digest,', '|_a, _b| true,'),
    ('selected proposed SHA collision blocked', 'core/src/selected_book.rs',
     'if value.book_id() == protected_book && value.digest != protected.digest', 'if false'),
    ('sleep torn Home blocks older reading', 'core/src/sleep_intent.rs',
     'RecordStore<\'a, B, SleepResumeCodec, BYTES, false>',
     'RecordStore<\'a, B, SleepResumeCodec, BYTES, true>'),
    ('sleep SD discriminator required', 'core/src/sleep_intent.rs',
     '(self.settings & 4) != 0', 'true'),
    ('sleep canonical Home required', 'core/src/sleep_intent.rs',
     'return self.checkpoint == 0 && self.settings == 0;', 'return true;'),
    ('strict scalar names required', 'core/src/library_name.rs',
     'core::str::from_utf8(name).is_ok_and', 'core::str::from_utf8(b"a.epub").is_ok_and'),
)
