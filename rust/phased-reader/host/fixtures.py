"""Handwritten heterogeneous EPUB inputs. Parent may invoke write(root).
No dependency acquisition, generated expectations or book-identity exceptions.
"""
from pathlib import Path
from zipfile import ZipFile, ZIP_STORED, ZIP_DEFLATED

CONTAINER = b'<container><rootfiles><rootfile full-path="OPS/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>'


def write(root: Path):
    root.mkdir(parents=True, exist_ok=True)
    write_menus(root)
    # Generic structures, never production title/filename identities.
    cases = [
        ("cross-spine", ZIP_STORED, b"<html><body><p id='start'>First chapter.</p></body></html>", None),
        ("plain", ZIP_STORED, b"<html><body><p id='start'>One two three.</p></body></html>", None),
        ("inline", ZIP_DEFLATED, b"<html><head><style>p { text-indent: 10%; font-weight: bold }</style></head><body><p id='start'>Alpha &amp; beta.</p><p>Gamma.</p></body></html>", None),
        ("linked", ZIP_STORED, b"<html><head><link rel='stylesheet' href='theme.css'/></head><body><p id='start'>A long paragraph. " + b"Words and spaces. " * 5000 + b"</p></body></html>", b"p { text-align: left; font-style: italic; }"),
        ("duplicate-id", ZIP_DEFLATED, b"<html><body><p id='start'>First.</p><p id='start'>Second.</p></body></html>", None),
        ("empty", ZIP_STORED, b"<html><body></body></html>", None),
        ("late-malformed", ZIP_DEFLATED, b"<html><body><p>" + b"Many words. " * 5000 + b"</wrong></body></html>", None),
    ]
    cases.extend([
        ("deferred", ZIP_STORED, b"<html><head>" + b" " * 17000 + b"</head><body><p id='start'>Deferred head.</p></body></html>", None),
        ("pending", ZIP_STORED, b"<html><head><style>p { font-weight: bold }</style></head><body>" + b"<div>" * 20 + b"<p id='start'>Nested.</p>" + b"</div>" * 20 + b"</body></html>", None),
        ("optional-malformed", ZIP_STORED, b"<html><body><p id='start'>Still readable.</p></body></html>", None),
        ("optional-live-fault", ZIP_STORED, b"<html><body><p id='start'>Live fault.</p></body></html>", None),
        ("prefix-anchor", ZIP_STORED, b"<html><body><p id='start'>" + b"Many words. " * 8000 + b"SUFFIX_FAULT" + b" suffix" * 1000 + b"</p></body></html>", None),
        # Known FNV-1a32 collision: costarring / liquid.
        ("collision", ZIP_STORED, b"<html><body><p id='costarring'>First.</p><p id='liquid'>Second.</p></body></html>", None),
    ])
    for name, compression, chapter, css in cases:
        manifest = '<item id="c" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="n" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>'
        if css is not None:
            manifest += '<item id="s" href="theme.css" media-type="text/css"/>'
        second = name == "cross-spine"
        if second:
            manifest += '<item id="d" href="second.xhtml" media-type="application/xhtml+xml"/>'
        spine = '<itemref idref="c"/>' + ('<itemref idref="d"/>' if second else '')
        opf = ('<package><metadata><title>Fixture</title></metadata><manifest>' + manifest + '</manifest><spine>' + spine + '</spine></package>').encode()
        nav = b'<html xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="chapter.xhtml#start">Start</a></li></ol></nav></body></html>'
        if second:
            nav = nav.replace(b"</ol>", b'<li><a href="second.xhtml#start">Second</a></li></ol>')
        if name == "optional-malformed":
            nav = nav.replace(b"</nav>", b"</wrong>")
        if name == "collision":
            nav = nav.replace(b"#start", b"#costarring")
        with ZipFile(root / (name + '.epub'), 'w', compression=compression) as z:
            z.writestr('mimetype', b'application/epub+zip', compress_type=ZIP_STORED)
            z.writestr('META-INF/container.xml', CONTAINER)
            z.writestr('OPS/book.opf', opf)
            z.writestr('OPS/nav.xhtml', nav)
            z.writestr('OPS/chapter.xhtml', chapter)
            if second:
                z.writestr('OPS/second.xhtml', b"<html><body><p id='start'>Second chapter.</p></body></html>")
            if css is not None:
                z.writestr('OPS/theme.css', css)


def write_languages(root: Path):
    root.mkdir(parents=True, exist_ok=True)
    for name, tag in (
        ("en", "en"), ("ja", " JA_jp "), ("zh", "zh-Hant-TW"), ("ko", "KO"),
        ("missing", None), ("invalid", "ja--JP"), ("empty", ""),
        ("unknown", "jargon"), ("overlong", "ja" + " " * 260 + "junk"),
    ):
        language = f"<dc:language>{tag}</dc:language>" if tag is not None else ""
        opf = ('<package xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata>'
               + language + '</metadata><manifest><item id="c" href="chapter.xhtml" '
               'media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/>'
               '</spine></package>')
        with ZipFile(root / f"language-{name}.epub", "w") as z:
            z.writestr("mimetype", "application/epub+zip")
            z.writestr("META-INF/container.xml", CONTAINER)
            z.writestr("OPS/book.opf", opf)
            # U+00A7 has UAX #14 class AI, resolved differently for CJK.
            z.writestr("OPS/chapter.xhtml", "<html><body><p>" + "§" * 80 + "</p></body></html>")


def write_menus(root: Path):
    root.mkdir(parents=True, exist_ok=True)
    for name, order, kind, compression in (
        ("menu", (0, 1, 2, 3, 4, 5, 6), "nav", ZIP_STORED),
        ("menu-ncx", (0, 1, 2, 3, 4, 5, 6), "ncx", ZIP_DEFLATED),
        ("menu-unordered", (3, 1, 5), "nav", ZIP_DEFLATED),
        ("menu-malformed", (0, 1, 2, 3, 4, 5, 6), "malformed", ZIP_STORED),
        ("menu-missing", (), "missing", ZIP_STORED),
        ("menu-fragments", (0, 0, 2), "fragments", ZIP_DEFLATED),
    ):
        manifest = ''.join(
            f'<item id="c{i}" href="c{i}.xhtml" media-type="application/xhtml+xml"/>'
            for i in range(7)
        )
        ncx = kind == "ncx"
        filename = 'nav.ncx' if ncx else 'nav.xhtml'
        manifest += (
            f'<item id="n" href="{filename}" media-type="application/x-dtbncx+xml"/>'
            if ncx else
            '<item id="n" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>'
        )
        spine = ''.join(f'<itemref idref="c{i}"/>' for i in range(7))
        opf = ('<package><metadata><title>Menu fixture</title></metadata><manifest>' + manifest
               + '</manifest><spine' + (' toc="n"' if ncx else '') + '>' + spine + '</spine></package>')
        rows = []
        for toc, spine_index in enumerate(order):
            fragment = ('#start' if toc != 1 else '#later') if kind == "fragments" else ''
            href = f'c{spine_index}.xhtml{fragment}'
            if ncx:
                rows.append(f'<navPoint id="p{toc}" playOrder="{toc + 1}"><navLabel><text>A{toc}</text></navLabel><content src="{href}"/></navPoint>')
            else:
                rows.append(f'<li><a href="{href}">A{toc}</a></li>')
        navigation = ('<ncx><navMap>' + ''.join(rows) + '</navMap></ncx>' if ncx else
                      '<html xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol>' + ''.join(rows) + '</ol></nav></body></html>')
        if kind == "malformed":
            navigation = navigation.replace('</nav>', '</wrong>')
        with ZipFile(root / f'{name}.epub', 'w', compression=compression) as z:
            z.writestr('mimetype', b'application/epub+zip', compress_type=ZIP_STORED)
            z.writestr('META-INF/container.xml', CONTAINER)
            z.writestr('OPS/book.opf', opf.encode())
            if kind != "missing":
                z.writestr('OPS/' + filename, navigation.encode())
            for i in range(7):
                z.writestr(f'OPS/c{i}.xhtml', b'<html><body><p id="start">Alpha.</p><p id="later">Beta.</p></body></html>')
