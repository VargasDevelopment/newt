#!/usr/bin/env python3
"""Opt-in wake snapshots, frozen page boundaries and unchanged shared receipts."""
import importlib.util
import json
from pathlib import Path
import shutil
import struct
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
from dev_usb_wake import decode_wake_timing
from gate_tier import negative_controls


class Decode(unittest.TestCase):
    def payload(self, words):
        return 'WAKE version=1 available=1 data=' + struct.pack('<24I', *words).hex()

    def test_frozen_snapshot_has_one_clock_and_explicit_missing_stages(self):
        words = [0] * 24
        words[0], words[13], words[14], words[15] = 0xfffffff0, 5, 10, 20
        words[16:] = [1, 0xe001, 0x101, 1, 0, 160, 0x10401, 2]
        result = decode_wake_timing(self.payload(words))
        self.assertIsNone(result['stamps']['sd'])
        self.assertEqual(result['since_entry_us']['page_complete'], 36)
        self.assertEqual((result['page_kind'], result['display_calls']), ('picture', 1))
        self.assertTrue(result['restored'])
        self.assertEqual(decode_wake_timing('WAKE version=1 available=0'), {'version': 1, 'available': False})
        for index, value in ((16, 2), (17, 1), (18, 0), (18, 3), (18, 0x10001), (4, 1)):
            bad = words.copy(); bad[index] = value
            with self.subTest(index=index, value=value), self.assertRaises(ValueError):
                decode_wake_timing(self.payload(bad))
        with self.assertRaises(ValueError): decode_wake_timing(self.payload(words) + ' extra=1')


def rust_checks():
    spec = importlib.util.spec_from_file_location('wake_support', ROOT/'rust/tests/inflate/run_support.py')
    support = importlib.util.module_from_spec(spec); spec.loader.exec_module(support)
    support.install_signal_handlers()
    scratch = ROOT/'.build/wake-tests'; scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='run-', dir=scratch) as directory:
        out = Path(directory); workspace = out/'rust'
        shutil.copytree(ROOT/'rust', workspace, ignore=shutil.ignore_patterns('target', '__pycache__'))
        (out/'logs').mkdir()
        run = support.Run(out/'logs', support.verify_tools(), time.monotonic()+300)
        run.env.update(RUSTFLAGS='-Dwarnings -Ccodegen-units=1')
        def check(*arguments, expected_failure=False):
            try:
                output = run.cargo(workspace, out/'target', 'test', '-p', 'newt-core', *arguments)
            except RuntimeError:
                if not expected_failure:
                    for kind in ('stdout', 'stderr'):
                        print((run.output/f'{len(run.commands)-1:03d}.{kind}').read_text(), file=sys.stderr)
                raise
            assert 'test result: ok.' in output, output
        check('--lib', 'wake_timing::tests')
        for features in ('dev-usb', 'dev-usb,wake-timing'):
            check('--test', 'wake_timing_bridge', '--features', features)
        if negative_controls(__file__):
            path = workspace/'core/src/wake_timing.rs'; original = path.read_text()
            controls = (
                ('freeze removed', 'if self.frozen()', 'if false'),
                ('non-page can complete', 'pub fn complete(&self, now: u32) {\n        if self.frozen() || self.metadata[2].get() & PAGE == 0 { return; }',
                 'pub fn complete(&self, now: u32) {\n        if self.frozen() { return; }'),
            )
            for name, good, bad in controls:
                assert good in original
                path.write_text(original.replace(good, bad))
                try:
                    number = len(run.commands)
                    try: check('--lib', 'wake_timing::tests', expected_failure=True)
                    except RuntimeError:
                        output = (run.output/f'{number:03d}.stdout').read_text()
                        assert 'test result: FAILED.' in output and 'only_a_real_page_freezes' in output, output
                    else: raise AssertionError('control survived: '+name)
                finally: path.write_text(original)
                print('PASS control:', name)
    print('PASS: two timeline tests and real bridge off/on snapshots')


def preview_checks():
    import rust_preview
    import preview_driver
    binary = rust_preview.build(wake_timing=True)
    def first_page(program):
        with tempfile.TemporaryDirectory(prefix='wake-hooks-', dir=ROOT/'.build') as directory:
            world = Path(directory)
            preview_driver.world(program, world, preview_driver.parse(['--library-fixture', 'healthy']))
            preview_driver.write_conf(world, {'wake.timing': 1})
            rust_preview.run(program, world, [])
            rows = (world/'receipt.txt').read_text().splitlines()
            home = decode_wake_timing(next(row for row in rows if row.startswith('WAKE ')))
            if home['available']: return 'Home froze a page timeline'
            rust_preview.run(program, world, [1, 1])
            rows = (world/'receipt.txt').read_text().splitlines()
            page = decode_wake_timing(next(row for row in rows if row.startswith('WAKE ')))
            if not page['available']: return 'the real reader never froze the timeline'
            for stage in ('entry', 'serial', 'probe', 'display_ready', 'sd', 'application',
                          'home_restore', 'reader_restore', 'input_ready', 'picture_candidate',
                          'source_ready', 'page_ready', 'display_start', 'display_return', 'page_complete'):
                if page['stamps'][stage] is None: return stage + ' missing'
            if page['page_kind'] != 'ordinary': return 'wrong page path'
    assert first_page(binary) is None
    if negative_controls(__file__):
        original = 'if actual_page && !covered { home.bus.board.wake_timeline().complete(home.bus.board.monotonic_clock().micros()); }'
        control = rust_preview.control([('reader_image_paint.rs', original, 'if false { home.bus.board.wake_timeline().complete(home.bus.board.monotonic_clock().micros()); }')],
                                      optimize=False, work=rust_preview.WORK/'wake-timing')
        assert first_page(control) == 'the real reader never froze the timeline'
        print('PASS control: removing the actual-page completion hook fails')
    print('PASS: real firmware excludes Home and publishes the first-page snapshot')


if __name__ == '__main__':
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(Decode)
    if not unittest.TextTestRunner().run(suite).wasSuccessful(): raise SystemExit(1)
    rust_checks()
    preview_checks()
