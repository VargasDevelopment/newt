"""Validate both replies independently before comparing any semantics."""
import re


def number(text, maximum=2**32-1, negative=False):
    pattern = r'-?(0|[1-9][0-9]*)' if negative else r'0|[1-9][0-9]*'
    if not re.fullmatch(pattern, text):
        raise ValueError('noncanonical integer')
    value = int(text)
    if value > maximum or value < (-1 if negative else 0) or text == '-0':
        raise ValueError('integer range')
    return value


def reply(text, request):
    if not text.endswith('\n') or text.count('\n') != 1 or '\r' in text:
        raise ValueError('response framing')
    words = text[:-1].split(' ')
    if any(not w for w in words) or len(words)<5 or words[0]!='PN1':
        raise ValueError('response tokens')
    status, nr, ne = (number(w) for w in words[1:4])
    if status not in (0,1,6,7,8) or not re.fullmatch('[0-9a-f]{6}',words[4]):
        raise ValueError('status/background')
    if len(words)!=5+nr+ne:
        raise ValueError('response count')
    req = request.rstrip('\n').split(' ')
    mode,bits,gamma,sw,sh,dw,dh,stop,step,fail = map(int,req[1:11])
    size=len(req[11])//2
    source_pos=0
    feed_end=min(step,size)
    for index,item in enumerate(words[5:5+nr]):
        fields=item.split(':')
        if len(fields)!=4 or fields[0] != ('F' if mode==0 else 'R'):
            raise ValueError('read/feed event')
        pos,want,result = number(fields[1]),number(fields[2]),number(fields[3],negative=True)
        if pos!=source_pos or want>size-pos or result>want:
            raise ValueError('read/feed bounds/order')
        if mode==1:
            expected=-1 if pos>=fail else min(want,step)
            if want!=min(4096,size-pos) or result!=expected:
                raise ValueError('read contract')
        elif want!=feed_end-source_pos:
            raise ValueError('feed schedule')
        if result<0:
            if index!=nr-1 or status!=(8 if mode==0 else 1):
                raise ValueError('error status/order')
        if result>0:
            source_pos+=result
        if mode==0 and result>=0 and (source_pos==feed_end or result==0):
            if feed_end==size:
                if index!=nr-1:
                    raise ValueError('continued after final feed')
            else:
                feed_end=min(feed_end+step,size)
    init=done=rows=0
    dimensions=None
    for item in words[5+nr:]:
        fields=item.split(':')
        if fields[0]=='I' and len(fields)==4 and mode==0:
            if init or done:
                raise ValueError('init order')
            dimensions=(number(fields[1]),number(fields[2]))
            number(fields[3],255);init+=1
        elif fields[0]=='D' and len(fields)==1 and mode==0:
            if init!=1 or done:
                raise ValueError('done order')
            done+=1
        elif fields[0]=='P' and len(fields)==6 and mode==0:
            if init!=1 or done:
                raise ValueError('pixel order')
            x,y,w,h=map(number,fields[1:5])
            if not re.fullmatch('[0-9a-f]{8}',fields[5]) or not w or not h or x+w>dimensions[0] or y+h>dimensions[1]:
                raise ValueError('pixel bounds/rgba')
        elif fields[0]=='O' and len(fields)==4 and mode==1:
            y,w=number(fields[1]),number(fields[2])
            if y!=rows or y>=dh or w!=dw or len(fields[3])!=2*w or not re.fullmatch('[0-9a-f]+',fields[3]):
                raise ValueError('row order/width/pixels')
            rows+=1
            if stop and rows>stop:
                raise ValueError('callback after stop')
        else:
            raise ValueError('unknown callback')
    return words
