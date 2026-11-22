"""Canonical response admission, independent of semantic expected-output comparison."""
import re

INTEGER = re.compile(r'(?:0|-[1-9][0-9]*|[1-9][0-9]*)\Z')

def number(word, low, high):
    if not INTEGER.fullmatch(word):
        raise ValueError('noncanonical reply integer')
    value = int(word)
    if not low <= value <= high:
        raise ValueError('reply integer domain')
    return value

def request_shape(request):
    t = iter(request.split())
    if next(t) != 'PR1':
        raise ValueError('request version')
    mode,w,h,stride,fmt,rot,fill,arena = [int(next(t)) for _ in range(8)]
    nf = int(next(t))
    for _ in range(nf):
        for _ in range(12): next(t)
    ne = int(next(t))
    selections = {-1}
    for _ in range(ne):
        bank, flags = int(next(t)), int(next(t))
        selections.add(bank*256+(flags & 3))
    nr = int(next(t)); text_bytes = 0
    for _ in range(nr):
        for _ in range(4): next(t)
        text = next(t); text_bytes += 0 if text == '-' else len(text)//2
    ni = int(next(t))
    for _ in range(ni):
        for _ in range(6): next(t)
        for _ in range(int(next(t))):
            for _ in range(3): next(t)
    queries = []
    for _ in range(int(next(t))):
        queries.append(int(next(t)))
        for _ in range(4): next(t)
    if next(t, None) is not None:
        raise ValueError('request trailing token')
    return h*stride, nf, ni, text_bytes, selections, queries

def validate(raw, request):
    frame_len,nf,ni,text_bytes,selections,queries = request_shape(request)
    if not raw.isascii() or '\r' in raw or not raw.endswith('\n'):
        raise ValueError('noncanonical LF reply')
    lines = raw.split('\n')
    if len(lines) != 4 or lines[-1] != '':
        raise ValueError('reply line count')
    fields = lines[0].split(' ')
    if len(fields) != 5 or fields[0] != 'R':
        raise ValueError('result grammar')
    number(fields[1], 0, text_bytes)
    number(fields[2], 0, 2097151)
    number(fields[3], 0, 10)
    if len(fields[4]) != frame_len*2 or not re.fullmatch('[0-9a-f]+',fields[4]):
        raise ValueError('complete canonical frame required')
    if not lines[1].startswith('T ') or not lines[2].startswith('Q '):
        raise ValueError('reply prefixes')
    # Each signature lists every integer field, including the event suffix ID.
    cp, size, style, bank = (0,2097151),(0,64),(0,255),(0,nf-1)
    image, row, width = (0,ni-1),(0,65535),(0,64)
    signatures = {'h':[cp,bank], 'r':[cp,size,bank], 'a':[cp,size,style,bank],
                  'k':[cp,cp,size,style,bank], 'g':[cp,cp,style,bank],
                  'l':[size,bank], 's':[size,bank], 'd':[image,(0,4096)],
                  'i':[image,row,width], 'e':[image,(0,10)]}
    trace = lines[1][2:]
    if trace != '-':
        for event in trace.split(','):
            signature = signatures.get(event[:1])
            if signature is None:
                raise ValueError('unknown trace event')
            values = event[1:].split(':')
            if len(values) != len(signature):
                raise ValueError('trace arity')
            for value, (low,high) in zip(values,signature): number(value,low,high)
    answers = [] if lines[2][2:] == '-' else lines[2][2:].split(' ')
    if len(answers) != len(queries):
        raise ValueError('query answer cardinality')
    domains = {0:(-32768,32767),1:(-32768,32767),2:(0,2097151),3:(0,1),
               4:(-32768,32767),5:(-32768,32767),6:(-1,1795),7:(0,7)}
    for answer, op in zip(answers,queries):
        value = number(answer,*domains[op])
        if op == 6 and value not in selections:
            raise ValueError('selection domain')
    return raw

def malformed_fixtures(cases):
    """Root runs these before any oracle/mutant semantic comparison."""
    plain = next(c for c in cases if c['name']=='rotation-0')
    queried = next(c for c in cases if c['name']=='empty-chain-metrics')
    strict = next(c for c in cases if c['name']=='strict-covers')
    r = plain['expected']; q = queried['expected']
    fixtures = [(plain,r[:-1]),(plain,r+'\n'),(plain,r.replace('\n','\r\n')),
                (plain,r.replace('T h','T z',1)),(plain,r.replace('h65:0','h65',1)),
                (plain,r.replace('h65:0','h65:1',1)),(plain,r.replace('h65:0','h065:0',1)),
                (plain,r.replace('r65:16:0','r65:65:0',1)),
                (plain,r.replace('Q -','Q 0')),(queried,q.replace('Q 16 16 0 -1','Q -')),
                (queried,q.replace('Q 16 16 0 -1','Q 16 16 0')),
                (queried,q.replace('Q 16 16 0 -1','Q 16 16 0 -1 0')),
                (queried,q.replace('Q 16','Q 32768',1)),
                (strict,strict['expected'].replace('Q 0','Q 2')),
                (plain,r.replace('R 0 999 0','R 0 999 11')),
                (plain,r.replace('T ','T d0:128,',1)),
                (plain,r.replace('T ','T h65:0,,',1)),
                (plain,r.replace('Q -','Q -0'))]
    for case, raw in fixtures:
        try: validate(raw,case['input'])
        except ValueError: continue
        raise ValueError('malformed response admitted before semantic comparison')
    return len(fixtures)
