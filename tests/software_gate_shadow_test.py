"""Shadow observations never omit the fresh gate or substitute device evidence."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
import software_gate_shadow as shadow
REAL_EXECUTE = shadow.execute

spec = importlib.util.spec_from_file_location('reading_ready_cli', Path(__file__).resolve().parents[1] / 'tools/reading-ready.py')
cli = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cli)


class ShadowTests(unittest.TestCase):
    def setUp(self):
        workspace = tempfile.TemporaryDirectory(prefix='newt-shadow-')
        self.addCleanup(workspace.cleanup)
        self.root = Path(workspace.name).resolve()
        for name in (*shadow.SOURCE_ROOTS, 'vendor/freeink-sdk', 'artifacts'):
            directory = self.root / name
            directory.mkdir(parents=True, exist_ok=True)
            (directory / 'input.cpp').write_text('authored fixture')
        (self.root / 'platformio.ini').write_text('configuration')
        (self.root / '.gitignore').write_text('.build/\n')
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True)
        subprocess.run(['git', 'add', '.'], cwd=self.root, check=True)
        for name, value in (('verify_source_pin', 'pinned'), ('compiler_identity', {'binary': 'compiler'}),
                            ('runtime_identity', {'binary': 'python'})):
            mock = patch.object(shadow, name, return_value=value)
            mock.start(); self.addCleanup(mock.stop)
        self.command = [sys.executable, '-B', str(self.root / 'tools/check.py')]
        self.calls = []
        self.code = 0
        self.mutate_during_gate = None
        def execute(root, command, log):
            self.calls.append(command)
            log.write_bytes(b'observed fresh gate output\n')
            if self.mutate_during_gate: self.mutate_during_gate()
            return self.code
        mock = patch.object(shadow, 'execute', side_effect=execute)
        mock.start(); self.addCleanup(mock.stop)
        self.report = self.root / '.build/software-gate-shadow/observation.json'

    def run_shadow(self):
        with contextlib.redirect_stderr(io.StringIO()):
            shadow.run_shadow(self.root, self.command)
        return json.loads(self.report.read_text())

    def test_cold_and_matching_warm_observations_both_execute(self):
        cold, warm = self.run_shadow(), self.run_shadow()
        self.assertFalse(cold['historical_observed_inputs_match'])
        self.assertTrue(warm['historical_observed_inputs_match'])
        self.assertTrue(warm['observed_inputs_stable'])
        self.assertEqual(self.calls, [self.command, self.command])
        for record in (cold, warm):
            self.assertFalse(record['reuse_enabled'])
            self.assertFalse(record['reuse_eligible'])
            self.assertFalse(record['complete_input_closure'])
            self.assertEqual(record['eliminated_gate_seconds'], 0)
            self.assertEqual(record['avoided_gate_invocations'], 0)

    def test_each_source_fixture_asset_generator_vendor_and_policy_mutation_is_observed(self):
        for name in (*shadow.SOURCE_ROOTS, 'vendor/freeink-sdk', 'artifacts'):
            with self.subTest(group=name):
                self.run_shadow()
                path = self.root / name / 'input.cpp'
                original = path.read_bytes()
                path.write_bytes(original + b' changed')
                self.assertFalse(self.run_shadow()['historical_observed_inputs_match'])
                path.write_bytes(original)
        self.run_shadow()
        (self.root / 'platformio.ini').write_text('different flags')
        self.assertFalse(self.run_shadow()['historical_observed_inputs_match'])

    def test_added_deleted_input_and_executable_cache_change_observations(self):
        self.run_shadow()
        cache = self.root / 'tools/__pycache__/gate.pyc'
        cache.parent.mkdir(); cache.write_bytes(b'actual executable bytecode')
        self.assertFalse(self.run_shadow()['historical_observed_inputs_match'])
        cache.unlink()
        self.assertFalse(self.run_shadow()['historical_observed_inputs_match'])
        path = self.root / 'tests/input.cpp'
        path.unlink()
        self.assertFalse(self.run_shadow()['historical_observed_inputs_match'])

    def test_compiler_runtime_environment_and_command_changes_invalidate_match(self):
        for target, value in (('compiler_identity', {'binary': 'changed'}),
                              ('runtime_identity', {'binary': 'changed'})):
            self.run_shadow()
            with patch.object(shadow, target, return_value=value):
                self.assertFalse(self.run_shadow()['historical_observed_inputs_match'])
        self.run_shadow()
        with patch.dict(shadow.os.environ, {'CXX': 'unbound compiler selector'}):
            self.assertFalse(self.run_shadow()['historical_observed_inputs_match'])
        self.run_shadow(); self.command.append('--different-flags')
        self.assertFalse(self.run_shadow()['historical_observed_inputs_match'])

    def test_missing_collector_inputs_and_mid_gate_change_never_grant_reuse(self):
        self.run_shadow()
        with patch.object(shadow, 'runtime_identity', side_effect=FileNotFoundError('missing runtime')):
            record = self.run_shadow()
        self.assertFalse(record['before']['available'])
        self.assertFalse(record['historical_observed_inputs_match'])
        self.mutate_during_gate = lambda: (self.root / 'src/input.cpp').write_text('changed during gate')
        self.assertFalse(self.run_shadow()['observed_inputs_stable'])
        self.assertEqual(len(self.calls), 3)

    def test_corrupt_duplicate_oversized_and_forged_success_cannot_skip_gate(self):
        self.run_shadow()
        for raw in ('bad JSON', '{"after":{},"after":{}}', '[' * 2000, ' ' * (1024 * 1024 + 1)):
            self.report.write_text(raw)
            self.run_shadow()
        forged = {'after': shadow.observation(self.root, self.command), 'reuse_enabled': True,
                  'reuse_eligible': True, 'fresh_gate': {'returncode': 0}, 'ready_to_use': True}
        self.report.write_text(json.dumps(forged))
        self.code = 7
        with self.assertRaises(subprocess.CalledProcessError) as error: self.run_shadow()
        self.assertEqual(error.exception.returncode, 7)
        record = json.loads(self.report.read_text())
        self.assertEqual(record['fresh_gate']['returncode'], 7)
        self.assertFalse(record['reuse_eligible'])
        self.assertEqual(len(self.calls), 6)

    def test_expected_fresh_failure_is_identical_with_cold_and_warm_history(self):
        self.code = 9
        for _ in range(2):
            with self.assertRaises(subprocess.CalledProcessError) as error: self.run_shadow()
            self.assertEqual((error.exception.returncode, error.exception.cmd), (9, self.command))
        self.assertEqual(len(self.calls), 2)

    def test_actual_subprocess_failure_and_output_are_identical_cold_and_warm(self):
        control = self.root / 'tools/expected_failure.py'
        control.write_text('print("Authored subprocess negative control"); raise SystemExit(11)\n')
        self.command = [sys.executable, '-B', str(control)]
        hashes = []
        with patch.object(shadow, 'execute', side_effect=REAL_EXECUTE):
            for _ in range(2):
                with self.assertRaises(subprocess.CalledProcessError) as error: self.run_shadow()
                self.assertEqual(error.exception.returncode, 11)
                hashes.append(json.loads(self.report.read_text())['fresh_gate']['log_sha256'])
        self.assertEqual(hashes[0], hashes[1])

    def test_default_readiness_has_no_shadow_or_reuse_path(self):
        with patch.object(sys, 'argv', ['reading-ready.py', 'software-verified']), \
             patch.object(cli, 'ROOT', self.root), patch.object(cli.subprocess, 'run') as gate, \
             patch.object(shadow, 'run_shadow') as observer, contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(cli.main(), 0)
            gate.assert_called_once_with(self.command, check=True, cwd=self.root)
            observer.assert_not_called()
            self.assertEqual(json.loads(output.getvalue())['state'], 'software-verified')
            self.assertFalse(json.loads(output.getvalue())['ready_to_use'])

    def test_aliased_shadow_output_is_rejected_before_any_write_or_admission(self):
        self.run_shadow()
        protected = self.root / 'protected'
        protected.write_text('preserve')
        self.report.unlink(); self.report.symlink_to(protected)
        with self.assertRaisesRegex(RuntimeError, 'unaliased'): self.run_shadow()
        self.assertEqual(protected.read_text(), 'preserve')
        self.assertEqual(len(self.calls), 1)

    def test_readiness_resource_ui_and_journey_validation_still_run(self):
        arguments = ['reading-ready.py', 'hardware-validated', '--software-proof-shadow',
                     '--variant', 'regional-dev-usb', '--install', 'synthetic-install', '--journey', 'synthetic-journey',
                     '--plan', 'synthetic-plan', '--target', 'synthetic-device', '--ui-review', 'synthetic-ui',
                     '--latency-conditions', 'synthetic-conditions']
        with patch.object(sys, 'argv', arguments), patch.object(cli, 'ROOT', self.root), \
             patch.object(shadow, 'run_shadow') as gate, patch.object(cli, 'current_build') as resources, \
             patch.object(cli, 'load', return_value={}), patch.object(cli, 'procedure_identity', return_value='proc'), \
             patch.object(cli, 'validate_journey', return_value={'state': 'hardware-validated'}) as journey, \
             patch.object(cli, 'latency_admission', return_value={'state': 'pass', 'groups': [], 'reasons': []}) as latency, \
             patch.object(cli.rust_preview, 'build', return_value='fresh-renderer'), \
             patch.object(cli.rust_preview, 'cli', return_value=['fresh-renderer']), \
             patch.object(cli, 'render_frames', return_value='fresh-frames'), \
             patch.object(cli, 'verify_reviewed') as review, contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(cli.main(), 0)
            gate.assert_called_once_with(self.root, self.command)
            resources.assert_called_once_with('regional-dev-usb')
            journey.assert_called_once()
            latency.assert_called_once()
            review.assert_called_once_with('fresh-frames', {})
            self.assertFalse(json.loads(output.getvalue())['ready_to_use'])
            for validator in (resources, journey, latency, review):
                validator.side_effect = ValueError('independent validation failed')
                with self.assertRaisesRegex(ValueError, 'independent validation'): cli.main()
                validator.side_effect = None


if __name__ == '__main__':
    unittest.main()
