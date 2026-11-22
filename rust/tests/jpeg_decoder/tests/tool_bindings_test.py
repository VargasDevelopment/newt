"""Packet declarations may only name the shared, identity-verified tools."""
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[4]
SHARED = ROOT / 'rust/tests/inflate/toolchains.json'


class PacketTools(unittest.TestCase):
    def assert_owned(self, declared, shared):
        self.assertNotIn('python', declared, 'Packet must not byte-pin an interpreter')
        for name, entry in declared.items():
            self.assertEqual(entry, shared.get(name),
                             'Outside shared verified tool set: ' + name)

    def test_every_packet_binding_uses_the_shared_owner(self):
        shared = json.loads(SHARED.read_text())['executables']
        packets = [path for path in (ROOT / 'rust/tests').rglob('*.json')
                   if path.name in ('tool_bindings.json', 'toolchains.json') and path != SHARED]
        self.assertTrue(packets)
        for path in packets:
            with self.subTest(packet=path.relative_to(ROOT).as_posix()):
                declared = json.loads(path.read_text())
                self.assert_owned(declared.get('executables', declared), shared)

    def test_old_host_local_python_binding_is_rejected(self):
        shared = json.loads(SHARED.read_text())['executables']
        old = {'python': {'path': '.toolchain/python/bin/python3.12',
                          'sha256': 'c7c1a234ed39ceef4bde5ee8d29db37f55b0f7de6339b82eff5812944ccb74da'}}
        with self.assertRaisesRegex(AssertionError, 'Packet must not byte-pin an interpreter'):
            self.assert_owned(old, shared)


if __name__ == '__main__':
    unittest.main()
