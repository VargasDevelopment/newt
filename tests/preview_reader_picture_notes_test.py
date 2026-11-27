#!/usr/bin/env python3
"""Host preview golden and pixel checks for #179: say where a picture is skipped.

The reader does not draw pictures inside chapters yet. Where it skips one, it
shows the picture's alt text in italics ("[Picture: ...]"), or "[Picture]" when
there is none. alt="" marks a decorative picture, which shows nothing.
Rendered through the real host-built firmware (`newt-preview`) and the SD book
path, which never finds picture sizes.
"""
import hashlib
import os
from pathlib import Path
import struct
import sys
import tempfile
import zipfile
import zlib

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'tools'), str(ROOT / 'rust/host/preview')]
import preview_driver  # noqa: E402
import rust_preview  # noqa: E402

CONFIRM = 1

# Visually inspected real host-preview PBM hash for the synthetic CC0 EPUB below.
GOLDEN = '8ed50d83274e145d4033f268f2e2b2414b984fd7e64ae9dfabf87f4664c7697f'

PICTURES = (
    '<p>The keeper climbed the stairs at dusk.</p>\n'
    '<p><img src="lamp.png" alt="A lighthouse lamp &amp; its lens"/></p>\n'
    '<p>One plain picture follows, between two decorative ones.</p>\n'
    '<img src="lamp.png" alt=""/><img src="lamp.png"/><img src="lamp.png" alt=""/>\n'
    '<p><a href="#chart"><img src="lamp.png" alt="Harbor chart"/></a></p>\n'
    '<figure><img src="lamp.png" alt="The north pier"/>'
    '<figcaption>Figure 1. The pier at low tide.</figcaption></figure>\n'
    '<p id="chart">The lamp was lit by nine.</p>\n'
)


def png(width: int, height: int) -> bytes:
    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    rows = b''.join(b'\x00' + b'\xff' * width for _ in range(height))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 0, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))


def make_epub(path: Path, body: str) -> None:
    """Deterministic CC0 synthetic EPUB with one chapter holding `body`."""
    with zipfile.ZipFile(path, 'w') as zf:
        zf.writestr('mimetype', 'application/epub+zip', compress_type=zipfile.ZIP_STORED)
        zf.writestr(
            'META-INF/container.xml',
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">'
            '<rootfiles><rootfile full-path="OEBPS/content.opf" '
            'media-type="application/oebps-package+xml"/></rootfiles></container>\n',
        )
        zf.writestr('OEBPS/lamp.png', png(240, 160))
        zf.writestr(
            'OEBPS/ch1.xhtml',
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en">'
            f'<head><title>Lamp</title></head><body>{body}</body></html>\n',
        )
        zf.writestr(
            'OEBPS/nav.xhtml',
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">'
            '<head><title>Contents</title></head><body><nav epub:type="toc"><ol>'
            '<li><a href="ch1.xhtml">Lamp</a></li></ol></nav></body></html>\n',
        )
        zf.writestr(
            'OEBPS/content.opf',
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<package xmlns="http://www.idpf.org/opf" version="3.0" unique-identifier="id">'
            '<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">'
            '<dc:identifier id="id">urn:newt-test:picture-notes-cc0</dc:identifier>'
            '<dc:title>Lamp Notes</dc:title><dc:language>en</dc:language></metadata>'
            '<manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>'
            '<item id="lamp" href="lamp.png" media-type="image/png"/>'
            '<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/></manifest>'
            '<spine><itemref idref="c1"/></spine></package>\n',
        )
    os.utime(path, (1790812800, 1790812800))


def first_page(binary: Path, body: str) -> bytes:
    with tempfile.TemporaryDirectory(prefix='newt-picture-notes-') as scratch:
        directory = Path(scratch)
        preview_driver.first_boots(binary, directory, [preview_driver.SKIP_QUICK_START])
        books = directory / 'card/Newt/Books'
        books.mkdir(parents=True, exist_ok=True)
        make_epub(books / '01-lamp.epub', body)
        preview_driver.fresh(directory)
        preview_driver.play(binary, directory, [CONFIRM, CONFIRM])
        return (directory / 'frame.pbm').read_bytes()


def main():
    binary = rust_preview.FIRMWARE if len(sys.argv) == 2 else rust_preview.build()
    pictures = first_page(binary, PICTURES)
    got = hashlib.sha256(pictures).hexdigest()
    assert got == GOLDEN, f'picture notes golden mismatch: got {got}, want {GOLDEN}'
    # Controls: the notes must show (removing the pictures changes the page),
    # and the alt text must reach layout (dropping every alt changes it too).
    no_pictures = PICTURES
    for _ in range(PICTURES.count("<img")):
        at = no_pictures.index('<img')
        no_pictures = no_pictures[:at] + no_pictures[no_pictures.index('/>', at) + 2:]
    assert first_page(binary, no_pictures) != pictures, 'skipped pictures leave no note'
    no_alt = PICTURES.replace(' alt="A lighthouse lamp &amp; its lens"', '').replace(' alt="Harbor chart"', '')
    no_alt = no_alt.replace(' alt="The north pier"', '')
    assert first_page(binary, no_alt) != pictures, 'alt text does not reach the page'
    print('PASS: picture-note preview golden; notes show and carry alt text')


if __name__ == '__main__':
    main()
