"""Gate tier and control ownership selection, against synthetic and real inputs."""
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
import gate_plan
import gate_tier


class ControlSelectionTests(unittest.TestCase):
    """The local tier runs the controls of each test whose sources name a changed tests/ file."""
    def test_shared_inputs_select_the_tests_that_use_them(self):
        for changed, user in (('tests/heap_clear_idf/heap_memory_layout.h', 'heap_clear_test.py'),  # by its folder
                              ('tests/heap_clear_model.cpp', 'heap_clear_test.py')):  # by path
            with self.subTest(changed=changed):
                self.assertIn(user, gate_tier.changed_tests(gate_plan.ROOT, [changed]))

    def test_a_changed_file_no_test_names_selects_every_control(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'tests').mkdir()
            for name, text in (('a_test.py', 'import helper\n'), ('helper.py', "SOURCE = 'tests/shared.h'\n"),
                               ('shared.h', ''), ('orphan.h', '')):
                (root / 'tests' / name).write_text(text)
            self.assertEqual(gate_tier.changed_tests(root, ['tests/shared.h']), ('a_test.py',))  # through a module
            self.assertEqual(gate_tier.changed_tests(root, ['tests/a.cpp']), ('a_test.py',))  # named after its test
            for changed in (['tests/orphan.h'], ['tests/shared.h', 'tests/orphan.h']):
                with self.subTest(changed=changed):
                    self.assertIsNone(gate_tier.changed_tests(root, changed))

    def test_rust_roots_shared_helpers_and_unowned_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, text in {
                'rust/xml_input_gate.py': 'import common_helper\n',
                'rust/css_gate.py': 'import common_helper\n',
                'rust/common_helper.py': 'SHARED = ROOT / "rust/tests/shared.json"\n',
                'rust/tests/shared.json': '{}',
                'rust/tests/xml_input/mutations.json': '{}',
                'rust/tests/css/mutations.json': '{}',
                'rust/tests/unregistered/mutations.json': '{}',
                'rust-health/rust_health.py': '',
            }.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text)
            self.assertEqual(gate_tier.changed_tests(root, ['rust/tests/xml_input/mutations.json']),
                             ('rust:xml_input',))
            self.assertEqual(gate_tier.changed_tests(root, ['rust/tests/css/mutations.json']), ('rust:css',))
            self.assertEqual(gate_tier.changed_tests(root, ['rust/tests/shared.json']), ('rust:css', 'rust:xml_input'))
            self.assertIsNone(gate_tier.changed_tests(root, ['rust/tests/unregistered/mutations.json']))
            self.assertEqual(gate_tier.changed_tests(root, ['rust-health/rust_health.py']), ('rust_health_test.py',))
            shared = gate_tier.changed_tests(root, ['rust/tests/inflate/run_support.py'])
            self.assertEqual({name for name in shared if name.startswith('rust:')},
                             {'rust:foundation', *('rust:' + name for name in gate_plan.ADAPTER_FAMILIES)})

    def test_each_full_only_suite_runs_for_its_product_source_but_not_unrelated_source(self):
        for family, sources in gate_plan.FULL_POSITIVE_SOURCES.items():
            source = sources[0] + ('/src/lib.rs' if family == 'reader_atlas' else '')
            with self.subTest(family=family):
                keys = set(gate_tier.changed_tests(gate_plan.ROOT, [source]))
                self.assertIn('rust-positive:' + family, keys)
                self.assertNotIn('rust:' + family, keys)
                phases = gate_plan.plan('linux', False, keys.__contains__)
                phase, = [p for p in phases if '--family=' + family in p.command]
                self.assertIn('--control-family=none', phase.command)
                unrelated = set(gate_tier.changed_tests(gate_plan.ROOT, ['rust/core/src/time_zone.rs']))
                self.assertFalse(any('--family=' + family in p.command
                    for p in gate_plan.plan('linux', False, unrelated.__contains__)))

    def test_real_rust_packets_are_path_owned(self):
        root = gate_plan.ROOT
        for path, owners in (
            ('rust/tests/xml_input/mutations.json', ('rust:xml_input',)),
            ('rust/tests/reader_atlas/gate.py', ('rust:reader_atlas',)),
            ('rust/phased-reader/host/fixtures.py', ()),
            ('rust/tests/slot_vectors/vectors.rs', ('slot_vectors_test.py',)),
            ('rust/tests/app_store/record.rs', ('app_store_test.py',)),
            ('rust/tests/dev_put/lease.rs', ('dev_put_test.py',)),
            ('rust/tests/large_epub_corpus/corpus.rs', ('large_epub_corpus_test.py',)),
            ('tools/large_epub_corpus.py', ('large_epub_corpus_test.py',)),
            ('rust/tests/family_dispatch_test.py', tuple(sorted({
                'family_dispatch_test.py', 'rust:foundation',
                *('rust:' + family for family in gate_plan.ADAPTER_FAMILIES)}))),
        ):
            with self.subTest(path=path):
                self.assertTrue((root / path).is_file())
                self.assertEqual(gate_tier.changed_tests(root, [path]), owners)
        # Equal basenames in other packets must not connect their owners.
        for packet in ('css', 'xml_session', 'package_parsers'):
            path = 'rust/tests/' + packet + '/mutations.json'
            self.assertTrue((root / path).is_file())
            self.assertNotIn('rust:xml_input', gate_tier.changed_tests(root, [path]))
        self.assertEqual(gate_tier.changed_tests(root, ['rust/tests/xml_input/deleted.json']),
                         ('rust:xml_input',))
        self.assertEqual(gate_tier.changed_tests(root, ['rust/tests/slot_vectors/deleted.rs']),
                         ('slot_vectors_test.py',))
        listed = subprocess.check_output(['git', 'ls-files', '--', *gate_tier.TEST_PATHS],
                                         cwd=root, text=True).splitlines()
        self.assertNotIn('rust/phased-reader/host/fixtures.py', listed)
        self.assertIn('rust/check.py', listed)
        self.assertIn('tools/large_epub_corpus.py', listed)

    def test_real_shared_validator_dependencies_plan_every_foundation_campaign(self):
        def require_shared_walk():
            for name in ('prefix_validation.py', 'persistence_protocol_validation.py',
                         'external_progress_validation.py'):
                selected = set(gate_tier.changed_tests(gate_plan.ROOT, ['rust/tests/' + name]))
                self.assertIn('rust:foundation', selected)
                self.assertTrue({'rust:' + family for family in
                    ('foundation', *gate_plan.ADAPTER_FAMILIES)} <= selected)
                planned = {arg.split('=', 1)[1] for phase in
                           gate_plan.plan('linux', False, selected.__contains__)
                           for arg in phase.command if arg.startswith('--foundation-controls=')}
                self.assertEqual(planned, set(gate_plan.FOUNDATION_CAMPAIGNS))
        require_shared_walk()
        # Without shared walk roots, incidental owners still match, hiding the
        # missing foundation campaigns from the unowned-input fallback.
        with patch.object(gate_tier, 'SHARED_INPUTS', ()), \
                self.assertRaisesRegex(AssertionError, 'rust:foundation'):
            require_shared_walk()

    def test_full_only_source_owners_cover_real_module_and_include_children(self):
        import importlib.util
        import rust_gate_families as families
        spec = importlib.util.spec_from_file_location('owner_rust_source',
                                                     gate_plan.ROOT / 'rust-health/rust_source.py')
        source = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(source)
        def assert_children_owned():
            root = gate_plan.ROOT
            for family, roots in families.FULL_POSITIVE_SOURCES.items():
                pending = [p for name in roots for p in
                    ((root / name).rglob('*.rs') if (root / name).is_dir() else [root / name])]
                reached = set()
                while pending:
                    path = pending.pop()
                    if path in reached:
                        continue
                    reached.add(path)
                    self.assertIn('rust-positive:' + family,
                                  families.positive_owners(path.relative_to(root).as_posix()),
                                  path.relative_to(root).as_posix())
                    text = path.read_text();code, _ = source.lex(text)
                    pending.extend(child for child, _ in source.module_declarations(path, text, code, []))
                    for match in re.finditer(r'\binclude!\s*\(', code):
                        literal = re.match(r'\s*"([^"]+)"', text[match.end():])
                        if literal:
                            pending.append(path.parent / literal[1])
        assert_children_owned()
        original = families.FULL_POSITIVE_SOURCES['epub_admission']
        with patch.dict(families.FULL_POSITIVE_SOURCES, epub_admission=tuple(
                p for p in original if not p.endswith('canonical_semantic.rs'))), \
                self.assertRaisesRegex(AssertionError, 'canonical_semantic'):
            assert_children_owned()

    def test_rust_control_discovery_diffs_test_inputs_without_product_code(self):
        calls = []
        def git(command, **kwargs):
            calls.append(command)
            return type('Result', (), {'returncode': 0, 'stdout': 'base\n' if command[1] == 'merge-base' else ''})()
        with patch.object(gate_tier.subprocess, 'run', side_effect=git):
            self.assertEqual(gate_tier.controls('local', gate_plan.ROOT), '')
        for command in calls[1:]:
            paths = command[command.index('--') + 1:]
            self.assertIn('rust/tests', paths)
            self.assertIn(':(glob)rust/*.py', paths)
            self.assertNotIn('rust/*.py', paths)
            self.assertIn('rust-health', paths)
            self.assertIn('tools/large_epub_corpus.py', paths)
            self.assertNotIn('rust', paths)
            self.assertFalse(any('*.rs' in path for path in paths))



if __name__ == '__main__':
    unittest.main()
