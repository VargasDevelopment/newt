"""Complete raw-LF grammar and exact independently expected corpus identities."""
import hashlib
import json
import re


def require(ok, message):
    if not ok:
        raise ValueError(message)


def integer(text, maximum):
    require(re.fullmatch(r'0|[1-9][0-9]*', text) is not None, 'noncanonical integer')
    value = int(text)
    require(value <= maximum, 'wire integer overflow')
    return value


def validate(commands, raw):
    require(raw.endswith(b'\n') and b'\r' not in raw, 'complete raw LF response required')
    try:
        lines = raw[:-1].decode('ascii').split('\n')
    except UnicodeDecodeError as error:
        raise ValueError('non-ASCII response') from error
    require(len(lines) == len(commands), 'complete response count required')
    for command, line in zip(commands, lines):
        request = command.split(' ')
        row = line.split(' ')
        require(all(row) and row[0] == request[0], 'response order or whitespace')
        if row[0] == 'I':
            require(len(row) == 3, 'init response arity')
            integer(row[1], 1); integer(row[2], 1)
        elif row[0] == 'R':
            require(len(row) == 2, 'ready response arity')
            integer(row[1], 1)
        elif row[0] == 'B':
            require(len(row) >= 2, 'break response arity')
            count = integer(row[1], int(request[1]))
            require(len(row) == count + 2, 'complete break positions required')
            for value in row[2:]: integer(value, 255)
        else:
            raise ValueError('unknown response')
    return lines


def receipt(case):
    return hashlib.sha256(json.dumps({'commands': case.commands, 'expected': case.expected,
                                    'native': case.native}, sort_keys=True).encode()).hexdigest()


def require_cases(cases, labels):
    names = [case.name for case in cases]
    require(len(names) == len(set(names)) and set(names) == set(labels), 'exact full hyphenation corpus required')
    for case in cases:
        require(receipt(case) == labels[case.name], 'corpus input/domain/expectation replaced')
