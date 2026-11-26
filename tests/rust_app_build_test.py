"""A relocated Rust image must use its own sources and patch policy."""
import configparser
import hashlib
from contextlib import nullcontext, redirect_stderr
import io
import importlib.util
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import tomllib
import types
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
SPEC = importlib.util.spec_from_file_location('rust_app_build', ROOT / 'tools/rust-app-build.py')
BUILD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILD)
import daily_firmware
import rust_app_archive


class CheckoutSources(unittest.TestCase):
    def _relocated(self, arguments, payload=b'unit-test certificate bundle'):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory) / 'detached'
            (project / 'rust/firmware/native').mkdir(parents=True)
            (project / 'rust/firmware/native/firmware_version.h').write_text(
                (ROOT / 'rust/firmware/native/firmware_version.h').read_text())
            (project / 'platformio-rust-app.ini').write_text(
                (ROOT / 'platformio-rust-app.ini').read_text())
            bundle = project / '.build/regional-dev-usb/crt/x509_crt_bundle'
            calls, loaded, exclusions = [], [], []

            def run(command, **options):
                calls.append((command, options))
                if command[0] == 'pio' and payload != b'unit-test certificate bundle':
                    self.fail('Native link dispatched before rejecting the changed bundle')
                if str(project / 'tools/crt_bundle.py') in command:
                    bundle.parent.mkdir(parents=True)
                    bundle.write_bytes(payload)
                value = 'a' * 12 if command[:2] == ['git', 'rev-parse'] else ''
                if command[:2] == ['git', 'show']:
                    value = '2026-11-15'
                return subprocess.CompletedProcess(command, 0, stdout=value)

            def load(name, file):
                loaded.append(Path(file))
                self.assertEqual(Path(file).parent, project / 'tools')
                if name == 'app_wifi_sections':
                    module = types.SimpleNamespace(pinned_text=lambda root: self.assertEqual(root, project) or 'pinned',
                                                   derive=lambda *a, **k: 'sections')
                else:
                    module = types.SimpleNamespace(build_environment=lambda: {},
                        require_toolchain=lambda: Path('/installed/pio'),
                        build_command=lambda _: ['pio'],
                        native_toolchain=types.SimpleNamespace(
                            quiescent=lambda p: exclusions.append(p) or nullcontext(),
                            restore=lambda p: exclusions.append(p)))
                return types.SimpleNamespace(loader=types.SimpleNamespace(exec_module=lambda _: None),
                                             module=module)

            with patch.object(BUILD, 'ROOT', project), \
                    patch.object(BUILD, 'CRT_BUNDLE_SHA256', hashlib.sha256(b'unit-test certificate bundle').hexdigest()), \
                    patch.object(daily_firmware, 'CRT_BUNDLE_SHA256', hashlib.sha256(b'unit-test certificate bundle').hexdigest()), \
                    patch.object(sys, 'argv', ['rust-app-build.py', *arguments]), \
                    patch.object(BUILD.subprocess, 'run', side_effect=run), \
                    patch.object(BUILD.importlib.util, 'spec_from_file_location', side_effect=load), \
                    patch.object(BUILD.importlib.util, 'module_from_spec', side_effect=lambda s: s.module):
                BUILD.main()
            self.assertEqual(exclusions, [project, project])
            self.assertTrue(any(str(project / 'tools/apply-freeink-patch.py') in c for c, _ in calls))
            self.assertEqual(len(loaded), 2)
            self.assertTrue(bundle.is_file())
            cargo = [(command, options['env']) for command, options in calls
                     if command[0] == str(project / '.toolchain/rust/toolchain/bin/cargo')]
            self.assertEqual(calls[0][0],
                             [sys.executable, '-B', str(project / 'tools/rust-toolchain.py'), '--verify'])
            self.assertEqual([command[1] for command, _ in cargo],
                             [] if '--native-only' in arguments else ['clean', 'build'])
            for command, _ in cargo:
                if command[1] == 'build':
                    self.assertIn('--locked', command)
            for _, environment in cargo:
                for name in ('RUSTC_WRAPPER', 'CARGO_ENCODED_RUSTFLAGS',
                             'LD_PRELOAD', 'LD_LIBRARY_PATH', 'PYTHONOPTIMIZE'):
                    self.assertNotIn(name, environment)
                self.assertEqual(environment['CARGO_HOME'], str(project / '.toolchain/rust/cargo-home'))
                self.assertEqual(environment['RUSTC'], str(project / '.toolchain/rust/toolchain/bin/rustc'))
                self.assertIn(f'--remap-path-prefix={project}=.', environment['RUSTFLAGS'])
            self.assertTrue(any(str(project / 'tools/crt_bundle.py') in command for command, _ in calls))
            configuration = project / (BUILD.CONFIGURATION if '--daily' in arguments
                                       else '.build/rust-app/platformio.ini')
            native = ['pio', '--project-dir', str(project), '--project-conf', str(configuration)]
            self.assertEqual([command for command, _ in calls if command[0] == 'pio'],
                             [native + ['-t', 'clean'], native] if '--clean' in arguments else [native])
            config = configparser.ConfigParser(interpolation=None)
            config.read(configuration)
            self.assertEqual(config['platformio']['core_dir'], '.toolchain/platformio')
            if '--daily' in arguments:
                self.assertEqual(config['platformio']['build_dir'], '.pio/regional-dev-usb/build')
                self.assertEqual(config['platformio']['libdeps_dir'], '.pio/regional-dev-usb/libdeps')
                command = next(command for command, _ in cargo if command[1] == 'build')
                self.assertEqual(command[command.index('--features') + 1],
                                 'rust-app,dev-usb,wifi-update,embedded-sample')
            flags = shlex.split(config['env:newt_x3']['build_flags'])
            for flag in flags:
                if flag.startswith(('-I', '-L')):
                    self.assertFalse(Path(flag[2:]).is_absolute(), flag)
            self.assertIn(str(bundle), ' '.join(flags))
            for dependency in config['env:newt_x3']['lib_deps'].split():
                self.assertTrue(dependency.split('=')[1].startswith('symlink://.build/'), dependency)

    def test_fixture_features_refuse_before_any_process_or_output(self):
        for features in ('checkpoint-fixture', 'dev-usb,checkpoint-fixture',
                         'newt-app-image/checkpoint-fixture', 'newt-core/pinned-harness', 'unreviewed-feature'):
            with self.subTest(features=features), tempfile.TemporaryDirectory() as directory:
                output = Path(directory)/'absent'
                with patch.object(sys, 'argv', ['rust-app-build.py', '--output', str(output),
                         '--features', features]), patch.object(BUILD.subprocess, 'run') as dispatch, \
                         patch('sys.stderr'), self.assertRaises(SystemExit):
                    BUILD.main()
                dispatch.assert_not_called()
                self.assertFalse(output.exists())

    def test_full_product_features_remain_available(self):
        self._relocated(['--clean', '--features', 'dev-usb,wifi-update,embedded-sample'])

    def test_daily_build_has_all_features_and_canonical_outputs(self):
        self._relocated(['--daily', '--clean'])

    def test_ambient_flags_and_compiler_wrappers_cannot_override_the_daily_build(self):
        with patch.dict(os.environ, {'RUSTC_WRAPPER': '/outside/compiler-wrapper',
                         'CARGO_ENCODED_RUSTFLAGS': '-Copt-level=0',
                         'LD_PRELOAD': '/outside/override.so', 'LD_LIBRARY_PATH': '/outside',
                         'PYTHONOPTIMIZE': '2'}):
            self._relocated(['--daily', '--clean'])

    def test_daily_build_refuses_feature_omissions_or_partial_builds(self):
        for options in (['--native-only'], ['--rust-only'], ['--features', 'dev-usb'],
                        ['--package', 'another-package']):
            with self.subTest(options=options), \
                    patch.object(sys, 'argv', ['rust-app-build.py', '--daily', *options]), \
                    patch.object(BUILD.subprocess, 'run') as dispatch, redirect_stderr(io.StringIO()), \
                    self.assertRaises(SystemExit):
                BUILD.main()
            dispatch.assert_not_called()

    def test_changed_daily_dependency_scope_dispatches_nothing(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory)
            (project / 'platformio-rust-app.ini').write_bytes(
                (ROOT / 'platformio-rust-app.ini').read_bytes() + b'\nextra_scripts = unreviewed.py\n')
            output = project / 'absent'
            with patch.object(BUILD, 'ROOT', project), \
                    patch.object(sys, 'argv', ['rust-app-build.py', '--daily', '--clean',
                                              '--output', str(output)]), \
                    patch.object(BUILD.subprocess, 'run') as dispatch, \
                    self.assertRaisesRegex(ValueError, 'reviewed dependency/build scope'):
                BUILD.main()
            dispatch.assert_not_called()
            self.assertFalse(output.exists())

    def test_failed_toolchain_verification_dispatches_no_build_or_output(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory) / 'project'
            output = project / 'absent'
            command = [sys.executable, '-B', str(project / 'tools/rust-toolchain.py'), '--verify']
            with patch.object(BUILD, 'ROOT', project), \
                    patch.object(sys, 'argv', ['rust-app-build.py', '--output', str(output)]), \
                    patch.object(BUILD.subprocess, 'run',
                                 side_effect=subprocess.CalledProcessError(1, command)) as dispatch, \
                    self.assertRaises(subprocess.CalledProcessError):
                BUILD.main()
            dispatch.assert_called_once_with(command, cwd=project, check=True)
            self.assertFalse(output.exists())

    def test_default_clean_image_uses_only_relocated_inputs(self):
        self._relocated(['--clean'])

    def test_native_only_keeps_the_existing_archive(self):
        self._relocated(['--native-only'])

    def test_changed_generated_bundle_refuses_link(self):
        with self.assertRaisesRegex(RuntimeError, 'TLS certificate bundle changed'):
            self._relocated(['--clean'], b'changed certificate bundle')

    def test_rust_only_does_not_load_the_sdk(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory)
            (project / 'rust/firmware/native').mkdir(parents=True)
            (project / 'rust/firmware/native/firmware_version.h').write_bytes(
                (ROOT / 'rust/firmware/native/firmware_version.h').read_bytes())
            def run(command, **options):
                value = 'a' * 12 if command[:2] == ['git', 'rev-parse'] else ''
                return subprocess.CompletedProcess(command, 0, stdout=value)
            with patch.object(BUILD, 'ROOT', project), \
                    patch.object(sys, 'argv', ['rust-app-build.py', '--rust-only']), \
                    patch.object(BUILD.subprocess, 'run', side_effect=run) as calls, \
                    patch.object(BUILD.importlib.util, 'spec_from_file_location',
                                 side_effect=AssertionError('SDK loaded for archive-only build')):
                BUILD.main()
            self.assertEqual(sum(call.args[0][0].endswith('/cargo')
                                 for call in calls.call_args_list), 1)


class DailyConfiguration(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / daily_firmware.SOURCE_CONFIGURATION).write_bytes(
            (ROOT / daily_firmware.SOURCE_CONFIGURATION).read_bytes())
        self.bundle = self.root / '.build/regional-dev-usb/crt/x509_crt_bundle'
        self.bundle.parent.mkdir(parents=True)
        self.bundle.write_bytes(b'fixed certificate fixture')
        pin = patch.object(daily_firmware, 'CRT_BUNDLE_SHA256',
                           hashlib.sha256(self.bundle.read_bytes()).hexdigest())
        pin.start()
        self.addCleanup(pin.stop)
        self.output = self.root / daily_firmware.CONFIGURATION

    def write(self, config):
        with self.output.open('w') as stream:
            config.write(stream)

    def test_generated_inputs_have_exactly_the_reviewed_scope(self):
        for identifier in ('a' * 12, 'a' * 12 + '-dirty'):
            self.write(daily_firmware.generated_configuration(self.root, identifier, '2026-11-16'))
            daily_firmware.require_generated_configuration(self.root, self.output)

    def test_extra_flags_scripts_dependencies_or_paths_are_rejected(self):
        for section, key, value in (
                ('env:newt_x3', 'extra_scripts', 'post:unreviewed.py'),
                ('env:newt_x3', 'lib_deps', 'https://example.invalid/dependency'),
                ('env:newt_x3', 'build_flags', '-std=gnu++2a'),
                ('env:newt_x3', 'build_unflags', '-fstack-usage'),
                ('platformio', 'extra_configs', 'other.ini'),
                ('platformio', 'build_dir', '/outside/build'),
                ('platformio', 'libdeps_dir', '/outside/dependencies'),
                ('DEFAULT', 'injected', 'unreviewed')):
            with self.subTest(section=section, key=key):
                config = daily_firmware.generated_configuration(self.root, 'a' * 12, '2026-11-16')
                config[section][key] = value
                self.write(config)
                with self.assertRaises(ValueError):
                    daily_firmware.require_generated_configuration(self.root, self.output)

    def test_identifier_format_and_pinned_bundle_cannot_drift(self):
        for identifier, date in (('a' * 12 + '-other', '2026-11-16'),
                                 ('a' * 12, 'Nov 16 2026')):
            with self.subTest(identifier=identifier, date=date), self.assertRaises(ValueError):
                daily_firmware.generated_configuration(self.root, identifier, date)
        self.bundle.write_bytes(b'changed bundle')
        with self.assertRaisesRegex(ValueError, 'certificate bundle changed'):
            daily_firmware.generated_configuration(self.root, 'a' * 12, '2026-11-16')


class PinnedHarness(unittest.TestCase):
    """The pinned-harness feature skips the open-time ZIP member check (#60)."""

    def test_only_host_harnesses_and_core_tests_enable_it(self):
        users = {}
        for manifest in sorted((ROOT / 'rust').glob('**/Cargo.toml')):
            if 'pinned-harness' in manifest.read_text():
                users[manifest.parent.relative_to(ROOT / 'rust').as_posix()] = tomllib.loads(manifest.read_text())
        self.assertEqual(set(users), {'core', 'host'})
        core = users['core']
        self.assertEqual(core['features']['pinned-harness'], [])
        self.assertEqual(core['dev-dependencies']['newt-core']['features'], ['pinned-harness'])
        self.assertNotIn('pinned-harness', core['features'].get('default', []))
        self.assertNotIn('pinned-harness', str(core.get('dependencies', {})))
        self.assertEqual(users['host']['dependencies']['newt-core']['features'], ['pinned-harness'])
        # The host harness crate is a leaf, so no other graph inherits its features.
        host = users['host']['package']['name']
        for manifest in (ROOT / 'rust').glob('**/Cargo.toml'):
            if manifest.parent.name != 'host':
                self.assertNotIn(host, manifest.read_text(), manifest)

    def resolved_core_features(self, package, features):
        """newt-core's features as Cargo resolves them for one RV32 build."""
        toolchain = ROOT / '.toolchain/rust/toolchain/bin'
        env = {k: v for k, v in os.environ.items() if not k.startswith(('CARGO_', 'RUST'))}
        with tempfile.TemporaryDirectory() as home:
            env.update(CARGO_HOME=home, RUSTC=str(toolchain / 'rustc'))
            tree = subprocess.run(
                [toolchain / 'cargo', 'tree', '--offline', '--locked', '-p', package,
                 '--target', rust_app_archive.TARGET, '-e', 'normal,build', '--prefix', 'none',
                 '--format', '{p} [{f}]', '--features', features],
                cwd=ROOT / 'rust', env=env, capture_output=True, text=True, check=True).stdout
        lines = {line for line in tree.splitlines() if line.startswith('newt-core ')}
        self.assertTrue(lines, tree)
        return {feature for line in lines for feature in line.split('[', 1)[1].split(']', 1)[0].split(',') if feature}

    def test_the_firmware_build_never_resolves_it(self):
        self.assertNotIn('pinned-harness', self.resolved_core_features('newt-app-image', ','.join(daily_firmware.FEATURES)))
        # Control: the same query does see it where a harness turns it on.
        self.assertIn('pinned-harness', self.resolved_core_features('newt-parity', 'xml-native'))

    def test_page_turn_trace_is_opt_in_and_excluded_from_shipping_image(self):
        daily = self.resolved_core_features('newt-app-image', ','.join(daily_firmware.FEATURES))
        self.assertNotIn('page-turn-trace', daily)
        diagnostic = self.resolved_core_features(
            'newt-app-image', ','.join(daily_firmware.FEATURES + ('page-turn-trace',)))
        self.assertIn('page-turn-trace', diagnostic)
        with self.assertRaises(ValueError):
            daily_firmware.require_daily_image({
                'image': daily_firmware.IMAGE,
                'features': list(daily_firmware.FEATURES) + ['page-turn-trace'],
            })
        spec = importlib.util.spec_from_file_location('firmware_build', ROOT / 'tools/firmware-build.py')
        fb = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(fb)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            receipt = root / 'artifacts/code-health-regional-dev-usb.json'
            receipt.parent.mkdir(parents=True)
            with patch.object(fb, 'ROOT', root), \
                    patch('code_health_resources.check_resources', return_value=['flash grew']) as check:
                with self.assertRaisesRegex(RuntimeError, 'Resource ratchet failed'):
                    fb.write_resource_report(receipt, {'ok': True}, {}, 'regional-dev-usb', diagnostic=False)
                check.assert_called_once()
                check.reset_mock()
                fb.write_resource_report(receipt, {'ok': True}, {}, 'regional-dev-usb', diagnostic=True)
                check.assert_not_called()
                self.assertTrue(receipt.is_file())

    def test_firmware_targets_refuse_it_at_compile_time(self):
        source = (ROOT / 'rust/core/src/member_check.rs').read_text()
        guard = '#[cfg(all(feature = "pinned-harness", target_arch = "riscv32"))]\ncompile_error!('
        self.assertTrue(guard in source, 'member_check.rs lost the RV32 pinned-harness guard')


if __name__ == '__main__':
    unittest.main()
