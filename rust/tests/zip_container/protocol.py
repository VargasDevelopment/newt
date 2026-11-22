"""Complete raw-LF grammar for synthetic ZIP host transports."""
import hashlib
import json
import re

NAT = r'(?:0|[1-9][0-9]*)'
SIGNED = r'(?:0|-?[1-9][0-9]*)'
HEX = r'(?:-|(?:[0-9a-f]{2})+)'
U32 = 0xffffffff
U64 = 0xffffffffffffffff


def require(condition, message):
    if not condition:
        raise ValueError(message)


def nat(value, maximum=U64):
    require(re.fullmatch(NAT, value) is not None, 'noncanonical natural number')
    result = int(value)
    require(result <= maximum, 'number outside declared wire width')
    return result


def signed(value):
    require(re.fullmatch(SIGNED, value) is not None, 'noncanonical signed number')
    result = int(value)
    require(-0x80000000 <= result <= 0x7fffffff, 'signed count outside i32')
    return result


def unhex(value):
    require(re.fullmatch(HEX, value) is not None, 'invalid byte encoding')
    return b'' if value == '-' else bytes.fromhex(value)


def request(line):
    require(isinstance(line, str) and '\n' not in line and '\r' not in line,
            'one request per canonical line')
    fields = line.split(' ')
    require(len(fields) == 9 and all(fields), 'request field count')
    data = unhex(fields[0])
    maximum = nat(fields[1], 0x7fffffff)
    require(re.fullmatch(SIGNED, fields[2]) is not None and int(fields[2]) >= -1, 'fault index')
    fault = int(fields[2])
    failure = signed(fields[3])
    require(failure <= 0, 'fault can only return zero or negative')
    names, entries, scratch = map(nat, fields[4:7])
    require(scratch in (0, 1), 'scratch availability')
    sizes = [nat(value, 1048576) for value in fields[7].split(',')]
    require(re.fullmatch(SIGNED, fields[8]) is not None, 'raw entry size')
    raw = int(fields[8])
    require(-1 <= raw <= U32, 'raw size outside u32')
    return dict(data=data, maximum=maximum, fault=fault, failure=failure,
                names=names, entries=entries, scratch=scratch, sizes=sizes, raw=raw)


def validate(line, raw, implementation, reopen=False):
    parameters = request(line)
    require(isinstance(raw, bytes) and raw.endswith(b'\n') and b'\r' not in raw,
            'complete raw LF output required')
    text = raw.decode('ascii')
    rows = text.split('\n')
    require(rows.pop() == '' and rows and rows[-1] == 'END', 'one terminal END required')
    fields = [row.split(' ') for row in rows]
    require(all(all(row) for row in fields), 'extra wire whitespace')
    cursor = 0
    resources = None
    if parameters['raw'] < 0:
        head = fields[cursor]; cursor += 1
        require(len(head) == 3 and head[0] == 'CAT', 'CAT header required')
        status, count = nat(head[1], 10), nat(head[2], 65535)
        for index in range(count):
            row = fields[cursor]; cursor += 1
            require(len(row) == 8 and row[0] == 'ENTRY' and nat(row[1]) == index, 'entry order')
            name = unhex(row[2])
            require(name and name[-1] == 0 and 0 not in name[:-1], 'complete exposed C-string')
            for value in row[3:7]: nat(value, U32)
            nat(row[7], 65535)
            row = fields[cursor]; cursor += 1
            require(len(row) == 4 and row[0] == 'FIND' and nat(row[1]) == index, 'lookup order')
            nat(row[2], U32);nat(row[3], U32)
        if reopen:
            row = fields[cursor];cursor += 1
            require(len(row) == 3 and row[0] == 'REOPEN', 'reopen receipt required')
            nat(row[1], 10);nat(row[2], 65535)
            count = 0  # This transport mode deliberately performs no entry reads.
        cap = fields[cursor]; cursor += 1
        if implementation == 'cpp':
            require(len(cap) == 5 and cap[0] == 'CPP_CAP', 'C++ resource facts required')
            element, used, high, failed = map(nat, cap[1:])
            require(element > 0 and used <= high <= parameters['entries'] * element + parameters['names'],
                    'C++ arena diagnostics')
            resources = dict(element_bytes=element, used=used, high_water=high, failed_alloc=failed)
        else:
            require(implementation == 'rust' and len(cap) == 5 and cap[0] == 'RUST_CAP', 'Rust resource facts required')
            slots, used, capacity, element = map(nat, cap[1:])
            require(element > 0, "nonempty typed metadata record required")
            require(slots == parameters['entries'] and used <= capacity == parameters['names'], 'Rust metadata/name diagnostics')
            require(count <= slots, 'Rust metadata capacity exceeded')
            resources = dict(slots=slots, names_used=used, name_capacity=capacity, element_bytes=element)
    else:
        require(fields[cursor] == ['RAW', str(parameters['raw'])], 'raw header')
        cursor += 1;count = 1
    for index in range(count):
        row = fields[cursor];cursor += 1
        require(len(row) == 3 and row[0] == 'OPEN' and nat(row[1]) == index, 'open order')
        status = nat(row[2], 10)
        if status:
            continue
        for want in parameters['sizes']:
            row = fields[cursor];cursor += 1
            require(len(row) == 6 and row[0] == 'OUT' and nat(row[1]) == index and nat(row[2]) == want,
                    'read order/requested size')
            got = signed(row[3]);nat(row[4], U32)
            require(got <= want, 'read over-return')
            data = unhex(row[5])
            require(len(data) == want, 'entire initialized destination required')
    calls = 0
    for row in fields[cursor:-1]:
        if row[0] == 'SIZE':
            require(len(row) == 2 and nat(row[1]) == len(parameters['data']), 'mock size receipt')
        else:
            require(len(row) == 4 and row[0] == 'READ', 'unknown or out-of-order trace')
            at, want, got = nat(row[1]), nat(row[2], U32), signed(row[3])
            expected = parameters['failure'] if calls == parameters['fault'] else min(
                max(0, len(parameters['data']) - at), want, parameters['maximum'])
            require(got == expected, 'mock read receipt does not match request/fault')
            calls += 1
    require(fields[-1] == ['END'], 'extra terminal fields')
    return {'semantic': [row for row in rows if not row.startswith(('CPP_CAP ', 'RUST_CAP '))],
            'resources': resources, 'source_read_calls': calls}


def semantic_digest(semantic):
    """Canonical SHA-256 over validated semantic wire lines (excluding *_CAP)."""
    encoded = json.dumps(semantic, separators=(',', ':'), ensure_ascii=True).encode('ascii')
    return hashlib.sha256(encoded).hexdigest()
