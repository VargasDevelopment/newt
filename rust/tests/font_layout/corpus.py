"""Independent synthetic OpenType builders; no acquired font data or oracle-derived expectations."""
import struct


def h(*values):
    return struct.pack('>' + 'H' * len(values), *values)


def coverage(glyph=10, fmt=1, index=0):
    return h(1, 1, glyph) if fmt == 1 else h(2, 1, glyph, glyph, index)


def value_record(fmt, advance):
    return b''.join(h(advance & 65535 if bit == 2 else 0x1234) for bit in range(16) if fmt & (1 << bit))


def pair1(value=-37, covfmt=1, vf1=4, vf2=0, right=20, duplicates=False):
    cov = coverage(fmt=covfmt)
    record = h(right) + value_record(vf1, value) + value_record(vf2, 213)
    records = record + (h(right) + value_record(vf1, -99) + value_record(vf2, 0) if duplicates else b'')
    pairs = h(2 if duplicates else 1) + records
    return h(1, 12, vf1, vf2, 1, 12 + len(cov)) + cov + pairs


def pair2(value=-61, classfmt=1, covfmt=2, vf1=4, vf2=0):
    cov = coverage(fmt=covfmt)
    c1 = h(1, 10, 1, 1) if classfmt == 1 else h(2, 1, 10, 10, 1)
    c2 = h(1, 20, 1, 1) if classfmt == 1 else h(2, 1, 20, 20, 1)
    matrix = b''.join(value_record(vf1, value if i == 3 else 0) + value_record(vf2, 213) for i in range(4))
    return h(2, 16 + len(matrix), vf1, vf2, 16 + len(matrix) + len(cov),
             16 + len(matrix) + len(cov) + len(c1), 2, 2) + matrix + cov + c1 + c2


def ligature(result=77, sequence=(10, 20), covfmt=1):
    cov = coverage(sequence[0], covfmt)
    return h(1, 8, 1, 8 + len(cov)) + cov + h(1, 4) + h(result, len(sequence), *sequence[1:])


def lookup(kind, subs, extension=False, far=False):
    if extension:
        pad = 70000 if far else 8
        subs = [h(1, kind) + struct.pack('>I', pad) + bytes(pad - 8) + s for s in subs]
        kind = 9 if kind == 2 else 7
    offsets, tail = [], b''
    for sub in subs:
        offsets.append(6 + 2 * len(subs) + len(tail)); tail += sub
    return h(kind, 0, len(subs), *offsets) + tail


def table(kind, subs, tag=None, extension=False, far=False, required=False,
          script=b'latn', inactive=False, late=0, high_lookup=0, scripts=None,
          features_extra=(), active=None, required_index=None):
    tag = tag or (b'kern' if kind == 2 else b'liga')
    body = lookup(kind, subs, extension, far)
    # Holes are absent records, not replicated table bytes.
    lookups = h(high_lookup + 1, *([0] * high_lookup), 2 + 2 * (high_lookup + 1)) + body
    records = [(b'zzzz', b'')] * late + [(tag, h(0, 1, high_lookup))] + list(features_extra)
    data, feature_records = b'', b''
    for name, feature in records:
        off = 2 + 6 * len(records) + len(data) if feature else 0
        feature_records += name + h(off); data += feature
    features = h(len(records)) + feature_records + data
    if active is None: active = [] if inactive or required else [late]
    required_index = late if required else 65535 if required_index is None else required_index
    lang = h(0, required_index, len(active), *active)
    script_body = h(4, 0) + lang
    if scripts is None: scripts = [(script, script_body)]
    data, script_records = b'', b''
    for name, body in sorted(scripts):
        script_records += name + h(2 + 6 * len(scripts) + len(data) if body else 0); data += body
    scripts_blob = h(len(scripts)) + script_records + data
    blobs = {'s': scripts_blob, 'f': features, 'l': lookups}
    order = ['s', 'l', 'f'] if len(features) > 40000 else ['s', 'f', 'l']
    offsets, data = {}, b''
    for name in order: offsets[name] = 10 + len(data); data += blobs[name]
    return h(1, 0, offsets['s'], offsets['f'], offsets['l']) + data


def corpus():
    cases = []
    def add(name, raw, expected, mode='P', glyphs=(10, 20)):
        cases.append(dict(name=name, input=f'{mode}\n{raw.hex() or "-"}\n{" ".join(map(str, glyphs))}\n',
                          expected=f'R {expected}\n'))
    add('pair-list', table(2, [pair1()]), -37)
    add('pair-coverage2', table(2, [pair1(covfmt=2)]), -37)
    add('pair-duplicate-first', table(2, [pair1(duplicates=True)]), -37)
    for c in (1, 2):
        add(f'matrix-class{c}', table(2, [pair2(classfmt=c)]), -61)
    for vf1, vf2 in [(7, 65535), (65535, 65535), (0x8004, 0x8000), (6, 1), (5, 2)]:
        add(f'widths-{vf1}-{vf2}', table(2, [pair1(vf1=vf1, vf2=vf2)]), -37)
        add(f'matrix-widths-{vf1}-{vf2}', table(2, [pair2(vf1=vf1, vf2=vf2)]), -61)
    # Distinct records make a wrong second-record width observable.
    cov = coverage()
    records = b''.join(h(g) + value_record(7, v) + value_record(65535, 213)
                       for g, v in [(19, -90), (20, -37)])
    sub = h(1, 12, 7, 65535, 1, 18) + cov + h(2) + records
    add('widths-multi', table(2, [sub]), -37)
    add('no-xadvance', table(2, [pair1(vf1=3)]), 0)
    add('first-zero', table(2, [pair1(value=0), pair1()]), 0)
    add('dense-zero', table(2, [pair2(value=0), pair1()]), 0)
    for kind in (2, 4):
        sub = pair1() if kind == 2 else ligature()
        mode = 'P' if kind == 2 else 'S'; expected = -37 if kind == 2 else 77
        for name, kwargs in [('required', {'required': True}), ('fallback', {'script': b'DFLT'}),
                             ('extension', {'extension': True}), ('extension32', {'extension': True, 'far': True}),
                             ('late-feature', {'late': 9000}), ('late-lookup', {'high_lookup': 9000})]:
            add(f'{mode}-{name}', table(kind, [sub], **kwargs), expected, mode)
        for name, kwargs in [('inactive', {'inactive': True}), ('unrelated-script', {'script': b'cyrl'})]:
            add(f'{mode}-{name}', table(kind, [sub], **kwargs), 0, mode)
        latin = h(4, 0) + h(0, 65535, 0)
        fallback = h(4, 0) + h(0, 65535, 1, 0)
        add(f'{mode}-latin-suppresses-fallback', table(kind, [sub], scripts=[(b'DFLT', fallback), (b'latn', latin)]), 0, mode)
        add(f'{mode}-bad-latin-fallback', table(kind, [sub], scripts=[(b'DFLT', fallback), (b'latn', h(0, 0))]), expected, mode)
        records = [(i.to_bytes(4, 'big'), b'') for i in range(5000)] + [(b'latn', fallback)]
        add(f'{mode}-high-script', table(kind, [sub], scripts=records), expected, mode)
    add('rlig', table(4, [ligature()], tag=b'rlig'), 77, 'S')
    add('ligature-three', table(4, [ligature(88, (10,20,30), 2)]), 88, 'S', (10,20,30))
    add('ligature-zero-stops-feature', table(4, [ligature(0), ligature(99)]), 0, 'S')
    # A matched zero ends this feature, but does not end the outer GSUB feature walk.
    scripts_blob = h(1) + b'latn' + h(8) + h(4, 0) + h(0, 65535, 2, 0, 1)
    features_blob = h(2) + b'liga' + h(14) + b'rlig' + h(20) + h(0, 1, 0) + h(0, 1, 1)
    first = lookup(4, [ligature(0), ligature(99)])
    second = lookup(4, [ligature(78)])
    lookups_blob = h(2, 6, 6 + len(first)) + first + second
    raw = h(1, 0, 10, 10 + len(scripts_blob), 10 + len(scripts_blob) + len(features_blob)) + scripts_blob + features_blob + lookups_blob
    add('ligature-zero-next-feature', raw, 78, 'S')

    for name, seq in [('wide-first', (65546,20)), ('wide-second', (10,65556)), ('max-u32', (4294967295,20)), ('zero', (0,20))]:
        add(name, table(2, [pair1()]), 0, glyphs=seq)
        add('S-'+name, table(4, [ligature()]), 0, 'S', seq)
    add('ligature-one', table(4, [ligature()]), 0, 'S', (10,))
    add('ligature-four', table(4, [ligature()]), 0, 'S', (10,20,30,40))
    # Full 16-bit sorted pair-set index: binary search must not truncate to a low cap.
    cov = coverage(); pairs = h(65535) + b''.join(h(i, (-17 if i == 65534 else 0) & 65535) for i in range(65535))
    sub = h(1, 12, 4, 0, 1, 18) + cov + pairs
    add('high-pair-index', table(2, [sub]), -17, glyphs=(10,65534))
    # Cap is applied to the count before bounds checking; later entries are ignored.
    cov = h(1, 8193) + b''.join(h(i) for i in range(8193))
    # PairPos2 needs only coverage membership, so uncapping cannot be masked by
    # an unrelated coverage-indexed pair-set bound. Both classes default to 0.
    sub = h(2, 18, 4, 0, 0, 0, 1, 1) + h((-37)&65535) + cov
    add('coverage-cap', table(2, [sub]), 0, glyphs=(8192,20))
    add('coverage-cap-included', table(2, [sub]), -37, glyphs=(8191,20))
    # Work exhausted by 4096 lookup references before reaching the useful one.
    add('work-cap', table(2, [pair1()], features_extra=[(b'kern',h(0,4097,*([65535]*4096),0))], active=[1]), 0)
    base = table(2, [pair1()])
    for n in range(len(base)):
        # Truncation before the final xAdvance cannot produce a complete pair record.
        add(f'truncated-{n}', base[:n], 0)
    for field in (4,6,8):
        broken = bytearray(base); broken[field:field+2] = h(0)
        add(f'null-root-{field}', bytes(broken), 0)
    # Coverage cap includes index 8191; the coverage-indexed set list is uncapped.
    cov = h(1, 8192) + b''.join(h(i) for i in range(8192))
    set_offsets = [0] * 8191 + [6 + 8192 * 2 + len(cov)]
    sub = h(1, 6 + 8192 * 2, 8192, *set_offsets) + cov + h(1, 4) + h(79, 2, 20)
    add('coverage-boundary', table(4, [sub]), 79, 'S', (8191,20))
    add('empty', b'', 0)
    add('all-ffff', bytes([255])*64, 0)
    # Dense matrix dimensions overflow 32-bit size_t; target policy rejects it.
    huge = h(2, 16, 65535, 65535, 0, 0, 65535, 65535) + coverage()
    add('matrix-rv32-overflow', table(2, [huge]), 0)
    for off in (0, 0xffffffff):
        wrapped = h(1, 2) + struct.pack('>I', off)
        add(f'extension-offset-{off}', table(9, [wrapped], tag=b'kern'), 0)
    add('extension-offset-valid', table(9, [h(1, 2) + struct.pack('>I', 8) + pair1()], tag=b'kern'), -37)
    # Format 2 may return a coverage index larger than a 16-bit set count.
    sub = h(1, 12, 4, 0, 1, 22) + h(2, 1, 1, 65535, 65535) + h(1,20,65500)
    add('coverage-index-wide', table(2, [sub]), 0, glyphs=(65535,20))

    return cases
