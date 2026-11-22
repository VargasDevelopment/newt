# SPDX-License-Identifier: MIT
"""Deterministic synthetic EPUBs only; no library/title/digest-derived input."""
from zipfile import ZipFile, ZipInfo, ZIP_STORED


def write(folder):
    folder.mkdir()
    for number in (1, 2):
        path = folder / f'fixture-{number:02d}.epub'
        entries = {
            'mimetype': b'application/epub+zip',
            'META-INF/container.xml': b'<container><rootfiles><rootfile full-path="OPS/p.opf" media-type="application/oebps-package+xml"/></rootfiles></container>',
            'OPS/p.opf': b'<package><metadata/><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/><item id="b" href="b.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="a"/><itemref idref="b"/></spine></package>',
            'OPS/a.xhtml': f'<html><body><p>Generated first section {number}.</p></body></html>'.encode(),
            'OPS/b.xhtml': f'<html><body><p>Generated second section {number}.</p></body></html>'.encode(),
        }
        with ZipFile(path, 'w', compression=ZIP_STORED) as book:
            for name, data in entries.items():
                info = ZipInfo(name, (2000, 1, 1, 0, 0, 0))
                info.compress_type = ZIP_STORED
                book.writestr(info, data)
    return folder / 'fixture-01.epub'
