"""Independent operation transcripts; not captured from either implementation."""


def wire(xml, producer=1, cancel=-1, method=0, local=0, size=None,
         actions='f,f,c', fail=-1, filtering=0, stop=0):
    return f'Y {xml.hex() or "-"} 2048 {fail} {filtering} {stop} {producer} {cancel} {method} {local} {len(xml) if size is None else size} {actions}\n'


def trace(*rows):
    return '\n'.join((*rows, 'END', ''))


VECTORS = []
def add(name, request, *rows):
    VECTORS.append((name, request, trace(*rows)))

xml = b'<r/>'
for mode in (0, 1):
    q = lambda i: [f'Q {i} 0'] if mode else []
    for method, local in ((0, 0), (0, 1), (8, 1)):
        # A single final stored DEFLATE block exercises the actual inflater.
        payload = xml if method == 0 else b'\x01\x04\x00\xfb\xff' + xml
        header = b'PK\x03\x04' + bytes(26) if local else b''
        request = wire(header + payload, producer=mode, method=method, local=local, size=4)
        reads = [f'R {30 if local else 0} {len(payload)} {len(payload)}']
        add(f'mode-{mode}-method-{method}-local-{local}', request,
            *q(0), *(['R 0 30 30'] if local else []), 'O Ok 0 1',
            *q(1), *reads, *q(2), 'S 72', 'E 72', *q(3), 'F Ok 4 0 1',
            *q(4), *q(5), *q(6), 'F Ok 4 1 1', 'C 4')
for cancel in range(4):
    prefix = ['Q 0 0', 'O Ok 0 1', 'Q 1 0', 'R 0 4 4', 'Q 2 0', 'S 72', 'E 72', 'Q 3 0']
    end = [0, 2, 4, 7][cancel]
    prefix = prefix[:end] + [f'Q {cancel} 1']
    if cancel == 0:
        prefix += ['O IoError 0 0', 'F Unsupported 0 0 0']
    else:
        prefix += ['F IoError 0 0 1']
    add(f'cancel-{cancel}', wire(xml, cancel=cancel, actions='f'), *prefix)
add('io-before-cancel', wire(xml, cancel=2, fail=0, actions='f'),
    'Q 0 0','O Ok 0 1','Q 1 0','R 0 4 -1','F IoError 0 0 1')
bad = b'<r></s>'
add('parse-before-cancel', wire(bad, cancel=3, actions='f'),
    'Q 0 0','O Ok 0 1','Q 1 0','R 0 7 7','Q 2 0','S 72','F ParseError 0 0 1')
add('final-cancel', wire(xml, cancel=6, actions='f,f'),
    'Q 0 0','O Ok 0 1','Q 1 0','R 0 4 4','Q 2 0','S 72','E 72','Q 3 0','F Ok 4 0 1',
    'Q 4 0','Q 5 0','Q 6 1','F IoError 4 0 1')
for name, xml in [('internal-dtd', b"<!DOCTYPE r [<!ENTITY x 'v'>]><r>&x;</r>"),
                  ('empty-subset', b'<!DOCTYPE r []><r/>')]:
    add(name, wire(xml, actions='f'), 'Q 0 0','O Ok 0 1','Q 1 0',
        f'R 0 {len(xml)} {len(xml)}','Q 2 0','F ParseError 0 0 1')
# Long attribute/comment/declaration tokens must be refused BEFORE completing.
for name, xml in [('attribute', b"<r a='" + b'x'*4090 + b"'/>"),
                  ('comment', b'<!--' + b'x'*4093 + b'--><r/>'),
                  ('declaration', b"<?xml version='1.0' " + b' '*4100 + b'?><r/>')]:
    remaining = len(xml) - 4096
    add('pending-' + name, wire(xml, actions='f,f,f'),
        'Q 0 0','O Ok 0 1','Q 1 0','R 0 2048 2048','Q 2 0','Q 3 0','F Ok 2048 0 1',
        'Q 4 0',f'R 2048 {min(2048,len(xml)-2048)} 2048','Q 5 0','Q 6 0','F Ok 4096 0 1',
        'Q 7 0',f'R 4096 {remaining} {remaining}','Q 8 0','F Unsupported 4096 0 1')
# Bound is inclusive; index advancement releases retained-token budget.
xml = b'<!--' + b'x'*4089 + b'--><r/>'
add('pending-exact', wire(xml, actions='f,f,f'),
    'Q 0 0','O Ok 0 1','Q 1 0','R 0 2048 2048','Q 2 0','Q 3 0','F Ok 2048 0 1',
    'Q 4 0','R 2048 2048 2048','Q 5 0','Q 6 0','F Ok 4096 0 1',
    'Q 7 0','R 4096 4 4','Q 8 0','S 72','E 72','Q 9 0','F Ok 4100 0 1')

xml = b"<!DOCTYPE r [<!ENTITY x 'v'>]><r>&x;</r>"
add('ordinary-internal-dtd', wire(xml, producer=0, actions='f'),
    'O Ok 0 1',f'R 0 {len(xml)} {len(xml)}','S 72','T 76','E 72',f'F Ok {len(xml)} 0 1')
xml = b'<!DOCTYPE r><r/>'
add('producer-doctype-without-subset', wire(xml, actions='f'),
    'Q 0 0','O Ok 0 1','Q 1 0',f'R 0 {len(xml)} {len(xml)}','Q 2 0',
    'S 72','E 72','Q 3 0',f'F Ok {len(xml)} 0 1')
add('producer-one-shot-stop', wire(b'<r/>', stop=1, actions='p'), 'Q 0 0', 'P Ok')
add('producer-one-shot', wire(b'<r/>', actions='p'),
    'Q 0 0','Q 1 0','R 0 4 4','Q 2 0','S 72','E 72','Q 3 0',
    'Q 4 0','Q 5 0','Q 6 0','P Ok')
xml = b'<r>x</s>'
add('producer-stop-parse-error', wire(xml, cancel=3, stop=2, actions='p'),
    'Q 0 0','Q 1 0',f'R 0 {len(xml)} {len(xml)}','Q 2 0','S 72','T 78','P ParseError')
xml = b'<r>&nbsp;</r>'
add('producer-filter-progress', wire(xml, filtering=1),
    'Q 0 0','O Ok 0 1','Q 1 0',f'R 0 {len(xml)} {len(xml)}','Q 2 0',
    'S 72','T c2a0','E 72','Q 3 0','F Ok 9 0 1',
    'Q 4 0','Q 5 0','Q 6 0','F Ok 9 1 1','C 9')
header = b'PK\x03\x04' + bytes(26)
add('local-header-io-error', wire(header+b'<r/>', local=1, size=4, fail=0, actions='f'),
    'Q 0 0','R 0 30 -1','O IoError 0 0','F Unsupported 0 0 0')
add('deflate-invalid-block', wire(header+b'\x07', method=8, local=1, size=4, actions='f'),
    'Q 0 0','R 0 30 30','O Ok 0 1','Q 1 0','R 30 1 1','F IoError 0 0 1')
add('raw-deflate-unsupported', wire(b'\x07', method=8, size=4, actions='f'),
    'Q 0 0','O Unsupported 0 0','F Unsupported 0 0 0')

# Local-header failures are real reader statuses, not malformed transport.
for mode in (0, 1):
    polls = ['Q 0 0'] if mode else []
    add(f'local-signature-mode-{mode}',
        wire(b'BAD!' + bytes(26) + b'<r/>', producer=mode, local=1, size=4, actions='f'),
        *polls, 'R 0 30 30', 'O NotZip 0 0', 'F Unsupported 0 0 0')
    add(f'local-stored-size-mode-{mode}',
        wire(header + b'<r/>', producer=mode, local=1, size=5, actions='f'),
        *polls, 'R 0 30 30', 'O Truncated 0 0', 'F Unsupported 0 0 0')

add('closed-eof-reset', wire(b'<r/>', actions='c,f'),
    'Q 0 0','O Ok 0 1','C 0','F Unsupported 0 0 0')
add('cancelled-open-eof-reset', wire(b'<r/>', cancel=0, actions='f,c,f'),
    'Q 0 1','O IoError 0 0','F Unsupported 0 0 0','C 0','F Unsupported 0 0 0')

add('fresh-eof-reset', wire(b'<r/>', actions='n'), 'F Unsupported 0 0 0')
