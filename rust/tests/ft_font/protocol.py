"""Command-specific reply admission before font evidence comparisons."""
import re


def require(ok, why):
    if not ok:
        raise ValueError(why)


def integer(word, low, high):
    require(re.fullmatch(r'0|-?[1-9][0-9]*', word) is not None, 'noncanonical integer')
    value = int(word)
    require(low <= value <= high, 'integer outside field domain')
    return value


def packed(word, count):
    if count == 0:
        require(word == '-', 'nonempty zero-sized buffer')
    else:
        require(len(word) == 2 * count and re.fullmatch('[0-9a-f]+', word) is not None,
                'noncanonical or wrong-sized buffer')


def admit(text, request):
    require(text.endswith('\n') and '\r' not in text, 'reply newline')
    rows = text[:-1].split('\n')
    commands = request.splitlines()[2:]
    require(len(rows) == len(commands) + 2 and rows[0] == 'FTFONT1' and rows[-1] == 'END',
            'reply envelope/cardinality')
    aliases = {'GMETRIC': 'METRIC', 'GRASTER': 'RASTER', 'GKERN': 'KERN'}
    for command, row in zip(commands, rows[1:-1]):
        incoming = command.split(' ')
        op = aliases.get(incoming[0], incoming[0])
        t = row.split(' ')
        require(all(t) and t[0] == op, 'reply opcode or empty field')
        if op in ('INIT', 'OPTIONS'):
            require(len(t) == 2, 'boolean shape')
            integer(t[1], 0, 1)
        elif op in ('METRIC', 'RASTER', 'LINE'):
            require(len(t) >= 2, 'result flag absent')
            success = integer(t[1], 0, 1)
            expected = {'METRIC': 7, 'RASTER': 8, 'LINE': 5}[op] if success else 2
            require(len(t) == expected, 'result-dependent shape')
            if not success:
                continue
            if op == 'METRIC':
                integer(t[2], -(2**31), 2**31 - 1)
                for value in t[3:5]:
                    integer(value, -32768, 32767)
                for value in t[5:7]:
                    integer(value, 0, 65535)
            elif op == 'LINE':
                for value in t[2:]:
                    integer(value, -(2**31), 2**31 - 1)
            else:
                width, height = (integer(value, 0, 65535) for value in t[2:4])
                for value in t[4:7]:
                    integer(value, -32768, 32767)
                packed(t[7], width * height)
        elif op in ('KERN', 'LIGA', 'LIGAG'):
            require(len(t) == 2, 'scalar shape')
            integer(t[1], -(2**31) if op == 'KERN' else 0,
                    2**31 - 1 if op == 'KERN' else 2**32 - 1)
        elif op == 'STATUS':
            require(len(t) == 6, 'status shape')
            integer(t[1], 0, 1)
            integer(t[2], 0, 5)
            integer(t[3], -(2**31), 2**31 - 1)
            integer(t[4], 0, 7)
            integer(t[5], -(2**31), 2**31 - 1)
        elif op == 'INSPECT':
            require(len(t) == 6, 'inspection shape')
            require(integer(t[1], -2, 0) in (-2, -1, 0), 'inspection status')
            integer(t[2], 0, 2**32 - 1)
            integer(t[3], 0, 65535)
            integer(t[4], 0, 1)
            packed(t[5], int(incoming[2]))
        else:
            require(op in ('BUDGET', 'RELEASE', 'ALLOC', 'DEINIT') and len(t) == 1,
                    'acknowledgement shape')
    return rows[1:-1]


def controls():
    request = 'FTFONT1\n00\nMETRIC 65 640\n'
    for line in ('METRIC 0', 'METRIC 1 384 1 7 4 7'):
        admit('FTFONT1\n' + line + '\nEND\n', request)
    invalid = [
        (request, 'METRIC 2'), (request, 'METRIC 0 384 1 7 4 7'),
        (request, 'METRIC 1'), (request, 'METRIC 1 384 32768 7 4 7'),
        (request, 'METRIC 1 384 1 7 -1 7'), (request, 'METRIC 01'),
        ('FTFONT1\n00\nSTATUS\n', 'STATUS 2 0 0 0 0'),
        ('FTFONT1\n00\nSTATUS\n', 'STATUS 1 6 0 0 0'),
        ('FTFONT1\n00\nSTATUS\n', 'STATUS 1 0 0 8 0'),
        ('FTFONT1\n00\nRASTER 65 640\n', 'RASTER 1 1 1 0 0 1 FF'),
        ('FTFONT1\n00\nRASTER 65 640\n', 'RASTER 1 1 1 0 0 1 -'),
        ('FTFONT1\n00\nINSPECT 0 1\n', 'INSPECT 1 0 400 0 00'),
        ('FTFONT1\n00\nINSPECT 0 1\n', 'INSPECT 0 0 400 0 -'),
    ]
    for wire, line in invalid:
        try:
            admit('FTFONT1\n' + line + '\nEND\n', wire)
        except ValueError:
            continue
        raise RuntimeError('malformed reply admitted: ' + line)
