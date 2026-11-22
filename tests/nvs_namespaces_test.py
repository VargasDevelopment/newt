#!/usr/bin/env python3
"""Synthetic NVS partition inventory and privacy regressions; no private data."""
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import nvs_namespaces as nvs


def entry(ns, kind=1, span=1, key=b'PRIVATE_KEY', value=b'SECRET!!'):
    raw = bytearray(b'\xff' * 32)
    raw[:4] = bytes((ns, kind, span, 0xff))
    raw[8:24] = key.ljust(16, b'\0')
    raw[24:32] = value
    struct.pack_into('<I', raw, 4, zlib.crc32(raw[:4] + raw[8:], 0xffffffff))
    return bytes(raw)


def namespace(identifier, name):
    return entry(0, key=name, value=bytes([identifier]) + b'\xff' * 7)


def page(entries, states=None, state=0xfffffffe, version=0xfe):
    raw = bytearray(b'\xff' * 4096)
    struct.pack_into('<II', raw, 0, state, 1)
    raw[8] = version
    struct.pack_into('<I', raw, 28, zlib.crc32(raw[4:28], 0xffffffff))
    bitmap = (1 << 256) - 1
    for i, data in enumerate(entries):
        raw[64 + i * 32:96 + i * 32] = data
        bitmap = (bitmap & ~(3 << (i * 2))) | ((states[i] if states else 2) << (i * 2))
    raw[32:64] = bitmap.to_bytes(32, 'little')
    return bytes(raw)


class NamespaceTests(unittest.TestCase):
    def test_inventory_counts_headers_not_payload_or_erased_entries(self):
        image = page([namespace(1, b'crosspoint'), namespace(2, b'newt_clock'),
                      namespace(3, b'empty'), entry(1), entry(2, kind=0x21, span=2, value=struct.pack('<HHI', 14, 0xffff, 0)),
                      b'SECRET_PAYLOAD'.ljust(32, b'!'), entry(1)], [2]*6 + [0])
        self.assertEqual(nvs.inventory(image + b'\xff' * 4096),
                         [('crosspoint', 1), ('empty', 0), ('newt_clock', 1)])

    def test_cross_page_namespace_and_supported_versions_states(self):
        for state in nvs.PAGE_STATES:
            for version in (0xff, 0xfe):
                self.assertEqual(nvs.inventory(page([entry(2)], state=state, version=version) +
                                              page([namespace(2, b'nvs.net')], state=0xfffffffc)), [('nvs.net', 1)])
        self.assertEqual(nvs.inventory(b'\xff' * 4096), [])

    def test_corrupt_and_unsupported_images_refuse(self):
        good = page([namespace(1, b'newt_clock'), entry(1)])
        mutations = []
        for at in (0, 4, 8, 28, 64 + 4, 64 + 32 + 8):
            raw = bytearray(good)
            raw[at] ^= 1
            mutations.append(raw)
        mutations += [b'', good[:-1], page([entry(7)]),
                      page([namespace(1, b'bad\x1bname')]),
                      page([namespace(1, b'one'), namespace(1, b'two')]),
                      page([namespace(1, b'one'), namespace(2, b'one')]),
                      page([namespace(1, b'one'), entry(1, span=2)], [2, 2]),
                      page([namespace(1, b'one')], [1]),
                      page([namespace(1, b'one'), entry(1, kind=0x77)])]
        for image in mutations:
            with self.subTest(case=mutations.index(image)), self.assertRaises(ValueError):
                nvs.inventory(image)

    def test_type_spans_never_expose_payload_as_namespace(self):
        payload = namespace(7, b'VALUE_SECRET')
        cases = []
        for kind in (0x21, 0x41, 0x42):
            for size, span in ((32, 1), (33, 2), (0, 2)):
                cases.append(page([namespace(1, b'owner'), entry(1, kind=kind, span=span,
                                         value=struct.pack('<HHI', size, 0xffff, 0)), payload]))
        for kind in (1, 0x11, 2, 0x12, 4, 0x14, 8, 0x18, 0x48):
            cases.append(page([namespace(1, b'owner'), entry(1, kind=kind, span=2), payload]))
        # A deleted owner cannot turn its still-written payload into a name.
        cases.append(page([namespace(1, b'owner'), entry(1, kind=0x42, span=2, value=struct.pack('<HHI', 32, 0xffff, 0)),
                           payload], [2, 0, 2]))
        # Even a fully erased payload may contain a CRC-valid namespace header.
        safe = page([entry(1, kind=0x42, span=2, value=struct.pack('<HHI', 32, 0xffff, 0)),
                     payload], [0, 0])
        self.assertEqual(nvs.inventory(safe), [])
        with tempfile.TemporaryDirectory(prefix='nvs-span-') as scratch:
            path = Path(scratch) / 'partition.bin'
            for image in cases:
                path.write_bytes(image)
                result = subprocess.run([sys.executable, '-B', str(ROOT / 'tools/nvs_namespaces.py'), str(path)],
                                        capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, 1, result.stdout)
                self.assertEqual(result.stdout, '', 'payload can never be published as a namespace')
                self.assertNotIn('VALUE_SECRET', result.stderr)

    def test_physical_totals_include_spans_and_reserved_page(self):
        data = page([namespace(1, b'newt'), entry(1, kind=0x42, span=2,
                    value=struct.pack('<HHI', 32, 0xffff, 0)), b'VALUE'.ljust(32, b'!'),
                    entry(1)], [2, 2, 2, 0]) + b'\xff' * 4096
        with tempfile.TemporaryDirectory(prefix='nvs-totals-') as scratch:
            path = Path(scratch) / 'partition.bin'
            path.write_bytes(data)
            result = subprocess.run([sys.executable, '-B', str(ROOT / 'tools/nvs_namespaces.py'), str(path)],
                                    capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, 'newt\t1\nTOTAL\tpages=2\tused=3\terased=1\tfree=248\tempty_pages=1\tappend_run=122\n')

    def test_full_or_freeing_pages_do_not_claim_append_room(self):
        for state in (0xfffffffc, 0xfffffff8):
            _, totals = nvs.inspect_partition(page([namespace(1, b'owner')], state=state) + b'\xff' * 4096)
            self.assertEqual(totals['free'], 251)
            self.assertEqual(totals['empty_pages'], 1)
            self.assertEqual(totals['append_run'], 0)

    def test_cli_never_prints_keys_values_or_partial_names_on_error(self):
        with tempfile.TemporaryDirectory(prefix='nvs-test-') as scratch:
            path = Path(scratch) / 'partition.bin'
            valid = page([namespace(1, b'newt_clock'), entry(1)])
            path.write_bytes(valid)
            def run():
                return subprocess.run([sys.executable, '-B', str(ROOT / 'tools/nvs_namespaces.py'), str(path)],
                                      capture_output=True, text=True, timeout=10)
            result = run()
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, 'newt_clock\t1\nTOTAL\tpages=1\tused=2\terased=0\tfree=124\tempty_pages=0\tappend_run=124\n')
            self.assertEqual(result.stderr, '')
            path.write_bytes(valid + page([entry(9)]))
            result = run()
            self.assertEqual(result.returncode, 1)
            self.assertEqual(result.stdout, '')
            self.assertEqual(result.stderr, 'FAIL: invalid, unsupported or unreadable NVS partition image\n')


if __name__ == '__main__':
    unittest.main()
