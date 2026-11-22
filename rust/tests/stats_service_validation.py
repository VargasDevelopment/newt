"""Complete SS1 grammar validation before any semantic/mutation comparison."""
import re
class ProtocolError(ValueError):
    pass
_TOKEN=re.compile(r'[A-Za-z0-9_.-]+\Z')
def token(value):
    if not _TOKEN.fullmatch(value):raise ProtocolError('invalid token')
def number(value,maximum=(1<<64)-1):
    if not re.fullmatch(r'0|[1-9][0-9]*',value) or int(value)>maximum:raise ProtocolError('invalid number')
def hexbytes(value):
    if value=='-':return b''
    if not re.fullmatch(r'(?:[0-9a-f]{2})+',value):raise ProtocolError('invalid hex')
    return bytes.fromhex(value)
def trace(value):
    if not value:return
    for event in value.split('|'):
        f=event.split(':')
        if event=='end':continue
        if len(f)==3 and f[0]=='begin' and f[2] in ('0','1'):token(f[1])
        elif len(f)==2 and f[0] in ('read','list','aside'):token(f[1])
        elif len(f)==3 and f[0] in ('put','repair'):token(f[1]);hexbytes(f[2])
        else:raise ProtocolError('invalid trace event')
def row(line,tag):
    f=line.split(' ')
    if any(not word for word in f) or f[0]!=tag:raise ProtocolError('noncanonical row')
    if tag=='R':
        if len(f)!=7 or f[1] not in ('seed','fold','handover','mark') or f[2] not in ('done','unseeded','unsupported','unavailable','waiting','cancelled','unconfirmed'):raise ProtocolError('invalid report')
        for value,limit in zip(f[3:],(2,(1<<32)-1,(1<<32)-1,(1<<64)-1)):number(value,limit)
    elif tag in ('S','B','E'):
        limits={'S':[(1<<32)-1]*3+[65535,(1<<32)-1,65535,65535,1,1,(1<<32)-1], 'B':[65535]*6,'E':[1,4,1,255,65535]}[tag]
        if len(f)!=len(limits)+1:raise ProtocolError('wrong returned-output arity')
        for value,limit in zip(f[1:],limits):number(value,limit)
    elif tag=='U':
        if len(f)!=4:raise ProtocolError('invalid unsaved receipt')
        number(f[1],(1<<32)-1)
        if f[1]=='0':
            if f[2:]!=['-','-']:raise ProtocolError('unused callback has a receipt')
        else:
            number(f[2]);number(f[3],(1<<32)-1)
    else:raise ProtocolError('unexpected tag')
def command_fields(command):
    f=command.split()
    if len(f)<2 or f[0]!='SS1':raise ProtocolError('invalid fixture command')
    op=f[1]
    if op in ('cold','clear','dump'):
        if len(f)!=2:raise ProtocolError('wrong command arity')
    elif op=='put':
        if len(f)!=5:raise ProtocolError('wrong injection arity')
        token(f[2]);token(f[3]);hexbytes(f[4])
    elif op=='sha':
        import hashlib
        if len(f)!=4:raise ProtocolError('wrong SHA arity')
        name=hexbytes(f[2]);digest=hexbytes(f[3])
        if digest!=hashlib.sha256(name).digest():raise ProtocolError('non-native fixture SHA')
    elif op=='fault':
        if len(f)!=8:raise ProtocolError('wrong fault arity')
        for value in f[2:]:number(value)
    elif op in ('nsfault','open'):
        if len(f)!=4:raise ProtocolError('wrong open/fault arity')
        token(f[2]);number(f[3],3 if op=='open' else (1<<64)-1)
    elif op=='run':
        if len(f)<12 or f[2] not in ('seed','seed-none','fold','mark','marked','refresh','summary','facts','sleep'):raise ProtocolError('unknown operation')
        if len(hexbytes(f[3]))!=32:raise ProtocolError('invalid digest')
        for value,limit in zip(f[4:12],((1<<64)-1,(1<<32)-1,(1<<32)-1,65535,255,1,65535,(1<<64)-1)):number(value,limit)
        sizes={'mark':(13,), 'marked':(17,), 'sleep':(12,13,20)}
        if len(f) not in sizes.get(f[2],(12,)):raise ProtocolError('wrong operation arity')
        limits={'mark':(1,), 'marked':(65535,2,(1<<64)-1,(1<<32)-1,(1<<32)-1), 'sleep':((1<<32)-1,1,1,1,4,1,255,65535)}
        for value,limit in zip(f[12:],limits.get(f[2],())):number(value,limit)
    else:raise ProtocolError('unknown fixture command')
    return f
def validate(transcript,commands):
    if not transcript.endswith("\n") or "\r" in transcript:raise ProtocolError("response must use LF framing")
    lines=transcript[:-1].split('\n');at=0
    def take():
        nonlocal at
        if at>=len(lines):raise ProtocolError('missing output row')
        value=lines[at];at+=1;return value
    for command in commands:
        f=command_fields(command)
        if f[1]=='run':
            if f[2] not in ('seed','seed-none','fold','mark','marked','refresh','summary','facts','sleep'):raise ProtocolError('unknown operation')
            for tag in {'summary':('S',),'facts':('B',),'sleep':('E','U')}.get(f[2],()):row(take(),tag)
            report=take();row(report,'R')
            allowed={'seed':('seed',),'seed-none':('seed',),'mark':('mark',),'fold':('fold','handover'),'marked':('fold','handover')}
            if report.split()[1] not in allowed.get(f[2],('fold',)):raise ProtocolError('wrong report kind')
        elif f[1]=='dump':
            previous={'V':None,'D':None};durable=False
            while True:
                line=take()
                if line.startswith('T '):trace(line[2:]);break
                words=line.split(' ')
                if len(words)!=4 or words[0] not in ('V','D'):raise ProtocolError('invalid state row')
                tag,name,key,data=words;token(name);token(key);hexbytes(data)
                if durable and tag=='V':raise ProtocolError('visible rows after durable rows')
                durable|=tag=='D';identity=(name,key)
                if previous[tag] is not None and identity<=previous[tag]:raise ProtocolError('noncanonical record map')
                previous[tag]=identity
        elif f[1] not in ('put','sha','fault','nsfault','open','cold','clear'):raise ProtocolError('unknown fixture command')
    if at!=len(lines):raise ProtocolError('extra output rows')

def validate_wire(reply, command):
    if not re.fullmatch(r'(?:[0-9a-f]{2})+',reply):raise ProtocolError('noncanonical wrapped reply')
    fields=command.split(' ')
    if len(fields)!=2 or fields[0]!='stats_service':raise ProtocolError('invalid wrapper command')
    script=bytes.fromhex(fields[1]).decode('utf-8')
    response=bytes.fromhex(reply).decode('utf-8')
    if not script.endswith('\n') or '\r' in script:raise ProtocolError('invalid script framing')
    validate(response,script[:-1].split('\n'))
    return response
