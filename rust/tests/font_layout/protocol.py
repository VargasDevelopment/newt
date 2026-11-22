"""Command-aware font-layout wire grammar; preserve raw LF bytes."""
import hashlib
import re

NAT = re.compile(r'(?:0|[1-9][0-9]*)\Z')
INT = re.compile(r'(?:0|-?[1-9][0-9]*)\Z')
HEX = re.compile(r'(?:[0-9a-f]{2})+\Z')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def number(value, low=0, high=0xffffffff):
    require(len(value) <= 11 and (INT if low < 0 else NAT).fullmatch(value) is not None,
            'canonical wire integer required')
    result = int(value)
    require(low <= result <= high, 'integer outside wire width')
    return result


def commands(text):
    require(isinstance(text, str) and text.isascii() and '\r' not in text
            and len(text) <= 8388608, 'bounded ASCII raw LF request required')
    rows = text.split('\n')
    require(len(rows) == 4 and rows[3] == '' and rows[0] in ('P', 'S'),
            'exact request mode and three LF rows required')
    require(rows[1] == '-' or HEX.fullmatch(rows[1]) is not None, 'canonical table hex required')
    glyphs = rows[2].split(' ')
    require(len(glyphs) == 2 if rows[0] == 'P' else 1 <= len(glyphs) <= 16,
            'wrong glyph count')
    for glyph in glyphs:
        number(glyph)
    return rows[0]


def validate(text, raw):
    mode = commands(text)
    require(isinstance(raw, bytes) and len(raw) <= 14 and raw.endswith(b'\n')
            and b'\r' not in raw, 'complete raw LF response required')
    try:
        row = raw[:-1].decode('ascii').split(' ')
    except UnicodeDecodeError as error:
        raise ValueError('ASCII response required') from error
    require(len(row) == 2 and row[0] == 'R', 'exactly one R response required')
    return number(row[1], -0x80000000 if mode == 'P' else 0,
                  0x7fffffff if mode == 'P' else 0xffffffff)


def fingerprint(case):
    return {'name': case['name'],
            'input_sha256': hashlib.sha256(case['input'].encode('ascii')).hexdigest(),
            'expected_sha256': hashlib.sha256(case['expected'].encode('ascii')).hexdigest()}


def require_cases(cases, manifest):
    require(isinstance(manifest, list) and bool(manifest), 'nonempty mandatory case manifest required')
    names = [entry['name'] for entry in manifest]
    require(len(names) == len(set(names)), 'duplicate mandatory case identity')
    require(len(cases) == len(manifest) and {case['name'] for case in cases} == set(names),
            'complete mandatory case identities required')
    expected = {entry['name']: entry for entry in manifest}
    for case in cases:
        validate(case['input'], case['expected'].encode('ascii'))
        require(fingerprint(case) == expected[case['name']], 'mandatory case bytes changed: ' + case['name'])
