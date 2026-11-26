#!/usr/bin/env python3
"""rust_preview.control() under gate phases that run at once.

Two phases may ask for the same control (the same edits) at the same time.
Each call gets a folder of its own that no other call touches, removed when
its process exits; a killed phase's folders go once its pid is gone. A fake
linker stands in for rustc, so this takes about a second.
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
# Links an executable that prints this link's own token. With FAKE_MEET set,
# each link waits until that many links have started, so callers really are
# inside the link together.
LINKER = '''import os, sys, time, uuid
program = sys.argv[sys.argv.index('-o') + 1]
log = os.environ['FAKE_LINKS']
with open(log, 'a') as stream:
    stream.write(program + '\\n')
meet = int(os.environ.get('FAKE_MEET', '0'))
deadline = time.monotonic() + 5
while meet and len(open(log).read().splitlines()) < meet:
    if time.monotonic() > deadline:
        sys.exit('the other link never started')
    time.sleep(0.01)
with open(program, 'w') as out:
    out.write('#!/bin/sh\\necho %s\\n' % uuid.uuid4().hex)
os.chmod(program, 0o755)
'''


def run(program):
    return subprocess.run([str(program)], capture_output=True, text=True, check=True, timeout=10).stdout


class ControlTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix='rust-preview-control-')
        self.addCleanup(scratch.cleanup)
        self.scratch = Path(scratch.name)
        work = self.scratch / 'rust-preview'
        (work / 'source').mkdir(parents=True)
        (work / 'source/reader.rs').write_text('fn timed_out() -> bool { true }\n')
        (work / 'host.rs').write_text(f'include!("{work / "source/reader.rs"}");\n')
        linker = self.scratch / 'linker.py'
        linker.write_text(LINKER)
        self.links = self.scratch / 'links.txt'
        self.links.touch()
        self.link = work / 'link.json'
        self.link.write_text(json.dumps({
            'command': [sys.executable, '-B', str(linker), '-Copt-level=1'],
            'environment': {'FAKE_LINKS': str(self.links)}}))
        patcher = mock.patch.object(rust_preview, 'WORK', work)
        patcher.start()
        self.addCleanup(patcher.stop)
        patcher = mock.patch.object(rust_preview, 'LINK', self.link)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.controls = work / 'controls'

    def meet(self, count):
        link = json.loads(self.link.read_text())
        link['environment']['FAKE_MEET'] = str(count)
        self.link.write_text(json.dumps(link))

    def test_two_callers_inside_the_same_link_both_keep_their_binary(self):
        self.meet(2)
        programs, errors = [], []

        def call():
            try:
                programs.append(rust_preview.control(EDIT, optimize=False))
            except Exception as error:  # surfaced below, not lost in the thread
                errors.append(error)

        threads = [threading.Thread(target=call) for _ in range(2)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join(20)
        self.assertEqual(errors, [])
        self.assertEqual(len(set(programs)), 2, 'two callers shared one control folder')
        self.assertEqual(len({run(program) for program in programs}), 2)

    def test_a_kept_path_still_runs_its_own_control_while_another_builds(self):
        first = rust_preview.control(EDIT, optimize=False)
        token = run(first)
        second = rust_preview.control(EDIT, optimize=False)
        self.assertEqual(run(first), token, "another call replaced or removed this caller's control")
        self.assertNotEqual(run(second), token)

    def test_the_sweep_removes_a_dead_owners_folder_and_keeps_a_live_one(self):
        self.controls.mkdir()
        child = subprocess.Popen([sys.executable, '-c', 'pass'])
        child.wait()
        dead = self.controls / f'0123abcd-{child.pid}-x'
        live = self.controls / f'0123abcd-{os.getpid()}-y'
        for folder in (dead, live):
            (folder / 'source').mkdir(parents=True)
        rust_preview.sweep_controls(self.controls)
        self.assertFalse(dead.exists())
        self.assertTrue(live.exists(), "the sweep removed a live owner's control")

    def test_a_finished_process_leaves_no_control_behind(self):
        script = (f'import sys; sys.path.insert(0, {str(ROOT / "tools")!r})\n'
                  'import importlib.util\nfrom pathlib import Path\n'
                  f'spec = importlib.util.spec_from_file_location("tool", {str(args.tool)!r})\n'
                  'tool = importlib.util.module_from_spec(spec); spec.loader.exec_module(tool)\n'
                  f'tool.WORK, tool.LINK = Path({str(rust_preview.WORK)!r}), Path({str(self.link)!r})\n'
                  f'assert tool.control({EDIT!r}, optimize=False).is_file()\n')
        subprocess.run([sys.executable, '-B', '-c', script], check=True, timeout=30)
        self.assertEqual(list(self.controls.iterdir()), [])


# Each puts one removed guard back into rust_preview.py; every named test must then fail.
CONTROLS = (
    # The old shared folder, deleted then rebuilt by every call.
    ("    out = Path(tempfile.mkdtemp(prefix=f'{key}-{os.getpid()}-', dir=controls))\n",
     "    out = controls / key\n    shutil.rmtree(out, ignore_errors=True)\n    out.mkdir()\n",
     ('test_two_callers_inside_the_same_link_both_keep_their_binary',
      'test_a_kept_path_still_runs_its_own_control_while_another_builds')),
    # A sweep that treats every owner as gone removes live folders.
    ('    return True\n\n\ndef sweep_controls(',
     '    return False\n\n\ndef sweep_controls(',
     ('test_the_sweep_removes_a_dead_owners_folder_and_keeps_a_live_one',
      'test_a_kept_path_still_runs_its_own_control_while_another_builds')),
)


def negative_controls():
    source = args.tool.read_text()
    for old, new, tests in CONTROLS:
        if source.count(old) != 1:
            raise RuntimeError('Negative control anchor drift: ' + ', '.join(tests))
        with tempfile.TemporaryDirectory(prefix='rust-preview-control-') as scratch:
            tool = Path(scratch) / 'tools/rust_preview.py'
            tool.parent.mkdir()
            tool.write_text(source.replace(old, new))
            for test in tests:
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
