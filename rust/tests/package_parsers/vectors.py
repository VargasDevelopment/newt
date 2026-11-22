"""Authored XML and structured expectations, independent of either parser."""
def h(value):
    return value.encode().hex() or '-'
def request(kind, xml, directory='', short=1048576, fail=-1, cap=1048576, buffer=1):
    return f'P {kind} {h(xml)} {h(directory)} {short} {fail} {cap} {buffer}\n'
def trace(xml, passes, short=1048576, fail=-1, buffer=1):
    rows=[]; call=0
    if not buffer:
        return rows
    size=len(xml.encode())
    for _ in range(passes):
        at=0
        while at<size:
            want=min(size-at,2048)
            n=-1 if call==fail else min(want,short)
            rows.append(f'R {at} {want} {n}'); call+=1
            if n<0:
                return rows
            at+=n
    return rows
VECTORS=[]
def case(name, kind, xml, rows=(), status='Ok', passes=2, **kw):
    wire=request(kind,xml,**kw)
    reads=trace(xml,passes,kw.get('short',1048576),kw.get('fail',-1),kw.get('buffer',1))
    VECTORS.append(dict(id=name,wire=wire,expected='\n'.join([*reads,'P '+status,*rows,'END'])+'\n'))
case('container-first-local','C','<c><rootfile media-type="wrong" full-path="bad"/><x:rootfile xmlns:x="u" x:full-path="OPS/a.opf"/><rootfile full-path="later"/></c>', ['C '+h('OPS/a.opf')],passes=1)
case('container-empty-first','C','<c><rootfile full-path=""/><rootfile full-path="later"/></c>',['C -'],passes=1)
case('container-missing','C','<c><rootfile media-type="wrong" full-path="bad"/></c>',status='NotEpub',passes=1)
case('container-oom','C','<rootfile full-path="x"/>',status='NotEpub',passes=1,cap=0)
case('malformed','C','<rootfile full-path="x"></bad>',status='ParseError',passes=1)
case('missing-buffer','C','<rootfile full-path="x"/>',status='OutOfMemory',passes=0,buffer=0)
case('io-precedence','C','<rootfile full-path="x"/>',status='IoError',passes=1,fail=0)
case('nav-exact','N','<nav type="toc"><a href="bad">bad</a></nav>')
case('nav-fragment','N','<nav epub:type="toc"><ol><li><a href="../a%20b#x"> A <b>B</b> C </a></li></ol></nav>', ['T '+h('A B C')+' '+h('a b')+' '+h('x')+' 0'],directory='OPS')
case('nav-nested-anchor','N','<nav epub:type="toc"><ol><a href="outer">outer<a href="inner">inner</a>tail</a></ol></nav>', ['T '+h('inner')+' '+h('inner')+' ~ 0'])
case('nav-reopen-failure','N','<nav epub:type="toc"><a href="a">A</a></nav>',status='IoError',fail=1)
case('ncx-missing-src','X','<ncx><navPoint><content/></navPoint></ncx>')
case('ncx-nested-label','X','<ncx><navPoint><navLabel><text>Outer</text></navLabel><navPoint><navLabel><text>Inner</text></navLabel><content src="i#z"/></navPoint><content src="o"/></navPoint></ncx>', ['T '+h('Inner')+' '+h('i')+' '+h('z')+' 1','T '+h('Inner')+' '+h('o')+' ~ 0'])
case('ncx-content-outside','X','<ncx><content src="bad"/><navPoint><content src="good"/></navPoint></ncx>',['T - '+h('good')+' ~ 0'])
# FNV values are independent literal known expectations, not imported from Rust.
case('package-fields','P','<package><metadata><title> </title><title>First</title><title>Second</title><creator>A</creator><language>en</language><identifier>ID</identifier><meta name="cover" content="a"/></metadata><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="n" href="n.ncx" media-type="application/x-dtbncx+xml"/><item href="invalid"/></manifest><spine toc="n"><itemref/><itemref idref="missing"/><itemref idref="a"/></spine></package>', ['M '+h('First')+' '+h('A')+' '+h('en')+' '+h('ID'), 'I '+h('a')+' '+h('OPS/a.xhtml')+' '+h('application/xhtml+xml')+' 3826002220 1 1', 'I '+h('n')+' '+h('OPS/n.ncx')+' '+h('application/x-dtbncx+xml')+' 3943445553 0 0','S 0','D 0 1'],directory='OPS')
case('package-missing-counts','P','<package><manifest/><spine/></package>',status='NotEpub',passes=1)
case('package-unknown-spine','P','<p><manifest><item id="a" href="a"/></manifest><spine><itemref idref="unknown"/></spine></p>',status='NotEpub')
case('package-oom','P','<p><metadata><title>A</title></metadata><manifest><item id="a" href="a"/></manifest><spine><itemref idref="a"/></spine></p>',status='OutOfMemory',passes=1,cap=0)
case('short-read','N','<nav epub:type="toc"><a href="a">A</a></nav>',['T '+h('A')+' '+h('a')+' ~ 0'],short=3)
case('text-cap','X','<n><navPoint><navLabel><text>'+'x'*260+'</text></navLabel><content src="a"/></navPoint></n>',['T '+h('x'*255)+' '+h('a')+' ~ 0'])
case('bounded-depth','N','<nav epub:type="toc">'+'<ol>'*257+'<a href="a">A</a>'+'</ol>'*257+'</nav>',['T '+h('A')+' '+h('a')+' ~ 254'])
# Nested metadata overwrites the capture kind but leaves capture active.
case('metadata-nesting','P','<p><metadata><title>Lost<b>suffix</b></title></metadata><manifest><item id="a" href="a"/></manifest><spine><itemref idref="a"/></spine></p>', ['M - - - -','I '+h('a')+' '+h('a')+' - 3826002220 0 0','S 0','D -1 -1'])
case('ncx-toc-id-fallback','P','<p><manifest><item id="a" href="a"/><item id="n" href="n"/></manifest><spine toc="n"><itemref idref="a"/></spine></p>', ['M - - - -','I '+h('a')+' '+h('a')+' - 3826002220 0 0','I '+h('n')+' '+h('n')+' - 3943445553 0 0','S 0','D -1 1'])
# Observe the 12-entry interning boundary through exhaustion in focused core tests;
# ordinary wire results deliberately do not expose implementation allocation totals.
case('empty-xml','C','',status='ParseError',passes=1)
case('ncx-saturating-depth','X','<n>'+'<navPoint>'*257+'<content src="a"/>'+'</navPoint>'*257+'</n>',['T - '+h('a')+' ~ 254'])
case('strict-html-entity','N','<nav epub:type="toc"><a href="a">&nbsp;</a></nav>',status='ParseError',passes=1)

# Separate resource-domain expectations: C++ charges arrays before reopening;
# Rust caller-provided arrays are already admitted, so its string attempt is later.
def resource_case(name, kind, xml, cpp_status='OutOfMemory', rust_status='OutOfMemory', fail=-1):
    wire=request(kind,xml,cap=0,fail=fail)
    cpp='\n'.join([*trace(xml,1),'P '+cpp_status,'END'])+'\n'
    rust='\n'.join([*trace(xml,2,fail=fail),'P '+rust_status,'END'])+'\n'
    VECTORS.append(dict(id=name,wire=wire,expected=cpp,rust_expected=rust))
resource_case('nav-fill-string-oom','N','<nav epub:type="toc"><a href="a">A</a></nav>')
resource_case('ncx-fill-string-oom','X','<n><navPoint><content src="a"/></navPoint></n>')
resource_case('package-fill-string-oom','P','<p><manifest><item id="a" href="a"/></manifest><spine><itemref idref="a"/></spine></p>')
resource_case('fill-io-precedes-string-oom','N','<nav epub:type="toc"><a href="a">A</a></nav>',rust_status='IoError',fail=1)


# The same literal semantic expectations through actual local ZIP entries.
# Source reads follow the bounded header/input extents, independently of either
# executable's traces. Small compressed fixtures fit one 2048-byte input fill.
import struct
import zlib

def local_request(kind, blob, directory, method, usize, fail=-1, buffer=1, decoder=1):
    return f'Z {kind} {blob.hex() or "-"} {directory} 1048576 {fail} 1048576 {buffer} {method} {usize} {decoder}\n'

def local_header(payload, raw, method):
    return struct.pack('<IHHHHHIIIHH', 0x04034b50, 20, 0, method, 0, 0,
                       zlib.crc32(raw), len(payload), len(raw), 0, 0)

for original in list(VECTORS):
    fields = original['wire'].split()
    if fields[5] != '-1' or fields[6] != '1048576' or fields[7] != '1':
        continue
    raw = b'' if fields[2] == '-' else bytes.fromhex(fields[2])
    passes = sum(row.startswith('R 0 ') for row in original['expected'].splitlines()) or 1
    semantic = [row for row in original['expected'].splitlines() if not row.startswith('R ')]
    for label, strategy in [('stored', None), ('fixed', zlib.Z_FIXED), ('default', zlib.Z_DEFAULT_STRATEGY)]:
        method = 0 if strategy is None else 8
        if method == 0:
            payload = raw
        else:
            compressor = zlib.compressobj(6, zlib.DEFLATED, -15, strategy=strategy)
            payload = compressor.compress(raw) + compressor.flush()
            assert len(payload) <= 2048
        blob = local_header(payload, raw, method) + payload
        reads = []
        for _ in range(passes):
            reads.append('R 0 30 30')
            if method == 8:
                # Even an empty DEFLATE stream must consume its end block.
                reads.append(f'R 30 {len(payload)} {len(payload)}')
            else:
                for at in range(0, len(raw), 2048):
                    n = min(2048, len(raw)-at)
                    reads.append(f'R {30+at} {n} {n}')
        VECTORS.append(dict(id=original['id']+'-local-'+label,
            wire=local_request(fields[1], blob, fields[3], method, len(raw)),
            expected='\n'.join(reads+semantic)+'\n'))

sample = b'<rootfile full-path="a.opf"/>'
compressor = zlib.compressobj(6, zlib.DEFLATED, -15, strategy=zlib.Z_FIXED)
payload = compressor.compress(sample)+compressor.flush()
blob = local_header(payload, sample, 8)+payload
for name, data, method, usize, fail, buffer, decoder, rows, status in [
    ('local-not-zip', b'BAD!'+bytes(26)+sample, 0, len(sample), -1, 1, 1, ['R 0 30 30'], 'NotZip'),
    ('local-size-mismatch', local_header(sample,sample,0)+sample, 0, len(sample)+1, -1, 1, 1, ['R 0 30 30'], 'Truncated'),
    ('local-unknown-method', local_header(sample,sample,0)+sample, 99, len(sample), -1, 1, 1, ['R 0 30 30'], 'Unsupported'),
    ('local-deflate-bad-block', local_header(b'\x07',sample,8)+b'\x07', 8, len(sample), -1, 1, 1, ['R 0 30 30','R 30 1 1'], 'IoError'),
    ('local-decoder-unavailable', blob, 8, len(sample), -1, 1, 0, ['R 0 30 30'], 'OutOfMemory'),
    ('local-buffer-unavailable', blob, 8, len(sample), -1, 0, 1, ['R 0 30 30'], 'OutOfMemory'),
    ('local-header-io', blob, 8, len(sample), 0, 1, 1, ['R 0 30 -1'], 'IoError'),
    ('local-input-io', blob, 8, len(sample), 1, 1, 1, ['R 0 30 30',f'R 30 {len(payload)} -1'], 'IoError'),
]:
    VECTORS.append(dict(id=name, wire=local_request('C',data,'-',method,usize,fail,buffer,decoder),
                        expected='\n'.join([*rows,'P '+status,'END'])+'\n'))

# A zero-entry TOC still performs fill when no book backing is available.
case('nav-empty-zero-backing','N','<nav epub:type="toc"/>',cap=0)
case('nav-empty-fill-read-failure','N','<nav epub:type="toc"/>',status='IoError',fail=1,cap=0)
case('ncx-empty-zero-backing','X','<ncx/>',cap=0)
case('ncx-empty-fill-read-failure','X','<ncx/>',status='IoError',fail=1,cap=0)
