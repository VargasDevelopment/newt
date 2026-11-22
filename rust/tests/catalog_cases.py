"""Pure catalog/path corpus. Synthetic names only; no filesystem or device I/O.

Authored without executing binaries. Selected expectations below come from API
contracts, fixed order lists and literal path bytes, never oracle recordings.
"""
import random


def hx(value):
    if value is None:
        return '-'
    if isinstance(value, str):
        value = value.encode()
    return value.hex() or '_'


def add(name, size=0, mode=0, count=1):
    base = f'A,{hx(name)},{size},{mode}'
    return base if count == 1 else f'{base},{count}'


def trace(*events, script='-'):
    return ' '.join(('catalog', script, *events))


def path(kind, directory, name, suffix=b'', capacity=256, output=True):
    return f'catalog_path {kind} {hx(directory)} {hx(name)} {hx(suffix)} {capacity} {int(output)}'


def semantic_cases():
    """row -> field assertions, not a duplicate catalog implementation."""
    cases = []
    def check(line, expected):
        cases.append((line, expected))
    check(trace(add('b.epub'), 'Q', 'F', 'F', add('c.epub'), 'R,0', 'F'), {
        0: {'size': 0, 'status': 0}, 2: {'size': 1, 'listed': ['b.epub']},
        4: {'status': 1, 'size': 0, 'copies': 0, 'names': 0}, 6: {'size': 0, 'status': 0}})
    check(trace(add(':Z:A.epub'), add('a.epub'), add('A.epub'), add(':a:A.epub'), add('z.epub'), add('é.epub'), 'F'), {
        6: {'listed': [':a:A.epub', ':Z:A.epub', 'A.epub', 'a.epub', 'z.epub', 'é.epub']}})
    check(trace('O,F,1', 'O,N,1', 'O,S,1', 'F', 'O,N,1', 'R,1', add('a.epub'), 'R,0', 'F'), {
        0: {'notice': 'Some folders are too deep or unreadable.'},
        1: {'notice': "Some books have names Newt can't show."},
        2: {'notice': 'The card has too many files to list all.'},
        4: {'names': 1}, 5: {'status': 1, 'folders': 0, 'names': 0, 'notice': None},
        6: {'status': 1, 'size': 0}, 8: {'status': 0}})
    check(trace(*[add(n) for n in (':Newt:Books:a.epub', ':A::a.epub', ':A:.dot:a.epub', ':Newt:Other:a.epub', ':Newt:Books:Sub:a.epub', 'a:b.epub')], 'F'), {
        6: {'names': 4, 'listed': [':Newt:Books:Sub:a.epub', 'a:b.epub']}})
    check(trace(add(':A:z.epub', 7, 2), add(':B:y.epub', 7, 2), add('a.epub', 7, 5), add('zero.epub', 0, 3), 'F', script='01'), {
        1: {'proofs': [(':A:z.epub', ':B:y.epub', 0)]},
        2: {'copies': 1, 'proofs': [(':A:z.epub', 'a.epub', 0), (':B:y.epub', 'a.epub', 1)], 'map': {0: 7, 1: 7}},
        3: {'proofs': []}, 4: {'listed': ['a.epub', ':A:z.epub', 'zero.epub']}})
    check(trace(add('a.epub', 1, 0), add('a.epub', 1, 0), 'F'), {2: {'size': 2, 'copies': 0, 'listed': ['a.epub', 'a.epub']}})
    check(trace(add(':Z:z.epub', 2, 1), add('a.epub', 2, 1), add('b.epub', 2, 3), 'F'), {
        2: {'copies': 1, 'proofs': [(':Z:z.epub', 'b.epub', 1)], 'map': {0: 2, 1: 2}},
        3: {'listed': ['a.epub', 'b.epub']}})
    many = [add(f':s:{i:03}.epub', i + 1, 1) for i in range(128)]
    check(trace(*many, add('zz.epub', 987, 1), add(':s:zzz.epub', 99, 1), 'F', 'P,127,256', 'P,128,256'), {
        128: {'full': 1, 'map': {0: 1, 126: 127, 127: 987}},
        129: {'full': 2, 'map': {127: 987}},
        130: {'size': 128, 'listed': [f':s:{i:03}.epub' for i in range(127)] + ['zz.epub'], 'notice': 'Only the first 128 books are shown.'},
        131: {'query_ok': 1, 'query_text': '/Newt/Books/zz.epub'}, 132: {'query_ok': 0, 'query_untouched': True}})
    check(trace(*many, add('copy.epub', 1, 3), 'F'), {
        128: {'full': 0, 'copies': 1, 'map': {0: 1},
              'proofs': [(':s:000.epub', 'copy.epub', 1)]},
        129: {'size': 128}})
    check(trace('O,N,65540', 'O,F,65540', add('a.epub', 9, 3), *[add('a.epub', 9, 3) for _ in range(260)], 'F'), {
        1: {'names': 65535, 'folders': 65535}, 263: {'copies': 255, 'size': 1}})
    # Both adapters repeat the same real add operation and emit one final row.
    check(trace(*many, add(':zz:overflow.epub', count=65540),
                'O,N,1', 'O,F,1', 'Q', 'O,S,1', 'F'), {
        128: {'full': 65535},
        131: {'full': 65535, 'names': 1, 'folders': 1,
              'notice': 'Only the first 128 books are shown.'},
        132: {'notice': 'The card has too many files to list all.'},
        133: {'size': 128}})
    return cases


def catalog_corpus():
    cases = [line for line, _ in semantic_cases()]
    saturation = cases.pop()  # Keep the costly full-counter witness after cheap cases.
    valid = ['a.epub', 'A.EPUB', 'two words.epub', 'a:b.epub', ':A:a.epub', ':a:A.epub',
             ':nEwT:bOoKs:sub:é.epub', ':根:😀.epub', 'a' * 90 + '.epub']
    invalid = [None, '', '.a.epub', 'a.txt', 'a/b.epub', 'a\\b.epub', ':A::a.epub',
               ':A:.dot:a.epub', ':Newt:Books:a.epub', ':Newt:Cache:a.epub',
               ':Newt:Books::a.epub', ':a.epub:', 'a' * 91 + '.epub', b'\xc0\xaf.epub',
               b'\xed\xa0\x80.epub', b'\xf4\x90\x80\x80.epub', b'a\x7f.epub', b'a\xc2\x85.epub', b'\xff.epub']
    names = valid + invalid + [b'a.epub\0ignored/bad', b':A:a.epub\0:Newt:Cache:x.epub']
    cases.append(trace(*[add(name, i % 4, 2) for i, name in enumerate(names)], 'F', 'P,0,0', 'P,0,1', 'P,999,64'))
    for name in names:
        for capacity in (0, 1, 7, 12, 13, 23, 24, 32, 95, 96, 256):
            cases.extend((path('P', '/sd/Newt/Books', name, capacity=capacity),
                          path('F', '/Newt/Covers', name, '.bin', capacity)))
        for directory in (None, '', '/Newt/Books', '/newt/books', '/sd/Newt/Books/', '/x/Newt/Books'):
            cases.extend((path('P', directory, name), path('F', directory, name, None)))
        cases.append(path('P', '/Newt/Books', name, output=False))
        cases.append(path('B', None, name))
    # Basename is intentionally not a validator, including non-card colons.
    cases.extend(path('B', None, n) for n in (':', ':x:', ':x:y', 'x:y', b'bad\xff:name'))
    rng = random.Random(15128)
    for iteration in range(10):
        pool = valid + [f':folder{i % 7}:Book{i:03}.epub' for i in range(140)]
        rng.shuffle(pool)
        events = [add(name, rng.choice((0, 1, 7, 0xffffffff)), rng.randrange(6)) for name in pool]
        events += ['F', 'P,0,256', 'P,127,256', 'P,128,256', 'F', 'O,S,1', add('after.epub'), 'R,0', add('fresh.epub'), 'F']
        cases.append(trace(*events, script='001010011010100101' * 32))
    cases.extend(path_goldens())
    cases.append(saturation)
    return list(dict.fromkeys(cases))


def path_reply(ok, shown=None, capacity=256):
    output = bytearray(b'\xa5' * 256)
    if shown is not None and capacity:
        count = min(len(shown), capacity - 1)
        output[:count] = shown[:count]
        output[count] = 0
    return f'{int(ok)} {output.hex()}'


def path_goldens():
    expected = {}
    for capacity in (0, 1, 7, 12, 13, 32, 256):
        complete = capacity > 12
        # snprintf failure leaves ':', while success converts all card separators.
        shown = b'/sd/A/a.epub' if complete else b'/sd:A:a.epub'
        expected[path('P', '/sd/Newt/Books', ':A:a.epub', capacity=capacity)] = path_reply(complete, shown, capacity)
    expected[path('P', '/Newt/Books', 'a:b.epub')] = path_reply(True, b'/Newt/Books/a:b.epub')
    expected[path('P', '/sd/newt/books', ':A:a.epub')] = path_reply(False)
    expected[path('P', '/Newt/Books', ':Newt:Books:a.epub')] = path_reply(False)
    expected[path('F', '/covers', ':A:a.epub', '.bin')] = path_reply(True, b'/covers/.80239d05b448683f.epub.bin')
    expected[path('F', '/covers', 'a.epub', '.bin')] = path_reply(True, b'/covers/a.epub.bin')
    expected[path('F', '/covers', 'a.epub', None)] = path_reply(False)
    expected[path('B', None, 'a:b.epub')] = hx('a:b.epub')
    expected[path('B', None, ':A:a.epub')] = hx('a.epub')
    expected[path('B', None, ':x:')] = '-'
    return expected


def assert_catalog_expectations(cases, replies):
    lookup = dict(zip(cases, replies))
    fields = 'status size copies full names folders stopped notice listed map proofs query_ok query_output'.split()
    for line, wanted in semantic_cases():
        if line not in lookup:
            continue
        rows = lookup[line].split(' ; ')
        for index, assertions in wanted.items():
            row = dict(zip(fields, rows[index].split()))
            for field, expect in assertions.items():
                if field == 'listed':
                    actual = [] if row[field] == '-' else [bytes.fromhex(n).decode() for n in row[field].split('/')]
                elif field == 'notice':
                    actual = None if row[field] == '-' else bytes.fromhex(row[field]).decode()
                elif field == 'map':
                    raw = bytes.fromhex(row[field])
                    actual = {i: int.from_bytes(raw[i*4:i*4+4], 'little') for i in expect}
                elif field == 'proofs':
                    actual = []
                    for entry in ([] if row[field] == '-' else row[field].split('/')):
                        kept, rest = entry.split('>'); copy, answer = rest.split('=')
                        actual.append((bytes.fromhex(kept).decode(), bytes.fromhex(copy).decode(), int(answer)))
                elif field == 'query_text':
                    actual = bytes.fromhex(row['query_output']).split(b'\0')[0].decode()
                elif field == 'query_untouched':
                    actual = bytes.fromhex(row['query_output']) == b'\xa5' * 256
                else:
                    actual = int(row[field])
                assert actual == expect, (line, index, field, expect, actual)
    for line, want in path_goldens().items():
        if line in lookup:
            assert lookup[line] == want, (line, want, lookup[line])
