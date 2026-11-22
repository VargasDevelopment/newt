"""Bounded N1 policy descriptors; syntax/state validation precedes comparison."""
import re
UINT=re.compile(r'0|[1-9][0-9]*\Z')
HEX=re.compile(r'(?:[0-9a-f]{2})+\Z')
MAX32=2**32-1
MAX64=2**64-1
RESERVED={b'META-INF/container.xml',b'OPS/meta.opf',b'OPS/nav.xhtml'}

def number(s,maximum):
    if not UINT.fullmatch(s) or int(s)>maximum:raise ValueError('noncanonical uint')
    return int(s)

def data(s,limit=1024):
    if s=='-':return b''
    if not HEX.fullmatch(s) or len(s)>limit*2:raise ValueError('hex')
    raw=bytes.fromhex(s)
    if b'\0' in raw:raise ValueError('C-string extent')
    return raw

def wire(case):
    h=['N1',str(len(case['spines'])),str(len(case['members'])),str(len(case['toc'])),str(case.get('selected',1)),str(case.get('required',65535)),str(case.get('page',10)),str(case.get('focus',MAX32)),str(len(case.get('anchors',[]))),'0']
    def field(v):return 'n' if v is None else v.hex() or '-'
    rows=[' '.join(h)]
    rows+=['S '+field(href)+' '+field(media) for href,media in case['spines']]
    rows+=['Z '+field(name)+' '+str(method)+' '+str(size) for name,method,size in case['members']]
    rows+=['T '+' '.join([field(title),field(href),field(fragment),str(depth)]) for title,href,fragment,depth in case['toc']]
    rows+=['A '+str(hashed)+' '+str(start) for hashed,start in case.get('anchors',[])]
    raw='\n'.join(rows+['END'])+'\n';request(raw);return raw

def request(raw):
    if not isinstance(raw,str) or len(raw.encode())>2097152 or not raw.endswith('\n') or '\r' in raw:raise ValueError('framing')
    rows=[r.split(' ') for r in raw[:-1].split('\n')];h=rows[0]
    if len(h)!=10 or h[0]!='N1':raise ValueError('header')
    ns,nz,nt=[number(v,128) for v in h[1:4]];number(h[4],MAX32);number(h[5],65535);number(h[6],MAX32);number(h[7],MAX32);na=number(h[8],256)
    if h[9]!='0' or (ns==0 and (nz or nt)) or len(rows)!=2+ns+nz+nt+na or rows[-1]!=['END']:raise ValueError('counts')
    pos=1
    for _ in range(ns):
        r=rows[pos];pos+=1
        if len(r)!=3 or r[0]!='S':raise ValueError('spine row')
        for v in r[1:]:
            if v!='n':data(v)
        if r[1]!='n' and data(r[1]) in RESERVED:raise ValueError('fixture scaffold collision')
    for _ in range(nz):
        r=rows[pos];pos+=1
        if len(r)!=4 or r[0]!='Z' or not data(r[1]) or data(r[1]) in RESERVED:raise ValueError('member row')
        number(r[2],65535);number(r[3],MAX32)
    for _ in range(nt):
        r=rows[pos];pos+=1
        if len(r)!=5 or r[0]!='T':raise ValueError('toc row')
        for v in r[1:4]:data(v)
        number(r[4],255)
    for _ in range(na):
        r=rows[pos];pos+=1
        if len(r)!=3 or r[0]!='A':raise ValueError('anchor row')
        number(r[1],MAX32);number(r[2],MAX32)
    return rows

def response(raw,case):
    request(wire(case))
    if not isinstance(raw,str) or not raw.endswith('\n') or '\r' in raw:raise ValueError('response framing')
    rows=[r.split(' ') for r in raw[:-1].split('\n')]
    at=0
    def take(tag,width):
        nonlocal at
        if at>=len(rows) or rows[at][0]!=tag or len(rows[at])!=width:raise ValueError('row/state '+tag)
        row=rows[at];at+=1;return row
    b=take('B',3);bound=number(b[1],1);number(b[2],511)
    p=take('P',7);weights=[number(v,MAX64) for v in p[1:4]];index=number(p[4],MAX32);count=number(p[5],128);valid=number(p[6],1)
    if valid and (not 0<weights[2]<=67108864 or weights[0]+weights[1]>weights[2]):raise ValueError('weight state')
    if not valid and weights!=[0,0,0]:raise ValueError('unpublished weights')
    if bound and (count!=len(case['spines']) or index!=case.get('selected',1)):raise ValueError('bound spine')
    if not bound and (count or index or valid):raise ValueError('failed bind state')
    g=take('G',3);number(g[1],511);targets=number(g[2],12)
    def section(row):
        number(row[1],65535);number(row[2],MAX32);number(row[3],MAX32);number(row[4],255)
        flags=[number(v,1) for v in row[5:9]]
        if flags[3] and not flags[2]:raise ValueError('anchor without target')
        data(row[9],95);data(row[10],639);data(row[11],639)
    for _ in range(targets):section(take('T',12))
    section(take('S',12))
    m=take('M',9);menu_bound=number(m[1],1);total=number(m[2],257);focus=number(m[3],256);first=number(m[4],256);visible=number(m[5],5)
    for v in m[6:]:number(v,1)
    if int(m[8])!=(1 if total else 0) or bool(menu_bound)!=bool(total) or (total and (focus>=total or first+visible>total)) or (not total and (focus or first or visible)):raise ValueError('menu state')
    for i in range(first,first+visible):
        row=take('R',9)
        if number(row[1],256)!=i or number(row[7],1)!=1:raise ValueError('visible row')
        number(row[2],MAX32);number(row[3],65535)
        for v in row[4:7]:number(v,1)
        data(row[8],95)
    u=take('U',3);number(u[1],1);number(u[2],256)
    take('END',1)
    if at!=len(rows):raise ValueError('extra output')
    return rows
