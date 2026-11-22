"""Shared integer domains for ordinary, producer and chapter XML traces."""
import re


def unsigned(word, maximum):
    if (not re.fullmatch(r'(?:0|[1-9][0-9]*)', word)
            or len(word) > len(str(maximum)) or int(word) > maximum):
        raise ValueError('XML integer outside declared domain')
    return int(word)


def validate_numeric_fields(rows):
    for row in rows:
        fields = row.split(' ')
        kind = fields[0]
        if kind == 'R':
            if len(fields) != 4:
                raise ValueError('XML read arity')
            unsigned(fields[1], (1 << 64) - 1)
            capacity = unsigned(fields[2], (1 << 32) - 1)
            count = -1 if fields[3] == '-1' else unsigned(fields[3], (1 << 31) - 1)
            if count > capacity:
                raise ValueError('XML read result exceeds requested capacity')
        elif kind == 'Q':
            unsigned(fields[1], (1 << 63) - 1)
        elif kind in ('O', 'F'):
            unsigned(fields[2], (1 << 64) - 1)
        elif kind == 'C':
            unsigned(fields[1], (1 << 64) - 1)
        elif kind not in ('S', 'T', 'E', 'P'):
            raise ValueError('unknown XML numeric record')
