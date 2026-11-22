"""Complete LF protocol, operation order, field widths and mock-source traces."""
import hashlib
import json
import re

NAT = re.compile(r'(?:0|[1-9][0-9]*)\Z')
INT = re.compile(r'(?:0|-?[1-9][0-9]*)\Z')
HEX = re.compile(r'(?:[0-9a-f]{2})+\Z')


def require(ok, message):
    if not ok: raise ValueError(message)


def number(s, low=0, high=0xffffffff):
    require((INT if low<0 else NAT).fullmatch(s) is not None, 'canonical integer required')
    n=int(s);require(low<=n<=high, 'wire width');return n


def data(s):
    if s=='_': return b''
    require(HEX.fullmatch(s) is not None, 'canonical hex required');return bytes.fromhex(s)


def commands(text):
    require(isinstance(text,str) and text.endswith('\n') and '\r' not in text, 'complete LF request required')
    rows=[line.split(' ') for line in text[:-1].split('\n')]
    for p in rows:
        require(len(p)==11 and p[0]=='P' and all(p), 'request arity/op')
        require(len(data(p[1]))<=1048576,'request bytes cap');number(p[2],high=65535)
        for s in p[3:6]: number(s)
        number(p[6],high=0xffffffffffffffff);number(p[7],-1,0x7fffffffffffffff)
        number(p[8],-0x80000000,0);number(p[9],high=1);number(p[10],high=2048)
    return rows


def validate(text, raw):
    requests=commands(text)
    require(isinstance(raw,bytes) and raw.endswith(b'\n') and b'\r' not in raw,'complete raw LF reply required')
    rows=[line.split(' ') for line in raw[:-1].decode('ascii').split('\n')]
    require(all(all(row) for row in rows),'extra whitespace')
    cursor=0
    def take(tag,n):
        nonlocal cursor
        require(cursor<len(rows),'truncated response');row=rows[cursor];cursor+=1
        require(len(row)==n and row[0]==tag,'wrong response order/arity');return row
    for op,p in enumerate(requests):
        require(number(take('OP',2)[1])==op,'operation sequence')
        number(take('STATUS',2)[1],high=10)
        info=take('INFO',5);number(info[1],high=2)
        number(info[2],high=65535);number(info[3],high=65535);number(info[4],high=1)
        number(take('SCRATCH',2)[1],high=1)
        blob=data(p[1]);chunk=int(p[6]);fault=int(p[7]);ret=int(p[8]);calls=0
        while cursor<len(rows) and rows[cursor][0]=='READ':
            row=take('READ',4);at=number(row[1],high=0xffffffffffffffff)
            want=number(row[2]);got=number(row[3],-0x80000000,0x7fffffff)
            expected=ret if calls==fault else min(max(0,len(blob)-at),chunk,want)
            require(want>0 and got==expected,'source read violates request/fault')
            calls+=1
        require(number(take('END',2)[1])==calls,'read count mismatch')
    require(cursor==len(rows),'unsolicited response')
    return [' '.join(row) for row in rows]


def fingerprint(case):
    return hashlib.sha256(json.dumps(case,sort_keys=True,separators=(',',':')).encode()).hexdigest()


def require_cases(cases, frozen):
    require(len(cases)==len(frozen) and len({c['name'] for c in cases})==len(cases),'mandatory case count/uniqueness')
    require({c['name']:fingerprint(c) for c in cases}==frozen,'frozen mandatory IDs/fingerprints changed')
    for case in cases: commands(case['input'])
