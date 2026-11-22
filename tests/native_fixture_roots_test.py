"""Short physical fixtures, stable compile roots and both gate guards."""
import fcntl
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import gate_scratch
import host_common
import host_jobs


class NativeRootsTests(unittest.TestCase):
    def test_long_parent_never_enters_physical_producer_or_storage_paths(self):
        with tempfile.TemporaryDirectory(prefix='long-native-' + 'x' * 120) as temporary:
            with patch.dict(os.environ, {'TMPDIR': temporary}), patch.object(tempfile, 'tempdir', temporary):
                for allocator in (host_common.native_storage_root,):
                    with allocator() as name:
                        root = ROOT / name
                        self.assertLess(len(os.fsencode(name)), 48)
                        self.assertGreaterEqual(len(os.fsencode(name)), len("/newt-sd/Newt/Cache"))
                        self.assertFalse(Path(name).is_absolute())
                        self.assertTrue(root.is_relative_to(ROOT / '.build'))
                        self.assertEqual(root, root.resolve())
                        self.assertTrue(all(not path.is_symlink() for path in (root, *root.parents)))
                        descriptor = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
                        os.close(descriptor)
                        self.assertEqual(root.stat().st_mode & 0o777, 0o700)
                    self.assertFalse(root.exists())

    def test_compile_input_stable_while_parallel_runtime_storage_is_private(self):
        with tempfile.TemporaryDirectory() as temporary:
            probes = [Path(temporary) / name for name in ('one.cpp', 'two.cpp')]
            for probe in probes:
                probe.write_text('int main() {}\n')
            with host_common.native_cache_root(probes[0]) as first, host_common.native_cache_root(probes[1]) as second:
                self.assertEqual(probes[0].read_bytes(), probes[1].read_bytes())
                self.assertNotEqual(first, second)
                for cwd, value in ((first, 'one'), (second, 'two')):
                    subprocess.run([sys.executable, '-B', '-c',
                                    'from pathlib import Path;import sys;Path("data/result").write_text(sys.argv[1])', value],
                                   cwd=cwd, check=True)
                self.assertEqual((first / 'data/result').read_text(), 'one')
                self.assertEqual((second / 'data/result').read_text(), 'two')

    def test_compiled_roots_keep_at_least_device_headroom(self):
        self.assertGreaterEqual(len(host_common.NATIVE_READING_ROOT), len('/newt-sd/Newt'))
        self.assertGreaterEqual(len(host_common.NATIVE_CACHE_ROOT), len('/newt-sd/Newt/Cache'))
        with tempfile.TemporaryDirectory() as temporary:
            probe=Path(temporary)/'probe.cpp';probe.write_text('int main() {}\n')
            with host_common.native_cache_root(probe) as cwd:
                self.assertIn('"'+host_common.NATIVE_CACHE_ROOT+'"',probe.read_text())
                self.assertTrue((cwd/host_common.NATIVE_CACHE_ROOT).is_dir())

    def test_build_paths_are_stable_locked_and_have_no_stale_headers(self):
        label = 'root-contract'
        previous = None
        for _ in range(2):
            with host_common.fixture_build_root(label) as directory:
                self.assertFalse((directory / 'stale.h').exists())
                self.assertEqual(directory.stat().st_mode & 0o777, 0o700)
                if previous is not None:
                    self.assertEqual(directory, previous)
                descriptor = os.open(directory.parent / (label + '.lock'), os.O_RDWR)
                try:
                    with self.assertRaises(BlockingIOError):
                        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
                finally:
                    os.close(descriptor)
                (directory / 'stale.h').write_text('old control')
                previous = directory
            self.assertFalse(directory.exists())
        with self.assertRaises(ValueError):
            with host_common.fixture_build_root('../escape'):
                self.fail('invalid label accepted')

    def test_gate_exit_rejects_every_scratch_spelling_even_after_failure(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            receipts = root / '.build/host-objects'
            receipts.mkdir(parents=True)
            environment = dict(os.environ)
            environment.pop('TMPDIR', None)
            for field in ('TMPDIR', host_jobs.NATIVE_ROOTS, 'relative'):
                for failed in (False, True):
                    with patch.dict(os.environ, environment, clear=True):
                        with self.assertRaisesRegex(RuntimeError, 'receipts contain per-run scratch'):
                            with gate_scratch.environment(root):
                                value = os.environ.get('TMPDIR' if field == 'TMPDIR' else host_jobs.NATIVE_ROOTS)
                                if field == 'relative':
                                    value = Path(value).relative_to(root).as_posix()
                                (receipts / 'bad.json').write_text(json.dumps({'preprocess_flags': ['-DROOT="' + value + '"']}))
                                if failed:
                                    raise ValueError('phase failed before the final audit')

    def test_a_receipt_stamped_in_the_start_tick_is_still_audited(self):
        """CI run 41263846804: file times trail time.time_ns() by up to a coarse
        clock tick, so a receipt written right after the start looked older."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            receipt = root / '.build/host-objects/bad.json'
            receipt.parent.mkdir(parents=True)
            environment = dict(os.environ)
            environment.pop('TMPDIR', None)
            ahead = time.time_ns() + 10_000_000  # The fine clock, a tick ahead of file times.
            with patch.dict(os.environ, environment, clear=True), patch('time.time_ns', return_value=ahead), \
                    self.assertRaisesRegex(RuntimeError, 'receipts contain per-run scratch'):
                with gate_scratch.environment(root) as scratch:
                    receipt.write_text(str(scratch))
                    # Stamped in the same tick as the start, as the coarse clock can.
                    start = Path(os.environ[host_jobs.NATIVE_ROOTS]).stat().st_mtime_ns
                    os.utime(receipt, ns=(start, start))

    def test_old_receipts_do_not_contaminate_this_invocation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            receipt = root / '.build/host-objects/old.json'
            receipt.parent.mkdir(parents=True)
            receipt.write_text(str(root / 'old-run'))
            gate_scratch.check_receipts(root, time.time_ns(), (root / 'old-run',))


if __name__ == '__main__':
    unittest.main()
