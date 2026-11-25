"""EA1 syntax and lifecycle validation. Protocol errors never count as kills."""
import re
U = re.compile(r'(?:0|[1-9][0-9]*)\Z')
H = re.compile(r'(?:[0-9a-f]{2})+\Z')
SHAPES = {'R':3,'Y':0,'E':8,'M':4,'P':3,'C':3,'F':1,'S':1,'D':6,'END':0}

def need(ok, why):
    if not ok:
        raise ValueError(why)

def num(s, lo=0, hi=2**64-1):
    need(bool(U.fullmatch(s)) or (lo < 0 and s.startswith('-') and bool(U.fullmatch(s[1:])) and s != '-0'), 'decimal')
    n = int(s)
    need(lo <= n <= hi, 'field bound')
    return n

def request(wire):
    need(wire.endswith('\n') and wire.count('\n') == 1 and '\r' not in wire, 'request framing')
    p = wire[:-1].split(' ')
    need(len(p) == 9 and p[:1] == ['EA1'] and p[1] in ('A','O','C'), 'request schema')
    need(p[2] == '-' or bool(H.fullmatch(p[2])) and len(p[2]) <= 4194304, 'archive hex')
    for i in (3,4,5,7,8): num(p[i], 0, 2097152)
    num(p[6], -1, 2097152)
    need(int(p[5]) > 0 and 0 < int(p[8]) <= 32768, 'read capacity')
    return p

def response(text, wire):
    p = request(wire)
    need(text.endswith('\n') and '\r' not in text and text.isascii(), 'response framing')
    rows = [line.split(' ') for line in text[:-1].split('\n')]
    for row in rows:
        need(row[0] in SHAPES and len(row) == SHAPES[row[0]]+1, 'row shape')
        if row[0] in ('M','C'):
            for value in (row[1:] if row[0]=='M' else row[3:]): need(value == '-' or bool(H.fullmatch(value)) and len(value) <= 4194304, 'hex data')
        else:
            for value in row[1:]: num(value, -1)
    need(len(rows) >= 3 and [r[0] for r in rows[-3:]] == ['S','D','END'], 'terminal order')
    for tag in ('S','D','END'): need(sum(r[0] == tag for r in rows) == 1, 'duplicate terminal')
    status = num(rows[-3][1], 0, 13)
    d = list(map(int, rows[-2][1:]))
    for i, cap in ((0,int(p[3])),(3,int(p[4]))):
        need(0 <= d[i] <= d[i+1] <= cap and 0 <= d[i+2] <= 2**32-1, 'arena invariants')
    stack = []
    inspected = catalogued = False
    compares = 0
    chapter_at = 0
    chapter_done = False
    finish = False
    metadata = package = False
    for row in rows[:-3]:
        tag = row[0]
        need(tag not in ('S','D','END'), 'early terminal')
        if tag == 'R':
            at, want, got = map(int,row[1:])
            need(0 <= at <= 2**64-1 and 0 <= want <= 2**32-1 and -1 <= got <= min(want,int(p[5])), 'read bounds')
            if got >= 0: need(at+got <= (0 if p[2]=='-' else len(p[2])//2) or got==0, 'read extent')
        elif tag == 'E':
            need(p[1] == 'A', 'events outside admission')
            k,phase,member,compressed,expanded,result,method,category = map(int,row[1:])
            need(0 <= k <= 10 and phase == 0 and 0 <= member <= 1023 and 0 <= compressed <= 2**32-1 and 0 <= expanded <= 2**32-1 and -1 <= result <= 2**31-1 and 0 <= method <= 255 and 0 <= category <= 2, 'event bounds/domain')
            need(k != 0, 'phase outside NoPolicy oracle domain')
            if k in (1,3,5,7,8):
                need(result == 0, 'begin result')
                if k == 1: need(not inspected and not stack, 'inspect order')
                if k == 3: need(inspected and not catalogued and not stack, 'catalog order')
                if k == 5: need(catalogued and not stack, 'compare order')
                if k == 7: need(catalogued and not stack, 'member order')
                if k == 8: need(stack and stack[-1][0] == 7, 'stream nesting')
                if k not in (7,): need(member == compressed == expanded == method == category == 0, 'nonmember fields')
                stack.append((k,row[2:6]+row[7:9]))
            else:
                begin = {2:1,4:3,6:5,9:8,10:7}[k]
                need(stack and stack[-1][0] == begin, 'unpaired event')
                _, fields = stack.pop()
                need(fields == row[2:6]+row[7:9], 'pair fields')
                if k in (2,4,10): need(0 <= result <= 13, 'status event')
                if k == 2: inspected = True
                if k == 4: catalogued = result == 0
                if k == 9: need(-1 <= result <= 2048, 'stream count')
                if k == 6:
                    # One pair for the whole duplicate-name check; result is its comparison total.
                    need(compares == 0 and result >= 0, 'comparison pair')
                    compares += 1
        elif tag == 'M':
            need(p[1]=='O' and status==0 and not metadata and not package, 'metadata order')
            metadata = True
        elif tag == 'P':
            need(metadata and not package, 'package pairing')
            num(row[1],0,1024); num(row[2],0,1024); num(row[3],0,1)
            package = True
        elif tag == 'C':
            need(p[1]=='C' and not chapter_done and not finish, 'chapter order')
            at, got = num(row[1]), num(row[2],-1,2048)
            need(at == chapter_at and -1 <= got <= min(2048,int(p[8])), 'chapter offset/count')
            need(len(b'' if row[3]=='-' else bytes.fromhex(row[3]))==max(got,0),'chapter payload extent')
            chapter_at += max(got,0)
            chapter_done = got <= 0
        elif tag == 'F':
            need(p[1]=='C' and chapter_done and not finish, 'finish order')
            num(row[1],0,1)
            finish = True
    need(not stack, 'unfinished event pair')
    need(metadata == package and (p[1]!='O' or status!=0 or metadata), 'missing metadata')
    need(p[1]!='C' or status!=0 or finish, 'missing chapter finish')
    if p[1]=='A':
        need(inspected, 'missing inspect')
        events=[r for r in rows if r[0]=='E']
        need(events and events[0][1]=='1', 'first event')
        for event in events:
            if event[1] in ('2','4','10') and int(event[6]) != 0:
                need(event is events[-1] and int(event[6])==status, 'failure terminal status/order')
        if status==0:
            need(catalogued and any(r[1]=='5' for r in events) and any(r[1]=='7' for r in events),'successful admission incomplete')
    return rows

def semantic(rows):
    return [row for row in rows if row[0] != 'D']
