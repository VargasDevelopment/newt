"""Independent expectations: Python strict codec + explicit ZIP policy literals.

No execution on staging. corpus() fits the existing line protocol runner;
expectations() must also check pinned C++ replies, not merely Rust parity.
"""
import itertools
import random


def utf8_reply(data, present, length, offset, cp, steps):
    # Search strict *whole* prefix decodes, independent of the port's bit parser.
    replies = []
    for _ in range(steps):
        ok = False
        if present and offset < length:
            start = offset
            offset += 1
            for end in range(start + 1, min(length, start + 4) + 1):
                try:
                    scalar = data[start:end].decode('utf-8', errors='strict')
                except UnicodeDecodeError:
                    continue
                if len(scalar) == 1:
                    ok, offset, cp = True, end, ord(scalar)
                    break
        replies.append(f'{int(ok)}:{offset}:{cp}')
    return ' '.join(replies)


def expectations():
    cases = {}

    def utf(data, present=1, length=None, offset=0, cp=0xdeadbeef, steps=None):
        length = len(data) if length is None else length
        steps = len(data) + 2 if steps is None else steps
        command = f'utf8_next {data.hex() or "_"} {present} {length} {offset} {cp} {steps}'
        cases[command] = utf8_reply(data, present, length, offset, cp, steps)

    # Every byte in isolation, with recovery and as an interior start.
    for first in range(256):
        utf(bytes([first]))
        utf(bytes([first, 0x41, 0, 0xc2, 0xa2]))
    scalars = [0, 1, 0x1f, 0x20, 0x7f, 0x80, 0x7ff, 0x800, 0xd7ff,
               0xe000, 0xfdd0, 0xfeff, 0xfffe, 0xffff, 0x10000, 0x10fffe, 0x10ffff]
    for cp in scalars:
        raw = chr(cp).encode('utf-8')
        for length in range(len(raw) + 1):
            utf(raw, length=length)
        for offset in range(len(raw) + 2):
            utf(raw + b'Z', offset=offset)
    malformed = ['c080', 'c1bf', 'e08080', 'e09fbf', 'eda080', 'edbfbf',
                 'f0808080', 'f08fbfbf', 'f4908080', 'f4bfbfbf', 'f5808080',
                 'e241ac', 'e28241', 'f0418080', 'f0904180', 'f0908041',
                 'c2ff', 'efbfbf80', 'e282e282ac', 'f09080f0908080']
    for text in malformed:
        raw = bytes.fromhex(text)
        utf(raw + b'Z')
        for length in range(len(raw) + 1):
            utf(raw, length=length)
    for raw in [b'', b'a', b'\0', b'\xf4\x8f\xbf\xbf']:
        for offset in [0, len(raw), len(raw) + 1, 0xffffffff]:
            for present in [0, 1]:
                utf(raw, present=present, offset=offset, cp=0xffffffff, steps=3)
    utf(b'', present=0, length=0xffffffff, steps=2)
    rng = random.Random(0x4e525553543136)
    for _ in range(128):
        raw = bytes(rng.randrange(256) for _ in range(rng.randrange(1, 33)))
        utf(raw, offset=rng.randrange(len(raw) + 1))

    cases['zip_constants'] = '511 4194304 67108864 57246 8257 32'
    cases['zip_table'] = 'encrypted ok ok ok ok unsupported encrypted ok ok ok ok ok ok encrypted ok ok'

    def flag_rule(flags):
        # Independent explicit offending bit positions, not the production table.
        if any(flags & (1 << bit) for bit in (0, 6, 13)):
            return 'encrypted'
        return 'unsupported' if flags & 32 else 'ok'

    flags = {0, 0xffff, 0xdf9e, 0x2041, 0x2061}
    for count in (1, 2, 3):
        flags.update(sum(1 << bit for bit in bits)
                     for bits in itertools.combinations(range(16), count))
    for value in sorted(flags):
        cases[f'zip_flags {value}'] = flag_rule(value)
    for flag, method, name, size in itertools.product(
            [0, 8, 0xdf9e, 1, 32, 64, 8192, 33, 96, 8224, 0x2061, 0xffff],
            [0, 8, 1, 9, 99, 65535], [0, 1, 510, 511, 512, 65535],
            [0, 1, 4194303, 4194304, 4194305, 0xffffffff]):
        result = flag_rule(flag)
        if result == 'ok' and (method not in (0, 8) or not 1 <= name <= 511 or size > 4194304):
            result = 'unsupported'
        cases[f'zip_central {flag} {method} {name} {size}'] = result
    # Exact equality is orthogonal to policy; all equal values below must match.
    for flags, method, name in itertools.product([0, 8, 32, 0x2041, 65535], [0, 8, 99, 65535], [0, 1, 511, 65535]):
        fields = [0x04034b50, flags, method, name, flags, method, name]
        cases['zip_local ' + ' '.join(map(str, fields))] = 'ok'
        for at in range(7):
            # Every flags bit must compare exactly, including descriptor bit 3.
            bits = range(16) if at in (1, 4) else (0,)
            for bit in bits:
                altered = fields.copy()
                altered[at] ^= 1 << bit
                cases['zip_local ' + ' '.join(map(str, altered))] = 'mismatch'
    cases['zip_local 0 0 0 0 0 0 0'] = 'mismatch'
    return cases


def corpus():
    return list(expectations())


def check_independent(cases, replies):
    """Call for pinned C++ AND Rust outputs; no executable/subprocess here."""
    if len(cases) != len(replies):
        raise AssertionError('admission reply count differs')
    expected = expectations()
    for command, reply in zip(cases, replies):
        if command in expected and reply != expected[command]:
            raise AssertionError(f'independent admission expectation: {command}: {reply!r} != {expected[command]!r}')
