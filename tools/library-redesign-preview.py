"""Original CC0 books and cover art for the Library previews, and their prepared covers.

No private inputs, card access or Library uploads. The host-built firmware
preview (tools/system_ui_contract.py) scans this card as the firmware does.
"""
import copy
import importlib.util
import os
from pathlib import Path
import struct
import sys
import tempfile
from xml.etree import ElementTree as ET
import zipfile
import zlib

from host_common import ROOT


def gray_png(width, height, pixels):
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    rows = b''.join(b'\0' + pixels[y * width:(y + 1) * width] for y in range(height))
    return (b'\x89PNG\r\n\x1a\n'
            + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 0, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(rows, 9)) + chunk(b'IEND', b''))


def cover_art(style):
    pixels = bytearray()
    for y in range(188):
        for x in range(144):
            dx, dy = x - 72, y - 80
            ink = (x in (8, 135) and 8 <= y <= 179) or (y in (8, 179) and 8 <= x <= 135)
            if style == 0:
                ink |= 54 <= x <= 90 and 48 <= y <= 113 and (x < 58 or x > 86 or y < 52 or y > 109)
                ink |= 26 <= x <= 118 and 132 <= y <= 151 and y % 7 == 0
            elif style == 1:
                for center in (40, 104):
                    ink |= (x - center) ** 2 + (y - 70) ** 2 < 380
                    ink |= center - 2 <= x <= center + 2 and 70 <= y <= 143
            elif style == 2:
                ink |= 36 <= x <= 108 and 48 <= y <= 100 and (y in (48, 100) or x in (36, 108) or y - 48 == (x - 36) // 2)
                ink |= 20 <= x <= 124 and 125 <= y <= 151 and (y + x // 10) % 9 == 0
            else:
                ink |= dx * dx + dy * dy < 700
                ink |= 20 <= x <= 124 and 128 <= y <= 154 and (y == 128 + x // 8 or y == 154 - x // 9)
            pixels.append(0 if ink else 245)
    return gray_png(144, 188, pixels)


def authored_books(directory):
    directory.mkdir(parents=True, exist_ok=True)
    opf = 'http://www.idpf.org/2007/opf'
    dc = 'http://purl.org/dc/elements/1.1/'
    names = ('A Map of Morning', 'Letters from the Shore', 'The Quiet Orchard', 'The Small Lantern')
    with zipfile.ZipFile(ROOT / 'tests/fixtures/the-small-lantern.epub') as source:
        for style, name in enumerate(names):
            package = ET.fromstring(source.read('OEBPS/content.opf'))
            package.find(f'.//{{{dc}}}title').text = name
            ET.SubElement(package.find(f'{{{opf}}}manifest'), f'{{{opf}}}item',
                          {'id': 'newt-cover', 'href': 'cover.png', 'media-type': 'image/png', 'properties': 'cover-image'})
            output = directory / (name + '.epub')
            with zipfile.ZipFile(output, 'w', compression=zipfile.ZIP_STORED) as archive:
                for entry in source.infolist():
                    archive.writestr(copy.copy(entry), ET.tostring(package) if entry.filename == 'OEBPS/content.opf' else source.read(entry))
                image = (ROOT / f'tests/fixtures/library-cover-images/style-{style}.png').read_bytes()
                archive.writestr('OEBPS/cover.png', image)
            os.utime(output, (1790812800, 1790812800))


def prepare_authored_covers(books, covers):
    """Prepare the preview's four authored covers using the locked fixture helper."""
    spec = importlib.util.spec_from_file_location('authored_cover_prepare', ROOT / 'tools/prepare-library-covers.py')
    tool = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(tool)
    with tempfile.TemporaryDirectory(prefix='newt-authored-covers-') as directory:
        result = tool.prepare(books, covers,
                              [sys.executable, '-I', '-B', str(ROOT / 'tools/library-cover-fixture.py')],
                              Path(directory))
    if result != (4, 0):
        raise RuntimeError('All four exact authored cover fixtures are required')
