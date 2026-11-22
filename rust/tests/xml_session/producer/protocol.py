"""Validate the complete command-aware trace before comparing any mutant."""
import re
import importlib.util
from pathlib import Path
_spec = importlib.util.spec_from_file_location('xml_numeric_fields', Path(__file__).resolve().parents[1] / 'protocol_fields.py')
_fields = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_fields)

N = r'(?:0|[1-9][0-9]*)'
H = r'(?:-|(?:[0-9a-f]{2})+)'
STATUS = r'(?:Ok|IoError|NotZip|Truncated|Unsupported|OutOfMemory|ParseError)'
RULES = [rf'R {N} {N} (?:-1|{N})', rf'Q {N} [01]',
         rf'S {H}(?: {H} {H})*', rf'E {H}', rf'T {H}',
         rf'O {STATUS} {N} [01]', rf'F {STATUS} {N} [01] [01]',
         rf'C {N}', rf'P {STATUS}']


def parse(text, wire):
    if not text or not text.endswith('\n') or '\r' in text:
        raise ValueError('incomplete transport')
    rows = text[:-1].split('\n')
    if rows[-1] != 'END' or rows.count('END') != 1:
        raise ValueError('END framing')
    for row in rows[:-1]:
        if not any(re.fullmatch(rule, row) for rule in RULES):
            raise ValueError('malformed record')
    _fields.validate_numeric_fields(rows[:-1])
    request = wire[:-1].split(' ')
    if not wire.endswith('\n') or len(request) != 12 or request[0] != 'Y':
        raise ValueError('request envelope')
    actions = request[-1]
    wanted = ['P'] if actions == 'p' else ['O'] + [a.upper() for a in actions.split(',')]
    markers = [(i, row[0]) for i, row in enumerate(rows) if row.startswith(('O ', 'P ', 'C ', 'F '))]
    if [mark for _, mark in markers] != wanted or markers[-1][0] != len(rows) - 2:
        raise ValueError('command results/order')
    # No callbacks/reads/polls belong to close; open may read a local header.
    previous = -1
    for end, command in markers:
        operations = rows[previous + 1:end]
        if command == 'C' and operations:
            raise ValueError('close has operations')
        if command == 'O' and any(r.startswith(('S ', 'T ', 'E ')) for r in operations):
            raise ValueError('callback before feed')
        previous = end
    polls = [int(r.split(' ')[1]) for r in rows if r.startswith('Q ')]
    if polls != list(range(len(polls))) or (request[6] == '0' and polls):
        raise ValueError('policy command order')
    return tuple(rows)


def malformed_controls():
    wire = 'Y 3c722f3e 2048 -1 0 0 1 -1 0 0 4 f\n'
    valid = 'Q 0 0\nO Ok 0 1\nQ 1 0\nR 0 2048 4\nQ 2 0\nS 72\nE 72\nQ 3 0\nF Ok 4 0 1\nEND\n'
    parse(valid, wire)
    fixtures = [valid[:-1], valid + 'END\n', valid.replace('Q 2', 'Q 8'),
                valid.replace('F Ok 4 0 1\n', ''), valid.replace('R 0', 'R 00'),
                valid.replace('END\n', 'junk\nEND\n'), valid.replace('S 72', 'S 7A'),
                valid.replace('END\n', 'R 0 1 1\nEND\n')]
    fixtures += [valid.replace('R 0 2048 4', replacement) for replacement in
                 ['R 0 2048 2049', 'R 18446744073709551616 2048 4',
                  'R 0 4294967296 4', 'R 0 4294967295 2147483648', 'R 0 2048 -2']]
    fixtures += [valid.replace('F Ok 4 0 1', 'F Ok 18446744073709551616 0 1'),
                 valid.replace('O Ok 0 1', 'O Ok 18446744073709551616 1')]
    for malformed in fixtures:
        try:
            parse(malformed, wire)
        except ValueError:
            continue
        raise AssertionError('malformed response accepted')
