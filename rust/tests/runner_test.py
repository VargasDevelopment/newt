"""Prove one check owns the build and pass evidence binds unchanged inputs."""
import contextlib
import importlib.util
import errno
import hashlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('rust_check', ROOT / 'rust/check.py')
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)
import run_support as support


def seed_native_headers(root):
    for name in checker.SHARED_SOURCE_FILES:
        if name.startswith('include/'):
            header = root / name
            header.parent.mkdir(parents=True, exist_ok=True)
            header.write_bytes((ROOT / name).read_bytes())


class RunnerTest(unittest.TestCase):
    def test_shared_python_comes_from_the_runtime_owner_and_seals_its_library(self):
        from types import SimpleNamespace
        from unittest.mock import Mock
        opening = {'resolved_path': str(Path(sys.executable).resolve()),
                   'runtime_sha256': 'opening-runtime'}
        changed = dict(opening, runtime_sha256='changed-library')
        owner = SimpleNamespace(runtime_identity=Mock(side_effect=[opening, dict(opening), changed]))
        spec = SimpleNamespace(loader=SimpleNamespace(exec_module=lambda module: None))
        with patch.object(support, '_python_identity', None), \
                patch.object(support.importlib.util, 'spec_from_file_location', return_value=spec), \
                patch.object(support.importlib.util, 'module_from_spec', return_value=owner):
            self.assertEqual(support.python_tool(), Path(opening['resolved_path']))
            self.assertEqual(support.python_tool(), Path(opening['resolved_path']))
            with self.assertRaisesRegex(RuntimeError, 'shared Python runtime changed'):
                support.python_tool()
        self.assertEqual(owner.runtime_identity.call_count, 3)

    def test_python_runtime_failure_names_changed_fields_and_relative_paths(self):
        from types import SimpleNamespace
        from unittest.mock import Mock
        opening = {"resolved_path": str(Path(sys.executable).resolve()), "runtime_sha256": "opening"}
        inventories = [{"lib/source.py": "old", "lib/removed.pyc": "old"},
                       {"lib/source.py": "new", "lib/added.pyc": "new"}]
        def identity(*, inventory):
            inventory.update(inventories.pop(0))
            return dict(opening, runtime_sha256="opening" if inventories else "changed")
        owner = SimpleNamespace(runtime_identity=identity)
        spec = SimpleNamespace(loader=SimpleNamespace(exec_module=Mock()))
        with patch.object(support, "_python_identity", None), \
                patch.object(support.importlib.util, "spec_from_file_location", return_value=spec), \
                patch.object(support.importlib.util, "module_from_spec", return_value=owner):
            support.python_tool()
            with self.assertRaises(RuntimeError) as error:
                support.python_tool()
        self.assertEqual(str(error.exception), "shared Python runtime changed during the check: "
                         "fields runtime_sha256; paths lib/added.pyc, lib/removed.pyc, lib/source.py")

    def test_workspace_copies_nested_members_once_in_any_order(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            source = Path(directory) / 'source'
            source.mkdir()
            for name in ('salamander/board', 'firmware', 'core'):
                folder = source / name
                folder.mkdir(parents=True, exist_ok=True)
                (folder / 'Cargo.toml').write_text('[package]\nname="fixture"\n')
                (folder / 'lib.rs').write_text(name)
            (source / 'Cargo.toml').write_text('[workspace]\nmembers=["salamander/board","core","firmware"]\n')
            (source / 'Cargo.lock').write_text('fixture')
            (source / 'tests').mkdir()
            (source / 'tests/expected.json').write_text('fixture input, not Cargo source')
            copied = Path(directory) / 'copied'
            support.copy_workspace(source, copied)
            expected = {name: value for name, value in support.source_inventory(source).items()
                        if not name.startswith('tests/')}
            self.assertEqual(expected, support.source_inventory(copied))
            self.assertFalse((copied / 'tests').exists())
            (source / 'Cargo.toml').write_text('[workspace]\nmembers=["../outside"]\n')
            invalid = Path(directory) / 'invalid'
            with self.assertRaisesRegex(RuntimeError, 'local crate member'):
                support.copy_workspace(source, invalid)
            self.assertFalse(invalid.exists())

    def test_source_tripwire_hashes_only_at_open_and_close(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            source = Path(directory) / 'source.rs'
            source.write_bytes(b'unchanged')
            with patch.object(support, 'digest', wraps=support.digest) as hashed:
                seal = support.SourceSeal(directory)
                self.assertEqual(hashed.call_count, 1)
                with support.source_commands([seal]):
                    pass
                self.assertEqual(hashed.call_count, 1)
                seal.close()
                self.assertEqual(hashed.call_count, 2)

    def test_source_tripwire_census_preserves_every_path_and_lstat_field(self):
        import stat
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            root = Path(directory)
            (root / 'empty').mkdir()
            (root / 'nested').mkdir()
            (root / 'nested/input.rs').write_bytes(b'source')
            expected = {}
            for path in [root, *root.rglob('*')]:
                entry = path.lstat()
                expected[str(path.relative_to(root))] = (stat.S_IFMT(entry.st_mode),
                    entry.st_dev, entry.st_ino, entry.st_size, entry.st_mtime_ns, entry.st_ctime_ns)
            self.assertEqual(support.source_metadata(root), expected)

    def test_baseline_commands_receive_snapshot_tripwire_and_clear_it(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            root = Path(directory)
            (root / 'rust').mkdir(); (root / 'tools').mkdir()
            path = root / 'rust/source.rs'; path.write_bytes(b'original')
            (root / 'tools/helper.py').write_text('synthetic')
            seed_native_headers(root)
            before = support.source_inventory(root)
            def baseline(*args):
                # Existing harness tests keep temporary outputs beside sealed inputs.
                (root / '.build/rust').mkdir(parents=True)
                checker.run([sys.executable, '-B', '-c',
                    'import os,sys; from pathlib import Path; p=Path(sys.argv[1]); '
                    's=p.stat(); b=p.read_bytes(); p.write_bytes(b"changed!"); '
                    'p.write_bytes(b); os.utime(p,ns=(s.st_atime_ns,s.st_mtime_ns))', path])
            with patch.object(checker, '_check_snapshot', side_effect=baseline), \
                    self.assertRaisesRegex(RuntimeError, 'sealed source metadata changed:'):
                checker.check_snapshot(None, before, root, None)
            self.assertEqual(checker._command_seals, ())

    def test_nonlanding_sdk_snapshot_rejects_restored_input_before_dispatch(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            root = Path(directory)
            for name in ('rust', 'tools', 'vendor/freeink-sdk'):
                (root / name).mkdir(parents=True)
            seed_native_headers(root)
            source = root / 'vendor/freeink-sdk/input.h'
            source.write_bytes(b'original')
            before = support.source_inventory(root)
            def baseline(*args):
                saved = source.stat()
                source.write_bytes(b'changed!'); source.write_bytes(b'original')
                os.utime(source, ns=(saved.st_atime_ns, saved.st_mtime_ns))
                checker.run([sys.executable, '-B', '-c', 'pass'])
            with patch.object(checker, '_check_snapshot', side_effect=baseline), \
                    patch.object(checker.host_jobs, 'run') as dispatch, \
                    self.assertRaisesRegex(RuntimeError, 'sealed source metadata changed:'):
                checker.check_snapshot(None, before, root, None)
            dispatch.assert_not_called()

    def test_host_command_has_a_pre_command_tripwire(self):
        guard = unittest.mock.Mock()
        guard.check.side_effect = RuntimeError('tripwire-before-command')
        with patch.object(checker, '_command_seals', (guard,)), \
                patch.object(checker.host_jobs, 'run') as dispatch, \
                self.assertRaisesRegex(RuntimeError, 'tripwire-before-command'):
            checker.run(['git', 'version'])
        dispatch.assert_not_called()

    def test_closing_failure_still_hashes_every_other_seal(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            roots = [Path(directory) / name for name in ('first', 'second')]
            for root in roots:
                root.mkdir(); (root / 'input.rs').write_bytes(b'original')
            seals = [support.SourceSeal(root) for root in roots]
            path = roots[0] / 'input.rs'; path.write_bytes(b'changed!'); path.write_bytes(b'original')
            seen = []
            original_digest = support.digest
            def track(path, entry=None):
                seen.append(path)
                return original_digest(path, entry)
            with patch.object(support, 'digest', side_effect=track), \
                    self.assertRaisesRegex(RuntimeError, 'sealed source metadata changed:'):
                support.close_sources(seals)
            self.assertEqual(seen, [root / 'input.rs' for root in roots])

    def test_closing_metadata_failure_precedes_evidence_replacement(self):
        import ast
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            base = Path(directory); root = base / 'source'; root.mkdir()
            path = root / 'input.rs'; path.write_bytes(b'original')
            before = support.source_inventory(root)
            seal = support.SourceSeal(root)
            path.write_bytes(b'changed!'); path.write_bytes(b'original')
            output = base / 'evidence.json'; output.write_bytes(b'previous evidence')
            tree = ast.parse(Path(checker.__file__).read_text())
            function = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == '_check_snapshot')
            starts = [i for i, n in enumerate(function.body) if isinstance(n, ast.Expr) and
                      isinstance(n.value, ast.Call) and isinstance(n.value.func, ast.Name) and
                      n.value.func.id == 'verify_sources']
            self.assertEqual(len(starts), 1)
            tail = ast.Module(body=function.body[starts[0]:], type_ignores=[])
            context = dict(checker.__dict__, snapshot=root, before=before, output=output,
                           evidence={'synthetic': True}, artifact=base / 'object', ROOT=base,
                           verify_sources=lambda before: None, _command_seals=(seal,))
            with self.assertRaisesRegex(RuntimeError, 'sealed source metadata changed:'):
                exec(compile(tail, checker.__file__, 'exec'), context)
            self.assertEqual(output.read_bytes(), b'previous evidence')
            self.assertFalse(output.with_suffix('.json.partial').exists())

    def test_command_tripwire_rejects_restored_bytes_and_mtime(self):
        code = ('import os,sys; from pathlib import Path; '
                'p=Path(sys.argv[1]); offset=int(sys.argv[2]); before=p.stat(); '
                'original=p.read_bytes(); changed=bytearray(original); '
                'changed[offset]^=1; p.write_bytes(changed); p.write_bytes(original); '
                'os.utime(p,ns=(before.st_atime_ns,before.st_mtime_ns))')
        for size, offset in ((31, 7), (1048576, 65553)):
            with self.subTest(size=size), tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
                source = Path(directory) / 'unused.rs'
                source.write_bytes(b'a' * size)
                seal = support.SourceSeal(directory)
                with self.assertRaisesRegex(RuntimeError, 'sealed source metadata changed:') as error:
                    with support.source_commands([seal]):
                        checker.run([sys.executable, '-B', '-c', code, source, str(offset)], cwd=directory)
                self.assertEqual(json.loads(str(error.exception).split(': ', 1)[1])['paths'], ['unused.rs'])
                self.assertEqual(source.read_bytes(), b'a' * size)
                self.assertEqual(source.stat().st_mtime_ns, seal.metadata['unused.rs'][-2])

    def test_tripwire_rejects_namespace_identity_and_type_changes(self):
        for change in ('hidden', 'delete', 'replace', 'symlink', 'directory', 'fifo', 'root-symlink'):
            with self.subTest(change=change), tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
                base = Path(directory)
                root = base / 'source'; root.mkdir()
                source = root / 'entry.rs'; source.write_bytes(b'original')
                seal = support.SourceSeal(root)
                if change == 'hidden':
                    (root / '.added').write_bytes(b'extra')
                elif change == 'delete':
                    source.unlink()
                elif change == 'replace':
                    source.replace(base / 'old'); source.write_bytes(b'original')
                elif change == 'symlink':
                    source.replace(base / 'old'); source.symlink_to(base / 'old')
                elif change == 'directory':
                    (root / 'empty').mkdir()
                elif change == 'fifo':
                    source.unlink(); os.mkfifo(source)
                else:
                    root.replace(base / 'old'); root.symlink_to(base / 'old', target_is_directory=True)
                with self.assertRaises((RuntimeError, FileNotFoundError)):
                    seal.check()

    def test_tripwire_and_closing_hash_guard_removal_controls(self):
        import types
        source_text = Path(support.__file__).read_text()
        faults = [('ctime-removed', 'entry.st_size, entry.st_mtime_ns, entry.st_ctime_ns)',
                   'entry.st_size, entry.st_mtime_ns)'),
                  ('closing-hash-removed', '        verify_source_inventory(self.root, self.hashes)\n',
                   '        pass\n')]
        for name, old, new in faults:
            self.assertEqual(source_text.count(old), 1)
            module = types.ModuleType('tripwire_' + name.replace('-', '_'))
            module.__file__ = support.__file__
            exec(compile(source_text.replace(old, new, 1), support.__file__, 'exec'), module.__dict__)
            for implementation, should_reject in ((support, True), (module, False)):
                with self.subTest(control=name, baseline=should_reject), \
                        tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
                    path = Path(directory) / 'source.rs'; path.write_bytes(b'original')
                    seal = implementation.SourceSeal(directory)
                    before = path.stat()
                    path.write_bytes(b'changed!')
                    if name == 'ctime-removed':
                        path.write_bytes(b'original')
                        os.utime(path, ns=(before.st_atime_ns, before.st_mtime_ns))
                        check = seal.check
                    else:
                        seal.check = lambda: None  # Prove closing bytes independently of metadata.
                        check = seal.close
                    rejected = False
                    try:
                        check()
                    except RuntimeError as error:
                        self.assertIn('sealed source', str(error))
                        rejected = True
                    self.assertEqual(rejected, should_reject, name)

    def test_sealed_digest_matches_streaming_sha256_at_size_boundaries(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            source = Path(directory) / 'source.rs'
            data = bytes(range(256)) * 4096
            for size in (0, 1, 65535, 65536, 65537, len(data)):
                with self.subTest(size=size):
                    source.write_bytes(data[:size])
                    with source.open('rb') as stream:
                        expected = hashlib.file_digest(stream, 'sha256').hexdigest()
                    self.assertEqual(support.digest(source), expected)
                    self.assertEqual(support.source_inventory(Path(directory)),
                                     {'source.rs': expected})

    def test_closing_seal_rejects_mid_command_edits_with_restored_timestamps(self):
        code = ('import os,sys; from pathlib import Path; '
                'p=Path(sys.argv[1]); offset=int(sys.argv[2]); before=p.stat(); '
                'data=bytearray(p.read_bytes()); data[offset]^=1; p.write_bytes(data); '
                'os.utime(p, ns=(before.st_atime_ns,before.st_mtime_ns))')
        for size, offset in ((31, 7), (1048576, 65553)):
            with self.subTest(size=size, offset=offset), \
                    tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
                source = Path(directory) / 'unused.rs'
                source.write_bytes(b'a' * size)
                before = checker.source_inventory(Path(directory))
                metadata = source.stat()
                result = checker.run([sys.executable, '-B', '-c', code, source, str(offset)],
                                     cwd=directory)
                self.assertEqual(result.returncode, 0)
                self.assertEqual(source.stat().st_size, metadata.st_size)
                self.assertEqual(source.stat().st_atime_ns, metadata.st_atime_ns)
                self.assertEqual(source.stat().st_mtime_ns, metadata.st_mtime_ns)
                with self.assertRaisesRegex(RuntimeError, 'sealed source inventory changed:') as error:
                    checker.verify_source_inventory(Path(directory), before)
                changes = json.loads(str(error.exception).split(': ', 1)[1])
                self.assertEqual(changes, {'modified': {'count': 1, 'paths': ['unused.rs']}})

    def test_seal_rejects_a_symlink_swapped_in_after_lstat(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            root = Path(directory) / 'source'; root.mkdir()
            source = root / 'entry.rs'; source.write_bytes(b'original')
            retained = Path(directory) / 'original.rs'
            original_open = os.open
            def swap(path, flags, *args, **kwargs):
                if Path(path) == source:
                    source.replace(retained)
                    source.symlink_to(retained)
                return original_open(path, flags, *args, **kwargs)
            with patch.object(support.os, 'open', side_effect=swap), \
                    self.assertRaises(OSError) as error:
                support.source_inventory(root)
            self.assertEqual(error.exception.errno, errno.ELOOP)
            self.assertEqual(error.exception.filename, str(source))

    def test_seal_rejects_a_different_open_inode_even_with_identical_bytes(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            root = Path(directory) / 'source'; root.mkdir()
            source = root / 'entry.rs'; source.write_bytes(b'original')
            retained = Path(directory) / 'original.rs'
            original_open = os.open
            def swap(path, flags, *args, **kwargs):
                if Path(path) == source:
                    source.replace(retained)
                    source.write_bytes(b'original')
                return original_open(path, flags, *args, **kwargs)
            with patch.object(support.os, 'open', side_effect=swap), \
                    self.assertRaisesRegex(RuntimeError, 'sealed source file changed before hashing:') as error:
                support.source_inventory(root)
            self.assertEqual(str(error.exception),
                             'sealed source file changed before hashing: ' + str(source))

    def test_rejection_launcher_admission_precedes_build_snapshot(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            root = Path(directory)
            (root / 'rust/tests/new-family').mkdir(parents=True)
            (root / 'tools').mkdir()
            for name in ('rust-toolchain.py', 'host_jobs.py', 'rust_gate_families.py', 'python_runtime.py'):
                (root / 'tools' / name).write_text('helper')
            driver = root / 'rust/tests/new-family/rejection.py'
            driver.write_text('support.inherited_child([])\n')
            with self.assertRaisesRegex(RuntimeError, 'Rejection driver ownership'):
                seed_native_headers(root)
                checker.source_hashes(root)
            driver.write_text('inherited_child = support.inherited_child\ninherited_child([])\n')
            checker.source_hashes(root)
            driver.write_text('import subprocess\ninherited_child([])\n')
            with self.assertRaisesRegex(RuntimeError, 'Rejection driver ownership'):
                seed_native_headers(root)
                checker.source_hashes(root)

    def test_live_edits_change_recorded_hashes_and_expected_edits_still_fail(self):
        import shutil
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            root = Path(directory)
            (root / 'rust').mkdir(); (root / 'tools').mkdir()
            for name in ('rust-toolchain.py', 'host_jobs.py', 'rust_gate_families.py', 'python_runtime.py'):
                (root / 'tools' / name).write_text('helper')
            for name in ('core.rs', 'check.py', 'font-sidecar.rs'):
                source = root / 'rust' / name; source.write_text('original')
                (root / 'rust/source_bindings.json').write_text(json.dumps(
                    {name: hashlib.sha256(source.read_bytes()).hexdigest()}))
                seed_native_headers(root)
                before = checker.source_hashes(root)
                source.write_text('legitimate edit')
                after = checker.source_hashes(root)
                self.assertNotEqual(before['rust/' + name], after['rust/' + name])
            for relative, missing in [('tests/xml_session/expected.json', False),
                                      ('tests/font_layout/expected.json', True),
                                      ('tests/foundation-restarts-expected.json.zlib', False)]:
                source = root / 'rust' / relative
                source.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / 'rust' / relative, source)
                original = source.read_bytes()
                seed_native_headers(root)
                before = checker.source_hashes(root)
                if missing: source.unlink()
                else: source.write_bytes(original + b'changed expected output')
                with self.assertRaisesRegex(RuntimeError, 'sources changed during the check'):
                    checker.verify_sources(before, root)
                source.write_bytes(original)

    def test_native_expat_tampering_is_rejected_before_child_dispatch(self):
        import shutil
        from types import SimpleNamespace
        book = 'libs/book/FreeInkBook/'
        for family, relative, missing in (
                ('xml_session', book + 'third_party/expat/expat.h', False),
                ('package_parsers', book + 'src/vendor/expat_xmlparse.c', True)):
            with self.subTest(family=family), tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
                root = Path(directory)
                packet = root / 'rust/tests' / family
                packet.mkdir(parents=True)
                origin = ROOT / 'rust/tests' / family
                shutil.copyfile(origin / 'PROVENANCE.json', packet / 'PROVENANCE.json')
                sdk = root / 'vendor/freeink-sdk'
                for entry in json.loads((origin / 'PROVENANCE.json').read_text())['files']:
                    name = entry['path']
                    if name.startswith(book + 'third_party/expat/') or name.startswith(book + 'src/vendor/expat_'):
                        target = sdk / name
                        target.parent.mkdir(parents=True, exist_ok=True)
                        shutil.copyfile(ROOT / 'vendor/freeink-sdk' / name, target)
                helper = root / 'rust/tests/xml_session/native_inputs.py'
                helper.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / 'rust/tests/xml_session/native_inputs.py', helper)
                spec = importlib.util.spec_from_file_location('native_input_gate', ROOT / 'rust' / (family + '_gate.py'))
                gate = importlib.util.module_from_spec(spec); spec.loader.exec_module(gate)
                gate.load(helper, 'native_input_positive').verify(packet, sdk)
                source = sdk / relative
                if missing: source.unlink()
                else: source.write_bytes(source.read_bytes() + b'changed native source')
                failure = self.assertRaises(FileNotFoundError) if missing else self.assertRaisesRegex(ValueError, 'Native Expat source changed')
                with patch('subprocess.Popen') as dispatch, failure as result:
                    gate.check(SimpleNamespace(ROOT=root, BUILD=root / 'build', SCRATCH=root / 'build'), root, True)
                dispatch.assert_not_called()
                if missing: self.assertEqual(Path(result.exception.filename), source)

    def test_default_child_environment_keeps_imported_source_tree_sealed(self):
        import sys
        from unittest.mock import patch
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            source = Path(directory)
            module = source / 'fixture.py'
            module.write_text('VALUE = 42\n')
            before = checker.source_inventory(source)
            code = ('import importlib.util; '
                    f'spec = importlib.util.spec_from_file_location("fixture", {str(module)!r}); '
                    'module = importlib.util.module_from_spec(spec); '
                    'spec.loader.exec_module(module); print(module.VALUE)')
            # Parent -B is not inherited by a fresh interpreter. The helper must
            # prevent bytecode itself, even without the caller supplying -B.
            with patch.dict(checker.os.environ, {'PYTHONDONTWRITEBYTECODE': ''}):
                result = checker.run([sys.executable, '-c', code], cwd=source)
            self.assertEqual(result.stdout, '42\n')
            checker.verify_source_inventory(source, before)

    def test_mutations_cover_affected_families_and_unknown_modules_fall_back_to_all(self):
        cases = ['progress_encode 0', 'progress_plan 0', 'store 0',
                 'preferences_store 0', 'frame_bw 0', 'clock_time 0']
        self.assertEqual(checker.control_indices('core/src/progress.rs', cases), [0, 1])
        self.assertEqual(checker.control_indices('core/src/durable.rs', cases), [2, 3])
        self.assertEqual(checker.control_indices('core/src/frame.rs', cases), [4])
        self.assertEqual(checker.control_indices('core/src/future.rs', cases), list(range(6)))
        with self.assertRaisesRegex(RuntimeError, 'no affected-family cases'):
            checker.control_indices('core/src/frame.rs', ['store 0'])

    def test_mutants_stop_at_first_witness_and_count_actual_processed_rows(self):
        from unittest.mock import patch
        cases = [f'case {i}' for i in range(5)]
        expected = ['ok'] * 5
        with patch.object(checker, 'responses', side_effect=[['ok', 'ok'], ['bad', 'ok']]) as invoke:
            mismatch, checked = checker.compare_batches('binary', cases, expected, 2)
            self.assertEqual((mismatch[0], checked), (2, 4))
            self.assertEqual(invoke.call_count, 2)
        with patch.object(checker, 'responses', side_effect=[['ok', 'ok'], ['ok', 'ok'], ['ok']]) as invoke:
            self.assertEqual(checker.compare_batches('binary', cases, expected, 2), (None, 5))
            self.assertEqual(invoke.call_count, 3)
        with self.assertRaises(ValueError):
            checker.compare_batches('binary', cases, expected, 0)

    def test_digest_expected_reply_detects_single_byte_flip(self):
        raw = '1 ' + 'a5' * 64
        digest = 'sha256:' + hashlib.sha256(raw.encode('utf-8')).hexdigest()
        flipped = raw[:-1] + ('4' if raw[-1] == '5' else '5')
        self.assertTrue(checker.reply_matches(digest, raw))
        self.assertFalse(checker.reply_matches(digest, flipped))
        self.assertFalse(checker.reply_matches(digest, digest))
        with patch.object(checker, 'responses', return_value=[raw]):
            self.assertEqual(checker.compare_batches('binary', ['frame_bw 0'], [digest], 1), (None, 1))
        with patch.object(checker, 'responses', return_value=[flipped]):
            mismatch, checked = checker.compare_batches('binary', ['frame_bw 0'], [digest], 1)
            self.assertEqual((mismatch[0], checked), (0, 1))

    def test_large_mutation_receipts_bind_input_without_storing_a_frame_fixture(self):
        import hashlib
        case = 'frame_bw ' + 'ab' * 52272
        receipt = checker.control_receipt('polarity', 42, case, 139)
        self.assertNotIn('input', receipt)
        self.assertEqual(receipt['input_sha256'], hashlib.sha256(case.encode()).hexdigest())
        self.assertEqual(len(receipt['input_prefix']), 2048)
        self.assertEqual(checker.control_receipt('small', 0, 'store 0', 1)['input'], 'store 0')

    def test_evidence_stays_in_build_and_feedback_cannot_replace_complete_evidence(self):
        complete = checker.evidence_path(False)
        feedback = checker.evidence_path(True)
        self.assertEqual(complete.parent, checker.BUILD)
        self.assertEqual(feedback.parent, checker.BUILD)
        self.assertNotEqual(complete, feedback)

    def test_incremental_mutants_must_report_fresh_compilations_for_both_crates(self):
        workspace = Path('/test/rust')
        def artifact(name, fresh=False):
            return json.dumps({'reason': 'compiler-artifact',
                               'manifest_path': str(workspace / name / 'Cargo.toml'),
                               'fresh': fresh})
        valid = artifact('core') + '\n' + artifact('host')
        checker.require_mutant_compilation(valid, workspace)
        for invalid in [artifact('core'), artifact('core', True) + '\n' + artifact('host'),
                        artifact('core') + '\n' + artifact('host', True)]:
            with self.assertRaisesRegex(RuntimeError, 'Mutation build'):
                checker.require_mutant_compilation(invalid, workspace)

    def test_a_finished_check_leaves_no_temporary_folder_even_after_a_failure(self):
        # #345: only receipts, locks and reused caches stay in .build/rust.
        import fcntl
        def snapshot(before, destination, root=None):
            (destination / 'rust').mkdir(parents=True)
            (destination / 'rust/toolchain.json').write_text('{}')
        def family(failure):
            def run(*args):
                # As a family's mkdtemp, its command logs and cargo's TMPDIR do.
                family_folder = Path(tempfile.mkdtemp(prefix='css-', dir=checker.SCRATCH))
                (family_folder / 'mutant-target').mkdir()
                (family_folder / '000.stderr').write_text('error[E0425]: the failing diagnostic\n')
                (Path(checker.environment()['TMPDIR']) / 'rustc-partial').write_text('partial')
                if failure:
                    raise failure
            return run
        default = checker.SCRATCH
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            build = Path(directory)
            (build / 'phase-inflate.tmp/target').mkdir(parents=True)  # its run was killed outright
            (build / 'phase-xml_input.tmp').mkdir()  # its run is live
            with (build / 'phase-xml_input.lock').open('a') as live:
                fcntl.flock(live, fcntl.LOCK_EX)
                for failure in (None, RuntimeError('family failed'), KeyboardInterrupt()):
                    with self.subTest(failure=failure), patch.object(checker, 'BUILD', build), \
                            patch.object(checker, 'source_hashes', return_value={}), \
                            patch.object(checker, 'snapshot_sources', side_effect=snapshot), \
                            patch.object(checker.toolchain, 'verify_installed', return_value={}), \
                            patch.object(checker, 'check_snapshot', side_effect=family(failure)):
                        args = checker.parse_args(['--family=css', '--tier=fast', '--control-family=css'])
                        shown = io.StringIO()
                        try:
                            with contextlib.redirect_stderr(shown):
                                checker.check(args)
                        except (RuntimeError, KeyboardInterrupt) as error:
                            self.assertIs(error, failure)
                        # The folder goes, so a failure prints its diagnostics first.
                        self.assertEqual('the failing diagnostic' in shown.getvalue(), failure is not None)
                        self.assertEqual(sorted(path.name for path in build.iterdir()),
                                         ['phase-css.lock', 'phase-inflate.lock', 'phase-xml_input.lock',
                                          'phase-xml_input.tmp'])
                        self.assertEqual(checker.SCRATCH, default)

    def test_only_runs_that_build_the_shared_target_hold_its_lock(self):
        import fcntl
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory, \
                patch.object(checker, 'BUILD', Path(directory)):
            lock = Path(directory) / 'target.lock'
            for arguments, holds in ((['--landing'], True), (['--target-probes'], True),
                                     (['--family=css', '--tier=fast', '--control-family=css'], False),
                                     (['--foundation-controls=records'], False)):
                with self.subTest(arguments=arguments), checker.shared_target(checker.parse_args(arguments)):
                    with lock.open('a') as other:
                        try:
                            fcntl.flock(other, fcntl.LOCK_EX | fcntl.LOCK_NB)
                            held = False
                            fcntl.flock(other, fcntl.LOCK_UN)
                        except BlockingIOError:
                            held = True
                    self.assertEqual(held, holds)

    def test_edits_additions_and_deletions_invalidate_source_binding(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            root = Path(directory)
            (root / 'rust').mkdir(); (root / 'tools').mkdir()
            source = root / 'rust/core.rs'; source.write_text('original')
            (root / 'tools/rust-toolchain.py').write_text('installer')
            (root / 'tools/host_jobs.py').write_text('host jobs')
            (root / 'tools/rust_gate_families.py').write_text('family registry')
            (root / 'tools/python_runtime.py').write_text('runtime owner')
            seed_native_headers(root)
            before = checker.source_hashes(root)
            checker.verify_sources(before, root)
            source.write_text('changed')
            with self.assertRaisesRegex(RuntimeError, 'sources changed'):
                checker.verify_sources(before, root)
            source.write_text('original')
            added = root / 'rust/added.rs'; added.write_text('sibling')
            with self.assertRaisesRegex(RuntimeError, 'sources changed'):
                checker.verify_sources(before, root)
            added.unlink(); source.unlink()
            with self.assertRaisesRegex(RuntimeError, 'sources changed'):
                checker.verify_sources(before, root)

    def test_build_snapshot_survives_a_live_edit_and_revert(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            root = Path(directory) / 'live'; frozen = Path(directory) / 'frozen'
            (root / 'rust').mkdir(parents=True); (root / 'tools').mkdir()
            source = root / 'rust/core.rs'; source.write_text('original')
            for helper in ['rust-toolchain.py', 'host_jobs.py', 'rust_gate_families.py', 'python_runtime.py']:
                (root / 'tools' / helper).write_text('helper')
            seed_native_headers(root)
            before = checker.source_hashes(root)
            checker.snapshot_sources(before, frozen, root)
            source.write_text('transient edit')
            self.assertEqual((frozen / 'rust/core.rs').read_text(), 'original')
            source.write_text('original')
            checker.verify_sources(before, root)
            self.assertEqual(checker.source_hashes(frozen), before)

    def test_toolchain_lease_excludes_installer_from_checks_and_checks_from_installer(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            base = Path(directory)
            with checker.toolchain.lease(shared=True, base=base):
                with self.assertRaisesRegex(RuntimeError, 'toolchain is in use'):
                    with checker.toolchain.lease(base=base):
                        self.fail('installer acquired a leased toolchain')
            with checker.toolchain.lease(base=base):
                with self.assertRaisesRegex(RuntimeError, 'toolchain is in use'):
                    with checker.toolchain.lease(shared=True, base=base):
                        self.fail('check acquired an installing toolchain')

    def test_toolchain_receipt_binds_lock_and_all_installed_files(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.build/rust') as directory:
            base = Path(directory); prefix = base / 'toolchain'
            (prefix / 'bin').mkdir(parents=True); (prefix / 'lib').mkdir()
            for name, output in [('rustc', 'release: 1.93.1'), ('cargo', 'cargo 1.93.1 (test)')]:
                script = prefix / 'bin' / name
                script.write_text("#!/bin/sh\nprintf '%s\\n' '" + output + "'\n")
                script.chmod(0o700)
            library = prefix / 'lib/core.rlib'; library.write_bytes(b'original')
            lock = {'version': '1.93.1', 'components': []}
            receipt = {'lock': lock, 'files': checker.toolchain.installed_identity(prefix)}
            (base / 'installed.json').write_text(json.dumps(receipt))
            checker.toolchain.verify_installed(lock, base)
            library.write_bytes(b'modified')
            with self.assertRaisesRegex(RuntimeError, 'files differ'):
                checker.toolchain.verify_installed(lock, base)
            library.write_bytes(b'original')
            extra = prefix / 'lib/extra.rlib'; extra.write_bytes(b'unexpected')
            with self.assertRaisesRegex(RuntimeError, 'files differ'):
                checker.toolchain.verify_installed(lock, base)
            extra.unlink()
            with self.assertRaisesRegex(RuntimeError, 'receipt differs'):
                checker.toolchain.verify_installed({'version': '1.94.0'}, base)




class TierSelection(unittest.TestCase):
    def parse_failure(self, arguments):
        import contextlib
        import io
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as error:
            checker.parse_args(arguments)
        self.assertEqual(error.exception.code, 2)

    def test_default_and_explicit_full_select_every_family(self):
        default = checker.parse_args([])
        explicit = checker.parse_args(['--tier', 'full'])
        self.assertEqual(default.control_families, explicit.control_families)
        self.assertEqual(len(default.control_families), 20)
        for family in default.control_families:
            self.assertFalse(checker.positive_only_for(default, family))
        self.assertTrue(checker.selection_evidence(default)['complete_rust_gate'])

    def test_fast_selection_has_correct_positive_only_polarity(self):
        args = checker.parse_args(['--tier', 'fast', '--control-family', 'foundation',
                                   '--control-family', 'xml_session'])
        for family in checker.parse_args([]).control_families:
            self.assertEqual(checker.positive_only_for(args, family),
                             family not in ('foundation', 'xml_session'))
        receipt = checker.selection_evidence(args)
        self.assertEqual(receipt['tier'], 'fast')
        self.assertEqual(receipt['selected_control_families'], ['foundation', 'xml_session'])
        self.assertEqual(receipt['requested_control_families'], ['foundation', 'xml_session'])
        self.assertFalse(receipt['complete_rust_gate'])

    def test_fast_all_and_explicit_none_remain_partial(self):
        for selector in ('all', 'none'):
            args = checker.parse_args(['--tier', 'fast', '--control-family', selector])
            for family in checker.parse_args([]).control_families:
                self.assertEqual(checker.positive_only_for(args, family), selector == 'none')
            receipt = checker.selection_evidence(args)
            self.assertEqual(receipt['requested_control_families'], [selector])
            self.assertFalse(receipt['complete_rust_gate'])

    def test_invalid_or_ambiguous_selection_is_rejected(self):
        for arguments in (['--tier', 'fast'], ['--tier', 'fast', '--control-family', 'unknown'],
                          ['--control-family', 'none'],
                          ['--tier', 'full', '--control-family', 'all'],
                          ['--tier', 'fast', '--control-family', 'all', '--control-family', 'css'],
                          ['--tier', 'fast', '--control-family', 'none', '--control-family', 'css'],
                          ['--positive-only', '--tier', 'fast'],
                          ['--positive-only', '--control-family', 'none']):
            with self.subTest(arguments=arguments):
                self.parse_failure(arguments)

    def test_evidence_paths_and_legacy_feedback_are_separate(self):
        modes = ([], ['--tier', 'fast', '--control-family', 'all'], ['--positive-only'])
        outputs = []
        for arguments in modes:
            args = checker.parse_args(arguments)
            outputs.append(checker.evidence_path(args.positive_only, args.tier))
        self.assertEqual([p.name for p in outputs],
                         ['evidence.json', 'evidence-fast.json', 'positive-feedback.json'])
        self.assertEqual(len(set(outputs)), 3)
        legacy = checker.parse_args(['--positive-only'])
        self.assertEqual(legacy.control_families, ())
        self.assertFalse(checker.selection_evidence(legacy)['complete_rust_gate'])
        for family in checker.parse_args([]).control_families:
            self.assertTrue(checker.positive_only_for(legacy, family))


class ExternalSelection(unittest.TestCase):
    def test_external_controls_require_an_external_witness(self):
        cases = ['progress_store example', 'xp.load 00', 'xp.save 00']
        self.assertEqual(checker.control_indices('core/src/progress_store.rs',cases,'external-durable-reconcile'),[1,2])
        self.assertEqual(checker.control_indices('core/src/progress_store.rs',cases),[0,1,2])

if __name__ == '__main__':
    (ROOT / '.build/rust').mkdir(parents=True, exist_ok=True)
    unittest.main()
