#!/usr/bin/env python3
"""tools/nightly.py in disposable repositories with a local bare origin: the
nightly version, the stamp commit and its tag, which pushes get none, the exact
stamp release_publish.py trusts, and that nightly tags never confuse the release
tools or Commitizen. --negative-controls removes each guard in a copy of its tool
and requires its test to fail. Never tags or pushes the checkout."""
import argparse
from contextlib import redirect_stderr
from datetime import datetime, timezone
import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--tool-root', type=Path, default=ROOT / 'tools')
parser.add_argument('--single', help='run one test (a negative control)')
parser.add_argument('--negative-controls', action='store_true')
args, _ = parser.parse_known_args()
sys.path.insert(0, str(ROOT / 'tests'))
sys.path.insert(0, str(ROOT / 'tools'))
sys.path.insert(0, str(args.tool_root))
import nightly
import release_tag
import release_tools as release
from release_tools_test import fixture_version

DAY = datetime(2026, 11, 22, 23, 59, tzinfo=timezone.utc)
FILES = ('.cz.toml', 'requirements-release.txt', release.VERSION_HEADER, 'tools/changelog.md.j2')


class NightlyTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix='nightly-')
        self.addCleanup(scratch.cleanup)
        self.root, self.origin = Path(scratch.name) / 'work', Path(scratch.name) / 'origin.git'
        self.environment = {**os.environ, 'GIT_AUTHOR_DATE': '2026-11-22T00:00:00+00:00',
                            'GIT_COMMITTER_DATE': '2026-11-22T00:00:00+00:00', 'GIT_AUTHOR_NAME': 'Fixture',
                            'GIT_AUTHOR_EMAIL': 'fixture@example.invalid', 'GIT_COMMITTER_NAME': 'Fixture',
                            'GIT_COMMITTER_EMAIL': 'fixture@example.invalid'}
        for name in FILES:
            (self.root / name).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / name, self.root / name)
        fixture_version(self.root, '0.2.0')
        (self.root / '.release-venv').symlink_to(release.release_python().parent.parent, target_is_directory=True)
        (self.root / '.gitignore').write_text('.release-venv\n')
        subprocess.run(['git', 'init', '-q', '--bare', '-b', 'main', str(self.origin)], check=True)
        self.git('init', '-q', '-b', 'main')
        self.git('config', 'core.hooksPath', str(self.root / 'no-hooks'))
        self.git('remote', 'add', 'origin', str(self.origin))
        self.git('add', '.')
        self.git('commit', '-qm', 'chore(release): 0.1.0 -> 0.2.0')
        self.git('tag', '-a', 'v0.2.0', '-m', 'Newt firmware 0.2.0')
        self.main = self.push()

    def git(self, *arguments):
        return subprocess.run(['git', *arguments], cwd=self.root, env=self.environment, check=True,
                              capture_output=True, text=True).stdout.strip()

    def push(self):
        """Pushes HEAD as origin's main and fetches it back, as CI's checkout sees main."""
        self.git('push', '-q', 'origin', 'HEAD:refs/heads/main')
        self.git('fetch', '-q', 'origin')
        return self.git('rev-parse', 'HEAD')

    def change(self, path, message):
        """A commit on main's tip that changes `path`, pushed."""
        self.git('checkout', '-q', '--detach', 'origin/main')
        (self.root / path).parent.mkdir(parents=True, exist_ok=True)
        (self.root / path).write_text(message + '\n')
        self.git('add', path)
        self.git('commit', '-qm', message)
        return self.push()

    def stamp(self, force=False):
        with redirect_stderr(io.StringIO()):
            return nightly.stamp(self.root, DAY, force)

    def test_the_version_is_the_next_patch_numbered_per_day(self):
        self.assertEqual(nightly.nightly_version('0.2.0', '20261122', []), '0.2.1-dev.20261122.1')
        tags = ['v0.2.1-dev.20261122.1', 'v0.2.1-dev.20261122.3', 'v0.2.1-dev.20261121.7',
                'v0.3.1-dev.20261122.9', 'v0.2.1', 'v0.2.1-dev.20261122.x', 'v0.2.1-dev.20261122.03']
        self.assertEqual(nightly.nightly_version('0.2.0', '20261122', tags), '0.2.1-dev.20261122.4')
        self.assertEqual(nightly.nightly_version('0.2.0', '20261123', tags), '0.2.1-dev.20261123.1')
        with self.assertRaisesRegex(ValueError, 'stable release'):
            nightly.nightly_version('0.3.0-alpha.1', '20261122', [])
        # Devices hold 23 characters of a version: the 23rd fits, a 24th is refused, never cut.
        self.assertEqual(nightly.nightly_version('10.10.10', '20261122', []), '10.10.11-dev.20261122.1')
        with self.assertRaisesRegex(ValueError, 'longer than the image identity holds .23 characters'):
            nightly.nightly_version('10.10.10', '20261122', ['v10.10.11-dev.20261122.9'])

    def test_the_stamp_commits_only_the_version_and_tags_it(self):
        version = self.stamp()
        stamp = self.git('rev-parse', 'HEAD')
        self.assertEqual(version, '0.2.1-dev.20261122.1')
        self.assertEqual(self.git('rev-parse', 'HEAD^'), self.main)
        self.assertEqual(self.git('diff', '--name-only', self.main, stamp).split(),
                         sorted(nightly.VERSION_LINES))
        self.assertEqual(release.committed_release(self.root, stamp), (version, 'v' + version))
        self.assertEqual(self.git('cat-file', '-t', 'v' + version), 'tag')
        self.assertEqual(self.git('rev-parse', f'v{version}^{{commit}}'), stamp)
        self.assertEqual(nightly.stamp_source(self.root, stamp), self.main)
        self.assertEqual(self.git('status', '--porcelain'), '')
        # Only the tag names it: main never moves.
        self.assertEqual(self.git('rev-parse', 'origin/main'), self.main)

    def test_only_a_change_the_image_is_built_from_gets_a_nightly(self):
        self.stamp()
        self.git('push', '-q', 'origin', 'refs/tags/v0.2.1-dev.20261122.1')
        for path in ('docs/RELEASING.md', 'tests/x_test.py', 'README.md', '.github/workflows/x.yml'):
            with self.subTest(path):
                self.change(path, f'docs: change {path}')
                self.assertIsNone(self.stamp())
        self.assertEqual(self.stamp(force=True), '0.2.1-dev.20261122.2')
        self.change('rust/core/src/x.rs', 'fix(core): change the image')
        self.assertEqual(self.stamp(), '0.2.1-dev.20261122.3')
        self.change('vendor/sdk.txt', 'build: change an input')
        self.assertEqual(self.stamp(), '0.2.1-dev.20261122.4')

    def test_no_nightly_once_main_has_moved_on(self):
        self.change('rust/a.rs', 'fix(core): one')
        self.change('rust/b.rs', 'fix(core): two')
        self.git('checkout', '-q', '--detach', 'HEAD^')
        self.assertIsNone(self.stamp(force=True))

    def stamp_of(self, source, version, extra=None):
        """A hand-made commit on `source` with `version` in the version files (and `extra` changed)."""
        self.git('checkout', '-q', '--detach', source)
        fixture_version(self.root, version)
        if extra:
            (self.root / extra).write_text('extra\n')
        self.git('add', '.')
        self.git('commit', '-qm', f'chore(release): nightly {version}')
        return self.git('rev-parse', 'HEAD')

    def test_the_source_of_anything_but_an_exact_stamp_is_none(self):
        self.assertEqual(nightly.stamp_source(self.root, self.stamp_of(self.main, '0.2.1-dev.20261122.1')), self.main)
        branch = self.stamp_of(self.main, '0.2.0', 'rust/branch.rs')
        cases = {'another change': self.stamp_of(self.main, '0.2.1-dev.20261122.1', 'rust/x.rs'),
                 'not the next release': self.stamp_of(self.main, '0.2.2-dev.20261122.1'),
                 'a stable version': self.stamp_of(self.main, '0.2.1'),
                 'off main': self.stamp_of(branch, '0.2.1-dev.20261122.1')}
        for name, commit in cases.items():
            with self.subTest(name):
                self.assertIsNone(nightly.stamp_source(self.root, commit))

    def test_nightly_tags_never_confuse_the_release_tools(self):
        self.stamp()
        self.change('rust/feature.rs', 'feat(reader): a feature')
        # The release gate's tag lookup and the tag tool read main's versions, never nightly tags.
        self.assertEqual(release.check_release_tagged(self.root), 'v0.2.0')
        self.assertEqual(release_tag.release_commit(self.root), self.main)
        # Commitizen's next release and its changelog skip the nightly tag off main.
        cz = [str(release.release_python()), '-I', '-B', '-m', 'commitizen', 'bump']
        run = lambda *extra: subprocess.run([*cz, *extra], cwd=self.root, env=self.environment, check=True,
                                            capture_output=True, text=True).stdout.strip()
        self.assertEqual(run('--get-next'), '0.3.0')
        run('--version-files-only', '--yes')
        changelog = (self.root / 'CHANGELOG.md').read_text()
        self.assertIn('v0.3.0', changelog)
        self.assertNotIn('-dev.', changelog)
        self.assertNotIn('nightly', changelog)
        # A nightly version never lands on a branch.
        fixture_version(self.root, '0.2.1-dev.20261122.1')
        with self.assertRaisesRegex(ValueError, 'never commit it'):
            release.gate(self.root)


# (tool, old, new, test): removing the guard must make its test fail.
CONTROLS = (
    ('nightly.py', '    taken = [n for', '    taken = [] and [n for', 'test_the_version_is_the_next_patch_numbered_per_day'),
    ('nightly.py', '    require(len(version) < NEWT_VERSION_BYTES,', '    require(True,',
     'test_the_version_is_the_next_patch_numbered_per_day'),
    ('nightly.py', "    require(found is not None, f'Nightlies", "    require(True, f'Nightlies",
     'test_the_version_is_the_next_patch_numbered_per_day'),
    ('nightly.py', '    if not force and since is not None and', '    if False and since is not None and',
     'test_only_a_change_the_image_is_built_from_gets_a_nightly'),
    ('nightly.py', "if not path.startswith(NOT_IMAGE) and", 'if', 'test_only_a_change_the_image_is_built_from_gets_a_nightly'),
    ('nightly.py', '    if live != head:', '    if False:', 'test_no_nightly_once_main_has_moved_on'),
    ('nightly.py', "sorted(changed) == sorted(VERSION_LINES) and same else None", 'True else None',
     'test_the_source_of_anything_but_an_exact_stamp_is_none'),
    ('nightly.py', "        nightly = re.fullmatch(NIGHTLY, version) and version.startswith(",
     "        nightly = True or version.startswith(", 'test_the_source_of_anything_but_an_exact_stamp_is_none'),
    ('nightly.py', "parents[0] not in git_output(root, 'rev-list', '--first-parent', DESTINATION).split():",
     'False:', 'test_the_source_of_anything_but_an_exact_stamp_is_none'),
    ('release_tools.py', '    if re.fullmatch(NIGHTLY, check_version(root)):', '    if check_version(root) and False:',
     'test_nightly_tags_never_confuse_the_release_tools'),
)


def negative_controls():
    for tool, old, new, test in CONTROLS:
        with tempfile.TemporaryDirectory(prefix='nightly-control-') as scratch:
            source = (args.tool_root / tool).read_text()
            if source.count(old) != 1:
                raise RuntimeError('Negative control anchor drift: ' + test)
            # A copied release_tools.py finds the release venv beside its own tools folder.
            tools = Path(scratch) / 'tools'
            tools.mkdir()
            (tools / tool).write_text(source.replace(old, new))
            (Path(scratch) / '.release-venv').symlink_to(release.release_python().parent.parent)
            shutil.copyfile(ROOT / 'requirements-release.txt', Path(scratch) / 'requirements-release.txt')
            result = subprocess.run([sys.executable, '-B', __file__, '--tool-root', tools, '--single', test],
                                    capture_output=True, text=True)
            if result.returncode == 0 or ('FAIL:' not in result.stderr and 'ERROR:' not in result.stderr):
                raise RuntimeError('Guard removal was not caught: ' + test + '\n' + result.stderr)
            print('PASS guard-removal control: ' + test, flush=True)


if __name__ == '__main__':
    loader = unittest.defaultTestLoader
    suite = (loader.loadTestsFromName(args.single, NightlyTests) if args.single
             else loader.loadTestsFromTestCase(NightlyTests))
    if not unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful():
        sys.exit(1)
    if args.negative_controls:
        negative_controls()
