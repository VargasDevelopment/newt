"""Complete LF grammar; accepted malformed output can never kill a mutant."""
import re
import importlib.util
from pathlib import Path
_spec = importlib.util.spec_from_file_location('xml_numeric_fields', Path(__file__).resolve().parent / 'protocol_fields.py')
_fields = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_fields)

STATUS = r'(?:Ok|IoError|Unsupported|OutOfMemory|ParseError)'
N = r'(?:0|[1-9][0-9]*)'
H = r'(?:-|(?:[0-9a-f]{2})+)'
RULES = [rf'O {STATUS} {N} [01]', rf'F {STATUS} {N} [01] [01]',
         rf'C {N}', rf'P {STATUS}', rf'R {N} {N} (?:-1|{N})',
         rf'S {H}(?: {H} {H})*', rf'E {H}', rf'T {H}']


def parse(text, wire=None):
    if not text or not text.endswith('\n') or '\r' in text:
        raise ValueError('incomplete LF transport')
    rows = text[:-1].split('\n')
    if rows[-1] != 'END' or rows.count('END') != 1:
        raise ValueError('missing/extra END')
    for row in rows[:-1]:
        if not any(re.fullmatch(rule, row) for rule in RULES):
            raise ValueError('malformed response: ' + repr(row))
    _fields.validate_numeric_fields(rows[:-1])
    # Exactly one opening or one-shot result; no hidden second response.
    if sum(row.startswith(('O ', 'P ')) for row in rows) != 1:
        raise ValueError('response envelope')
    if any(row.startswith('P ') for row in rows) and rows[-2][:2] != 'P ':
        raise ValueError('one-shot terminal result')
    if any(row.startswith('O ') for row in rows) and not rows[0].startswith('O '):
        raise ValueError('opening order')
    markers = [(index, row[0]) for index, row in enumerate(rows)
               if row.startswith(('O ', 'F ', 'C ', 'P '))]
    if markers[-1][0] != len(rows) - 2:
        raise ValueError('operations after terminal result')
    previous = -1
    for end, command in markers:
        if command == 'C' and rows[previous + 1:end]:
            raise ValueError('close has operations')
        previous = end
    if wire is not None:
        actions = wire.rstrip('\n').split(' ')[-1]
        markers = [row[0] for row in rows if row.startswith(('O ', 'F ', 'C ', 'P '))]
        wanted = ['P'] if actions == 'p' else ['O'] + [a.upper() for a in actions.split(',')]
        if markers != wanted:
            raise ValueError('missing/extra action result')
    return tuple(rows)
