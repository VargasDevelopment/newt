"""Independent synthetic wire fixtures; no assets, SDK imports or decoder calls."""
import struct
import zlib

UNKNOWN = 'INFO 0 0 0 0'
PNG = 'INFO 1 513 257 0'
JPEG = 'INFO 2 513 257 0'


def png(w=513, h=257):
    return b'\x89PNG\r\n\x1a\n' + struct.pack('>I', 13) + b'IHDR' + struct.pack('>II', w, h) + b'\x08\x02'


def sof(marker=0xc0, w=513, h=257, length=7):
    return bytes([255, marker]) + struct.pack('>HBHH', length, 8, h, w)


def segment(n, marker=0xe1):
    return bytes([255, marker]) + struct.pack('>H', n + 2) + b'x' * n


def jpeg(prefix=b'', marker=0xc0, w=513, h=257):
    return (b'\xff\xd8' + prefix + sof(marker, w, h)).ljust(26, b'\0')


def command(data, method=0, comp=None, size=None, offset=0xffffffff,
            chunk=999999, fail=-1, ret=-1, decode=1, cap=1024):
    return f'P {data.hex() if data else "_"} {method} {len(data) if comp is None else comp} {len(data) if size is None else size} {offset} {chunk} {fail} {ret} {decode} {cap}'


def local(payload, method):
    return struct.pack('<IHHHHHIIIHH', 0x04034b50, 20, 0, method, 0, 0, 0, len(payload), len(payload), 0, 0) + payload


def corpus():
    cases = []
    def add(name, data, info=UNKNOWN, status=0, **kw):
        cases.append({'name': name, 'input': command(data, **kw)+'\n',
                      'expected_lines': [f'STATUS {status}', *([info] if info is not None else []), 'SCRATCH 1']})
    add('png-basic', png(), PNG)
    add('jpeg-basic', jpeg(), JPEG)
    add('jpeg-progressive', jpeg(marker=0xc2), 'INFO 2 513 257 1')
    add('jpeg-zero-dimensions', jpeg(w=0, h=0), 'INFO 2 0 0 0')
    add('jpeg-sof-short-declared-length', (b'\xff\xd8'+sof(length=2)).ljust(26,b'\0'), JPEG)
    for w,h in [(0,1),(1,0),(65536,1),(1,65536),(0xffffffff,0xffffffff),(65535,65535),(1,1)]:
        info = f'INFO 1 {w} {h} 0' if 0<w<=65535 and 0<h<=65535 else UNKNOWN
        add(f'png-dimensions-{w}-{h}', png(w,h), info)
    for idx in [0,1,7,12,13,15]:
        b=bytearray(png());b[idx]^=1;add(f'png-invalid-byte-{idx}', bytes(b))
    b=bytearray(png());b[8:12]=b'\0'*4;add('png-ignored-chunk-length',bytes(b),PNG)
    for marker in range(0xc0,0xd0):
        if marker in (0xc4,0xc8,0xcc):
            add(f'jpeg-excluded-{marker:02x}', jpeg(segment(5,marker)), JPEG)
        else:
            add(f'jpeg-sof-{marker:02x}', jpeg(marker=marker), f'INFO 2 513 257 {int(marker==0xc2)}')
    for marker in [0xd8,*range(0xd0,0xd8),1]:
        add(f'jpeg-standalone-{marker:02x}', jpeg(bytes([255,marker])),JPEG)
    for marker in [0xd9,0xda]: add(f'jpeg-stop-{marker:02x}',jpeg(segment(0,marker)))
    for marker in [0,0xff]: add(f'jpeg-length-bearing-{marker:02x}',jpeg(segment(3,marker)),JPEG)
    add('jpeg-missing-marker-prefix', jpeg(b'xx'))
    for length in [0,1]: add(f'jpeg-invalid-segment-length-{length}',jpeg(b'\xff\xe1'+struct.pack('>H',length)))
    for n in [0,1,13,14,15,16,17,18,19,20,21,22,23,24,25,26,1023,1024,1025,2049]:
        add(f'jpeg-segment-boundary-{n}',jpeg(segment(n)),JPEG)
    # The loop checks physical consumed bytes only between segments. Pending
    # replay never counts twice; a SOF begun just below the cap can finish above it.
    add('jpeg-limit-before', jpeg(segment(65533)+segment(65525)),JPEG)
    add('jpeg-limit-at', jpeg(segment(65533)+segment(65529)))
    add('jpeg-limit-over', jpeg(segment(65533)+segment(65533)))
    for label,data,info in [('png',png(),PNG),('jpeg',jpeg(segment(29)),JPEG)]:
        for n in range(len(data)):
            add(f'{label}-truncate-{n}',data[:n])
        for chunk in [1,2,3,5,7,13,23,24,25,26,27,1024]:
            add(f'{label}-raw-chunk-{chunk}',data,info,chunk=chunk)
            add(f'{label}-stored-chunk-{chunk}',local(data,0),info,comp=len(data),size=len(data),offset=0,chunk=chunk)
        for strategy_name,strategy in [('default',zlib.Z_DEFAULT_STRATEGY),('fixed',zlib.Z_FIXED),('huffman',zlib.Z_HUFFMAN_ONLY)]:
            z=zlib.compressobj(wbits=-15,strategy=strategy);packed=z.compress(data)+z.flush()
            for chunk in [1,2,7,17,2048]:
                add(f'{label}-deflated-{strategy_name}-{chunk}',local(packed,8),info,method=8,comp=len(packed),size=len(data),offset=0,chunk=chunk)
            if strategy_name=='default':
                # A truncated compressed stream may expose complete probe bytes
                # before EOF; compare exact info/traces to the actual original,
                # independently require Ok plus scratch restoration here.
                for n in range(len(packed)):
                    add(f'{label}-deflate-truncated-{n}',local(packed[:n],8),info=None,method=8,comp=len(packed),size=len(data),offset=0)
                add(f'{label}-decoder-missing',local(packed,8),method=8,comp=len(packed),size=len(data),offset=0,decode=0,status=7)
                add(f'{label}-decoder-and-staging-missing',local(packed,8),method=8,comp=len(packed),size=len(data),offset=0,decode=0,cap=0,status=7)
                for cap in [0,1023,1024,2048]:
                    add(f'{label}-deflate-staging-{cap}',local(packed,8),info if cap>=1024 else UNKNOWN,method=8,comp=len(packed),size=len(data),offset=0,cap=cap,status=0 if cap>=1024 else 7)
        for ret in [0,-1,-7]:
            for call in [0,1,2,3,4,5,12,25]:
                add(f'{label}-raw-fault-{call}-{ret}',data,info if call>=len(data) else UNKNOWN,chunk=1,fail=call,ret=ret)
            add(f'{label}-open-fault-{ret}',local(data,0),status=1,comp=len(data),size=len(data),offset=0,fail=0,ret=ret,cap=0)
        for cap in [0,1,1023,1024,1025,2048]:
            add(f'{label}-staging-{cap}',data,info if cap>=1024 else UNKNOWN,status=0 if cap>=1024 else 7,cap=cap)
    # Faults after the complete initial header preserve Ok/Unknown, including
    # short reads midway through next() or skipN().
    for n in [24,25,26,27,28,29,30,31]:
        add(f'jpeg-late-fault-{n}',jpeg(segment(24)),chunk=1,fail=n)
    add('raw-open-precedence',png(),method=8,cap=0,status=6)
    add('local-unsupported-precedence',local(png(),99),method=99,offset=0,cap=0,status=6)
    add('local-invalid-precedence',b'x'*30,offset=0,cap=0,status=2)
    add('local-truncated-size-precedence',local(png(),0),comp=1,size=2,offset=0,cap=0,status=5)
    for label,data in [('gif',b'GIF89a'),('svg',b'<svg width="100"/>'),('webp',b'RIFF0000WEBP'),('unknown',b'unknown')]:
        add(f'unsupported-{label}',data.ljust(40,b'\0'))
    for label,data,info in [('png',png(),PNG),('jpeg',jpeg(segment(29)),JPEG)]:
        add(f'{label}-zero-source-chunk',data,chunk=0)
        for mode in [0,1,9]:
            z=zlib.compressobj(level=mode,wbits=-15);packed=z.compress(data)+z.flush()
            for call in [30,31]:
                for ret in [0,-7]:
                    add(f'{label}-deflate-fault-{mode}-{call}-{ret}',local(packed,8),method=8,comp=len(packed),size=len(data),offset=0,chunk=1,fail=call,ret=ret)
        add(f'{label}-corrupt-deflate',local(b'\x07',8),method=8,comp=1,size=len(data),offset=0)
    for label,data,info in [('skip-wrap',jpeg(segment(65533)),JPEG),('scan-cap',jpeg(segment(65533)+segment(65529)),UNKNOWN)]:
        for mode in [0,9]:
            z=zlib.compressobj(level=mode,wbits=-15);packed=z.compress(data)+z.flush()
            for chunk in [7,2048]:
                add(f'jpeg-deflate-{label}-{mode}-{chunk}',local(packed,8),info,method=8,comp=len(packed),size=len(data),offset=0,chunk=chunk)
    # Multiple calls exercise output poisoning/reset and full operation accounting.
    cases.append({'name':'operation-sequence','input':'\n'.join([command(png()),command(b'x'*26),command(jpeg(marker=0xc2))])+'\n',
                  'expected_lines':[PNG,UNKNOWN,'INFO 2 513 257 1']})
    return cases
