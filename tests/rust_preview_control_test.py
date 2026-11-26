#!/usr/bin/env python3
"""rust_preview.control() under gate phases that run at once.

Two phases may ask for the same control (the same edits): each must get a
binary that stays in place while it runs, and the control links once. A
rebuilt positive preview makes a fresh control. A fake linker stands in for
rustc, so this takes about a second.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--tool', type=Path, default=ROOT / 'tools/rust_preview.py')
parser.add_argument('--single', help='run one test (a negative control)')
parser.add_argument('--negative-controls', action='store_true')
args, _ = parser.parse_known_args()
sys.path.insert(0, str(ROOT / 'tools'))
spec = importlib.util.spec_from_file_location('rust_preview_under_test', args.tool)
rust_preview = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rust_preview)

EDIT = [('reader.rs', 'fn timed_out() -> bool { true }', 'fn timed_out() -> bool { false }')]
# Writes the program after a pause, so a deleted folder stays missing long
# enough to see, and logs each link.
LINKER = '''import os, sys, time
program = sys.argv[sys.argv.index('-o') + 1]
with open(os.environ['FAKE_LINKS'], 'a') as log:
    log.write(program + '\\n')
time.sleep(0.3)
with open(program, 'w') as out:
    out.write('linked\\n')
'''


class ControlTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix='rust-preview-control-')
        self.addCleanup(scratch.cleanup)
        work = Path(scratch.name) / 'rust-preview'
        (work / 'source').mkdir(parents=True)
        (work / 'source/reader.rs').write_text('fn timed_out() -> bool { true }\n')
        (work / 'host.rs').write_text(f'include!("{work / "source/reader.rs"}");\n')
        (work / 'firmware').write_text('positive\n')
        linker = Path(scratch.name) / 'linker.py'
        linker.write_text(LINKER)
        self.links = Path(scratch.name) / 'links.txt'
        self.links.touch()
        (work / 'link.json').write_text(json.dumps({
            'command': [sys.executable, '-B', str(linker), '-Copt-level=1'],
            'environment': {'FAKE_LINKS': str(self.links)}}))
        for name, value in (('WORK', work), ('LINK', work / 'link.json'), ('FIRMWARE', work / 'firmware')):
            patcher = mock.patch.object(rust_preview, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)
        self.work = work

    def link_count(self):
        return len(self.links.read_text().splitlines())

    def test_concurrent_same_key_controls_share_one_build(self):
        built, started, done = threading.Event(), threading.Event(), threading.Event()
        missing, errors = [], []

        def first():
            try:
                program = rust_preview.control(EDIT, optimize=False)
                built.set()
                started.wait(5)
                # This phase is "running" its binary while the other asks for it.
                while not done.is_set():
                    if not program.is_file():
                        missing.append(program)
                    time.sleep(0.005)
            except Exception as error:  # surfaced below, not lost in the thread
                errors.append(error)
                built.set()

        def second():
            built.wait(5)
            started.set()
            try:
                self.assertEqual(rust_preview.control(EDIT, optimize=False).read_text(), 'linked\n')
            except Exception as error:
                errors.append(error)
            finally:
                done.set()

        threads = [threading.Thread(target=first), threading.Thread(target=second)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join(10)
        self.assertEqual(errors, [])
        self.assertEqual(missing, [], 'a same-key control deleted a binary another phase was running')
        self.assertEqual(self.link_count(), 1)

    def test_a_rebuilt_positive_preview_relinks_its_controls(self):
        first = rust_preview.control(EDIT, optimize=False)
        (self.work / 'firmware').write_text('positive, rebuilt\n')
        self.assertEqual(rust_preview.control(EDIT, optimize=False), first)
        self.assertEqual(self.link_count(), 2)

    def test_a_failed_link_is_never_reused(self):
        link = json.loads((self.work / 'link.json').read_text())
        failing = dict(link, command=[sys.executable, '-B', '-c', 'raise SystemExit(1)', '-Copt-level=1'])
        (self.work / 'link.json').write_text(json.dumps(failing))
        with self.assertRaises(subprocess.CalledProcessError):
            rust_preview.control(EDIT, optimize=False)
        (self.work / 'link.json').write_text(json.dumps(link))
        self.assertEqual(rust_preview.control(EDIT, optimize=False).read_text(), 'linked\n')
        self.assertEqual(self.link_count(), 1)


# Each removes one guard from rust_preview.py; the named test must then fail.
CONTROLS = (
    # The old delete-then-build: every call relinks the shared folder.
    ('        if program.is_file() and stamp.is_file() and stamp.read_text() == inputs:\n',
     '        if False:\n', 'test_concurrent_same_key_controls_share_one_build'),
    ("    for part in (link_text.encode(), FIRMWARE.read_bytes(), (WORK / 'host.rs').read_bytes(),\n",
     "    for part in (link_text.encode(), (WORK / 'host.rs').read_bytes(),\n",
     'test_a_rebuilt_positive_preview_relinks_its_controls'),
)


def negative_controls():
    source = args.tool.read_text()
    for old, new, test in CONTROLS:
        if source.count(old) != 1:
            raise RuntimeError('Negative control anchor drift: ' + test)
        with tempfile.TemporaryDirectory(prefix='rust-preview-control-') as scratch:
            tool = Path(scratch) / 'tools/rust_preview.py'
            tool.parent.mkdir()
            tool.write_text(source.replace(old, new))
            result = subprocess.run([sys.executable, '-B', __file__, '--tool', str(tool), '--single', test],
                                    capture_output=True, text=True, timeout=60)
            if result.returncode == 0 or ('FAIL:' not in result.stderr and 'ERROR:' not in result.stderr):
                raise RuntimeError('Guard removal was not caught: ' + test + '\n' + result.stderr)
            print('PASS guard-removal control: ' + test, flush=True)


if __name__ == '__main__':
    loader = unittest.defaultTestLoader
    suite = (loader.loadTestsFromName(args.single, ControlTests) if args.single
             else loader.loadTestsFromTestCase(ControlTests))
    if not unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful():
        sys.exit(1)
    if args.negative_controls:
        negative_controls()
