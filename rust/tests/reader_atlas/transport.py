"""Strict language-neutral host wire; every field and frame byte is admitted."""
import hashlib
import re

MAX_INPUT = 1048576
FRAME_BYTES = 52272
BIT_COUNTS = bytes(value.bit_count() for value in range(256))

def encode(request):
    if len(request)!=12 or not all(type(v) is int for v in request):
        raise ValueError('request schema')
    limits=[(0,2**32-1)]*2+[(0,65535),(0,255)]+[(-2**31,2**31-1)]*6+[(0,52273),(0,255)]
    if any(not lo<=n<=hi for n,(lo,hi) in zip(request,limits)):
        raise ValueError('request bounds')
    return 'A1 '+' '.join(map(str,request))+'\n'

def decode(raw,cases):
    # Run returns text decoded without universal-newline conversion.
    if not isinstance(raw,str) or '\r' in raw or not raw.endswith('\n'):
        raise ValueError('output framing')
    lines=raw[:-1].split('\n')
    if len(lines)!=len(cases): raise ValueError('output row count')
    results=[]
    for line,c in zip(lines,cases):
        f=line.split(' ')
        if len(f)!=8 or f[0]!='A1': raise ValueError('output schema')
        if not all(re.fullmatch(r'0|-?[1-9][0-9]*',v) for v in f[1:7]):
            raise ValueError('output integer')
        metrics=list(map(int,f[1:6])); status=int(f[6]); pixels=f[7]
        if any(not -32768<=n<=32767 for n in metrics[:4]) or metrics[4] not in (0,1) or status not in (0,1,2): raise ValueError('output range')
        if len(pixels)!=2*c['request'][10] or not re.fullmatch(r'[0-9a-f]*',pixels):
            raise ValueError('output packed frame')
        data=bytes.fromhex(pixels)
        results.append({'metrics':metrics,'status':status,'frame_sha256':hashlib.sha256(data).hexdigest(),'ink_bits':sum(data.translate(BIT_COUNTS))})
    return results
