"""Authenticate the Expat product closure retained after oracle removal."""
import hashlib
import json
from pathlib import Path


def verify(packet, sdk):
    packet = Path(packet)
    provenance = json.loads((packet / 'PROVENANCE.json').read_text())
    book = 'libs/book/FreeInkBook/'
    third_party = book + 'third_party/expat/'
    wrappers = {book + 'src/vendor/expat_' + name + '.c'
                for name in ('xmlparse', 'xmltok', 'xmlrole')}
    pins = {entry['path']: entry['sha256'] for entry in provenance['files']
            if entry['path'].startswith(third_party) or entry['path'] in wrappers}
    if not wrappers.issubset(pins):
        raise ValueError('Incomplete native Expat source pins')
    frozen = Path(sdk)
    actual = {str(path.relative_to(frozen)) for path in (frozen / third_party).rglob('*')
              if path.is_file()}
    if actual != {name for name in pins if name.startswith(third_party)}:
        raise ValueError('Native Expat file set changed')
    for name, expected in pins.items():
        path = frozen / name
        if path.is_symlink() or '..' in Path(name).parts:
            raise ValueError('Invalid native Expat source path')
        with path.open('rb') as stream:
            if hashlib.file_digest(stream, 'sha256').hexdigest() != expected:
                raise ValueError('Native Expat source changed: ' + name)
