"""Strict complete chapter traces with declared C++/Rust field domains."""
import re
HEX=re.compile(r'(?:-|(?:[0-9a-f]{2})+)\Z');UINT=re.compile(r'(?:0|[1-9][0-9]*)\Z');INT=re.compile(r'(?:0|-?[1-9][0-9]*)\Z')
STATUS={'Ok','IoError','Unsupported','OutOfMemory','ParseError','Truncated','NotZip'}
STATE={'Idle','Parsing','Done','Failed'}
U8=(0,255);U16=(0,65535);U32=(0,4294967295);U64=(0,18446744073709551615);I16=(-32768,32767);BOOL=(0,1)
FONT={'FA':[U32,U16,U8,I16],'FH':[U16,I16],'FB':[U16,I16],'FC':[U32,BOOL],'FK':[U32,U32,U16,U8,I16],'FL':[U32,U32,U8,U32]}
def integer(s,domain=U64):
    low,high=domain
    if not (INT if low<0 else UINT).fullmatch(s):raise ValueError('integer syntax')
    n=int(s)
    if not low<=n<=high:raise ValueError('integer domain')
    return n
def fields(line,n):
    p=line.split(' ')
    if len(p)!=n or any(not x for x in p):raise ValueError('arity')
    return p
def values(words,domains):return tuple(integer(w,d) for w,d in zip(words,domains))
def parse(raw):
    if not raw.isascii() or not raw.endswith('\n') or '\r' in raw or '\0' in raw:raise ValueError('LF protocol')
    pages=[];font=[];reads=[];anchors=[];steps=[];opened=None;ended=False;polls=[]
    lines=raw[:-1].split('\n');i=0
    while i<len(lines):
        line=lines[i];p=line.split(' ');i+=1;t=p[0]
        if ended:raise ValueError('trailing')
        if t=='END':
            if p!=['END'] or opened is None:raise ValueError('end')
            ended=True
        elif t=='Q':
            fields(line,3);n,b=values(p[1:],[U64,BOOL])
            if n!=len(polls):raise ValueError('poll order')
            polls.append((n,b))
        elif t in ('R','B'):
            fields(line,4);at,cap,n=values(p[1:],[U64,U32,(-1,2147483647)])
            if n>cap:raise ValueError('source result exceeds request')
            reads.append((at,cap,n))
        elif t in FONT:
            fields(line,len(FONT[t])+1);font.append((t,*values(p[1:],FONT[t])))
        elif t=='A':
            fields(line,3);anchors.append(values(p[1:],[U32,U32]))
        elif t=='O':
            fields(line,4)
            if opened is not None or p[1] not in STATUS or p[3] not in STATE:raise ValueError('open')
            opened=(p[1],integer(p[2]),p[3])
        elif t=='S':
            fields(line,6)
            if opened is None or p[1] not in STATUS or p[3] not in STATE:raise ValueError('step')
            steps.append((p[1],integer(p[2]),p[3],integer(p[4],U32),integer(p[5],U32)))
        elif t=='P':
            fields(line,6);header=values(p[1:],[U32,U32,U16,U16,U16]);idx,chars,nr,ni,nl=header
            if idx!=len(pages):raise ValueError('page order')
            payload=[]
            for tag,count,nhex,domains in [('T',nr,1,[I16,I16,U16,U8]),('I',ni,1,[I16,I16,U16,U16]),('K',nl,2,[I16,I16,U16,U16])]:
                for _ in range(count):
                    if i>=len(lines):raise ValueError('truncated page')
                    q=fields(lines[i],1+nhex+len(domains));i+=1
                    if q[0]!=tag or any(not HEX.fullmatch(x) or (x!='-' and len(x)//2>65535) for x in q[1:1+nhex]):raise ValueError('page record')
                    payload.append((tag,*q[1:1+nhex],*values(q[1+nhex:],domains)))
            pages.append((header,payload))
        else:raise ValueError('opcode')
    if not ended or opened is None:raise ValueError('incomplete')
    return dict(open=opened,pages=pages,font=font,reads=reads,anchors=anchors,steps=steps,polls=polls,rows=tuple(lines))
def parse_reply(raw,wire):
    result=parse(raw);request=wire.rstrip('\n').split(' ')
    if len(result['steps'])!=(1 if request[0]=='T' else int(request[12])):raise ValueError('step cardinality')
    if request[0]!='X' and result['polls']:raise ValueError('ordinary policy polled')
    if [r[0] for r in result['rows'] if r.startswith(('O ','S '))]!=['O']+['S']*len(result['steps']):raise ValueError('result ordering')
    if result['rows'][-2].split(' ')[0] not in ('O','S'):raise ValueError('operations after result')
    return result
def malformed_controls():
    wire='L 61 80 32 8 left 0 4294967295 4294967295 0 4096 -1 1\n'
    raw='O Ok 0 Parsing\nFC 65 1\nP 0 0 1 0 0\nT 61 0 8 8 0\nS Ok 1 Done 1 1\nEND\n';parse_reply(raw,wire)
    for old,new in [('FC 65 1','FC 65 2'),('T 61 0 8 8 0','T 61 0 8 -1 0'),('T 61 0 8 8 0','T 61 32768 8 8 0'),('T 61 0 8 8 0','T 61 0 8 8 256'),('FC 65 1','FC 4294967296 1'),('P 0 0 1 0 0','P 0 0 65536 0 0'),('O Ok 0','O Ok 18446744073709551616')]:
        try:parse_reply(raw.replace(old,new),wire)
        except ValueError:pass
        else:raise AssertionError('malformed field domain admitted')
