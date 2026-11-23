"""Original CC0 books and prepared cover fixtures for the Library previews.

No private inputs, card access or Library uploads. The host-built firmware
preview (tools/system_ui_contract.py) scans this card as the firmware does.
"""
import copy
import importlib.util
import os
from pathlib import Path
import sys
import tempfile
from xml.etree import ElementTree as ET
import zipfile

from host_common import ROOT


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
