from protocol import parse
from vectors import VECTORS

def controls():
    for v in VECTORS:
        parse(v['expected'],v['wire'])
        parse(v.get('rust_expected',v['expected']),v['wire'])
    wire='P C - - 1 -1 1 1\n'
    invalid=['','P Ok\nC -\nEND','P Ok\r\nC -\nEND\n','P Ok\nEND\n',
             'P NotEpub\nC -\nEND\n','P Ok\nC GG\nEND\n','P Ok\nC ~\nEND\n',
             'P Ok\nC -\nEND\nEND\n','R 0 1 2\nP NotEpub\nEND\n',
             'P Ok\nC -\nP Ok\nEND\n','P Ok\nC -\nEND\ntrailing\n',
             'P Ok\nR 0 1 1\nC -\nEND\n','P Ok\nC -\n\nEND\n']
    for text in invalid:
        try:
            parse(text,wire)
        except ValueError:
            continue
        raise RuntimeError('accepted invalid response '+repr(text))
if __name__=='__main__':
    controls()
