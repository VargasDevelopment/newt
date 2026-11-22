"""Canonical SDK admission must keep the original pins after copy removal."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('expat_native_inputs', Path(__file__).with_name('native_inputs.py'))
INPUTS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INPUTS)
BOOK = 'libs/book/FreeInkBook/'


class NativeInputsTests(unittest.TestCase):
    def test_canonical_inputs_and_rejection_controls(self):
        for change in ('none', 'changed', 'missing', 'added', 'symlink', 'missing-wrapper-pin'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                packet, sdk = root / 'packet', root / 'vendor/freeink-sdk'
                packet.mkdir()
                names = [BOOK + 'src/vendor/expat_' + name + '.c'
                         for name in ('xmlparse', 'xmltok', 'xmlrole')]
                names += [BOOK + 'third_party/expat/expat.h']
                pins = []
                for name in names:
                    path = sdk / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(b'admitted source\n')
                    pins.append(dict(path=name, sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
                selected = sdk / names[-1]
                if change == 'changed':
                    selected.write_bytes(b'changed after admission\n')
                elif change == 'missing':
                    selected.unlink()
                elif change == 'added':
                    selected.with_name('extra.h').write_bytes(b'unadmitted\n')
                elif change == 'symlink':
                    target = root / 'outside.h'
                    selected.rename(target)
                    selected.symlink_to(target)
                elif change == 'missing-wrapper-pin':
                    pins.pop(0)
                (packet / 'PROVENANCE.json').write_text(json.dumps(dict(files=pins)))
                if change == 'none':
                    INPUTS.verify(packet, sdk)
                    self.assertFalse((packet / 'frozen').exists())
                else:
                    with self.assertRaises(ValueError):
                        INPUTS.verify(packet, sdk)


if __name__ == '__main__':
    unittest.main()
