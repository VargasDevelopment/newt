"""Strict complete single-response LF protocol, including order and counts."""
import re
N=r'(?:0|[1-9][0-9]*)'
H=r'(?:-|(?:[0-9a-f]{2})+)'
Z=rf'(?:~|{H})'
STATUS='Ok|IoError|NotEpub|NotZip|Truncated|Unsupported|OutOfMemory|ParseError|MetadataTooLarge|PathTooLong'
def parse(text, wire):
    if not text or not text.endswith('\n') or '\r' in text:
        raise ValueError('incomplete LF')
    rows=text[:-1].split('\n'); at=0
    while at<len(rows) and rows[at].startswith('R '):
        if not re.fullmatch(rf'R {N} {N} (?:-1|{N})',rows[at]):
            raise ValueError('read grammar')
        _,off,cap,n=rows[at].split()
        if int(n)>int(cap):
            raise ValueError('read count')
        at+=1
    if at>=len(rows) or not re.fullmatch(rf'P (?:{STATUS})',rows[at]):
        raise ValueError('status')
    ok=rows[at]=='P Ok'; at+=1
    body=rows[at:-1]
    if rows[-1]!='END' or rows.count('END')!=1:
        raise ValueError('envelope')
    kind=wire.split(' ')[1]
    if not ok:
        if body:
            raise ValueError('failure publication')
    elif kind=='C':
        if len(body)!=1 or not re.fullmatch(rf'C {H}',body[0]):
            raise ValueError('container')
    elif kind=='P':
        if not body or not re.fullmatch(rf'M {H} {H} {H} {H}',body[0]):
            raise ValueError('metadata')
        i=1; manifests=0; spines=0
        while i<len(body) and body[i].startswith('I '):
            if not re.fullmatch(rf'I {H} {H} {H} {N} [01] [01]',body[i]) or int(body[i].split()[4])>2**32-1:
                raise ValueError('manifest')
            i+=1; manifests+=1
        while i<len(body) and body[i].startswith('S '):
            if not re.fullmatch(rf'S {N}',body[i]) or int(body[i].split()[1])>=min(manifests,65536):
                raise ValueError('spine')
            i+=1; spines+=1
        if i!=len(body)-1 or not re.fullmatch(rf'D (?:-1|{N}) (?:-1|{N})',body[i]) or not manifests or not spines:
            raise ValueError('package envelope')
        if any(int(v)>=manifests for v in body[i].split()[1:]):
            raise ValueError('selection index')
    elif kind in ('N','X'):
        for row in body:
            if not re.fullmatch(rf'T {H} {H} {Z} {N}',row) or int(row.split()[-1])>254:
                raise ValueError('toc')
    else:
        raise ValueError('request kind')
    return tuple(rows)
