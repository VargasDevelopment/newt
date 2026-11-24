# SPDX-License-Identifier: MIT
"""Unexecuted frame protocol corpus and independent failure invariants.

MIT. See the repository LICENSE.
Extend the existing corpus with corpus(); compare each entire reply line from
the pinned C++ oracle and Rust host, then call assert_invariants(case, reply).
This module has no subprocesses, file writes, compiler, or test entry point.
"""

WIDTH, HEIGHT = 528, 792
NATIVE_WIDTH, NATIVE_HEIGHT = HEIGHT, WIDTH
FRAME_BYTES = WIDTH * HEIGHT // 8
PLANS = (
    ((0, 0, 792, 528),),
    ((0, 0, 8, 1), (784, 527, 8, 1)),
    ((568, 374, 8, 108), (568, 210, 8, 108), (568, 46, 8, 108)),
    ((8, 7, 24, 3), (16, 8, 8, 1)),
    ((0, 0, 8, 1), (8, 1, 0, 1)),
    ((0, 0, 8, 1), (8, 1, 8, 0)),
    ((0, 0, 8, 1), (1, 1, 8, 1)),
    ((0, 0, 8, 1), (8, 1, 7, 1)),
    ((0, 0, 8, 1), (792, 1, 8, 1)),
    ((0, 0, 8, 1), (8, 528, 8, 1)),
    ((0, 0, 8, 1), (784, 1, 16, 1)),
    ((0, 0, 8, 1), (8, 527, 8, 2)),
    ((0, 0, 8, 1), (65528, 65535, 65528, 65535)),
    ((0, 527, 792, 1), (784, 0, 8, 528)),
)


def packed(points, width=WIDTH):
    result = bytearray(FRAME_BYTES)
    for x, y in points:
        at = y * width + x
        result[at // 8] |= 0x80 >> (at % 8)
    return bytes(result)


def noise(seed):
    result = bytearray(FRAME_BYTES)
    state = seed
    for index in range(FRAME_BYTES):
        state ^= (state << 13) & 0xffffffff
        state ^= state >> 17
        state ^= (state << 5) & 0xffffffff
        result[index] = state & 255
    return bytes(result)


def corpus():
    zero, ones = bytes(FRAME_BYTES), b'\xff' * FRAME_BYTES
    ramp = bytes(((at * 173) ^ (at >> 5)) & 255 for at in range(FRAME_BYTES))
    random = noise(0x4e657774)
    checker = bytes(0xaa if (at // (WIDTH // 8)) % 2 == 0 else 0x55
                    for at in range(FRAME_BYTES))
    boundary = packed(((7, 0), (8, 0), (0, 1), (527, 1), (7, 790), (8, 791)))
    patterns = [zero, ones, ramp, random, checker, boundary]
    patterns += [packed((point,)) for point in ((0, 0), (527, 0), (0, 791), (527, 791))]

    def line(*fields):
        return ' '.join(value.hex() if isinstance(value, bytes) else str(value)
                        for value in fields)

    for source in patterns:
        for orientation in (0, 1):
            for op in ('frame_bw', 'frame_selectors'):
                yield line(op, orientation, source, ramp)

    # Native row is 99 bytes: span both row and 4096-byte loan boundaries.
    for orientation in (0, 1):
        for count in (0, 1, 2, 98, 99, 100, 4095, 4096, 4097,
                      FRAME_BYTES - 1, FRAME_BYTES, FRAME_BYTES + 1, 0xffffffff):
            yield line('frame_prefix', orientation, count, random, ramp)

    # Every invalid plan has a valid first region. Atomicity must cover a
    # rejection after that first region, not merely a bad first coordinate.
    for source in (zero, ones, random):
        for plan in range(len(PLANS)):
            yield line('frame_region', plan, source, ramp)

    patches = ((0, 0, 255), (0, 1, 128), (98, 3, 85),
               (FRAME_BYTES // 2, 23, 170), (FRAME_BYTES - 1, 1, 1),
               (0, FRAME_BYTES, 255))
    for ready in (0, 1):
        for begin, count, mask in patches:
            yield line('frame_publish', ready, begin, count, mask, random, checker)
        for action_count in (0, 1, 4096, FRAME_BYTES):
            yield line('frame_action', ready, FRAME_BYTES - 9, 9, 0x81,
                       action_count, 0x69, random, checker)

    for rect in ((0, 0, 0, 0), (WIDTH, 0, 0, HEIGHT), (0, HEIGHT, WIDTH, 0),
                 (0, 0, WIDTH, HEIGHT), (0, 0, 1, 1), (WIDTH - 1, 0, 1, 1),
                 (0, HEIGHT - 1, 1, 1), (WIDTH - 1, HEIGHT - 1, 1, 1),
                 (7, 7, 2, 2), (103, 635, 311, 9), (16, 744, 496, 4)):
        yield line('frame_rect', *rect, ramp)


def assert_invariants(case, reply):
    """Independent publication/atomicity checks in addition to full parity."""
    fields, output = case.split(), reply.split()
    op = fields[0]
    if op in ('frame_publish', 'frame_action'):
        action = op == 'frame_action'
        published = bytearray.fromhex(fields[7 if action else 5])
        stage = published.copy()
        begin, count, mask = map(int, fields[2:5])
        for at in range(begin, begin + count):
            stage[at] ^= mask
        if action:
            action_count, value = map(int, fields[5:7])
            published[:action_count] = bytes((value,)) * action_count
        ready = fields[1] == '1'
        expected = ['1' if ready else '0', (stage if ready else published).hex(), stage.hex()]
        assert output == expected, 'publication changed the wrong complete frame'
        return
    assert len(output) == 2 and len(bytes.fromhex(output[1])) == FRAME_BYTES
    native = bytes.fromhex(output[1])
    if op == 'frame_prefix':
        count = int(fields[2])
        prior = bytes.fromhex(fields[4])
        valid = count <= FRAME_BYTES
        assert output[0] == str(int(valid))
        assert native[count if valid else 0:] == prior[count if valid else 0:]
    elif op == 'frame_region':
        plan = int(fields[1])
        prior = bytes.fromhex(fields[3])
        valid = plan in (0, 1, 2, 3, 13)
        assert output[0] == str(int(valid))
        if not valid:
            assert native == prior, 'invalid later region partially mutated native frame'
            return
        inside = set()
        for x, y, width, height in PLANS[plan]:
            for row in range(y, y + height):
                inside.update(range(row * NATIVE_WIDTH // 8 + x // 8,
                                    row * NATIVE_WIDTH // 8 + (x + width) // 8))
        assert all(native[at] == prior[at] for at in range(FRAME_BYTES) if at not in inside)
    elif op == 'frame_bw':
        source = bytes.fromhex(fields[2])
        if source == bytes(FRAME_BYTES):
            assert native == b'\xff' * FRAME_BYTES
        elif source == b'\xff' * FRAME_BYTES:
            assert native == bytes(FRAME_BYTES)
        # Independently fixed portrait corner goldens (native x,y), MSB first.
        corners = (((0, 0), (0, 527)), ((527, 0), (0, 0)),
                   ((0, 791), (791, 527)), ((527, 791), (791, 0)))
        for source_point, native_point in corners:
            if fields[1] == '0' and source == packed((source_point,)):
                expected = bytes(value ^ 255 for value in packed((native_point,), NATIVE_WIDTH))
                assert native == expected


# Apply each independently to a disposable integrated workspace. A control
# must compile and then fail parity/invariants; compilation failure is no pass.
CONTROLS = (
    # Both landscapes share landscape(); each caller passes its ink, so these
    # mutate the full-frame polarity and the selector copy at their callers.
    ('BW landscape polarity', 'core/src/frame.rs',
     'if landscape(logical, native, orientation, |byte| !byte) {',
     'if landscape(logical, native, orientation, |byte| byte) {'),
    ('landscape gray selectors retained', 'core/src/frame.rs',
     'if landscape(logical, native, orientation, |byte| byte) {',
     'if landscape(logical, native, orientation, |_| 0) {'),
    ('complete prefix accepted', 'core/src/frame.rs',
     'if bytes > FRAME_BYTES {', 'if bytes >= FRAME_BYTES {'),
    ('invalid region atomicity', 'core/src/frame.rs',
     '|| height > NATIVE_HEIGHT - y\n        {\n            return false;',
     '|| height > NATIVE_HEIGHT - y\n        {\n            native[0] ^= 255;\n            return false;'),
    ('last region row restored', 'core/src/frame.rs',
     'for row in y..y + height {', 'for row in y..y + height - 1 {'),
    ('failed staged render unpublished', 'core/src/frame.rs',
     'if !render(staging) {\n        return false;',
     'if !render(staging) {\n        published.copy_from_slice(staging);\n        return false;'),
    ('action staging seeded before scratch use', 'core/src/frame.rs',
     'staging.copy_from_slice(published);\n    if !render(staging, published)',
     'staging.fill(0);\n    if !render(staging, published)'),
)
