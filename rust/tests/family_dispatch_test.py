"""Scoped dispatch keeps every family in the gate without repeating unrelated work."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('family_dispatch_checker', ROOT / 'rust/check.py')
checker = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = checker
spec.loader.exec_module(checker)


class FamilyDispatchTest(unittest.TestCase):
    def test_navigation_control_anchors_match_current_sources_once(self):
        spec = importlib.util.spec_from_file_location('navigation_controls',
            ROOT / 'rust/tests/reader_navigation/controls.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        for name, file, before, after, witnesses in module.CONTROLS:
            with self.subTest(control=name):
                self.assertEqual((ROOT / 'rust/reader-navigation/src' / file).read_text().count(before), 1)
                self.assertNotEqual(before, after)
                self.assertTrue(witnesses)

    def dispatch(self, args, *, closing_error=None):
        with tempfile.TemporaryDirectory() as directory, contextlib.ExitStack() as stack:
            root = Path(directory)
            snapshot = root / 'snapshot'
            (snapshot / 'rust').mkdir(parents=True)
            for name in ('reference.json', 'toolchain.json'):
                (snapshot / 'rust' / name).write_text('{}')
            build = root / '.build/rust'
            build.mkdir(parents=True)
            stack.enter_context(patch.object(checker, 'ROOT', root))
            stack.enter_context(patch.object(checker, 'BUILD', build))
            stack.enter_context(patch.object(sys, 'path', list(sys.path)))
            hooks = {}
            values = dict(verify_source_inventory=None, verify_sources=None, close_sources=None,
                          run=SimpleNamespace(stdout='', stderr=''), digest='digest',
                          foundation=(['clock'], ['expected'], build / 'binary'),
                          foundation_inputs=(['clock'], ['expected']), controls=[{'name': 'control'}],
                          target_probes={'target_archive_sha256': 'target'})
            for name, value in values.items():
                hooks[name] = stack.enter_context(patch.object(checker, name, return_value=value))
            hooks['close_sources'].side_effect = closing_error
            family = Mock()
            family.check.return_value = dict(comparisons=1, semantic_controls=[{'name': 'control'}],
                                            controls=[{'name': 'control'}])
            hooks['load_family'] = stack.enter_context(patch.object(checker, 'load_family', return_value=family))
            stack.enter_context(patch.object(checker.toolchain, 'verify_installed', return_value={}))
            stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
            if closing_error:
                with self.assertRaisesRegex(RuntimeError, 'closing failure'):
                    checker._check_snapshot(args, {}, snapshot, {})
                self.assertEqual(list(build.glob('*.json')), [])
                return hooks, family, None
            checker._check_snapshot(args, {}, snapshot, {})
            outputs = list(build.glob('*.json'))
            self.assertEqual(len(outputs), 1)
            return hooks, family, json.loads(outputs[0].read_text())

    def test_each_adapter_runs_once_and_never_runs_other_families(self):
        for name in checker.ADAPTER_FAMILIES:
            for selected in (name, 'none'):
                with self.subTest(family=name, controls=selected):
                    args = checker.parse_args([f'--family={name}', '--tier=fast', f'--control-family={selected}'])
                    hooks, family, receipt = self.dispatch(args)
                    self.assertEqual(hooks['load_family'].call_args.args[1], name)
                    hooks['load_family'].assert_called_once()
                    self.assertEqual(family.check.call_args.args[-1], selected == 'none')
                    for skipped in ('foundation', 'foundation_inputs', 'controls', 'target_probes', 'run'):
                        hooks[skipped].assert_not_called()
                    hooks['close_sources'].assert_called_once()
                    self.assertFalse(receipt['complete_rust_gate'])
                    self.assertEqual(receipt['scope'], name)

    def test_foundation_controls_load_inputs_without_positive_sweep(self):
        for campaign in checker.FOUNDATION_CAMPAIGNS:
            with self.subTest(campaign=campaign):
                hooks, family, receipt = self.dispatch(checker.parse_args([
                    '--foundation-controls=' + campaign]))
                hooks['foundation_inputs'].assert_called_once()
                hooks['controls'].assert_called_once()
                self.assertEqual(hooks['controls'].call_args.args[-1], campaign)
                for skipped in ('foundation', 'load_family', 'target_probes', 'run'):
                    hooks[skipped].assert_not_called()
                self.assertEqual(receipt['total_comparisons'], 0)
                self.assertEqual(receipt['total_negative_controls'], 1)
                self.assertEqual(receipt['scope'], 'foundation-controls-' + campaign)
                self.assertFalse(receipt['complete_rust_gate'])

    def test_full_plan_partitions_live_registry_and_future_controls_follow_their_module(self):
        import gate_plan
        registry = checker.foundation_registry()
        partition = checker.foundation_partition(registry)
        planned = {arg.split('=', 1)[1] for phase in gate_plan.plan('linux', False)
                   for arg in phase.command if arg.startswith('--foundation-controls=')}
        self.assertEqual(planned, set(partition))
        whole = [control for module in registry.values() for control in module]
        actual = [control for campaign in partition.values() for control in campaign]
        self.assertCountEqual(actual, whole)
        for module, controls in registry.items():
            marker = ('new ' + module, 'synthetic.rs', 'before', 'after')
            updated = dict(registry, **{module: (*controls, marker)})
            changed = checker.foundation_partition(updated)
            owner, = [name for name, modules in checker.FOUNDATION_CAMPAIGNS.items() if module in modules]
            self.assertEqual(tuple(control for control in changed[owner] if control != marker),
                             partition[owner])
            for name in partition.keys() - {owner}:
                self.assertEqual(changed[name], partition[name])
            self.assertEqual(sum(c.count(marker) for c in changed.values()), 1)

    def test_unassigned_or_doubly_assigned_foundation_controls_are_rejected(self):
        registry = checker.foundation_registry()
        with self.assertRaisesRegex(ValueError, 'exactly one campaign'):
            checker.foundation_partition(dict(registry, unassigned=(('new', 'x', 'a', 'b'),)))
        first, second, *_ = registry
        duplicate = dict(registry, **{second: (*registry[second], registry[first][0])})
        with self.assertRaisesRegex(ValueError, 'registered more than once'):
            checker.foundation_partition(duplicate)
        overlapping = dict(checker.FOUNDATION_CAMPAIGNS)
        overlapping['clock-frame'] = (*overlapping['clock-frame'], 'base')
        with self.assertRaisesRegex(ValueError, 'exactly one campaign'):
            checker.foundation_partition(registry, overlapping)

    def test_target_phase_runs_runner_regressions_and_target_checks_only(self):
        hooks, family, receipt = self.dispatch(checker.parse_args(['--target-probes']))
        hooks['target_probes'].assert_called_once()
        hooks['run'].assert_called_once()
        self.assertEqual(Path(hooks['run'].call_args.args[0][-1]).name, 'runner_test.py')
        for skipped in ('foundation', 'foundation_inputs', 'controls', 'load_family'):
            hooks[skipped].assert_not_called()
        self.assertFalse(receipt['complete_rust_gate'])

    def test_target_verifiers_cannot_disappear_or_be_renamed(self):
        import inspect
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            artifact = target / 'riscv32imc-unknown-none-elf/release/libnewt_target_probe.a'
            artifact.parent.mkdir(parents=True)
            artifact.write_bytes(b'synthetic archive')
            required = set(checker.ADAPTER_FAMILIES) - {'reader_atlas', 'reader_navigation', 'epub_admission'}
            def load(snapshot, family):
                self.assertIn(family, required)
                return SimpleNamespace(verify_target=lambda *args: 'verified')
            with patch.object(checker, 'ROOT', target), patch.object(checker, 'cargo'), patch.object(checker, 'run', return_value=
                    SimpleNamespace(stdout=inspect.getsource(checker.target_probes))), \
                    patch.object(checker, 'load_family', side_effect=load) as loaded:
                receipt = checker.target_probes(Path(directory), target)
                self.assertEqual({call.args[1] for call in loaded.call_args_list}, required)
                self.assertEqual(sum(key.endswith(('_probe_symbol', '_probe_symbols')) for key in receipt),
                                 len(required))
                for missing in required:
                    with self.subTest(missing=missing), self.assertRaises(AttributeError):
                        loaded.side_effect = lambda snapshot, family: (SimpleNamespace(verify_target_renamed=None)
                            if family == missing else load(snapshot, family))
                        checker.target_probes(Path(directory), target)

    def test_standalone_full_still_runs_all_families_and_foundation(self):
        hooks, family, receipt = self.dispatch(checker.parse_args([]))
        hooks['foundation'].assert_called_once()
        hooks['controls'].assert_called_once()
        hooks['target_probes'].assert_called_once()
        self.assertEqual([call.args[1] for call in hooks['load_family'].call_args_list],
                         list(checker.ADAPTER_FAMILIES))
        self.assertEqual(family.check.call_count, 19)
        self.assertTrue(all(call.args[-1] is False for call in family.check.call_args_list))
        self.assertEqual(receipt['total_comparisons'], 20)
        self.assertEqual(receipt['total_negative_controls'], 20)
        self.assertTrue(receipt['complete_rust_gate'])

    def test_closing_failure_prevents_every_scoped_receipt(self):
        for options in (['--family=css'], ['--foundation-controls=records'], ['--target-probes']):
            with self.subTest(options=options):
                self.dispatch(checker.parse_args(options), closing_error=RuntimeError('closing failure'))

    def test_scoped_reruns_invalidate_old_receipts_before_input_validation(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(checker, 'BUILD', Path(directory)), \
                patch.object(checker, 'source_hashes', side_effect=RuntimeError('bad inputs')):
            for options, name in ((['--landing'], 'landing.json'), (['--family=css'], 'phase-css.json'),
                                  (['--foundation-controls=records'], 'phase-foundation-controls-records.json'),
                                  (['--target-probes'], 'phase-target-probes.json'), ([], 'evidence.json')):
                output = checker.BUILD / name
                output.write_text('previous')
                with self.subTest(options=options), self.assertRaisesRegex(RuntimeError, 'bad inputs'):
                    checker.check(checker.parse_args(options))
                self.assertEqual(output.exists(), not options)

    def test_scope_and_control_arguments_cannot_disagree(self):
        for options in (['--family=css', '--landing'], ['--family=css', '--target-probes'],
                        ['--family=css', '--foundation-controls=records'],
                        ['--family=css', '--tier=fast', '--control-family=inflate'],
                        ['--foundation-controls=records', '--positive-only'], ['--target-probes', '--tier=full']):
            with self.subTest(options=options), contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                checker.parse_args(options)


if __name__ == '__main__':
    unittest.main()
