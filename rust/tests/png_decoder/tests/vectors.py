"""Independently generate PNG bytes, never capture an oracle's output."""
import binascii
import struct
import zlib


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', binascii.crc32(kind + data))


def paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p-a), abs(p-b), abs(p-c)
    return a if pa <= pb and pa <= pc else b if pb <= pc else c


def pack(samples, depth):
    if depth == 16:
        return b''.join(struct.pack('>H', s) for s in samples)
    if depth == 8:
        return bytes(samples)
    result = bytearray((len(samples)*depth + 7)//8)
    for i, value in enumerate(samples):
        result[i*depth//8] |= value << (8-depth-(i*depth % 8))
    return bytes(result)


def png(width=9, height=7, color=6, depth=8, filters=(0,1,2,3,4),
        interlace=0, transparent=False, gamma=None, split=0, background=False, image=None):
    channels = {0:1,2:3,3:1,4:2,6:4}[color]
    maximum = (1 << depth)-1
    count = min(1 << depth, 16)
    def pixel(x, y):
        if color == 3:
            return [(x*3+y*5) % count]
        return [((x*31+y*71+c*107)*maximum//255) & maximum for c in range(channels)]
    image = image if image is not None else [[pixel(x,y) for x in range(width)] for y in range(height)]
    # Adam7 is generated from independent coordinates, not the decoder's pass tables.
    passes = [(0,0,1,1)] if not interlace else [(0,0,8,8),(4,0,8,8),(0,4,4,8),(2,0,4,4),(0,2,2,4),(1,0,2,2),(0,1,1,2)]
    raw = bytearray()
    for x0,y0,xstep,ystep in passes:
        xs, ys = list(range(x0,width,xstep)), list(range(y0,height,ystep))
        if not xs or not ys:
            continue
        previous = bytes((len(xs)*channels*depth+7)//8)
        bpp = (channels*depth+7)//8
        for j,y in enumerate(ys):
            row = pack([s for x in xs for s in image[y][x]], depth)
            filter_type = filters[j % len(filters)]
            raw.append(filter_type)
            for i,value in enumerate(row):
                a = row[i-bpp] if i>=bpp else 0
                b = previous[i]
                c = previous[i-bpp] if i>=bpp else 0
                predictor = (0,a,b,(a+b)//2,paeth(a,b,c))[filter_type]
                raw.append((value-predictor)&255)
            previous = row
    head = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR',struct.pack('>IIBBBBB',width,height,depth,color,0,0,interlace))
    if gamma is not None:
        head += chunk(b'gAMA',struct.pack('>I',gamma))
    if color == 3:
        head += chunk(b'PLTE',bytes(v for i in range(count) for v in ((i*17)&255,(255-i*13)&255,(i*59)&255)))
    if transparent and color in (0,2,3):
        data = bytes((i*37)&255 for i in range(count)) if color == 3 else b''.join(struct.pack('>H',s) for s in image[0][0])
        head += chunk(b'tRNS',data)
    if background:
        head += chunk(b'bKGD',b'\x00' if color == 3 else b'\x00\x00')
    compressed = zlib.compress(bytes(raw))
    pieces = [compressed] if not split else [compressed[i:i+split] for i in range(0,len(compressed),split)]
    return head + b''.join(chunk(b'IDAT',p) for p in pieces) + chunk(b'IEND',b'')


def request(data, *, mode=0, bits=0, gamma=0, sw=9, sh=7, dw=4, dh=3, stop=0, step=4096, fail=1048576):
    return f'PN1 {mode} {bits} {gamma} {sw} {sh} {dw} {dh} {stop} {step} {fail} {data.hex()}\n'


def cases():
    for color,depths in ((0,(1,2,4,8,16)),(2,(8,16)),(3,(1,2,4,8)),(4,(8,16)),(6,(8,16))):
        for depth in depths:
            for filter_type in range(5):
                for interlace in (0,1):
                    name = f'c{color}-d{depth}-f{filter_type}-a{interlace}'
                    data = png(color=color,depth=depth,filters=(filter_type,),interlace=interlace,transparent=True,split=7)
                    yield name,request(data)
                    if not interlace:
                        yield name+'-scaled',request(data,mode=1)
            for display in (1000,2200):
                yield f'gamma-c{color}-d{depth}-{display}',request(png(color=color,depth=depth,gamma=45455),gamma=display)
    base = png()
    for bits in (0,1):
        for step in (1,3,8,13,31,4096):
            yield f'feed-{step}-bits-{bits}',request(base,step=step,bits=bits)
    for sw,sh,dw,dh in ((1,1,1,1),(9,7,1,1),(9,7,17,11),(257,3,2,1),(2,257,1,2),(257,40,7,3),(257,257,1,1)):
        data = png(width=sw,height=sh)
        for stop in (0,1,2):
            yield f'scale-{sw}-{sh}-{dw}-{dh}-stop{stop}',request(data,mode=1,sw=sw,sh=sh,dw=dw,dh=dh,stop=stop)
    for size in (1,1,2,3,5,8):
        yield f'adam7-tiny-{size}',request(png(width=size,height=size,interlace=1))
    yield 'wrapper-adam7',request(png(interlace=1),mode=1)
    for cut in (1,7,8,15,26,32,33,len(base)-13,len(base)-1):
        for mode in (0,1):
            yield f'truncated-{cut}-mode{mode}',request(base[:cut],mode=mode)
    bad = bytearray(base);bad[-1] ^= 1
    yield 'bad-iend-crc',request(bad)
    bad = bytearray(base);bad[29] ^= 1
    yield 'bad-ihdr-crc',request(bad)
    for mode in (0,1):
        yield f'bad-signature-{mode}',request(b'not a png'+base[9:],mode=mode)
    yield 'io-first-read',request(base,mode=1,fail=0)
    yield 'io-after-header',request(base,mode=1,step=33,fail=33)
    # Correct CRC with corrupt filter, Adler trailer, palette reference and chunk order.
    def raw_png(scan, color=0, depth=8, extra=b''):
        return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',1,1,depth,color,0,0,0))+extra+chunk(b'IDAT',zlib.compress(scan))+chunk(b'IEND',b'')
    yield 'invalid-filter',request(raw_png(b'\x05\x11'))
    yield 'palette-out-of-range',request(raw_png(b'\x00\x01',3,8,chunk(b'PLTE',b'\xff\x00\x00')))
    yield 'missing-palette',request(raw_png(b'\x00\x00',3))
    yield 'duplicate-ihdr',request(base[:33]+base[8:33]+base[33:])
    yield 'idat-before-ihdr',request(base[:8]+chunk(b'IDAT',b'x'))
    yield 'iend-before-idat',request(base[:33]+chunk(b'IEND',b''))
    yield 'unknown-critical',request(base[:33]+chunk(b'XXXX',b'hello')+base[33:])
    yield 'unknown-empty',request(base[:33]+chunk(b'tEXt',b'')+base[33:])
    yield 'trailing-after-eof',request(base+b'anything')
    stream = zlib.compress(b'\x00\x71'); corrupt=stream[:-1]+bytes([stream[-1]^1])
    yield 'bad-adler',request(base[:8]+chunk(b'IHDR',struct.pack('>IIBBBBB',1,1,8,0,0,0,0))+chunk(b'IDAT',corrupt)+chunk(b'IEND',b''))
    for color in (0,3,4):
        yield f'background-{color}',request(png(color=color,background=True))

    for kind,data in ((b'PLTE',b'xx'),(b'tRNS',b'x'),(b'IDAT',b''),(b'IHDR',b'x'),(b'IEND',b'x')):
        yield 'invalid-chunk-'+kind.decode(),request(base[:33]+chunk(kind,data)+base[33:])
    yield 'duplicate-palette',request(png(color=3)[:33]+chunk(b'PLTE',b'\x00\x00\x00')+png(color=3)[33:])
    yield 'duplicate-transparency',request(png(color=0,transparent=True)[:33]+chunk(b'tRNS',b'\x00\x00')+png(color=0,transparent=True)[33:])
    for width,height in ((1,17),(17,1),(2,3),(3,2)):
        yield f'adam7-empty-{width}-{height}',request(png(width=width,height=height,interlace=1,color=0,depth=1))

    # At the second pixel of row two: a=0, b=3, c=2; Paeth ties a/c at distance 1.
    tie=b'\x00\x02\x03\x04\xfe\x64'
    yield 'paeth-a-c-tie',request(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',2,2,8,0,0,0,0))+chunk(b'IDAT',zlib.compress(tie))+chunk(b'IEND',b''))
