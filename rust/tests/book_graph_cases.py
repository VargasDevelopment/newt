# SPDX-License-Identifier: MIT
"""Independent little-endian fixtures and expected results, never derived from either port.
Parent gate calls cases(), compares BOTH executables to expected, then each other.
MIT; see LICENSE.
"""
import struct

D = bytes([7]) * 32
U32 = 0xffffffff

def n(v, width):
    return v.to_bytes(width, 'little')

def words(v, width=4):
    return b''.join(n(x, width) for x in v)

def checksum(b):
    h = 2166136261
    for x in b:
        h = ((h ^ x) * 16777619) & U32
    return n(h, 4)

def seal(b):
    return bytes(b[:-4]) + checksum(b[:-4])

def head(since=0, block=0, watermark=0, periods=(0,0,0,0), imported=(0,0,0,0,0,0,0), days=None, generation=0, digest=D, flags=0):
    b = bytearray(232)
    b[:12] = words((0x4454574e, 1, generation))
    b[12:44] = digest
    b[44:50] = words((since, block, flags), 2)
    b[52:60] = n(watermark, 8)
    b[60:104] = words((*periods, *imported))
    b[104:228] = words(days or [0]*62, 2)
    return seal(b)

def archive(block=153, days=None, generation=0, digest=D):
    b = bytearray(176)
    b[:12] = words((0x4554574e,1,generation))
    b[12:44] = digest
    b[44:46] = n(block,2)
    b[48:172] = words(days or [0]*62,2)
    return seal(b)

def block(b=153, days=None):
    return n(b,2)+words(days or [0]*62,2)

def graph(last=0, count=0, kinds=(), seconds=()):
    return n(last,2)+n(count,1)+bytes(list(kinds)+[0]*(42-len(kinds)))+words(list(seconds)+[0]*(42-len(seconds)))

def periods(seconds=(0,0,0,0), unknown=0):
    return words((*seconds, unknown))

def log(calls=()):
    return n(len(calls),2)+words(calls,2)

def blob(b):
    return n(len(b),2)+b

def month(m, buckets=(0,0,0,0), days=None):
    return n(m,2)+n(0,4)+words(buckets)+words(days or [0]*31,2)

def ledger():
    # Valid NWTS, Jan..Feb 2026, current February. Must enter through decodeLedger.
    b = bytearray(180)
    b[:12] = words((0x5354574e,1,0))
    b[12:16] = words((24312,24313),2)
    b[30:32] = n(24313,2)
    b[88:90] = n(65535,2)  # default BookFacts startPermille
    b[96:112] = words((10,20,30,40))
    b[112:114] = n(13,2)  # February 1
    return seal(b)

def cases():
    """Yield (unique name, request bytes, independently specified expected bytes)."""
    for rest, expected in ((59,0),(60,60),(61,61),(U32+1,U32)):
        request = b'\0'+words((1,2,3,4))+n(10+rest,8)+words((10000,9900),2)+words((42,2))+words((10000,9958,9900),2)
        yield f'unknown-{rest}',request,n(expected,4)+n(9916,2)+bytes([2])+n(9958,2)+n(161,2)
    h=head(); a=archive(); initial=block()
    candidates=[('valid',h,a,True,True),('head-short',h[:-1],a,False,True),('archive-short',h,a[:-1],True,False)]
    for label, offset, value in [('reserved',50,1),('flags',48,2),('version',4,2),('digest',12,8),('undated-block',46,153),('undated-period',60,1),('since-invalid',44,1)]:
        bad=bytearray(h); bad[offset]=value
        candidates.append((label,seal(bad),a,False,True))
    for label, offset, value in [('reserved',46,1),('version',4,2),('digest',12,8),('block',44,154)]:
        bad=bytearray(a); bad[offset]=value
        candidates.append(('archive-'+label,h,seal(bad),True,False))
    for offset in range(232):
        bad=bytearray(h); bad[offset]^=1
        candidates.append((f'head-bit-{offset}',bytes(bad),a,False,True))
    for offset in range(176):
        bad=bytearray(a); bad[offset]^=1
        candidates.append((f'archive-bit-{offset}',h,bytes(bad),True,False))
    for label,hb,ab,hvalid,avalid in candidates:
        request=b'\1'+D+h+initial+blob(hb)+blob(ab)+n(153,2)+n(U32,4)
        yield label,request,bytes([hvalid,avalid])+head(generation=U32)+archive(generation=U32)
    # Explicit fold arithmetic: delta, retained subsecond, lost-save and resets.
    for label,old,before,counters,day,hour,target,need,changed,result,p in [
        ('subsecond',h,(0,0,0),(999,0,0),9497,5,None,0,False,h,(0,0,0,0)),
        ('one-second',h,(0,0,0),(1999,0,0),9497,5,None,0,True,head(9497,153,1000,(1,0,0,0),days=[0]*11+[1]+[0]*50),(1,0,0,0)),
        ('lost-save',head(watermark=1000),(5000,0,0),(6500,0,0),9497,12,None,0,True,head(9497,153,6000,(0,1,0,0),days=[0]*11+[1]+[0]*50),(0,1,0,0)),
        ('reset-record',head(watermark=9000),(1000,0,0),(3500,0,0),9497,17,None,0,True,head(9497,153,3000,(0,0,2,0),days=[0]*11+[2]+[0]*50),(0,0,2,0)),
        ('rollback-pages',head(watermark=9000),(0,2,0),(3500,1,0),9497,17,None,0,True,head(watermark=3500),(0,0,0,0)),
        ('rollback-sessions',head(watermark=9000),(0,0,2),(3500,0,1),9497,17,None,0,True,head(watermark=3500),(0,0,0,0)),
        ('rollback-ms',head(watermark=9000),(4000,0,0),(3500,0,0),9497,17,None,0,True,head(watermark=3500),(0,0,0,0)),
        ('undated',h,(0,0,0),(2500,0,0),0,0,None,0,True,head(watermark=2000),(0,0,0,0)),
        ('missing-target',head(9497,153),(0,0,0),(2000,0,0),9600,21,None,154,True,head(9497,153,2000),(0,0,0,0)),
        ('wrong-target',head(9497,153),(0,0,0),(2000,0,0),9600,21,block(155),154,True,head(9497,153,2000),(0,0,0,0)),
        ('target',head(9497,153),(0,0,0),(2000,0,0),9600,21,block(154),154,True,head(9497,154,2000,(0,0,0,2),days=[0]*52+[2]+[0]*9),(0,0,0,2)),
        ('pre-since',head(9500,153),(0,0,0),(2000,0,0),9497,5,None,0,True,head(9500,153,2000),(0,0,0,0)),
    ]:
        request=b'\2'+D+old+n(before[0],8)+words(before[1:])+n(counters[0],8)+words(counters[1:])+n(day,2)+bytes([hour,target is not None])+(target or b'')
        yield label,request,n(need,2)+bytes([changed])+result+periods(p)
    for status in (0,1,2):
        # 9509..9550 spans block 153 then 154; injected block IDs are deliberately wrong.
        old=head(9497,155)
        replies=bytes([status])+block(0,[17]*62)
        num=42 if status==0 else 2
        expected_k=0 if status==2 else 1
        calls=[153]*39+[154]*3 if status==0 else [153,154]
        request=b'\3'+D+old+n(9550,2)+n(1000,4)+n(num,2)+replies*num
        yield f'block-status-{status}',request,graph(9550,42,[expected_k]*42,[17 if status==0 else 0]*42)+log(calls)
    for last,count in ((0,42),(9496,42),(36525,42),(65535,42),(9550,0)):
        yield f'invalid-window-{last}-{count}',b'\3'+D+head()+n(last,2)+n(count,4)+n(0,2),graph(last if last==9550 else 0)+log()
    # Month callbacks are independent for days/periods. Wrong loaded month is forced.
    jan=[0]*31; jan[30]=9
    for status in (0,1,2):
        reply=bytes([status])+month(1,(U32,1,2,3),jan)
        request=b'\4'+ledger()+n(0,4)+bytes(92)+words([0]*7)+words([0]*4)+n(9528,2)+n(2,4)+n(200,4)+n(1,2)+reply
        secs=[9,13] if status==0 else [0,13]
        buckets=(9,21,32,43) if status==0 else (10,20,30,40)
        yield f'library-month-{status}',request,n(9497,2)+graph(9528,2,[1,1],secs)+log([24312])+periods(buckets,95 if status==0 else 100)+log([24312])
    # CrossInk whole-bitmap weekday divisor: newest and seven days ago read.
    empty=bytearray(ledger()); empty[12:16]=bytes(4); empty[30:32]=bytes(2); empty[92:174]=bytes(82); empty=seal(empty)
    bitmap=bytes([0x81])+bytes(91)
    request=b'\4'+empty+n(9504,4)+bitmap+words([200000]*7)+words([0]*4)+n(9504,2)+n(1,4)+n(0,4)+n(0,2)
    yield 'weekday-whole-bitmap-cap',request,n(9497,2)+graph(9504,1,[2],[86400])+log()+periods()+log()
    # Every supported CrossInk book layout plus invalid/newer global/book variants.
    fixtures=[('empty',b'',False,False),('global-v1',bytes([1])+bytes(12),False,False),('global-v2',bytes([2])+bytes(16),False,False)]
    g=bytearray(159); g[0]=3; g[17:33]=words((11,12,13,14)); g[33:61]=words(range(21,28))
    fixtures.append(('global-v3',bytes(g),True,False))
    for version,size in ((1,11),(2,12),(3,16),(4,69),(5,73)):
        b=bytearray(size); b[0]=version; b[1:3]=n(3,2); b[3:7]=n(5,4); b[7:11]=n(7,4)
        if version>=4: b[25:41]=words((1,2,3,4))
        fixtures.append((f'book-v{version}',bytes(b),False,True))
    for label,offset,value in [('global-padding',156,4),('global-newer',0,4)]:
        bad=bytearray(g); bad[offset]=value; fixtures.append((label,bytes(bad),False,False))
    bad=bytearray(69); bad[0]=4; bad[16]=4; fixtures.append(('book-invalid-status',bytes(bad),False,False))
    for label,b,global_ok,book_ok in fixtures:
        initial_totals=[U32-1]*7; p=[91,92,93,94]; w=list(range(101,108)); bp=book_ok and len(b)>=69
        request=b'\5'+blob(b)+words(initial_totals)+words(p)+words(w)
        gp=[11,12,13,14] if global_ok else p
        result=bytes([global_ok])+words(gp)+bytes([global_ok])+words(range(21,28) if global_ok else w)+bytes([bp])+words((1,2,3,4) if bp else gp)+bytes([book_ok])+words([U32]*3 if book_ok else initial_totals[:3])+words([U32,U32,U32,U32] if bp else initial_totals[3:])
        yield label,request,result
    # Valid nontrivial head/archive payloads verify every public encode field.
    rich=head(9497,153,0x123456789abcdef0,(1,2,3,U32),(5,6,7,8,9,10,11),[0]*11+list(range(1,52)),flags=1)
    rich_a=archive(154,list(range(62)))
    yield 'rich-codecs',b'\1'+D+h+initial+blob(rich)+blob(rich_a)+n(154,2)+n(0,4),bytes([1,1])+rich+rich_a
    # Resealed but semantically invalid dated records leave original outputs intact.
    for label,bad in [('before-since',head(9497,153,days=[1]+[0]*61)),('invalid-current',head(9497,152)),('invalid-since-high',head(36525,589))]:
        yield label,b'\1'+D+h+initial+blob(bad)+blob(a)+n(153,2)+n(0,4),bytes([0,1])+h+a
    # Count zero still initializes firstDatedDay (callback ordering contract).
    reply=b'\0'+month(1,(U32,1,2,3),jan)
    request=b'\4'+ledger()+n(0,4)+bytes(92)+words([0]*7)+words([0]*4)+n(9528,2)+n(0,4)+n(200,4)+n(1,2)+reply
    yield 'library-zero-count-still-loads-first',request,n(9497,2)+graph(9528,0)+log([24312])+periods((9,21,32,43),95)+log([24312])
    # First nonzero Jan31 makes Jan30 unrecorded; dates after Feb remain timed zero.
    for label,last,kinds,secs,day_calls in [('before-first',9527,[0,1],[0,9],[24312]),('after-newest',9557,[1,1],[0,0],[24312]),('invalid-clock',0,[],[],[])]:
        request=b'\4'+ledger()+n(0,4)+bytes(92)+words([0]*7)+words([0]*4)+n(last,2)+n(2,4)+n(200,4)+n(1,2)+reply
        yield 'library-'+label,request,n(9497,2)+graph(last,2 if last else 0,kinds,secs)+log(day_calls)+periods((9,21,32,43),95)+log([24312])
    # Matching zero digests reach the actual codecs, bypassing fixture decoding.
    unchanged_archive = [11] + [0] * 61
    yield 'matching-zero-digest', b'\6' + bytes(32) + blob(head(digest=bytes(32))) + blob(archive(digest=bytes(32))), bytes([0, 0]) + head(watermark=99) + archive(154, unchanged_archive)
    # Credit starts at zero: truncation cannot hide behind already saturated counters.
    for label, milliseconds, rounded in [('large-credit', 4294967296999, 4294967296000), ('max-credit', (1 << 64) - 1, 18446744073709551000)]:
        request = b'\2' + D + head() + n(0, 8) + words((0, 0)) + n(milliseconds, 8) + words((0, 0)) + n(9497, 2) + bytes((5, 0))
        expected = n(0, 2) + bytes([1]) + head(9497, 153, rounded, (U32, 0, 0, 0), days=[0]*11+[65535]+[0]*50) + periods((U32, 0, 0, 0))
        yield label, request, expected
    overflowing = head(9497, 153, periods=(U32, 1, 0, 5), imported=(0, 0, 0, 1, U32, 0, 0))
    request = b'\2' + D + overflowing + n(0, 8) + words((0, 0)) + n(0, 8) + words((0, 0)) + n(9497, 2) + bytes((5, 0))
    yield 'book-period-overflow', request, n(0, 2) + bytes([0]) + overflowing + periods((U32, U32, 0, 5))


def verify(host_call, oracle_call):
    """Parent-owned transports: call(bytes)->bytes; no subprocess/tool assumptions."""
    count = 0
    for name, request, expected in cases():
        for implementation, call in (('Rust', host_call), ('original C++', oracle_call)):
            actual = call(request)
            if actual != expected:
                raise AssertionError(f'{name}: {implementation}: expected {expected.hex()}, got {actual.hex()}')
        count += 1
    return count


def corpus():
    return ['book_graph ' + request.hex() for _, request, _ in cases()]

def assert_expectations(inputs, replies):
    expected = {'book_graph ' + request.hex(): (name, result.hex()) for name, request, result in cases()}
    for command, reply in zip(inputs, replies):
        if command in expected:
            name, wanted = expected[command]
            if reply != wanted:
                raise AssertionError(f'Book graph independent expectation {name}: {reply} != {wanted}')
