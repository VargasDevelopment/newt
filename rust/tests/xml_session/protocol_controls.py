"""Positive grammar witnesses and malformed response negative controls."""
from protocol import parse
from vectors import VECTORS

for _, expected in VECTORS:
    parse(expected)
good = VECTORS[0][1]
close_marker = next(row for row in good.splitlines() if row.startswith('C '))
for bad in [good[:-1], good.replace('\n','\r\n'), good+'END\n', good+'\n',
            good.replace('Ok','Bogus',1), good.replace('S 72','S 7',1),
            good.replace('O Ok 0 1','O Ok 00 1'),
            good.replace('O Ok 0 1','O Ok 0 2'), good.replace('END\n',''),
            good.replace('T 78','T 7A'), good.replace('S 72 61 76','S 72 61'),
            good.replace('END\n', 'R 0 1 1\nEND\n'),
            good.replace('END\n', 'S 72\nEND\n'),
            good.replace(close_marker + '\n', 'R 0 1 1\n' + close_marker + '\n', 1)]:
    if bad == good:
        raise RuntimeError('negative fixture did not change its baseline')
    try:
        parse(bad)
    except ValueError:
        continue
    raise AssertionError(('malformed response accepted', bad))
