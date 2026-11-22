# SPDX-License-Identifier: MIT
# MIT; see LICENSE.
"""Validate transport grammar, without validating the record policies under test."""
import re

NAME = re.compile(r'[A-Za-z0-9_]+\Z')
HEX = re.compile(r'(?:[0-9a-f]{2})*\Z')


def validate_hex(value):
    if not HEX.fullmatch(value):
        raise AssertionError('invalid hex; not a mutation kill')


def validate_records(value):
    if not value:
        return
    names = []
    for entry in value.split(';'):
        if entry.count(':') != 1:
            raise AssertionError('invalid record map; not a mutation kill')
        name, pair = entry.split(':')
        if not NAME.fullmatch(name) or pair.count(',') != 1:
            raise AssertionError('invalid record map; not a mutation kill')
        for slot in pair.split(','):
            validate_hex(slot)
        names.append(name)
    if names != sorted(set(names)):
        raise AssertionError('noncanonical record map; not a mutation kill')


def validate_trace(value):
    if not value:
        return
    for entry in value.split(';'):
        fields = entry.split(':')
        command = fields[0]
        valid = False
        if command == 'end':
            valid = len(fields) == 1
        elif command == 'begin':
            valid = len(fields) == 3 and NAME.fullmatch(fields[1]) and fields[2] in ('0', '1')
        elif command == 'read':
            valid = len(fields) == 2 and fields[1] in ('slot0', 'slot1')
        elif command == 'put':
            valid = len(fields) == 3 and fields[1] in ('slot0', 'slot1')
            if valid:
                validate_hex(fields[2])
        elif command == 'aside':
            valid = len(fields) == 2 and NAME.fullmatch(fields[1])
        elif command == 'repair':
            valid = len(fields) == 3 and NAME.fullmatch(fields[1])
            if valid:
                validate_hex(fields[2])
        if not valid:
            raise AssertionError('invalid trace; not a mutation kill')


def validate(line, request):
    fields = line.split('|')
    if len(fields) != 5:
        raise AssertionError('invalid response fields; not a mutation kill')
    code, output, trace, visible, durable = fields
    op, kind, cancelled = request[:3]
    allowed = {0, 1, 2, 3, 4}
    if op in (2, 3) or op == 0 and kind == 7 or op == 7 and kind == 1:
        allowed = {0, 1}
    elif op in (5, 6) or op == 7 and kind == 0:
        allowed = {0}
    if cancelled and op not in (0, 6):
        allowed.add(255)
    if code not in {str(value) for value in allowed}:
        raise AssertionError('invalid result code; not a mutation kill')
    validate_hex(output)
    validate_trace(trace)
    validate_records(visible)
    validate_records(durable)
    return line
