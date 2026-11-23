"""CI's persistent store (tools/ci_persist.py) on the self-hosted runner."""
from contextlib import redirect_stdout
import io
import hashlib
import importlib.util
import os
import subprocess
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
import ci_persist
import ci_hosted
import ci_toolchain


class PersistControls(unittest.TestCase):
    """The per-push tier reuses only a toolchain snapshot made from these exact
    locks, here, whose every copied byte matches its recorded manifest."""
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root, self.store = Path(temporary.name, 'work/newt'), Path(temporary.name, 'work/newt-ci-store')
        for name in ci_persist.STAMPED:
            (self.root / name).parent.mkdir(parents=True, exist_ok=True)
            (self.root / name).write_text('{"lock": 1}\n')
        tool = self.root / '.toolchain/llvm/bin/clang'
        tool.parent.mkdir(parents=True)
        tool.write_text('#!/bin/sh\n'); tool.chmod(0o755)
        (self.root / '.toolchain/downloads').mkdir()
        (self.root / '.toolchain/downloads/raw.tar.xz').write_text('raw')
        (self.root / '.release-venv/bin').mkdir(parents=True)
        (self.root / '.release-venv/bin/python').symlink_to('/usr/bin/python3')
        ci_persist.snapshot(self.root, self.store)
        self.made = ci_persist.manifest(self.root)

    def clean_checkout(self):
        for tree in ci_persist.TREES:
            shutil.rmtree(self.root / tree)

    def test_reuse_copies_exactly_what_a_fresh_materialization_made(self):
        self.assertNotIn('.toolchain/downloads/raw.tar.xz', json.dumps(self.made))
        self.clean_checkout()
        with redirect_stdout(io.StringIO()):
            self.assertTrue(ci_persist.reuse(self.root, self.store))
        self.assertEqual(ci_persist.manifest(self.root), self.made)

    def test_changed_snapshot_bytes_mode_or_locks_are_never_reused(self):
        for change in ('bytes', 'mode', 'lock', 'plugin', 'workspace'):
            with self.subTest(change=change):
                self.setUp()
                self.clean_checkout()
                tool = self.store / 'toolchain/.toolchain/llvm/bin/clang'
                if change == 'bytes':
                    tool.write_text('#!/bin/sh\nexit 0\n')
                elif change == 'mode':
                    tool.chmod(0o644)
                elif change == 'lock':
                    (self.root / ci_persist.LOCKS[0]).write_text('{"lock": 2}\n')
                elif change == 'plugin':
                    (self.root / ci_persist.PLUGIN[0]).write_text('# a changed release plugin\n')
                else:
                    self.root = self.root.rename(self.root.with_name('other'))
                with redirect_stdout(io.StringIO()) as output:
                    if change in ('bytes', 'mode'):
                        with self.assertRaisesRegex(RuntimeError, 'differs from the manifest'):
                            ci_persist.reuse(self.root, self.store)
                    else:
                        self.assertFalse(ci_persist.reuse(self.root, self.store))
                        self.assertIn('STALE', output.getvalue())

    def test_full_tier_starts_without_objects_and_push_keeps_them(self):
        objects, seconds = self.store / 'host-objects', self.root / ci_persist.gate_tier.SECONDS
        objects.mkdir()
        (objects / 'object.o').write_text('o')
        (self.store / 'gate-phase-seconds.json').write_text('{"a_test.py": 1}\n')
        ci_persist.restore_objects(self.root, self.store, 'push')
        self.assertTrue((self.root / ci_persist.OBJECTS / 'object.o').is_file())
        self.assertTrue(seconds.is_file())
        ci_persist.save_objects(self.root, self.store)
        self.assertTrue((objects / 'object.o').is_file())
        ci_persist.restore_objects(self.root, self.store, 'full')
        self.assertFalse(objects.exists())
        self.assertFalse((self.root / ci_persist.OBJECTS).exists())
        self.assertTrue(seconds.is_file())  # Phase seconds only order starts: every tier keeps them.


class RustPreparationControls(unittest.TestCase):
    def test_approval_is_required_before_cache_or_acquisition_for_both_runners(self):
        for approval in ('', 'true', '1.93.0'):
            for runner in (ci_persist, ci_hosted):
                with self.subTest(approval=approval, runner=runner.__name__), \
                        patch.dict(os.environ, {'NEWT_RUST_ACQUISITION_APPROVED': approval}), \
                        patch.object(runner, 'reuse') as reuse, \
                        patch.object(subprocess, 'run') as run:
                    with self.assertRaisesRegex(RuntimeError, 'owner approval'):
                        if runner is ci_persist:
                            runner.prepare(Path('/unused'), Path('/unused-store'), 'push')
                        else:
                            runner.prepare(Path('/unused'), 'local')
                    reuse.assert_not_called()
                    run.assert_not_called()

    def test_old_approval_cannot_authorize_a_changed_archive_lock(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'rust').mkdir()
            lock = root / 'rust/toolchain.json'
            lock.write_text((ci_persist.ROOT / 'rust/toolchain.json').read_text())
            approval = 'sha256:' + ci_persist.sha256(lock)
            with patch.dict(os.environ, {'NEWT_RUST_ACQUISITION_APPROVED': approval}):
                ci_persist.require_rust_approval(root)
                changed = json.loads(lock.read_text())
                changed['components'][0]['sha256'] = '0' * 64
                lock.write_text(json.dumps(changed))
                with self.assertRaisesRegex(RuntimeError, 'owner approval'):
                    ci_persist.require_rust_approval(root)

    def test_authenticated_cache_still_requires_rust_receipt_verification(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'rust').mkdir()
            (root / 'rust/toolchain.json').write_text('{}')
            for runner in (ci_persist, ci_hosted):
                with self.subTest(runner=runner.__name__), \
                        patch.dict(os.environ, {'NEWT_RUST_ACQUISITION_APPROVED': 'sha256:' + ci_persist.sha256(root / 'rust/toolchain.json')}), \
                        patch.object(runner, 'reuse', return_value=True), \
                        patch.object(subprocess, 'run', side_effect=subprocess.CalledProcessError(1, 'verify')) as run:
                    with self.assertRaises(subprocess.CalledProcessError):
                        if runner is ci_persist:
                            runner.prepare(root, root / 'store', 'push')
                        else:
                            runner.prepare(root, 'local')
                    run.assert_called_once_with(
                        [sys.executable, '-B', 'tools/rust-toolchain.py', '--verify'], cwd=root, check=True)

    def test_fresh_runners_install_locked_rust_before_recording_cache(self):
        self.assertEqual(ci_persist.PREPARE[-1], ('tools/rust-toolchain.py', '--acquire', '--ci-cache'))
        for runner in (ci_persist, ci_hosted):
            with self.subTest(runner=runner.__name__), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / '.build').mkdir()
                (root / 'rust').mkdir()
                (root / 'rust/toolchain.json').write_text('{}')
                with patch.dict(os.environ, {'NEWT_RUST_ACQUISITION_APPROVED': 'sha256:' + ci_persist.sha256(root / 'rust/toolchain.json')}), \
                        patch.object(subprocess, 'run', side_effect=subprocess.CalledProcessError(1, 'install')) as run, \
                        patch.object(ci_persist, 'PREPARE', (ci_persist.PREPARE[-1],)), \
                        patch.object(ci_persist, 'snapshot') as snapshot:
                    with self.assertRaises(subprocess.CalledProcessError):
                        if runner is ci_persist:
                            runner.prepare(root, root / 'store', 'full')
                        else:
                            runner.prepare(root, 'full')
                    snapshot.assert_not_called()
                    self.assertFalse((root / ci_hosted.RECORD).exists())
                    run.assert_called_once_with(
                        [sys.executable, '-B', 'tools/rust-toolchain.py', '--acquire', '--ci-cache'], cwd=root, check=True)

    def test_rust_lock_and_installer_invalidate_existing_snapshots(self):
        for name in ('rust/toolchain.json', 'tools/rust-toolchain.py'):
            with self.subTest(name=name):
                fixture = PersistControls()
                fixture.setUp()
                try:
                    fixture.clean_checkout()
                    (fixture.root / name).write_text('changed')
                    self.assertFalse(ci_persist.reuse(fixture.root, fixture.store))
                finally:
                    fixture.doCleanups()

    def test_ci_installer_reads_existing_raw_cache_before_any_acquisition(self):
        spec = importlib.util.spec_from_file_location(
            'ci_rust_downloads', ci_persist.ROOT / 'tools/rust-toolchain.py')
        tool = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(tool)
        lock = json.loads((ci_persist.ROOT / 'rust/toolchain.json').read_text())
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'rust').mkdir()
            (root / 'rust/toolchain.json').write_text(json.dumps(lock))
            archive = root / tool.CI_CACHE / lock['components'][0]['url'].rsplit('/', 1)[1]
            archive.parent.mkdir(parents=True)
            archive.write_bytes(b'cached archive stand-in')
            with patch.object(tool, 'ROOT', root), patch.object(tool, 'BASE', root / '.toolchain/rust'), \
                    patch.object(tool, 'digest', return_value=lock['components'][0]['sha256']), \
                    patch.object(tool.urllib.request, 'urlopen') as download, \
                    patch.object(tool.tarfile, 'open', side_effect=RuntimeError('stop before extraction')) as extract:
                with self.assertRaisesRegex(RuntimeError, 'stop before extraction'):
                    tool.install(acquire=True, ci_cache=True)
                download.assert_not_called()
                extract.assert_called_once_with(archive)

    def test_second_fresh_prepare_keeps_rust_out_of_the_native_raw_cache(self):
        spec = importlib.util.spec_from_file_location('fresh_rust_cache', ci_persist.ROOT / 'tools/rust-toolchain.py')
        tool = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(tool)
        url = json.loads((ci_persist.ROOT / 'rust/toolchain.json').read_text())['components'][0]['url']
        payload = b'authored raw Rust archive'
        lock = {'components': [{'url': url, 'sha256': hashlib.sha256(payload).hexdigest()}]}
        class ArchiveReady(Exception):
            pass
        for reverted in (False, True):
            cache = '.toolchain/downloads/rust' if reverted else tool.CI_CACHE
            with self.subTest(cache=cache), tempfile.TemporaryDirectory() as directory:
                root, store = Path(directory) / 'root', Path(directory) / 'store'
                for name in ci_persist.STAMPED:
                    (root / name).parent.mkdir(parents=True, exist_ok=True)
                    (root / name).write_text('{}')
                (root / 'rust/toolchain.json').write_text(json.dumps(lock))
                archive = store / 'raw' / Path(tool.CI_CACHE).name / url.rsplit('/', 1)[1]
                archive.parent.mkdir(parents=True)
                archive.write_bytes(payload)
                def run(command, **options):
                    if command[2] == 'tools/ci_toolchain.py':
                        ci_toolchain.validate_cache(root, root / '.toolchain/downloads', [])
                    elif command[2] == 'tools/ci_environment.py':
                        (root / '.release-venv').mkdir()
                    else:
                        self.assertEqual(command[2], 'tools/rust-toolchain.py')
                        with self.assertRaises(ArchiveReady):
                            tool.install(acquire=True, ci_cache=True)
                        installed = root / '.toolchain/rust/installed.json'
                        installed.parent.mkdir(parents=True, exist_ok=True)
                        installed.write_text('fixture receipt')
                with patch.dict(os.environ, {'NEWT_RUST_ACQUISITION_APPROVED': 'sha256:' + ci_persist.sha256(root / 'rust/toolchain.json')}), \
                        patch.object(tool, 'ROOT', root), patch.object(tool, 'BASE', root / '.toolchain/rust'), \
                        patch.object(tool, 'CI_CACHE', cache), \
                        patch.object(tool.urllib.request, 'urlopen', side_effect=lambda *a, **k: io.BytesIO(payload)) as download, \
                        patch.object(tool.tarfile, 'open', side_effect=ArchiveReady), \
                        patch.object(subprocess, 'run', side_effect=run), patch.object(ci_persist, 'verify_rust'):
                    ci_persist.prepare(root, store, 'full')
                    if reverted:
                        with patch.object(ci_persist, 'migrate_legacy_rust_cache'), \
                                self.assertRaisesRegex(RuntimeError, 'Unexpected restored raw cache input: rust'):
                            ci_persist.prepare(root, store, 'full')
                        self.assertEqual(download.call_count, 1)
                    else:
                        ci_persist.prepare(root, store, 'full')
                        download.assert_not_called()
                        self.assertEqual(archive.read_bytes(), payload)
                        self.assertNotIn(archive.name, json.dumps(ci_persist.manifest(store / 'toolchain')))

    def test_legacy_rust_store_migrates_before_two_fresh_prepares(self):
        for existing, migrate in ((False, True), (True, True), (False, False)):
            with self.subTest(existing=existing, migrate=migrate), tempfile.TemporaryDirectory() as directory:
                root, store = Path(directory) / 'root', Path(directory) / 'store'
                for name in ci_persist.STAMPED:
                    (root / name).parent.mkdir(parents=True, exist_ok=True)
                    (root / name).write_text('{}')
                legacy = store / 'raw/downloads/rust'
                legacy.mkdir(parents=True)
                (legacy / 'pinned.tar.xz').write_bytes(b'legacy')
                current = store / 'raw/ci-rust-downloads/pinned.tar.xz'
                if existing:
                    current.parent.mkdir()
                    current.write_bytes(b'current')
                expected = b'current' if existing else b'legacy'
                def run(command, **options):
                    if command[2] == 'tools/ci_toolchain.py':
                        ci_toolchain.validate_cache(root, root / '.toolchain/downloads', [])
                    elif command[2] == 'tools/rust-toolchain.py':
                        self.assertEqual((root / '.build/ci-rust-downloads/pinned.tar.xz').read_bytes(), expected)
                    (root / '.release-venv').mkdir(exist_ok=True)
                migration = ci_persist.migrate_legacy_rust_cache if migrate else lambda store: None
                with patch.dict(os.environ, {'NEWT_RUST_ACQUISITION_APPROVED': 'sha256:' + ci_persist.sha256(root / 'rust/toolchain.json')}), \
                        patch.object(ci_persist, 'migrate_legacy_rust_cache', side_effect=migration), \
                        patch.object(subprocess, 'run', side_effect=run), patch.object(ci_persist, 'verify_rust'):
                    if migrate:
                        ci_persist.prepare(root, store, 'full')
                        ci_persist.prepare(root, store, 'full')
                        self.assertFalse(legacy.exists())
                        self.assertEqual(current.read_bytes(), expected)
                    else:
                        with self.assertRaisesRegex(RuntimeError, '^Unexpected restored raw cache input: rust$'):
                            ci_persist.prepare(root, store, 'full')

    def test_legacy_rust_migration_refuses_files_and_aliases(self):
        for kind in ('file', 'symlink', 'parent-symlink'):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as directory:
                store = Path(directory) / 'store'
                raw = store / 'raw'
                raw.mkdir(parents=True)
                outside = Path(directory) / 'outside'
                outside.mkdir()
                (outside / 'sentinel').write_bytes(b'unchanged')
                if kind == 'parent-symlink':
                    (raw / 'downloads').symlink_to(outside, target_is_directory=True)
                    (outside / 'rust').mkdir()
                else:
                    (raw / 'downloads').mkdir()
                    legacy = raw / 'downloads/rust'
                    if kind == 'file':
                        legacy.write_bytes(b'unchanged')
                    else:
                        legacy.symlink_to(outside, target_is_directory=True)
                with self.assertRaises(RuntimeError):
                    ci_persist.migrate_legacy_rust_cache(store)
                self.assertEqual((outside / 'sentinel').read_bytes(), b'unchanged')

    def test_receipt_rejects_changed_files_lock_and_missing_target_before_execution(self):
        spec = importlib.util.spec_from_file_location(
            'ci_rust_toolchain', ci_persist.ROOT / 'tools/rust-toolchain.py')
        tool = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(tool)
        lock = json.loads((ci_persist.ROOT / 'rust/toolchain.json').read_text())
        for change in ('bytes', 'mode', 'lock', 'target', 'receipt'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as directory:
                base = Path(directory)
                prefix = base / 'toolchain'
                compiler = prefix / 'bin/rustc'
                compiler.parent.mkdir(parents=True)
                compiler.write_text('fake compiler')
                compiler.chmod(0o755)
                target = prefix / 'lib/rustlib/riscv32imc-unknown-none-elf/lib/core.rlib'
                target.parent.mkdir(parents=True)
                target.write_text('fake target')
                receipt = {'lock': lock, 'files': tool.installed_identity(prefix)}
                (base / 'installed.json').write_text(json.dumps(receipt))
                version = subprocess.CompletedProcess([], 0, stdout='release: 1.93.1\n')
                cargo = subprocess.CompletedProcess([], 0, stdout='cargo 1.93.1 (test)\n')
                with patch.object(tool.subprocess, 'run', side_effect=[version, cargo]):
                    tool.verify_installed(lock, base)
                if change == 'bytes':
                    compiler.write_text('tampered compiler')
                elif change == 'mode':
                    compiler.chmod(0o644)
                elif change == 'lock':
                    receipt['lock'] = {'version': 'wrong'}
                    (base / 'installed.json').write_text(json.dumps(receipt))
                elif change == 'target':
                    target.unlink()
                else:
                    (base / 'installed.json').unlink()
                with patch.object(tool.subprocess, 'run') as run:
                    with self.assertRaises((RuntimeError, FileNotFoundError)):
                        tool.verify_installed(lock, base)
                    run.assert_not_called()


if __name__ == '__main__':
    unittest.main()
