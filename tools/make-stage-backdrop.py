#!/usr/bin/env python3
"""Generate Stage Home's static backdrop from the frozen approved brand PNG.

Follows design/home-refresh/SPEC.md (issue #29): the Newt mascot far out of
focus, frosted toward the edges, cleared behind the title and header, dithered
to Home's four tones, with the approved wordmark crop multiplied on top as
the spec mockup does it. The result is independent of book, focus and time.
Standard library only; every step uses exact integer or IEEE basic arithmetic
(no libm transcendental or version-dependent summation), so --check
reproduces the bytes on any host.

Output: two packed selector planes (LSB then MSB), canonical MSB-first rows,
2 * 52,272 bytes, written to both tracked copies: the asset and the one
rust/stage-home embeds.
"""
import argparse
from decimal import Decimal, getcontext
import hashlib
import math
from operator import mul
from pathlib import Path

from brand_source import brand_pixels

ROOT = Path(__file__).resolve().parents[1]
OUTPUTS = (ROOT / 'assets/frosted-home/stage-backdrop.bin',
           ROOT / 'rust/stage-home/assets/frosted-home/stage-backdrop.bin')
W, H = 528, 792
PLANE = W * H // 8
SOURCE_W = 1024
MASCOT_CROP = (214, 296, 600, 640)   # x, y, w, h in the brand reference
MASCOT_BOX = (-2, 44, 532, 567)      # placed and scaled on the screen
WORDMARK_CROP = (200, 951, 628, 210)  # the approved Home wordmark crop
WORDMARK_BOX = (28, 12, 86, 28)
SIGMA, RADIUS, WEIGHT_SCALE = 22, 66, 1 << 16


def brand_gray():
    rgb = brand_pixels()
    return [(rgb[i] * 299 + rgb[i + 1] * 587 + rgb[i + 2] * 114) // 1000 for i in range(0, len(rgb), 3)]


def area_average(gray, crop, width, height):
    """Box average over exact integer source spans; returns (sum, count) pairs."""
    x0, y0, cw, ch = crop
    spans = lambda size, source, i: (i * source // size, max(i * source // size + 1, (i + 1) * source // size))
    result = []
    for y in range(height):
        top, bottom = spans(height, ch, y)
        rows = [gray[(y0 + yy) * SOURCE_W + x0:(y0 + yy) * SOURCE_W + x0 + cw] for yy in range(top, bottom)]
        for x in range(width):
            left, right = spans(width, cw, x)
            result.append((sum(sum(row[left:right]) for row in rows), (bottom - top) * (right - left)))
    return result


def gaussian_weights():
    getcontext().prec = 40
    return [int((Decimal(-i * i) / Decimal(2 * SIGMA * SIGMA)).exp() * WEIGHT_SCALE + Decimal('0.5'))
            for i in range(-RADIUS, RADIUS + 1)]


def blur(layer, weights):
    """Separable integer Gaussian with clamped edges; returns exact sums."""
    def line(values):
        padded = [values[0]] * RADIUS + values + [values[-1]] * RADIUS
        if min(values) == max(values):
            return [values[0] * sum(weights)] * len(values)
        return [sum(map(mul, padded[i:i + len(weights)], weights)) for i in range(len(values))]
    rows = [line(layer[y * W:(y + 1) * W]) for y in range(H)]
    columns = [line([rows[y][x] for y in range(H)]) for x in range(W)]
    return [columns[x][y] for y in range(H) for x in range(W)], sum(weights) ** 2


def frost(gray):
    """Spec steps 1-8: continuous tones in [0, 255] before dithering."""
    layer = [255] * (W * H)
    bx, by, bw, bh = MASCOT_BOX
    for i, (total, count) in enumerate(area_average(gray, MASCOT_CROP, bw, bh)):
        x, y = bx + i % bw, by + i // bw
        if 0 <= x < W and 0 <= y < H:
            layer[y * W + x] = (2 * total + count) // (2 * count)
    sums, scale = blur(layer, gaussian_weights())
    values = []
    for index, total in enumerate(sums):
        x, y = index % W, index // W
        v = total / scale
        v = max(v + (255 - v) * 0.30, 128.0)                       # veil, then floor
        f = min(max(1 - min(x, y, W - 1 - x, H - 1 - y) / 150, 0.0), 1.0)
        v = min(v, 255 - 92 * f * f)                                # edge frost
        dx, dy = (x - 264) / 300, (y - 420) / 118
        d = math.sqrt(dx * dx + dy * dy)                            # sqrt is correctly rounded
        v += (255 - v) * 0.95 * min(max((1 - d) / 0.55, 0.0), 1.0)  # title clearing
        d = max(y - 40, 0)                                          # header box (0,0,528,40)
        v += (255 - v) * 0.9 * min(max(1 - d / 30, 0.0), 1.0)       # header clearing
        values.append(v)
    return values


def dither(values):
    """Spec step 9: serpentine Floyd-Steinberg to tones 0..3 (round half up)."""
    work, tones = list(values), bytearray(W * H)
    for y in range(H):
        sign = -1 if y % 2 else 1
        for n in range(W):
            x = n if sign > 0 else W - 1 - n
            i = y * W + x
            tone = min(3, max(0, math.floor(work[i] / 85 + 0.5)))
            tones[i] = tone
            error = work[i] - tone * 85
            for dx, dy, k in ((sign, 0, 7), (-sign, 1, 3), (0, 1, 5), (sign, 1, 1)):
                if 0 <= x + dx < W and y + dy < H:
                    work[(y + dy) * W + x + dx] += error * k / 16
    return tones


def stamp_wordmark(tones, gray):
    """The spec mockup's wordmark: the approved crop area-averaged to 86x28
    (not Frosted Home's high-quality scaling), multiplied over the frost."""
    x0, y0, w, h = WORDMARK_BOX
    for i, (total, count) in enumerate(area_average(gray, WORDMARK_CROP, w, h)):
        at = (y0 + i // w) * W + x0 + i % w
        tones[at] = (2 * tones[at] * total + 255 * count) // (510 * count)


def pack(tones):
    planes = bytearray(2 * PLANE)
    for i, tone in enumerate(tones):
        bit = 0x80 >> (i % 8)
        if tone & 1: planes[i // 8] |= bit
        if tone & 2: planes[PLANE + i // 8] |= bit
    return bytes(planes)


def backdrop():
    gray = brand_gray()
    tones = dither(frost(gray))
    stamp_wordmark(tones, gray)
    return pack(tones)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='fail unless the committed asset is reproduced')
    args = parser.parse_args()
    data = backdrop()
    digest = hashlib.sha256(data).hexdigest()
    if args.check:
        if any(not output.is_file() or output.read_bytes() != data for output in OUTPUTS):
            raise SystemExit('Stage backdrop differs from its generator; regenerate and review')
    else:
        for output in OUTPUTS:
            output.write_bytes(data)
    print(f'PASS: Stage backdrop {len(data)} bytes (two {PLANE}-byte selector planes), '
          f'SHA-256 {digest}')


if __name__ == '__main__':
    main()
