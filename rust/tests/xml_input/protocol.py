"""Command-aware XML-input wire grammar; preserve raw LF bytes throughout."""
import hashlib
import json
import re


def require(ok, message):
    if not ok:
        raise ValueError(message)


def number(value, low=0, high=0xffffffffffffffff):
    require(re.fullmatch(r'0|-?[1-9][0-9]*', value) is not None, 'noncanonical integer')
    result = int(value)
    require(low <= result <= high, 'integer out of range')
    return result


def data(value, nullable=False, request=False):
    if nullable and value == 'null':
        return None
    if value == '-':
        return b''
    pattern = r'(?:[0-9a-fA-F]{2})+' if request else r'(?:[0-9a-f]{2})+'
    require(re.fullmatch(pattern, value) is not None, 'invalid hex bytes')
    return bytes.fromhex(value)


def commands(text):
    require(isinstance(text, str) and text.endswith('\n') and '\r' not in text, 'raw LF request required')
    rows = [line.split(' ') for line in text[:-1].split('\n')]
    for row in rows:
        require(all(row), 'empty request field')
        op = row[0]
        if op == 'F' and len(row) == 7:
            data(row[1], request=True); number(row[2], high=1048576)
            number(row[3], -1, 1048576); number(row[4], -0x80000000, 0)
            require(row[5] == '0', 'reserved F field')
            for action in row[6].split(','):
                if action != 'r': number(action, high=1048576)
        elif op == 'D' and len(row) == 3:
            data(row[1], request=True); number(row[2], high=1048576)
        elif op == 'P' and len(row) == 5:
            number(row[1], high=1048576)
            data(row[2], True, True); data(row[3], True, True); number(row[4], high=1)
        elif op in ('L', 'H') and len(row) == 3:
            data(row[1], op == 'H', True); data(row[2], request=True)
        elif op in ('A', 'E') and len(row) >= 3 and (len(row)-3) % 2 == 0:
            require(row[1] in ('local', 'exact'), 'attribute mode')
            for field in row[2:]: data(field, request=True)
        elif op == 'T' and len(row) > 1:
            for action in row[1:]:
                if action not in ('b', 'e', 't', 'n'):
                    require(action.startswith('a'), 'text action')
                    data(action[1:], request=True)
        else:
            raise ValueError('unknown command or arity')
    return rows


def validate(text, raw):
    requests = commands(text)
    require(isinstance(raw, bytes) and raw.endswith(b'\n') and b'\r' not in raw, 'complete raw LF output required')
    try:
        rows = [line.split(' ') for line in raw[:-1].decode('ascii').split('\n')]
    except UnicodeDecodeError as error:
        raise ValueError('non-ASCII output') from error
    require(all(all(row) for row in rows), 'output whitespace')
    cursor = 0
    payload = bytearray()

    def take(tag, count):
        nonlocal cursor
        require(cursor < len(rows), 'truncated output')
        row = rows[cursor]; cursor += 1
        require(row[0] == tag and len(row) == count, 'response order/arity')
        return row

    for request in requests:
        op = request[0]
        if op == 'F':
            for action in request[6].split(','):
                if action == 'r':
                    take('RESET', 1)
                    continue
                cap = int(action)
                row = take('F', 5)
                got = number(row[1], -1, cap)
                destination = data(row[2])
                require(len(destination) == cap, 'full destination required')
                number(row[3]); number(row[4], high=0xffffffff)
                if got > 0: payload.extend(destination[:got])
        elif op == 'D':
            row = take('D', 3); number(row[1], high=1)
            require(len(data(row[2])) == int(request[2]), 'directory destination length')
        elif op == 'P':
            row = take('P', 6)
            for field in row[1:3]:
                value = data(field, True)
                require(value is None or (value.endswith(b'\0') and b'\0' not in value[:-1]), 'NUL-terminated path/fragment')
            for field in row[3:]: number(field)
        elif op == 'L':
            row = take('L', 3); data(row[1]); number(row[2], high=1)
        elif op in ('H', 'E'):
            number(take(op, 2)[1], high=1)
        elif op == 'A':
            data(take('A', 2)[1], True)
        elif op == 'T':
            for _ in request[1:]:
                row = take('T', 5)
                length = number(row[1], high=255); number(row[2], high=1)
                data(row[3], True)
                buf = data(row[4])
                require(len(buf) == length + 1 and buf[-1] == 0, 'text capture length/terminator')
        take('END', 1)
    require(cursor == len(rows), 'unsolicited output')
    return {'lines': [' '.join(row) for row in rows], 'payload': bytes(payload)}


def fingerprint(case):
    expected = {key: (case[key].hex() if key == 'payload' else case[key])
                for key in ('payload', 'lines') if key in case}
    return {'input_sha256': hashlib.sha256(case['input'].encode()).hexdigest(),
            'expectation_sha256': hashlib.sha256(json.dumps(expected, sort_keys=True).encode()).hexdigest()}


def require_cases(cases, labels):
    names = [case['name'] for case in cases]
    require(len(names) == len(set(names)) and set(names) == set(labels), 'complete exact frozen XML-input case set required')
    for case in cases:
        commands(case['input'] + '\n')
        require(fingerprint(case) == labels[case['name']], 'case erased/replaced or independent expectation changed')


def independent(case, response):
    for field in ('payload', 'lines'):
        if field in case:
            require(response[field] == case[field], f'independent {field} expectation: {case["name"]}')
