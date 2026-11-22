"""Command-aware complete CSS transport grammar; raw bytes, no newline folding."""
import re

NAT = re.compile(r'(?:0|[1-9][0-9]*)\Z')
INT = re.compile(r'(?:0|-?[1-9][0-9]*)\Z')
HEX = re.compile(r'(?:[0-9a-f]{2})+\Z')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def number(value, low=0, high=0xffffffff):
    require((INT if low < 0 else NAT).fullmatch(value) is not None, 'canonical integer required')
    result = int(value)
    require(low <= result <= high, 'integer outside wire width')
    return result


def data(value, nullable=False):
    if nullable and value == '-': return None
    if value == '_': return b''
    require(HEX.fullmatch(value) is not None, 'canonical bytes required')
    return bytes.fromhex(value)


def commands(text):
    require(isinstance(text, str) and text.endswith('\n') and '\r' not in text, 'raw LF request required')
    rows = [line.split(' ') for line in text[:-1].split('\n')]
    for row in rows:
        require(all(row), 'empty request field')
        op = row[0]
        if op == 'B' and len(row) == 2: number(row[1], high=1024)
        elif op == 'T' and len(row) == 2: data(row[1])
        elif op in ('I', 'A') and len(row) == 2: data(row[1], True)
        elif op == 'C' and len(row) == 4:
            data(row[1], True);data(row[2], True);number(row[3], high=1)
        elif op == 'F' and len(row) == 1: pass
        elif op == 'S' and len(row) == 11:
            data(row[1]);number(row[2], high=65535)
            for value in row[3:6]: number(value)
            number(row[6], high=0xffffffffffffffff)
            number(row[7], -1, 0x7fffffffffffffff);number(row[8], -0x80000000, 0)
            number(row[9], high=1);number(row[10], high=65537)
        else: raise ValueError('unknown command or arity')
    return rows


def validate(text, raw):
    requests = commands(text)
    require(isinstance(raw, bytes) and raw.endswith(b'\n') and b'\r' not in raw,
            'complete raw LF output required')
    rows = [line.split(' ') for line in raw[:-1].decode('ascii').split('\n')]
    require(all(all(row) for row in rows), 'extra output whitespace')
    cursor = 0

    def take(tag, fields):
        nonlocal cursor
        require(cursor < len(rows), 'truncated response')
        row = rows[cursor];cursor += 1
        require(len(row) == fields and row[0] == tag, 'wrong response/order/arity')
        return row

    def decl():
        row = take('DECL', 13)
        number(row[1], high=65535);number(row[2], high=1)
        number(row[3], -128, 127);number(row[4], -128, 127);number(row[5], high=4)
        for value in row[6:10]: number(value, -32768, 32767)
        for value in row[10:]: number(value, -128, 127)

    for request in requests:
        op = request[0]
        if op == 'B': number(take('BEGIN', 2)[1], high=1)
        elif op in ('I', 'A', 'C'): decl()
        elif op == 'F':
            row = take('SHEET', 4)
            count = number(row[1], high=65535);number(row[2]);number(row[3], high=65535)
            for _ in range(count):
                rule = take('RULE', 3);number(rule[1]);number(rule[2]);decl()
        elif op == 'S':
            number(take('STATUS', 2)[1], high=10)
            number(take('SCRATCH', 2)[1], high=1)
            source = data(request[1]);maximum = int(request[6]);fault = int(request[7]);failure = int(request[8])
            call = 0
            while cursor < len(rows) and rows[cursor][0] == 'READ':
                row = take('READ', 4)
                at = number(row[1], high=0xffffffffffffffff)
                want = number(row[2]);got = number(row[3], -0x80000000, 0x7fffffff)
                expected = failure if call == fault else min(max(0, len(source) - at), want, maximum)
                require(got == expected, 'mock source read differs from declared request/fault')
                call += 1
    require(cursor == len(rows), 'unsolicited response rows')
    return [' '.join(row) for row in rows]


def require_cases(cases, labels):
    expected = set(labels)
    require(len(expected) == len(labels), 'duplicate frozen CSS case identity')
    require(len(cases) == len(expected) and {case['name'] for case in cases} == expected,
            'complete frozen CSS case identities required')
