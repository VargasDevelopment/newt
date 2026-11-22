# SPDX-License-Identifier: MIT
"""Independent prefix fixtures/goldens; proposed, not executed.

MIT; see LICENSE.
Use the existing frame_prefix transport; compare whole outputs against C++.
"""
from frame_cases import FRAME_BYTES, noise, packed

_PRIOR = bytes((at * 37 + 11) & 255 for at in range(FRAME_BYTES))
_ZERO = bytes(FRAME_BYTES)
_ONES = b'\xff' * FRAME_BYTES
# Fixed geometric corners: logical pixel -> (native byte, black-bit mask).
_CORNERS = (
    ((0, 0), 52173, 0x80),
    ((527, 0), 0, 0x80),
    ((0, 791), 52271, 0x01),
    ((527, 791), 98, 0x01),
)


def corpus():
    stripes = bytes(((at // 66) * 29 ^ (at % 66) * 173) & 255
                    for at in range(FRAME_BYTES))
    patterns = (_ZERO, _ONES, stripes, noise(0x91e10da5))
    specs = set()
    for orientation in (0, 1):
        for source in patterns:
            for count in (98, 99, 100, 197, 198, 199, 1023, 1024, 1025):
                specs.add((orientation, count, source))
        for point, location, _ in _CORNERS:
            for count in (0, location, location + 1, FRAME_BYTES):
                specs.add((orientation, count, packed((point,))))
        for source in (_ZERO, _ONES):
            for count in (0, FRAME_BYTES - 1, FRAME_BYTES, FRAME_BYTES + 1):
                specs.add((orientation, count, source))
    for orientation, count, source in sorted(specs):
        yield f'frame_prefix {orientation} {count} {source.hex()} {_PRIOR.hex()}'


def assert_expectations(cases, replies):
    corners = {packed((point,)): (location, mask)
               for point, location, mask in _CORNERS}
    for command, reply in zip(cases, replies):
        fields = command.split()
        if fields[0] != 'frame_prefix':
            continue
        orientation, count = int(fields[1]), int(fields[2])
        source, prior = bytes.fromhex(fields[3]), bytes.fromhex(fields[4])
        expected = bytearray(prior)
        valid = count <= FRAME_BYTES
        if valid and orientation == 1:
            expected[:count] = bytes(byte ^ 255 for byte in source[:count])
        elif valid:
            if source == _ONES:
                expected[:count] = bytes(count)
            elif source == _ZERO or source in corners:
                expected[:count] = b'\xff' * count
                if source in corners:
                    location, mask = corners[source]
                    if location < count:
                        expected[location] &= 255 ^ mask
            else:
                # General portraits use original C++ plus the existing suffix
                # invariant; only fixed geometry is independently frozen here.
                continue
        ready, output = reply.split()
        if ready != str(int(valid)) or bytes.fromhex(output) != expected:
            raise AssertionError('independent prefix polarity/corner/atomicity mismatch')
