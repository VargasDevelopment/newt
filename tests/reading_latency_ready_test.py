"""Synthetic readiness CLI controls; no software build, USB or private evidence."""
import copy
from contextlib import ExitStack, contextmanager, redirect_stderr, redirect_stdout
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import stat
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import reading_latency
import release_tools


def imported(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


ready = imported('authored_reading_ready', ROOT / 'tools/reading-ready.py')
fixtures = imported('authored_latency_ready_fixtures', ROOT / 'tests/reading_latency_test.py')


class ReadingLatencyReadyTest(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='newt-authored-ready-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.stack = ExitStack()
        self.addCleanup(self.stack.close)
        self.current = fixtures.bundle()
        self.source = {'firmware_version': '0.1.0', 'git_revision': self.current['source_revision'], 'dirty': False}
        self.expected = self.current['expected']
        self.resource = {'identity': {'inputs_sha256': self.expected['inputs_sha256']},
                         'flash_bytes': self.current['image_bytes'],
                         'artifacts': {'.pio/regional-dev-usb/build/newt_x3/firmware.bin': self.expected['binary_sha256']}}
        self.native = {**self.source, 'variant': 'regional-dev-usb',
                       'binary_sha256': self.expected['binary_sha256'],
                       'resource_identity': copy.deepcopy(self.resource['identity'])}
        self.stack.enter_context(patch.object(ready, 'ROOT', self.root))
        self.stack.enter_context(patch.object(ready, 'procedure_identity', return_value=self.current['procedure_sha256']))
        self.stack.enter_context(patch.object(release_tools, 'source_identity', return_value=self.source))
        self.stack.enter_context(patch.object(ready.time, 'time', return_value=1200))
        self.software = self.stack.enter_context(patch.object(ready.subprocess, 'run'))
        self.resources = self.stack.enter_context(patch.object(ready, 'check_resources', return_value=[]))
        self.stack.enter_context(patch.object(ready, 'read_baseline', return_value={}))
        preview = self.stack.enter_context(patch.object(ready, 'rust_preview'))
        preview.cli.return_value = self.root / 'authored-ui'
        self.compile = preview.build
        self.stack.enter_context(patch.object(ready, 'render_frames', return_value=['authored-frame']))
        self.ui = self.stack.enter_context(patch.object(ready, 'verify_reviewed'))
        self.write('artifacts/code-health-regional-dev-usb.json', self.resource)
        self.write('artifacts/firmware-identity-regional-dev-usb.json', self.native)
        config = self.root / '.build/regional-dev-usb/platformio.ini'
        config.parent.mkdir(parents=True)
        config.write_text('build_flags = -DNEWT_DEV_BUILD_ID="' + self.expected['build'] + '"\n')
        self.write('docs/reading-latency-policy.json', json.loads((ROOT / 'docs/reading-latency-policy.json').read_text()))
        (self.root / 'docs/reading-latency-contract.json').write_bytes(fixtures.CONTRACT)
        self.install_candidate(self.current)
        self.write('.build/ui.json', {'authored': True})

    def write(self, relative, value):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value, sort_keys=True))
        return path

    def install_candidate(self, candidate):
        self.current = candidate
        for key in ('journey', 'install', 'plan', 'conditions'):
            self.write('.build/' + key + '.json', candidate[key])
        self.write('.build/owner.json', {'journey_sha256': hashlib.sha256((self.root / '.build/journey.json').read_bytes()).hexdigest(),
                                       'battery_wake': 'accepted', 'optical_buttons': 'accepted', 'owner': 'accepted'})

    def approve(self, candidate, baseline=None):
        policy, raw = fixtures.approved(candidate, baseline)
        self.write('docs/reading-latency-policy.json', policy)
        path = self.root / '.build/baseline.json'
        path.write_bytes(raw)
        return ['--latency-baseline', str(path)]

    def arguments(self, state):
        args = ['reading-ready.py', state, '--variant', 'regional-dev-usb']
        if state in ('hardware-validated', 'owner-accepted', 'journey'):
            for flag, name in (('install', 'install'), ('plan', 'plan')):
                args.extend(['--' + flag, str(self.root / '.build' / (name + '.json'))])
            args.extend(['--target', self.current['target']])
        if state in ('hardware-validated', 'owner-accepted'):
            for flag, name in (('journey', 'journey'), ('ui-review', 'ui'), ('latency-conditions', 'conditions')):
                args.extend(['--' + flag, str(self.root / '.build' / (name + '.json'))])
        if state == 'owner-accepted':
            args.extend(['--owner-review', str(self.root / '.build/owner.json')])
        return args

    def invoke(self, state, extra=()):
        output = io.StringIO()
        with patch.object(sys, 'argv', self.arguments(state) + list(extra)), redirect_stdout(output):
            code = ready.main()
        self.software.assert_called_with([sys.executable, '-B', str(self.root / 'tools/check.py')],
                                         check=True, cwd=self.root)
        return code, json.loads(output.getvalue())

    def test_current_source_and_native_image_authority_cannot_come_from_journey(self):
        for mutation in ('dirty', 'source', 'version', 'image', 'variant', 'resource', 'inputs', 'extra-field'):
            source, native, resource = copy.deepcopy(self.source), copy.deepcopy(self.native), copy.deepcopy(self.resource)
            if mutation == 'dirty': source['dirty'] = True
            elif mutation == 'source': source['git_revision'] = 'a' * 40
            elif mutation == 'version': native['firmware_version'] = '0.0.0'
            elif mutation == 'image': native['binary_sha256'] = 'f' * 64
            elif mutation == 'variant': native['variant'] = 'dev-usb'
            elif mutation == 'resource': native['resource_identity']['inputs_sha256'] = 'f' * 64
            elif mutation == 'inputs': resource['identity']['inputs_sha256'] = 'f' * 64
            else: native['self_approved'] = True
            self.write('artifacts/firmware-identity-regional-dev-usb.json', native)
            self.write('artifacts/code-health-regional-dev-usb.json', resource)
            with self.subTest(mutation=mutation), patch.object(release_tools, 'source_identity', return_value=source):
                with self.assertRaisesRegex(ValueError, 'current clean native source/image'):
                    ready.latency_bundle(self.current['journey'], self.current['install'], self.current['plan'],
                                         self.expected, self.current['target'], 'regional-dev-usb', self.current['conditions'])

    def test_native_resource_rejection_stops_cli_before_latency_or_ui(self):
        with patch.object(ready, 'check_resources', return_value=['authored resource mismatch']), \
                patch.object(reading_latency, 'evaluate_latency') as evaluate:
            with self.assertRaisesRegex(ValueError, 'authored resource mismatch'):
                self.invoke('owner-accepted')
            evaluate.assert_not_called()
        self.compile.assert_not_called()

    def test_hardware_and_owner_cannot_promote_blocked_regression_or_missing_samples(self):
        for state in ('hardware-validated', 'owner-accepted'):
            for condition in ('no-baseline', 'regression', 'missing-samples'):
                with self.subTest(state=state, condition=condition):
                    self.compile.reset_mock(); self.ui.reset_mock()
                    candidate = fixtures.bundle(sampled=condition != 'missing-samples', factor=2 if condition == 'regression' else 1)
                    self.install_candidate(candidate)
                    extras = self.approve(candidate, fixtures.bundle()) if condition == 'regression' else self.approve(candidate)
                    if condition == 'no-baseline':
                        policy = json.loads((self.root / 'docs/reading-latency-policy.json').read_text())
                        policy['approved_baseline_sha256'] = None
                        self.write('docs/reading-latency-policy.json', policy)
                        extras = []
                    code, result = self.invoke(state, extras)
                    self.assertEqual(code, 1)
                    self.assertEqual(result['state'], 'correctness-validated')
                    self.assertFalse(result['ready_to_use'])
                    self.assertEqual(result['latency']['state'], 'regression' if condition == 'regression' else 'blocked')
                    self.assertFalse(result['latency']['physical_optical_measurement'])
                    self.compile.assert_not_called(); self.ui.assert_not_called()

    def test_stable_above_goal_pass_preserves_ui_and_manual_owner_checks(self):
        self.install_candidate(fixtures.bundle(factor=200))
        extras = self.approve(self.current)
        report = reading_latency.evaluate_latency(self.current,
                    json.loads((self.root / 'docs/reading-latency-policy.json').read_text()),
                    fixtures.CONTRACT, 1200, (self.root / '.build/baseline.json').read_bytes())
        self.assertEqual(report['state'], 'pass')
        self.assertTrue(all(not group['goals'][-1]['within_goal']
                            for group in report['groups'] if group['required']))
        for state in ('hardware-validated', 'owner-accepted'):
            with self.subTest(state=state):
                code, result = self.invoke(state, extras)
                self.assertEqual(code, 0)
                self.assertEqual(result['state'], state)
                self.assertEqual(result['ready_to_use'], state == 'owner-accepted')
                self.assertEqual(result['latency']['state'], 'pass')
        self.assertEqual(self.ui.call_count, 2)
        self.write('.build/owner.json', {'journey_sha256': 'f' * 64, 'battery_wake': 'accepted',
                                       'optical_buttons': 'accepted', 'owner': 'accepted'})
        with self.assertRaisesRegex(ValueError, 'Manual physical/owner acceptance'):
            self.invoke('owner-accepted', extras)

    def test_installable_and_journey_need_no_approved_latency_baseline(self):
        with patch.object(reading_latency, 'evaluate_latency') as evaluate:
            code, result = self.invoke('installable')
            self.assertEqual((code, result['state'], result['ready_to_use']), (0, 'installable', False))
            serial = Mock(name='authored serial module')
            held = False
            @contextmanager
            def lease(port):
                nonlocal held
                held = True
                try:
                    yield
                finally:
                    held = False
            usb = SimpleNamespace(approved_serial=Mock(return_value=serial), SerialClient=Mock(return_value=object()),
                                  lease_port=Mock(side_effect=lease))
            spec = SimpleNamespace(loader=SimpleNamespace(exec_module=Mock()))
            def recorded(destination, factory, *args, **kwargs):
                self.assertTrue(held)
                factory()  # Exercise only the explicitly mocked connection boundary.
                self.assertTrue(held)
                return {'terminal': 'complete'}
            with patch.object(ready.importlib.util, 'spec_from_file_location', return_value=spec), \
                    patch.object(ready.importlib.util, 'module_from_spec', return_value=usb), \
                    patch.object(ready, 'write_journey', side_effect=recorded):
                code, result = self.invoke('journey', ['--port', '/dev/authored-only', '--output',
                                                     str(self.root / '.build/authored-journey.json')])
            self.assertEqual((code, result['state'], result['readiness']), (0, 'journey-recorded', 'not-evaluated'))
            self.assertFalse(result['ready_to_use'])
            usb.SerialClient.assert_called_once_with(serial, '/dev/authored-only', 30)
            usb.lease_port.assert_called_once_with('/dev/authored-only')
            self.assertFalse(held)
            evaluate.assert_not_called()

    def test_private_latency_outputs_are_exclusive_0600_and_cannot_escape_build(self):
        bundle = self.root / '.build/private/candidate.json'
        report = self.root / '.build/private/report.json'
        code, result = self.invoke('hardware-validated', self.approve(self.current) +
                                  ['--latency-bundle', str(bundle), '--latency-report', str(report)])
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(report.read_text())['state'], 'pass')
        self.assertEqual(json.loads(bundle.read_text())['source_revision'], self.source['git_revision'])
        for path in (bundle, report):
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            before = path.read_bytes()
            with self.assertRaises(FileExistsError):
                ready.private_json(path, {'overwritten': True})
            self.assertEqual(path.read_bytes(), before)
        outside = self.root / 'authored-outside'
        outside.mkdir()
        (self.root / '.build/escape').symlink_to(outside, target_is_directory=True)
        for path in (outside / 'report.json', self.root / '.build/../escaped.json',
                     self.root / '.build/escape/report.json'):
            with self.subTest(path=path), self.assertRaisesRegex(ValueError, 'stay below ignored .build'):
                ready.private_json(path, {'authored': True})
            self.assertFalse(path.exists())
        for private in (self.current['conditions']['power'], self.current['install']['device'],
                        self.current['plan']['media_sha256']):
            self.assertNotIn(private, json.dumps(result))

    def test_oversized_baseline_is_bounded_before_evaluator(self):
        baseline = self.root / '.build/oversized.json'
        reads = []
        class BoundedFixture(io.BytesIO):
            def read(self, size=-1):
                reads.append(size)
                return super().read(size)
        original_open = Path.open
        def opened(path, *args, **kwargs):
            if path == baseline:
                return BoundedFixture(b' ' * (2 * 1024 * 1024))
            return original_open(path, *args, **kwargs)
        with patch.object(Path, 'open', opened), patch.object(reading_latency, 'evaluate_latency') as evaluate:
            with self.assertRaisesRegex(ValueError, 'baseline exceeds evidence limit'):
                self.invoke('hardware-validated', ['--latency-baseline', str(baseline)])
            evaluate.assert_not_called()
        self.assertEqual(reads, [1024 * 1024 + 1])
        self.compile.assert_not_called()

    def test_latency_arguments_are_rejected_for_non_admission_states_before_operations(self):
        for state in ('software-verified', 'installable', 'journey'):
            with self.subTest(state=state), patch.object(sys, 'argv', self.arguments(state) +
                                                       ['--latency-report', str(self.root / '.build/report.json')]), \
                    redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as rejected:
                    ready.main()
                self.assertEqual(rejected.exception.code, 2)
        self.software.assert_not_called()
        self.resources.assert_not_called()


if __name__ == '__main__':
    unittest.main()
