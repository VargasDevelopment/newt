"""Decode the frozen approved brand PNG, identity-checked against its freeze
manifest. Shared by make-stage-backdrop.py and make-install-wordmark.py;
standard library only."""
import hashlib
import json
from pathlib import Path
import struct
import sys
import zlib

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'assets/frosted-home/source'))
from png_tools import paeth  # noqa: E402

SOURCE = ROOT / 'assets/frosted-home/source/assets/newt-brand-reference.png'


def brand_pixels():
    """Decoded RGB rows (1024x1536) of the frozen, identity-checked brand PNG."""
    raw = SOURCE.read_bytes()
    # Reuse the frozen manifest's exact source identity.
    manifest = json.loads((SOURCE.parents[1] / 'freeze-manifest.json').read_text())
    identity = next(x for x in manifest['files'] if x['path'] == 'assets/newt-brand-reference.png')
    if hashlib.sha256(raw).hexdigest() != identity['sha256']:
        raise ValueError('Approved brand source changed')
    position, compressed = 8, bytearray()
    while position < len(raw):
        size = struct.unpack('>I', raw[position:position+4])[0]
        kind, body = raw[position+4:position+8], raw[position+8:position+8+size]
        if zlib.crc32(kind+body) != struct.unpack('>I', raw[position+8+size:position+12+size])[0]:
            raise ValueError('PNG checksum mismatch')
        if kind == b'IHDR':
            if struct.unpack('>IIBBBBB', body) != (1024,1536,8,2,0,0,0):
                raise ValueError('Unexpected brand PNG format')
        if kind == b'IDAT': compressed.extend(body)
        position += size + 12
    data = zlib.decompress(compressed)
    stride = 1024*3
    if len(data) != (stride+1)*1536: raise ValueError('Unexpected brand raster length')
    previous, pixels = bytearray(stride), bytearray()
    for y in range(1536):
        method = data[y*(stride+1)]
        row = bytearray(data[y*(stride+1)+1:(y+1)*(stride+1)])
        if method > 4: raise ValueError('Unsupported PNG filter')
        for x in range(stride):
            a = row[x-3] if x >= 3 else 0
            b = previous[x]
            c = previous[x-3] if x >= 3 else 0
            row[x] = (row[x]+(0,a,b,(a+b)//2,paeth(a,b,c))[method]) & 255
        pixels.extend(row); previous = row
    return pixels
