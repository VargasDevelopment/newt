"""Test-only decoder for four exact authored UI PNGs; never a general decoder.

These source/payload pairs were compared byte-for-byte with the unchanged Mac
ImageIO helper. No JPEG, color, transform, resize or general PNG parity follows.
Unknown bytes fail; this tool never updates its allowlist or reviewed UI goldens.
"""
import hashlib
from pathlib import Path
import struct
import sys
import zlib

CORPUS = {
    'ed95cfd8e85ac4ff918ca5465f6d0eb5963a6178dae458e096395ce2648f8e26':
        'e0005b07e48d3a67b1a2a455fc1d188969b9af7d216cf4de2124b68274d3c50a',
    '8461c87a7d407c8fa762a897a4bdaae5eac3b51b294e08eade35584b473261a7':
        '66ba9c40f4fc4a2fcbf6bf312119229b757cd55ad0e16ad479d9a16c8f8d40e9',
    '5d8c441f295c1ed1b8abc104b1202b3b8281ca541998b593c8cad1a3a4cfaac9':
        '86561349d8ce0c58d761324ea45189018b8a38b0daab6510f6d9e09e3afcb387',
    '26bc40f3f346d0ca71c0ed81b5592ad56310a0b72e005af7f917c8aec52ddfe9':
        '0aef9897494a931ac46eb14173e0ca04e68de3c43455e429b7d9a67556e96a86',
}


def bitmap(image):
    expected = CORPUS.get(hashlib.sha256(image).hexdigest())
    if not expected or len(image) > 4096 or image[:8] != b'\x89PNG\r\n\x1a\n':
        raise ValueError('Only the four exact authored UI cover PNGs are supported')
    chunks, offset = [], 8
    while offset < len(image):
        length, = struct.unpack_from('>I', image, offset)
        kind = image[offset + 4:offset + 8]
        body = image[offset + 8:offset + 8 + length]
        crc, = struct.unpack_from('>I', image, offset + 8 + length)
        if zlib.crc32(kind + body) != crc:
            raise ValueError('Invalid authored PNG checksum')
        chunks.append((kind, body))
        offset += length + 12
    if ([kind for kind, _ in chunks] != [b'IHDR', b'IDAT', b'IEND']
            or chunks[0][1] != struct.pack('>2I5B', 144, 188, 8, 0, 0, 0, 0)
            or chunks[-1][1] or offset != len(image)):
        raise ValueError('Unexpected authored PNG shape')
    raw = zlib.decompress(chunks[1][1])
    if len(raw) != 145 * 188 or any(raw[y * 145] for y in range(188)):
        raise ValueError('Unexpected authored PNG scanlines')
    pixels = b''.join(raw[y * 145 + 1:(y + 1) * 145] for y in range(188))
    if set(pixels) != {0, 245}:
        raise ValueError('Unexpected authored grayscale tones')
    current, following = [0.0] * 146, [0.0] * 146
    packed = bytearray(3384)
    # Match the Swift helper's Float64 operation order, including each division.
    for y in range(188):
        for x in range(144):
            value = max(0, min(255, float(pixels[y * 144 + x]) + current[x + 1]))
            tone = 0.0 if value < 128 else 255.0
            if tone == 0:
                packed[y * 18 + x // 8] |= 0x80 >> (x % 8)
            error = value - tone
            current[x + 2] += error * 7 / 16
            following[x] += error * 3 / 16
            following[x + 1] += error * 5 / 16
            following[x + 2] += error / 16
        current, following = following, current
        for x in range(146):
            following[x] = 0.0
    if hashlib.sha256(packed).hexdigest() != expected:
        raise ValueError('Authored fixture differs from locked ImageIO payload')
    return bytes(packed)


def main():
    if len(sys.argv) != 3:
        raise ValueError('Expected authored fixture PNG and output bitmap')
    with Path(sys.argv[1]).open('rb') as source:
        image = source.read(4097)
    Path(sys.argv[2]).write_bytes(bitmap(image))


if __name__ == '__main__':
    main()
