"""Transport framing only. Product outcomes and event values are expectations' job."""
import re


class ProtocolError(ValueError):
    pass


def integer(word, bits=64, signed=False):
    pattern = r'(?:0|-[1-9][0-9]*|[1-9][0-9]*)' if signed else r'(?:0|[1-9][0-9]*)'
    if not re.fullmatch(pattern, word):
        raise ProtocolError('noncanonical integer')
    value = int(word)
    low, high = (-(1 << (bits - 1)), (1 << (bits - 1)) - 1) if signed else (0, (1 << bits) - 1)
    if not low <= value <= high:
        raise ProtocolError('integer out of range')
    return value


def requests(stdin):
    lines = stdin.splitlines()
    header = lines[0].split(' ')
    if len(header) != 15:
        raise ProtocolError('header arity')
    for i, word in enumerate(header):
        integer(word, 32 if i == 5 else 64, i == 5)
    commands = []
    for line in lines[1:]:
        fields = line.split(' ')
        if len(fields) != 4 or fields[0] != 'R':
            raise ProtocolError('request arity/opcode')
        integer(fields[1])
        size = integer(fields[2], 32)
        if size > 16384 or fields[3] not in ('0', '1'):
            raise ProtocolError('request limits')
        commands.append(size)
    return commands


def validate(stdin, stdout):
    sizes = requests(stdin)
    if not stdout.endswith('\n') or '\r' in stdout:
        raise ProtocolError('response must end with LF only')
    lines = stdout[:-1].split('\n')
    allocated = dropped = False
    outputs = 0
    for index, line in enumerate(lines):
        fields = line.split(' ')
        tag = fields[0]
        if dropped:
            raise ProtocolError('record after D')
        if tag == 'O':
            if not allocated or outputs >= len(sizes) or len(fields) != 3:
                raise ProtocolError('unexpected O')
            if fields[1] not in ('0', '1') or not re.fullmatch(r'[0-9a-f]*', fields[2]):
                raise ProtocolError('output flag/hex')
            if len(fields[2]) != 2 * sizes[outputs]:
                raise ProtocolError('output length for command')
            outputs += 1
            continue
        arities = {'S': 2, 'P': 3, 'R': 4, 'Y': 1, 'A': 4, 'D': 5}
        if tag not in arities or len(fields) != arities[tag]:
            raise ProtocolError('record opcode/arity')
        for field_index, word in enumerate(fields[1:], 1):
            signed = (tag == 'P' and field_index == 2) or (tag == 'R' and field_index == 3)
            bits = 32 if signed or (tag == 'R' and field_index == 2) else 64
            integer(word, bits, signed)
        if index == 0 and tag != 'S':
            raise ProtocolError('first record must be S')
        if tag == 'A':
            if allocated:
                raise ProtocolError('duplicate A')
            allocated = True
        elif tag == 'D':
            if not allocated or outputs != len(sizes):
                raise ProtocolError('early D or missing outputs')
            dropped = True
        elif not allocated:
            if tag not in ('S', 'P'):
                raise ProtocolError('read/yield before A')
        elif outputs >= len(sizes):
            raise ProtocolError('event without pending command')
        if tag == 'R' and fields[2] == '0':
            raise ProtocolError('zero physical read must use P')
    if not allocated or not dropped or outputs != len(sizes):
        raise ProtocolError('incomplete response')
    return lines
