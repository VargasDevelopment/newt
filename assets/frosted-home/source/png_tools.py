"""Read the actual grayscale PNG bytes using Python's standard library.

Image metrics describe exported mockups; they are not panel performance estimates.
"""
import json
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent

def paeth(a, b, c):
    p = a + b - c
    distances = (abs(p-a), abs(p-b), abs(p-c))
    return (a, b, c)[distances.index(min(distances))]

def read_png(path):
    raw = path.read_bytes()
    if raw[:8] != b'\x89PNG\r\n\x1a\n':
        raise ValueError('Not a PNG')
    pos, stream, header, ended = 8, bytearray(), None, False
    while pos < len(raw):
        length = struct.unpack('>I', raw[pos:pos+4])[0]
        kind = raw[pos+4:pos+8]
        body = raw[pos+8:pos+8+length]
        crc = struct.unpack('>I', raw[pos+8+length:pos+12+length])[0]
        if zlib.crc32(kind+body) != crc:
            raise ValueError('PNG CRC mismatch')
        if kind == b'IHDR':
            header = struct.unpack('>IIBBBBB', body)
        elif kind == b'IDAT':
            stream.extend(body)
        elif kind == b'IEND':
            ended = True
            break
        pos += length + 12
    if not header or not ended:
        raise ValueError('Incomplete PNG')
    w, h, depth, color, compression, filter_method, interlace = header
    if (depth, color, compression, filter_method, interlace) != (8, 0, 0, 0, 0):
        raise ValueError('Expected an 8-bit grayscale noninterlaced renderer export')
    data = zlib.decompress(stream)
    if len(data) != (w+1)*h:
        raise ValueError('Unexpected raster length')
    pixels, previous = bytearray(), bytearray(w)
    for y in range(h):
        at = y*(w+1)
        method = data[at]
        row = bytearray(data[at+1:at+1+w])
        if method > 4:
            raise ValueError('Unsupported filter')
        for x in range(w):
            a = row[x-1] if x else 0
            b = previous[x]
            c = previous[x-1] if x else 0
            prediction = (0, a, b, (a+b)//2, paeth(a,b,c))[method]
            row[x] = (row[x]+prediction) & 255
        pixels.extend(row)
        previous = row
    return w, h, pixels

def main():
    report = {'limits':'Export metrics only. No hardware, accessibility, energy, latency or ghosting claim.', 'screens':[], 'transitions':[]}
    for name in ['still','understory','folio','pebble']:
        frames = {}
        for state in ['home','sleep']:
            p = ROOT/'round2/screens'/f'{name}-{state}.png'
            w,h,pixels = read_png(p)
            values = sorted(set(pixels))
            if (w,h) != (528,792) or values != [0,255]:
                raise ValueError(f'Invalid fixture: {p}')
            report['screens'].append({'file':str(p.relative_to(ROOT)), 'width':w, 'height':h, 'pixel_values':values, 'ink_percent':round(pixels.count(0)*100/len(pixels),2), 'passed':True})
            frames[state] = pixels
        changes = sum(a!=b for a,b in zip(frames['home'],frames['sleep']))
        assert sum(a!=b for a,b in zip(frames['home'],frames['home'])) == 0
        report['transitions'].append({'direction':name, 'from':'home','to':'sleep', 'changed_pixels':changes, 'changed_pixel_percent':round(changes*100/(528*792),2)})
    (OUT/'baseline-metrics.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))

if __name__ == '__main__':
    main()
