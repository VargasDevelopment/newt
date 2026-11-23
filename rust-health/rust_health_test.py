#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Focused fixtures for rust_health: one clean tree, one failing tree per check,
and the advisory size warning.

The negative control removes each fixture's check and requires the fixture to
pass; that proves each fixture fails because of its own check alone.
"""
from __future__ import annotations

import contextlib
import io
import shutil
import sys
import tempfile
import unittest
from collections import Counter
from collections.abc import Callable
from pathlib import Path
from unittest.mock import patch

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from gate_tier import negative_controls
from rust_health import (CHECKS, Baseline, baseline_from, main, measure, run_checks, size_warnings,  # noqa: E402
                         write_baseline)

Mutation = Callable[[Path], None]


def manifest(name: str, *dependencies: str) -> str:
    lines = [f'[package]\nname = "fx-{name}"\nversion = "0.0.0"\nedition = "2024"\n\n[dependencies]']
    return '\n'.join([*lines, *(f'fx-{dep} = {{ path = "../{dep}" }}' for dep in dependencies)]) + '\n'


CORE_LIB = '''#![forbid(unsafe_code)]
//! Text is not code: unsafe { x.unwrap() } and todo!() in docs.
extern crate alloc;
pub fn parse(text: &str) -> Option<u8> {
    let _quoted = "panic!(\\"unsafe {}\\") .unwrap()";
    let _raw = r#"todo!() " .expect("#;
    let _brace = '{';
    text.parse().ok()
}

#[cfg(test)]
mod helpers;

#[cfg(test)]
mod tests {
    #[test]
    fn parses() {
        super::parse("1").unwrap();
        panic!("only in tests");
    }
}
'''

FFI_LIB = '''unsafe extern "C" {
    fn c_value() -> u8;
}

pub fn value() -> u8 {
    // SAFETY: c_value has no preconditions.
    unsafe { c_value() }
}
'''

FIXTURE = {
    'rust/Cargo.toml': '[workspace]\nmembers = ["core", "ui", "ffi", "host"]\n',
    'rust/Cargo.lock': 'version = 4\n' + ''.join(f'\n[[package]]\nname = "fx-{name}"\nversion = "0.0.0"\n'
                                               for name in ('core', 'ui', 'ffi', 'host')),
    'rust/core/Cargo.toml': manifest('core'),
    'rust/core/src/lib.rs': CORE_LIB,
    'rust/core/src/helpers.rs': 'pub fn helper() -> u8 { None.unwrap() }\n',
    'rust/ui/Cargo.toml': manifest('ui', 'core'),
    'rust/ui/src/lib.rs': 'pub fn show() -> u8 { 1 }\n',
    'rust/ui/src/data.rs': '0,\n' * 600,
    'rust/ffi/Cargo.toml': manifest('ffi', 'core'),
    'rust/ffi/src/lib.rs': FFI_LIB,
    'rust/host/Cargo.toml': manifest('host', 'core', 'ffi'),
    'rust/host/src/main.rs': 'fn main() { std::env::args().next().unwrap(); }\npub extern "C" fn hook() {}\n',
    'rust/tests/ignored.rs': 'fn later() { todo!() }\n',
}

POLICY = Baseline(line_limit=500, generated=['rust/ui/src/data.rs'], platform_crates=['ffi', 'host'],
                  platform_edge_exceptions=[], native_owners=['ffi'], native_exceptions={},
                  external_packages=[], files={})


def write(path: str, text: str) -> Mutation:
    return lambda root: (root / path).write_text(text)


def append(path: str, text: str) -> Mutation:
    return lambda root: (root / path).write_text((root / path).read_text() + text)


def replace(path: str, old: str, new: str) -> Mutation:
    return lambda root: (root / path).write_text((root / path).read_text().replace(old, new))


NEW_UNWRAP = append('rust/ui/src/lib.rs', 'pub fn first(v: &[u8]) -> u8 { *v.first().unwrap() }\n')
DOCUMENTED_READ = 'pub fn read(p: *const u8) -> u8 {\n    // SAFETY: callers pass a valid pointer.\n    unsafe { *p }\n}\n'
NEGATIVE: dict[str, tuple[str, Mutation]] = {
    'more unsafe in an existing file': ('unsafe', append('rust/ffi/src/lib.rs', DOCUMENTED_READ)),
    'undocumented unsafe block': ('safety', write('rust/ffi/src/raw.rs',
                                                  'pub fn read(p: *const u8) -> u8 {\n    unsafe { *p }\n}\n')),
    'unsafe in a zero-unsafe crate': ('unsafe_crates', write('rust/ui/src/raw.rs', DOCUMENTED_READ)),
    'new allow': ('suppressions', append('rust/core/src/lib.rs', '#[allow(dead_code)]\nfn unused() {}\n')),
    'new unwrap': ('panic_surface', NEW_UNWRAP),
    'todo!': ('banned', append('rust/ui/src/lib.rs', 'pub fn later() { todo!() }\n')),
    'new external crate': ('externals', append(
        'rust/Cargo.lock', '\n[[package]]\nname = "serde"\nversion = "1.0.0"\n'
                           'source = "registry+https://github.com/rust-lang/crates.io-index"\n')),
    'forbidden crate edge': ('crate_edges', append('rust/ui/Cargo.toml', 'fx-ffi = { path = "../ffi" }\n')),
    'extern "C" outside the native owners': ('natives', write('rust/ui/src/export.rs',
                                                              'pub extern "C" fn exported() -> u8 { 1 }\n')),
    'a grown native exception': ('natives', append('rust/host/src/main.rs', 'pub extern "C" fn other() {}\n')),
}


class RustHealthTest(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = Path(tempfile.mkdtemp(prefix='rust-health-'))
        self.addCleanup(shutil.rmtree, self.directory)
        self.clean = self.directory / 'clean'
        for path, text in FIXTURE.items():
            (self.clean / path).parent.mkdir(parents=True, exist_ok=True)
            (self.clean / path).write_text(text)
        self.baseline = baseline_from(measure(self.clean), POLICY)

    def fixture(self, *mutations: Mutation) -> Path:
        root = Path(tempfile.mkdtemp(dir=self.directory))
        shutil.copytree(self.clean, root, dirs_exist_ok=True)
        for mutate in mutations:
            mutate(root)
        return root

    def test_clean_fixture_passes(self) -> None:
        tree = measure(self.clean)
        self.assertEqual(run_checks(tree, self.baseline), {})
        # Docs, strings, char braces, test modules and test-only files count nothing.
        self.assertEqual(tree.sources['rust/core/src/lib.rs'].metrics, Counter())
        self.assertEqual(tree.test_only, ['rust/core/src/helpers.rs'])
        self.assertNotIn('rust/tests/ignored.rs', tree.sources)
        self.assertEqual(self.baseline['files']['rust/ffi/src/lib.rs'],
                         {'unsafe_block': 1, 'unsafe_extern': 1, 'unsafe_undocumented': 1})
        self.assertEqual(tree.forbid_gaps, ['rust/host/src/main.rs', 'rust/ui/src/lib.rs'])
        # The owner's extern block is free; the host's export is a counted exception.
        self.assertEqual(self.baseline['native_exceptions'], {'rust/host/src/main.rs': 1})

    def test_each_negative_fixture_fails_its_check(self) -> None:
        for name, (check, mutate) in NEGATIVE.items():
            with self.subTest(name):
                self.assertEqual(list(run_checks(measure(self.fixture(mutate)), self.baseline)), [check])

    def test_test_only_scope_reaches_descendants_but_not_shared_production(self) -> None:
        root = self.fixture()
        helpers = root/'rust/core/src/helpers.rs'
        helpers.write_text('#[path="nested.rs"] mod nested;\n')
        child = helpers.parent/'nested.rs'
        child.write_text('pub fn test_only() { None.unwrap(); }\n')
        tree = measure(root)
        self.assertNotIn('rust/core/src/nested.rs', tree.sources)
        self.assertIn('rust/core/src/nested.rs', tree.test_only)
        library = root/'rust/core/src/lib.rs'
        library.write_text(library.read_text() + '\nmod nested;\n')
        tree = measure(root)
        self.assertIn('rust/core/src/nested.rs', tree.sources)
        self.assertEqual(list(run_checks(tree, self.baseline)), ['panic_surface'])

    def test_unreferenced_module_cycle_remains_production(self) -> None:
        root = self.fixture()
        source = root/'rust/core/src'
        (source/'left.rs').write_text('#[path="right.rs"] mod right;\n')
        (source/'right.rs').write_text('#[path="left.rs"] mod left;\nfn fail() { None.unwrap(); }\n')
        self.assertIn('rust/core/src/right.rs', measure(root).sources)
        self.assertEqual(list(run_checks(measure(root), self.baseline)), ['panic_surface'])

    @unittest.skipUnless(negative_controls(__file__), 'negative control not selected by the gate tier')
    def test_negative_control_without_its_check_each_fixture_passes(self) -> None:
        for name, (check, mutate) in NEGATIVE.items():
            with self.subTest(name):
                remaining = {key: value for key, value in CHECKS.items() if key != check}
                self.assertEqual(run_checks(measure(self.fixture(mutate)), self.baseline, remaining), {})

    def test_an_oversized_file_warns_and_never_fails(self) -> None:
        """Size is advisory: main passes and prints the warning. Fails if size
        becomes a failing check again."""
        root = self.fixture(write('rust/ui/src/big.rs', '// line\n' * 501))
        tree = measure(root)
        self.assertEqual(run_checks(tree, self.baseline), {})
        self.assertEqual([path for path, _ in size_warnings(tree, self.baseline)], ['rust/ui/src/big.rs'])
        path = root / 'baseline.json'
        write_baseline(path, self.baseline)
        for actions, prefix in (('', 'WARN '), ('true', '::warning file=rust/ui/src/big.rs::')):
            with self.subTest(actions=actions):
                output = io.StringIO()
                with patch.dict('os.environ', GITHUB_ACTIONS=actions), contextlib.redirect_stdout(output):
                    self.assertEqual(main(['--root', str(root), '--baseline', str(path)]), 0)
                self.assertIn(f'{prefix}rust/ui/src/big.rs: 501 lines, over the 500-line guide', output.getvalue())

    def test_a_stale_generated_exclusion_fails_and_blocks_update(self) -> None:
        root = self.fixture(lambda root: (root / 'rust/ui/src/data.rs').unlink())
        self.assertEqual(list(run_checks(measure(root), self.baseline)), ['generated'])
        path = root / 'baseline.json'
        write_baseline(path, self.baseline)
        before = path.read_bytes()
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(main(['--root', str(root), '--baseline', str(path), '--update']), 1)
        self.assertEqual(path.read_bytes(), before)

    def test_update_only_lowers_a_recorded_size(self) -> None:
        recorded = {**self.baseline, 'files': {**self.baseline['files'], 'rust/ui/src/big.rs': {'lines': 520}}}
        for lines, expected in ((510, 510), (530, 520), (400, None)):
            with self.subTest(lines=lines):
                tree = measure(self.fixture(write('rust/ui/src/big.rs', '// line\n' * lines)))
                self.assertEqual(baseline_from(tree, recorded)['files']['rust/ui/src/big.rs'].get('lines'), expected)
        # A new oversized file gets no recorded size, so it keeps warning.
        tree = measure(self.fixture(write('rust/ui/src/big.rs', '// line\n' * 530)))
        self.assertNotIn('lines', baseline_from(tree, self.baseline)['files']['rust/ui/src/big.rs'])

    def test_update_tightens_a_passing_tree_and_refuses_a_failing_one(self) -> None:
        documented = replace('rust/ffi/src/lib.rs', 'unsafe extern', '// SAFETY: C declares this signature.\nunsafe extern')
        root = self.fixture(documented)
        path = root / 'baseline.json'
        write_baseline(path, self.baseline)
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(main(['--root', str(root), '--baseline', str(path), '--update']), 0)
            self.assertNotIn('unsafe_undocumented', path.read_text())
            tightened = path.read_bytes()
            NEW_UNWRAP(root)
            self.assertEqual(main(['--root', str(root), '--baseline', str(path), '--update']), 1)
        self.assertEqual(path.read_bytes(), tightened)


if __name__ == '__main__':
    unittest.main()
