#!/usr/bin/env python3
"""Cut Newt's wordmark images from the frozen approved brand PNG.

The source is assets/frosted-home/source/assets/newt-brand-reference.png,
identity-checked against its freeze manifest (tools/brand_source.py), and
the crop is Stage Home's approved wordmark crop (WORDMARK_CROP,
tools/make-stage-backdrop.py; assets/frosted-home/source/README.md).

- installer/src/newt-wordmark.png: the crop's RGB pixels, unchanged; the
  install page multiplies it over its background.
- docs/newt-wordmark-light.png and docs/newt-wordmark-dark.png: the README's
  light- and dark-theme images. Each pixel's ink, 255 minus its gray level,
  becomes alpha, so the crop's anti-aliasing survives on any background. The
  light image is black ink (over white it reproduces the crop); the dark image
  is white ink.

Standard library only.

  make-install-wordmark.py           write all three images
  make-install-wordmark.py --check   the committed files hold exactly these pixels
"""
import argparse
import importlib.util
from pathlib import Path
import struct
import sys
import zlib

from brand_source import brand_pixels

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / 'installer/src/newt-wordmark.png'
# README theme images: path -> ink colour.
THEMED = {ROOT / 'docs/newt-wordmark-light.png': (0, 0, 0),
          ROOT / 'docs/newt-wordmark-dark.png': (255, 255, 255)}
SOURCE_W = 1024


def tool(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), ROOT / 'tools' / f'{name}.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def crop():
    """(width, height, filtered scanlines): each row a 0 filter byte and its RGB bytes."""
    x, y, width, height = tool('make-stage-backdrop').WORDMARK_CROP
    pixels = brand_pixels()
    rows = (b'\0' + bytes(pixels[((y + row) * SOURCE_W + x) * 3:((y + row) * SOURCE_W + x + width) * 3])
            for row in range(height))
    return width, height, b''.join(rows)


def themed(width, scanlines, ink):
    """RGBA scanlines: `ink` everywhere, with alpha from each RGB pixel's ink coverage."""
    rows = []
    for start in range(0, len(scanlines), width * 3 + 1):
        rgb = scanlines[start + 1:start + 1 + width * 3]
        row = bytearray(b'\0')
        for i in range(0, len(rgb), 3):
            row += bytes(ink) + bytes((255 - (rgb[i] + rgb[i + 1] + rgb[i + 2] + 1) // 3,))
        rows.append(bytes(row))
    return b''.join(rows)


def chunk(kind, body):
    return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))


def ihdr(width, height, colour_type):
    return struct.pack('>IIBBBBB', width, height, 8, colour_type, 0, 0, 0)


def png(header, scanlines):
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', header) + chunk(b'IDAT', zlib.compress(scanlines, 9))
            + chunk(b'IEND', b''))


def decoded(data):
    """(IHDR body, scanlines) of a PNG this tool wrote; compression may differ by zlib build."""
    position, header, stream = 8, None, bytearray()
    while position < len(data):
        size = struct.unpack('>I', data[position:position + 4])[0]
        kind, body = data[position + 4:position + 8], data[position + 8:position + 8 + size]
        if kind == b'IHDR':
            header = body
        elif kind == b'IDAT':
            stream.extend(body)
        position += size + 12
    return header, zlib.decompress(stream)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    width, height, scanlines = crop()
    images = {OUTPUT: (ihdr(width, height, 2), scanlines)}
    images.update({path: (ihdr(width, height, 6), themed(width, scanlines, ink)) for path, ink in THEMED.items()})
    failed = False
    for path, expected in images.items():
        name = path.relative_to(ROOT)
        if not args.check:
            path.write_bytes(png(*expected))
            print(f'Wrote {name} ({width}x{height})')
        elif not path.is_file() or decoded(path.read_bytes()) != expected:
            print(f'FAIL {name} is not cut from the approved wordmark crop; run tools/make-install-wordmark.py')
            failed = True
        else:
            print(f'PASS {name} is cut from the approved wordmark crop ({width}x{height})')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
