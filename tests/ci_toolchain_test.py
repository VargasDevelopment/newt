"""Small authored fixtures for CI acquisition boundaries; never install or build."""
import base64
import hashlib
import io
import json
import os
from pathlib import Path
import stat
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import ci_toolchain as ci


class CiToolchainTests(unittest.TestCase):
    def fixture(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name).resolve()
        (root / 'docs').mkdir()
        return root

    def record(self, name, content=b'authored archive bytes'):
        return {'filename': name, 'size': len(content), 'sha256': hashlib.sha256(content).hexdigest(),
                'url': 'https://github.com/example/project/releases/download/v1/' + name}

    def lock(self, root):
        names = {ci.PLATFORM_ARCHIVE, ci.CORE_ARCHIVE}
        names.update(name for pair in ci.ARCHIVES.values() for name in pair if name)
        records = [self.record(name) for name in sorted(names)]
        records.extend(self.record('fixture-' + str(n) + '-py3-none-any.whl') for n in range(51))
        result = {'artifacts': records, 'download_bytes': sum(item['size'] for item in records)}
        (root / 'docs/toolchain-lock.json').write_text(json.dumps(result))
        return result

    def zip(self, root, entries):
        archive = root / 'fixture.zip'
        with zipfile.ZipFile(archive, 'w') as stream:
            for name, content in entries:
                stream.writestr(name, content)
        return archive

    def linux_lock(self, root):
        names = {ci.PLATFORM_ARCHIVE, ci.CORE_ARCHIVE}
        names.update(name for pair in ci.archives('linux-x86_64').values() for name in pair if name)
        records = [self.record(name) for name in sorted(names)]
        # A small authoritative Linux inventory proves counts are recipe-derived.
        wheels = [dict(self.record(name + '-1.0-py3-none-any.whl'), requirement=name + '==1.0')
                  for name in ('shared-native-dependency', 'linux-only-dependency')]
        records.extend(wheels)
        result = {'target': 'linux-x86_64', 'python': '3.12.6', 'artifacts': records,
                  'wheel_requirements': [item['requirement'] for item in wheels],
                  'download_bytes': sum(item['size'] for item in records)}
        (root / 'docs/toolchain-linux-lock.json').write_text(json.dumps(result))
        (root / 'docs/toolchain-python-linux.lock').write_text('\n'.join(
            item['requirement'] + ' --hash=sha256:' + item['sha256'] for item in wheels) + '\n')
        (root / 'docs/toolchain-python.lock').write_text(
            wheels[0]['requirement'] + ' --hash=sha256:' + wheels[0]['sha256'] + '\n')
        return result

    def test_linux_inventory_is_explicit_and_counts_follow_its_recipe(self):
        root = self.fixture()
        lock = self.linux_lock(root)
        report = ci.plan(root, 'linux-x86_64')
        self.assertEqual(report['artifact_count'], len(lock['artifacts']))
        self.assertEqual(report['wheel_count'], 2)
        self.assertEqual(report['host'], 'Linux x86_64')
        self.assertEqual(report['python'], '3.12.6')
        self.assertFalse(report['proof_reuse_available'])
        self.assertFalse((root / '.toolchain').exists())
        actual = ci.plan(ROOT, 'linux-x86_64')
        self.assertEqual(actual['wheel_count'], 51)
        self.assertEqual(actual['download_bytes'], 640088158)
        names = [item['filename'] for item in actual['artifacts']]
        self.assertIn('riscv32-esp-elf-14.2.0_20251107-x86_64-linux-gnu.tar.xz', names)
        self.assertFalse(any('macosx' in name or 'apple-darwin' in name for name in names))
        self.assertEqual(ci.plan(ROOT)['host'], 'Darwin arm64')

    def test_linux_hash_constraints_and_shared_versions_fail_closed(self):
        root = self.fixture()
        self.linux_lock(root)
        (root / 'docs/toolchain-python-linux.lock').write_text('changed==1.0 --hash=sha256:' + '0' * 64)
        with self.assertRaises(RuntimeError):
            ci.read_lock(root, 'linux-x86_64')
        root = self.fixture()
        lock = self.linux_lock(root)
        item = lock['artifacts'].pop(-2)
        lock['wheel_requirements'].remove(item['requirement'])
        lock['download_bytes'] -= item['size']
        (root / 'docs/toolchain-linux-lock.json').write_text(json.dumps(lock))
        remaining = lock['artifacts'][-1]
        (root / 'docs/toolchain-python-linux.lock').write_text(
            remaining['requirement'] + ' --hash=sha256:' + remaining['sha256'])
        with self.assertRaises(RuntimeError):
            ci.read_lock(root, 'linux-x86_64')
        with self.assertRaises(RuntimeError):
            ci.plan(ROOT, 'unreviewed-platform')

    def test_linux_runner_requires_exact_python_architecture_and_glibc_floor(self):
        with patch.dict(os.environ, {'GITHUB_ACTIONS': 'true'}), \
             patch.object(ci.platform, 'system', return_value='Linux'), \
             patch.object(ci.platform, 'machine', return_value='x86_64'), \
             patch.object(ci.sys, 'version_info', (3, 12, 6)), \
             patch.object(ci.platform, 'libc_ver', return_value=('glibc', '2.35')):
            ci.require_runner('linux-x86_64')
            for library in (('glibc', '2.34'), ('musl', '2.40')):
                with patch.object(ci.platform, 'libc_ver', return_value=library):
                    with self.assertRaises(RuntimeError):
                        ci.require_runner('linux-x86_64')
            with patch.object(ci.sys, 'version_info', (3, 12, 10)):
                with self.assertRaises(RuntimeError):
                    ci.require_runner('linux-x86_64')

    def test_linux_release_inventory_preserves_exact_requirements(self):
        release = json.loads((ROOT / '.github/ci-release-linux-lock.json').read_text())
        pins = [line for line in (ROOT / 'requirements-release.txt').read_text().splitlines()
                if line and not line.startswith('#')]
        self.assertEqual([item['requirement'] for item in release['artifacts']], pins)
        self.assertEqual(release['python'], '3.12.6')
        self.assertFalse(any('macosx' in item['filename'] for item in release['artifacts']))
        for item in release['artifacts']:
            ci.validate_record(item)

    def tar(self, root, entries):
        archive = root / 'fixture.tar.gz'
        with tarfile.open(archive, 'w:gz') as stream:
            for name, content, kind, link in entries:
                item = tarfile.TarInfo(name)
                item.type, item.linkname = kind, link
                item.mode = 0o755 if name.endswith('/compiler') else 0o644
                item.size = len(content) if kind == tarfile.REGTYPE else 0
                stream.addfile(item, io.BytesIO(content) if kind == tarfile.REGTYPE else None)
        return archive

    def test_plan_reads_exact_inventory_without_creating_outputs(self):
        root = self.fixture()
        lock = self.lock(root)
        report = ci.plan(root)
        self.assertEqual(len(report['artifacts']), 63)
        self.assertEqual(report['artifacts'], lock['artifacts'])
        self.assertEqual(report['python'], '3.12.6')
        self.assertEqual(report['host'], 'Darwin arm64')
        self.assertTrue(report['fresh_gates_required'])
        self.assertFalse(report['proof_reuse_available'])
        self.assertEqual(sorted(p.name for p in root.iterdir()), ['docs'])
        actual = ci.plan(ROOT)
        self.assertEqual(actual['download_bytes'], 614665932)
        self.assertEqual(len(actual['artifacts']), 63)

    def test_lock_rejects_changed_inventory_and_unsafe_pins(self):
        mutations = [lambda lock: lock['artifacts'].pop(),
                     lambda lock: lock['artifacts'][0].update(filename='../outside'),
                     lambda lock: lock['artifacts'][0].update(url='http://github.com/package'),
                     lambda lock: lock['artifacts'][0].update(url='https://user:secret@github.com/package'),
                     lambda lock: lock['artifacts'][0].update(sha256='not pinned'),
                     lambda lock: lock['artifacts'][0].update(size=True),
                     lambda lock: lock.update(download_bytes=0)]
        for mutate in mutations:
            with self.subTest(mutation=mutate):
                root = self.fixture()
                lock = self.lock(root)
                mutate(lock)
                (root / 'docs/toolchain-lock.json').write_text(json.dumps(lock))
                with self.assertRaises(RuntimeError):
                    ci.plan(root)
        root = self.fixture()
        self.lock(root)
        (root / 'docs/toolchain-lock.json').write_text('{"artifacts":[],"artifacts":[]}')
        with self.assertRaises(RuntimeError):
            ci.read_lock(root)

    def test_cache_rechecks_size_and_digest_and_never_executes_cached_files(self):
        for content in (b'short', b'x' * len(b'authored archive bytes')):
            with self.subTest(content=content):
                root = self.fixture()
                cache = root / 'downloads'
                cache.mkdir()
                (cache / 'input.zip').write_bytes(content)
                with patch.object(ci.urllib.request, 'build_opener') as network:
                    with self.assertRaises(RuntimeError):
                        ci.fetch(root, cache, self.record('input.zip'))
                    network.assert_not_called()
        root = self.fixture()
        cache = root / 'downloads'
        cache.mkdir()
        (cache / 'input.zip').write_bytes(b'authored archive bytes')
        with patch.object(ci.urllib.request, 'build_opener') as network:
            self.assertEqual(ci.fetch(root, cache, self.record('input.zip')), cache / 'input.zip')
            network.assert_not_called()

    def test_cache_aliases_reject_before_read_or_network(self):
        for kind in ('symlink', 'hardlink', 'directory', 'parent-alias'):
            with self.subTest(kind=kind):
                root = self.fixture()
                protected = root / 'protected'
                protected.write_bytes(b'authored archive bytes')
                cache = root / 'downloads'
                if kind == 'parent-alias':
                    cache.symlink_to(root / 'docs', target_is_directory=True)
                else:
                    cache.mkdir()
                    target = cache / 'input.zip'
                    if kind == 'symlink':
                        target.symlink_to(protected)
                    elif kind == 'hardlink':
                        target.hardlink_to(protected)
                    else:
                        target.mkdir()
                with patch.object(ci.urllib.request, 'build_opener') as network:
                    with self.assertRaises(RuntimeError):
                        ci.fetch(root, cache, self.record('input.zip'))
                    network.assert_not_called()
                self.assertEqual(protected.read_bytes(), b'authored archive bytes')

    def test_offline_cache_rejects_unknown_files_and_nested_search_paths(self):
        for name in ('unlocked.whl', 'nested'):
            root = self.fixture()
            cache = root / 'downloads'
            cache.mkdir()
            if name == 'nested':
                (cache / name).mkdir()
            else:
                (cache / name).write_bytes(b'unknown wheel')
            with self.assertRaisesRegex(RuntimeError, 'Unexpected restored'):
                ci.validate_cache(root, cache, [self.record('input.zip')])
        root = self.fixture()
        cache = root / 'downloads'
        cache.mkdir()
        (cache / 'input.zip').write_bytes(b'authored archive bytes')
        ci.validate_cache(root, cache, [self.record('input.zip')])

    def test_fetch_checks_download_size_and_digest_before_publication(self):
        for payload in (b'authored archive bytes', b'too short', b'z' * 99):
            root = self.fixture()
            response = io.BytesIO(payload)
            response.geturl = lambda: 'https://release-assets.githubusercontent.com/public/input.zip'
            with patch.object(ci.urllib.request, 'build_opener') as opener:
                opener.return_value.open.return_value = response
                if payload == b'authored archive bytes':
                    output = ci.fetch(root, root / 'downloads', self.record('input.zip'))
                    self.assertEqual(output.read_bytes(), payload)
                else:
                    with self.assertRaises(RuntimeError):
                        ci.fetch(root, root / 'downloads', self.record('input.zip'))
                    self.assertFalse((root / 'downloads/input.zip').exists())
                self.assertFalse((root / 'downloads/.input.zip.partial').exists())

    def test_all_acquisition_callers_reject_unsafe_records_and_allow_official_source_redirect(self):
        for mutation in ({'filename': '../outside'}, {'size': True}, {'sha256': 'missing'},
                         {'url': 'https://private.example/input.zip'}):
            root = self.fixture()
            record = {**self.record('input.zip'), **mutation}
            with patch.object(ci.urllib.request, 'build_opener') as network:
                with self.assertRaises(RuntimeError):
                    ci.acquire(root, root / 'downloads', record)
                network.assert_not_called()
            self.assertFalse((root / 'downloads').exists())
        ci.allowed_url('https://codeload.github.com/pioarduino/platformio-core/zip/refs/tags/v6.1.19', redirect=True)

    def test_zip_and_tar_traversal_rejected_even_in_omitted_target(self):
        for name in ('../outside', '/outside', 'root/../../outside', 'C:/outside', 'root\\outside'):
            for archive_kind in ('zip', 'tar'):
                root = self.fixture()
                archive = (self.zip(root, [(name, b'bad')]) if archive_kind == 'zip' else
                           self.tar(root, [(name, b'bad', tarfile.REGTYPE, '')]))
                with self.subTest(name=name, archive=archive_kind):
                    with self.assertRaises(RuntimeError):
                        ci.extract(root, archive, root / 'output', c3_only=True)
                    self.assertFalse((root / 'outside').exists())

    def test_zip_special_entries_and_tar_escaping_links_rejected(self):
        root = self.fixture()
        archive = root / 'bad.zip'
        with zipfile.ZipFile(archive, 'w') as stream:
            item = zipfile.ZipInfo('root/link')
            item.external_attr = (stat.S_IFLNK | 0o777) << 16
            stream.writestr(item, '../protected')
        with self.assertRaises(RuntimeError):
            ci.extract(root, archive, root / 'output')
        for kind, link in ((tarfile.SYMTYPE, '../../outside'), (tarfile.LNKTYPE, '../outside'),
                           (tarfile.FIFOTYPE, '')):
            root = self.fixture()
            archive = self.tar(root, [('root/file', b'', kind, link)])
            with self.assertRaises(RuntimeError):
                ci.extract(root, archive, root / 'output')

    def test_c3_selection_keeps_root_metadata_and_only_c3_payload(self):
        root = self.fixture()
        archive = self.tar(root, [(name, content, tarfile.REGTYPE, '') for name, content in
                                 [('bundle/package.json', b'{}'), ('bundle/tools.json', b'{}'),
                                  ('bundle/versions.txt', b'3.3.7'),
                                  ('bundle/esp32c3/lib/lib.a', b'c3'),
                                  ('bundle/esp32/lib/lib.a', b'other target')]])
        ci.extract(root, archive, root / 'output', c3_only=True)
        self.assertEqual((root / 'output/esp32c3/lib/lib.a').read_bytes(), b'c3')
        self.assertEqual((root / 'output/versions.txt').read_bytes(), b'3.3.7')
        self.assertTrue((root / 'output/tools.json').is_file())
        self.assertFalse((root / 'output/esp32').exists())
        with self.assertRaises(RuntimeError):
            ci.extract(root, archive, root / 'output', c3_only=True)

    def test_internal_tar_links_are_contained_and_hardlinks_materialized_unaliased(self):
        root = self.fixture()
        archive = self.tar(root, [('bundle/bin/compiler', b'compiler', tarfile.REGTYPE, ''),
                                 ('bundle/bin/compiler-alias', b'', tarfile.SYMTYPE, 'compiler'),
                                 ('bundle/bin/compiler-copy', b'', tarfile.LNKTYPE, 'bundle/bin/compiler')])
        ci.extract(root, archive, root / 'output')
        self.assertEqual((root / 'output/bin/compiler-alias').read_bytes(), b'compiler')
        self.assertTrue((root / 'output/bin/compiler-alias').is_symlink())
        self.assertEqual((root / 'output/bin/compiler-copy').stat().st_nlink, 1)
        self.assertEqual((root / 'output/bin/compiler').stat().st_nlink, 1)

    def test_archive_and_output_aliases_rejected(self):
        root = self.fixture()
        archive = self.zip(root, [('payload', b'bytes')])
        archive_alias = root / 'alias.zip'
        archive_alias.symlink_to(archive)
        with self.assertRaises(RuntimeError):
            ci.extract(root, archive_alias, root / 'output')
        output = root / 'aliased-output'
        output.symlink_to(root / 'docs', target_is_directory=True)
        with self.assertRaises(RuntimeError):
            ci.extract(root, archive, output)
        self.assertFalse((root / 'docs/payload').exists())

    def test_offline_commands_keep_stock_metadata_and_editable_esptool(self):
        commands = ci.installation_commands(Path('/fixture/penv/bin/python'),
                                             [Path('/fixture/input.whl')], Path('/fixture/core'),
                                             Path('/fixture/packages/esptool'), Path('/fixture/wheels'))
        self.assertEqual(len(commands), 5)
        for command in commands:
            self.assertEqual(command[:5], ['/fixture/penv/bin/python', '-I', '-B', '-m', 'pip'])
        for command in commands[:4]:
            self.assertIn('--no-index', command)
            self.assertIn('--no-deps', command)
        self.assertIn('--no-build-isolation', commands[1])
        self.assertIn('--editable', commands[3])
        self.assertIn('/fixture/packages/esptool', commands[3])
        self.assertFalse(any('uninstall' in command or '--upgrade' in command for command in commands))
        with patch.dict(os.environ, {'PIP_INDEX_URL': 'https://private.example', 'PYTHONPATH': '/foreign'}):
            environment = ci.offline_environment(Path('/fixture'), Path('/fixture/downloads'))
        self.assertNotIn('PIP_INDEX_URL', environment)
        self.assertNotIn('PYTHONPATH', environment)
        self.assertEqual(environment['PIP_NO_INDEX'], '1')
        self.assertEqual(environment['UV_OFFLINE'], '1')

    def test_fork_record_matches_installed_module_bytes(self):
        root = self.fixture()
        site = root / 'penv/lib/python3.12/site-packages'
        (site / 'platformio').mkdir(parents=True)
        for name in ('platformio', 'pioarduino_core'):
            (site / (name + '-6.1.19.dist-info')).mkdir()
        payload = b'authored fork module'
        (site / 'platformio/__init__.py').write_bytes(payload)
        encoded = base64.urlsafe_b64encode(hashlib.sha256(payload).digest()).decode().rstrip('=')
        archive = self.zip(root, [('pioarduino_core-6.1.19.dist-info/RECORD',
                                   'platformio/__init__.py,sha256=' + encoded + ',' + str(len(payload)) + '\n')])
        ci.verify_core(root / 'penv', archive)
        (site / 'platformio/__init__.py').write_bytes(b'stock overwrite')
        with self.assertRaises(RuntimeError):
            ci.verify_core(root / 'penv', archive)

    def test_prepare_refuses_local_or_wrong_runner_before_any_side_effect(self):
        scenarios = [('false', 'Darwin', 'arm64', (3, 12, 6)),
                     ('true', 'Linux', 'arm64', (3, 12, 6)),
                     ('true', 'Darwin', 'x86_64', (3, 12, 6)),
                     ('true', 'Darwin', 'arm64', (3, 12, 10))]
        for actions, system, machine, version in scenarios:
            root = self.fixture()
            with patch.dict(os.environ, {'GITHUB_ACTIONS': actions}), \
                 patch.object(ci.platform, 'system', return_value=system), \
                 patch.object(ci.platform, 'machine', return_value=machine), \
                 patch.object(ci.sys, 'version_info', version), \
                 patch.object(ci, 'read_lock') as lock, \
                 patch.object(ci.subprocess, 'run') as child:
                with self.assertRaises(RuntimeError):
                    ci.prepare(root)
                lock.assert_not_called()
                child.assert_not_called()
                self.assertEqual(sorted(p.name for p in root.iterdir()), ['docs'])


if __name__ == '__main__':
    unittest.main()
