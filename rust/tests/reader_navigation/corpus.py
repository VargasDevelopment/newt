"""Synthetic metadata only. Independent fixed expectations plus differential stress."""
from copy import deepcopy
import random
MEDIA=b'application/xhtml+xml'
MAX32=2**32-1

def hashed(s):
    h=2166136261
    for b in s:h=((h^b)*16777619)&MAX32
    return h

def base():
    return dict(spines=[(b'front',MEDIA),(b'one',MEDIA),(b'two',MEDIA)],
        members=[(b'front',0,10),(b'one',8,30),(b'two',0,60)],
        toc=[(b'Part',b'one',b'',0),(b'First',b'one',b'a',1),(b'Next',b'one',b'b',1)],
        anchors=[(hashed(b'a'),0),(hashed(b'b'),10)],selected=1,page=10)

BASE_EXPECTED='''B 1 4
P 10 30 100 1 3 1
G 0 3
T 0 1 0 0 0 0 1 1 50617274 6f6e65 -
T 1 1 0 1 0 0 1 1 4669727374 6f6e65 61
T 2 1 10 1 0 0 1 1 4e657874 6f6e65 62
S 2 1 10 1 0 0 1 1 4e657874 6f6e65 62
M 1 4 3 0 4 1 1 1
R 0 0 65535 1 0 0 1 53656374696f6e2031
R 1 1 0 0 0 0 1 50617274
R 2 1 1 0 1 0 1 4669727374
R 3 1 2 0 1 1 1 4e657874
U 1 0
END
'''

def cases():
    result=[]
    def add(name,q):result.append((name,q))
    add('independent-resolved',base())
    for selected in (0,1,2,3,MAX32):
        q=base();q['selected']=selected;add('selected-'+str(selected),q)
    for focus in (0,1,2,MAX32):
        q=base();q['focus']=focus;add('focus-'+str(focus),q)
    for page in (0,9,10,11,MAX32):
        q=base();q['page']=page;add('page-'+str(page),q)
    for observations in ([],[(hashed(b'a'),0)],[(hashed(b'a'),0),(hashed(b'b'),10),(hashed(b'b'),10)],[(hashed(b'a'),0),(hashed(b'b'),10),(hashed(b'b'),11)],[(hashed(b'a'),0),(hashed(b'b'),0)]):
        q=base();q['anchors']=observations;add('observations-'+str(len(result)),q)
    for change in ('duplicate-spine','missing-text','resource','no-toc','unordered','declared-conflict','long-invalid-title','short-invalid-title','no-complete-title','too-large','bad-method','empty-href','null-href','null-media','zero-weight'):
        q=base()
        if change=='duplicate-spine':q['spines'].append((b'one',MEDIA))
        elif change=='missing-text':q['members'].pop()
        elif change=='resource':q['spines'].append((b'art',b'image/png'))
        elif change=='no-toc':q['toc']=[]
        elif change=='unordered':q['toc'].append((b'Earlier',b'front',b'',0));q['selected']=2
        elif change=='declared-conflict':q['toc'].append((b'Conflict',b'one',b'b',1))
        elif change=='long-invalid-title':q['toc']=[(b'a'*94+b'\xe7\x81',b'one',b'',0)]
        elif change=='short-invalid-title':q['toc']=[(b'a\xe7\x81',b'one',b'',0)]
        elif change=='no-complete-title':q['toc']=[(b'\xe7\x81',b'one',b'',0)]
        elif change=='too-large':q['members'][1]=(b'one',0,4194305)
        elif change=='bad-method':q['members'][1]=(b'one',9,30)
        elif change=='empty-href':q['spines'][1]=(b'',MEDIA)
        elif change=='null-href':q['spines'][1]=(None,MEDIA)
        elif change=='null-media':q['spines'][1]=(b'one',None)
        elif change=='zero-weight':q['members']=[(name,method,0) for name,method,_ in q['members']]
        add(change,q)
    for required in (65535,0,10,11,12):
        q=base();q['toc']=[(b'same',b'one',b'a',0)]*13;q['required']=required;add('record-pressure-'+str(required),q)
    q=base();q['toc']=[(bytes([65+i])*90,b'one',b'',0) for i in range(8)];add('string-pressure',q)
    q=base();q['toc']=[(b'part'+str(i).encode(),b'one',str(i).encode(),0) for i in range(20)]
    for focus in (0,3,4,7,19,MAX32):
        r=deepcopy(q);r['focus']=focus;add('menu-window-'+str(focus),r)
    # A tiny independent full collision: deterministic search, not book identity.
    rng=random.Random(1979);seen={};pair=None
    for _ in range(300000):
        s=('anchor-%016x'%rng.getrandbits(64)).encode();h=hashed(s)
        if h in seen and seen[h]!=s:pair=(seen[h],s,h);break
        seen[h]=s
    if pair is None:raise RuntimeError('authored hash collision unavailable')
    q=base();q['toc']=[(b'One',b'one',pair[0],0),(b'Two',b'one',pair[1],0)];q['anchors']=[(pair[2],0)];add('full-id-collision',q)
    add('empty-book',dict(spines=[],members=[],toc=[],selected=0,anchors=[]))
    rng=random.Random(72)
    for i in range(60):
        q=base();q['toc']=[];q['anchors']=[]
        for j in range(rng.randrange(0,17)):
            href=rng.choice([b'front',b'one',b'two',b'absent']);frag=rng.choice([b'',b'a',b'b']);title=rng.choice([b'A',b'B',b'\xe7\x81',b'Z'*100]);depth=rng.randrange(3)
            q['toc'].append((title,href,frag,depth))
        q['selected']=rng.randrange(4);q['page']=rng.randrange(20);q['focus']=rng.choice([0,3,8,MAX32]);q['anchors']=[(hashed(b'a'),rng.randrange(10)),(hashed(b'b'),rng.randrange(10))]
        add('generated-'+str(i),q)
    # A cached page has no observed anchor identity; preserve its generic row.
    q=base();q['selected']=0;q['page']=0;q['anchors']=[]
    q['toc']=[(b'First',b'front',b'a',0),(b'Second',b'two',b'a',0)]
    add('independent-cached-generic',q)
    return result
