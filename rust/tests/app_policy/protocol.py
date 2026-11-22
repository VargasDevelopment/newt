"""Admit complete, command-specific output before any semantic comparison."""
import re
def number(word, maximum):
    if re.fullmatch(r'0|[1-9][0-9]*',word) is None or int(word)>maximum:
        raise ValueError('invalid integer field')
def reply(raw, commands):
    if not raw.endswith('\n') or '\r' in raw:raise ValueError('framing')
    rows=raw[:-1].split('\n')
    if len(rows)!=len(commands)+2 or rows[0]!='AP1' or rows[-1]!='END':raise ValueError('cardinality/envelope')
    for row,command in zip(rows[1:-1],commands):
        t=row.split(' ');op=command.split(' ')[0]
        if not all(t)or t[0]!=op:raise ValueError('opcode/spaces')
        if op in ('D','W'):
            start=2 if op=='D' else 1
            if len(t)!=start+13:raise ValueError('state shape')
            if op=='D':number(t[1],1)
            for word,maximum in zip(t[start:],[7,7,2**32-1,2**32-1,255,4,1,1,9,1,255,10,7]):number(word,maximum)
        elif op=='H':
            if len(t)!=4 or t[2]not in ('Books','Apps','Settings'):raise ValueError('home shape')
            number(t[1],2);number(t[3],6)
        elif op=='G':
            if len(t)!=5:raise ValueError('graph shape')
            for word,maximum in zip(t[1:],[1,255,2,63]):number(word,maximum)
        else:raise ValueError('unknown reply')
    return rows[1:-1]
