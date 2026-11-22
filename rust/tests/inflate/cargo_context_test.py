"""Adversarial checks of Cargo context admission, without invoking a compiler."""
import hashlib
from pathlib import Path
import sys
import tempfile
import unittest
from cargo_context import cargo_context, verify_no_cargo_configuration


class ContextTests(unittest.TestCase):
    def test_configuration_in_physical_ancestors_is_not_hidden_by_a_build_symlink(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            physical = root / '.cache/newt/rust/project'
            physical.mkdir(parents=True)
            (root / '.build').mkdir()
            (root / '.build/rust').symlink_to(physical.parent, target_is_directory=True)
            config_dir = root / '.cache/newt/.cargo'
            config_dir.mkdir()
            config = config_dir / 'config.toml'
            config.write_text('[target.x86_64-unknown-linux-gnu]\nrunner="unsealed-runner"\n')
            env = {'CARGO_HOME': str(root / 'cargo-home')}
            with self.assertRaisesRegex(RuntimeError, 'Unsealed Cargo configuration'):
                verify_no_cargo_configuration(root / '.build/rust/project', env)
            config.unlink()
            verify_no_cargo_configuration(root / '.build/rust/project', env)

    def test_optimized_validator_is_rejected_before_assertions_can_disappear(self):
        from run_support import inherited_child
        code = ('import sys;sys.path.insert(0,' + repr(str(Path(__file__).resolve().parent))
                + ');from cargo_context import reject_optimized_python;'
                + 'reject_optimized_python();print("unexpected acceptance")')
        result = inherited_child([sys.executable, '-B', '-O', '-c', code], input=b'')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b'')
        self.assertIn(b'Validation requires Python assertions', result.stderr)

    def test_ancestor_config_and_legacy_config_cannot_supply_an_unsealed_wrapper(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            workspace = root / 'outer/source/workspace'
            workspace.mkdir(parents=True)
            env = {'CARGO_HOME': str(root / 'cargo-home')}
            for directory in (workspace / '.cargo', root / 'outer/.cargo', root / '.cargo',
                              Path(env['CARGO_HOME'])):
                directory.mkdir()
                for name in ('config', 'config.toml'):
                    config = directory / name
                    config.write_text('[build]\nrustc-wrapper="unsealed-wrapper"\n')
                    with self.assertRaisesRegex(RuntimeError, 'Unsealed Cargo configuration'):
                        verify_no_cargo_configuration(workspace, env)
                    config.unlink()
                directory.rmdir()
            verify_no_cargo_configuration(workspace, env)

    def test_symlinked_config_and_config_directory_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            workspace = root / 'source'; workspace.mkdir()
            env = {'CARGO_HOME': str(root / 'cargo-home')}
            config_dir = workspace / '.cargo'
            config_dir.symlink_to(root / 'not-created', target_is_directory=True)
            with self.assertRaisesRegex(RuntimeError, 'directory is symlinked'):
                verify_no_cargo_configuration(workspace, env)
            config_dir.unlink(); config_dir.mkdir()
            (config_dir / 'config').symlink_to(root / 'missing')
            with self.assertRaisesRegex(RuntimeError, 'Unsealed Cargo configuration'):
                verify_no_cargo_configuration(workspace, env)

    def test_final_check_catches_configuration_added_during_a_command(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); source = root / 'source'; source.mkdir()
            executable = Path(sys.executable)
            entry = {'path': executable, 'sha256': hashlib.sha256(executable.read_bytes()).hexdigest()}
            tools = {'clang++': entry, 'native-ld': entry}
            env = {'CARGO_HOME': str(root / 'cargo-home')}
            with self.assertRaisesRegex(RuntimeError, 'Unsealed Cargo configuration'):
                with cargo_context(['build'], source, env, tools):
                    config = source / '.cargo'; config.mkdir()
                    (config / 'config.toml').write_text('[build]\n')

    def test_host_linker_is_explicit_and_host_flags_do_not_reach_bare_target(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); source = root / 'source'; source.mkdir()
            executable = Path(sys.executable)
            entry = {'path': executable, 'sha256': hashlib.sha256(executable.read_bytes()).hexdigest()}
            tools = {'clang++': entry, 'native-ld': entry}
            env = {'CARGO_HOME': str(root / 'cargo-home'), 'RUSTFLAGS': '-Dwarnings',
                   'PYTHONOPTIMIZE': '1', 'RUSTC_WRAPPER': 'unexpected',
                   'CCC_OVERRIDE_OPTIONS': '+-fuse-ld=/absolute/unpinned-ld'}
            with cargo_context(['build'], source, env, tools) as clean:
                self.assertIn('-Clinker='+str(executable), clean['RUSTFLAGS'])
                self.assertIn('-fuse-ld='+str(executable), clean['RUSTFLAGS'])
                self.assertNotIn('PYTHONOPTIMIZE', clean)
                self.assertNotIn('CCC_OVERRIDE_OPTIONS', clean)
                self.assertEqual(clean['RUSTC_WRAPPER'], '')
            with cargo_context(['build', '--target', 'riscv32imc-unknown-none-elf'], source, env, tools) as clean:
                self.assertEqual(clean['RUSTFLAGS'], '-Dwarnings')
            self.assertEqual(env['RUSTFLAGS'], '-Dwarnings')


if __name__ == '__main__':
    unittest.main()
