"""Validate canonical replies before equivalence is considered."""
import re

def reply(raw,request):
    assert raw.endswith('\n') and raw.count('\n')==1
    t=raw[:-1].split(' ');assert len(t)>=3 and t[0]=='JP1'
    for s in t[1:3]:assert re.fullmatch(r'0|[1-9][0-9]*',s)
    assert int(t[1])<=10 and len(t)==3+int(t[2])
    mode=request.split(' ')[1]
    row_index=0
    fields=request.split(' ');width=int(fields[2]);height=int(fields[3])
    for e in t[3:]:
        p=e.split(':')
        if mode=='I':
            assert len(p)==4 and p[0]=='I' and int(t[2])==1
            assert all(re.fullmatch(r'0|[1-9][0-9]*',v) for v in p[1:])
            assert int(p[1])<=65535 and int(p[2])<=65535 and p[3] in ('0','1')
            continue
        assert p[0]==('B' if mode=='D' else 'O')
        if mode=='D':
            assert len(p)==6
            for n in p[1:5]:assert re.fullmatch(r'0|[1-9][0-9]*',n)
            x,y,r,b=map(int,p[1:5]);assert 0<=x<=r<65535 and 0<=y<=b<65535
            assert len(p[5])==2*(r-x+1)*(b-y+1)
        else:
            assert len(p)==3 and re.fullmatch(r'0|[1-9][0-9]*',p[1])
            assert int(p[1])==row_index and row_index<height
            assert 0<len(p[2])<=2*width
            if mode=='P':assert len(p[2])==2*width
            row_index+=1
        assert re.fullmatch(r'(?:[0-9a-f]{2})+',p[-1])
