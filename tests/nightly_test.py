#!/usr/bin/env python3
"""tools/nightly.py in disposable repositories with a local bare origin: the
fixed-width UTC nightly version, GitHub text-order agreement with the shipped
Rust SemVer comparator, same-minute collision wait/recheck/refusal, the stamp
commit and its tag, which pushes get none (judged from published releases), the
exact stamp release_publish.py trusts, and that nightly tags never confuse the
release tools or Commitizen. --negative-controls removes each guard in a copy of
its tool and requires its test to fail. Never tags or pushes the checkout."""
import argparse
from contextlib import redirect_stderr
from datetime import datetime, timedelta, timezone
import io
import json
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
from rust_app_archive import verify_toolchain

START = datetime(2026, 11, 22, 23, 50, tzinfo=timezone.utc)
REPOSITORY = 'VargasDevelopment/newt'

RUST_SEMVER_HARNESS = r"""
#[path = @MANIFEST@]
pub mod update_manifest;
use std::ffi::CString;
use update_manifest::{semver2_compare, semver2_is_nightly, semver2_valid};

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("check") => for version in args {
            let v = CString::new(version).unwrap();
            println!("{} {}", u8::from(semver2_valid(Some(&v))), u8::from(semver2_is_nightly(Some(&v))));
        },
        Some("compare") => {
            let values: Vec<CString> = args.map(|s| CString::new(s).unwrap()).collect();
            for pair in values.chunks_exact(2) {
                println!("{}", semver2_compare(Some(&pair[0]), Some(&pair[1])));
            }
        },
        _ => std::process::exit(2),
    }
}
"""


def gh_answer(status, release=None):
    """gh api --include's whole answer as gh gives it: status line, headers, blank line, body; exit 1
    unless 2xx. Status None is no answer at all (the network failed)."""
    if status is None:
        return subprocess.CompletedProcess([], 1, b'', b'error connecting to api.github.com')
    body = json.dumps(release if release is not None else {'message': 'Not Found', 'status': str(status)})
    head = f'HTTP/2.0 {status} {"OK" if status == 200 else "Error"}\nContent-Type: application/json\n'
    return subprocess.CompletedProcess([], 0 if status == 200 else 1, (head + '\n' + body).encode(),
                                       b'' if status == 200 else f'gh: Error (HTTP {status})'.encode())


FILES = ('.cz.toml', 'requirements-release.txt', release.VERSION_HEADER, 'tools/changelog.md.j2', 'tools/cz_newt/cz_newt.py')


class NightlyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.semver_scratch = tempfile.TemporaryDirectory(prefix='nightly-semver-')
        cls.addClassCleanup(cls.semver_scratch.cleanup)
        rustc = verify_toolchain(ROOT) / 'rustc'
        harness = Path(cls.semver_scratch.name) / 'semver.rs'
        harness.write_text(RUST_SEMVER_HARNESS.replace('@MANIFEST@', json.dumps(str(ROOT / 'rust/core/src/update_manifest.rs'))))
        cls.semver_bin = Path(cls.semver_scratch.name) / 'semver'
        subprocess.run([str(rustc), '--edition=2024', '-Dwarnings', '--crate-name', 'nightly_semver',
                        str(harness), '-o', str(cls.semver_bin)], check=True)

    def semver_run(self, *arguments):
        return subprocess.run([str(self.semver_bin), *arguments], check=True,
                              capture_output=True, text=True).stdout.splitlines()

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
        # nightly.stamp commits in this repo with the process environment; CI runners have no identity.
        self.git('config', 'user.name', 'Fixture')
        self.git('config', 'user.email', 'fixture@example.invalid')
        self.git('remote', 'add', 'origin', str(self.origin))
        self.git('add', '.')
        self.git('commit', '-qm', 'chore(release): 0.1.0 -> 0.2.0')
        self.git('tag', '-a', 'v0.2.0', '-m', 'Newt firmware 0.2.0')
        self.main = self.push()
        # GitHub's releases by tag: {tag: draft?}. Its tag endpoint shows a draft as 404.
        self.releases = {}
        self.clock = START

    def published(self, tag):
        return nightly.published(REPOSITORY, tag, lambda command: self.answer(command[-1].rsplit('/', 1)[1]))

    def answer(self, tag):
        if self.releases.get(tag) is False:
            return gh_answer(200, {'id': 1, 'tag_name': tag, 'draft': False, 'prerelease': True})
        return gh_answer(404)

    def release(self, version, draft=False):
        """Pushes the nightly's tag and gives it a release (only a draft, if `draft`)."""
        self.git('push', '-q', 'origin', f'refs/tags/v{version}')
        self.releases['v' + version] = draft

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

    def next_minute(self):
        self.clock += timedelta(minutes=1)
        return self.clock

    def stamp(self, force=False, drafts=(), now=None, sleep=None):
        moment = self.next_minute() if now is None else now
        with redirect_stderr(io.StringIO()):
            return nightly.stamp(self.published, self.root, moment, force, drafts, sleep or (lambda _: None))

    def test_the_version_is_the_next_patch_stamped_in_utc(self):
        self.assertEqual(nightly.nightly_version('0.2.0', '2611222359', []), '0.2.1-dev.2611222359')
        # Accepts a timezone-aware datetime and converts to UTC; refuses naive or out-of-range years.
        offset = timezone(timedelta(hours=-5))
        self.assertEqual(nightly.nightly_version('0.5.0', datetime(2026, 11, 25, 10, 0, tzinfo=offset), []),
                         '0.5.1-dev.2611251500')
        with self.assertRaisesRegex(ValueError, 'timezone-aware'):
            nightly.format_stamp(datetime(2026, 11, 25, 15, 0))
        for year in (2020, 2100):
            with self.subTest(year=year), self.assertRaisesRegex(ValueError, 'outside the nightly range'):
                nightly.format_stamp(datetime(year, 1, 1, 0, 0, tzinfo=timezone.utc))
        # Legacy tags and other cores do not block a newer fixed-width stamp; collisions and older stamps refuse.
        tags = ['v0.5.1-dev.20261125.9', 'v0.5.1-dev.20261125.17', 'v0.5.1-dev.2611251500',
                'v0.6.1-dev.2611251559', 'v0.5.1', 'v0.5.1-dev.bad']
        self.assertEqual(nightly.nightly_version('0.5.0', '2611251501', tags), '0.5.1-dev.2611251501')
        for bad in ('2611251500', '2611251459'):
            with self.subTest(bad=bad), self.assertRaisesRegex(ValueError, 'collides with or precedes'):
                nightly.nightly_version('0.5.0', bad, tags)
        for bad in ('202611251500', '0611251500', '2613251500', '2611252400', '2611251560', '261125150'):
            with self.subTest(bad=bad), self.assertRaisesRegex(ValueError, 'YYMMDDHHMM'):
                nightly.nightly_version('0.5.0', bad, [])
        # A pre-release's core decides: the patch after it.
        self.assertEqual(nightly.nightly_version('0.2.1-rc.1', '2611222359', []), '0.2.2-dev.2611222359')
        with self.assertRaisesRegex(ValueError, 'not a release version'):
            nightly.nightly_version('0.2', '2611222359', [])
        # Devices hold 23 characters of a version: the 23rd fits, a 24th is refused, never cut.
        self.assertEqual(nightly.nightly_version('10.10.10', '2611222359', []), '10.10.11-dev.2611222359')
        with self.assertRaisesRegex(ValueError, 'longer than the image identity holds .23 characters'):
            nightly.nightly_version('100.10.10', '2611222359', [])

    def test_github_text_order_puts_newest_nightly_first_and_matches_firmware_semver(self):
        # Historical tags: .9 sorts above .10..17 in GitHub's text order even though SemVer orders .10 > .9.
        legacy = [f'0.5.1-dev.20261125.{n}' for n in range(1, 18)]
        self.assertGreater('v' + legacy[8], 'v' + legacy[9])
        self.assertEqual(self.semver_run('compare', legacy[9], legacy[8]), ['1'])
        # Generate 11 consecutive minutes (past the .9 -> .10 boundary) plus hour/day/month/year/2042 rollovers.
        moments = [datetime(2026, 11, 25, 15, m, tzinfo=timezone.utc) for m in range(11)] + [
            datetime(2026, 11, 25, 23, 59, tzinfo=timezone.utc),
            datetime(2026, 11, 26, 0, 0, tzinfo=timezone.utc),
            datetime(2026, 11, 30, 23, 59, tzinfo=timezone.utc),
            datetime(2026, 12, 1, 0, 0, tzinfo=timezone.utc),
            datetime(2026, 12, 31, 23, 59, tzinfo=timezone.utc),
            datetime(2027, 1, 1, 0, 0, tzinfo=timezone.utc),
            datetime(2042, 12, 31, 23, 59, tzinfo=timezone.utc),
        ]
        all_tags = ['v0.5.0', *('v' + v for v in legacy)]
        generated = []
        for moment in moments:
            version = nightly.nightly_version('0.5.0', moment, all_tags)
            generated.append(version)
            all_tags.append('v' + version)
            # At every step, GitHub's descending text sort returns the newest generated nightly first.
            self.assertEqual(sorted(all_tags, reverse=True)[0], 'v' + version)
        # Every generated nightly is valid SemVer, classified as nightly by firmware, and fits u32 through 2042.
        self.assertEqual(self.semver_run('check', *generated), ['1 1'] * len(generated))
        # The first fixed-width nightly outranks v0.5.0 and legacy .9/.17 under the shipped Rust comparator,
        # and every generated step increases under both text order and Rust SemVer while staying below 0.5.1.
        pairs = [generated[0], '0.5.0', generated[0], '0.5.1-dev.20261125.9', generated[0], '0.5.1-dev.20261125.17',
                 '0.5.1', generated[-1]]
        for older, newer in zip(generated, generated[1:]):
            self.assertGreater('v' + newer, 'v' + older)
            pairs.extend((newer, older))
        self.assertEqual(self.semver_run('compare', *pairs), ['1'] * (len(pairs) // 2))

    def test_the_stamp_commits_only_the_version_and_tags_it(self):
        version = self.stamp(now=datetime(2026, 11, 22, 23, 59, tzinfo=timezone.utc))
        stamp = self.git('rev-parse', 'HEAD')
        self.assertEqual(version, '0.2.1-dev.2611222359')
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
        self.release(self.stamp())
        for path in ('docs/RELEASING.md', 'tests/x_test.py', 'README.md', '.github/workflows/x.yml'):
            with self.subTest(path):
                self.change(path, f'docs: change {path}')
                self.assertIsNone(self.stamp())
        self.assertEqual(self.stamp(force=True), '0.2.1-dev.2611222356')
        self.change('rust/core/src/x.rs', 'fix(core): change the image')
        self.release(self.stamp())
        self.change('vendor/sdk.txt', 'build: change an input')
        self.release(self.stamp())
        # The firmware toolchain locks live under docs/ but choose what builds the image.
        self.change('docs/toolchain-linux-lock.json', 'build(toolchain): change a lock')
        self.assertEqual(self.stamp(), '0.2.1-dev.2611222359')

    def test_only_a_published_nightly_counts_as_the_last_one(self):
        # A tag whose publish failed, then one with only a draft: neither suppresses the next nightly,
        # and each run takes the next UTC minute. Each run starts, as CI's does, at main's tip.
        def run():
            self.git('checkout', '-q', '--detach', 'origin/main')
            return self.stamp()
        self.git('push', '-q', 'origin', f'refs/tags/v{run()}')
        self.release(run(), draft=True)
        third = run()
        self.assertEqual(third, '0.2.1-dev.2611222353')
        self.release(third)
        self.assertIsNone(run())

    def test_legacy_published_nightly_still_counts_as_last_source(self):
        legacy = self.stamp_of(self.main, '0.2.1-dev.20261122.17')
        self.git('tag', '-a', 'v0.2.1-dev.20261122.17', '-m', 'Legacy nightly', legacy)
        self.release('0.2.1-dev.20261122.17')
        self.assertIsNone(nightly.stamp_source(self.root, legacy))
        self.assertEqual(nightly.stamp_source(self.root, legacy, legacy=True), self.main)
        self.change('docs/RELEASING.md', 'docs: no image change')
        self.assertIsNone(self.stamp())
        self.change('rust/a.rs', 'fix(core): firmware change after legacy')
        self.assertEqual(self.stamp(now=datetime(2026, 11, 22, 23, 59, tzinfo=timezone.utc)), '0.2.1-dev.2611222359')

    def test_same_minute_collision_waits_once_rechecks_and_refuses_if_still_taken(self):
        # A declined draft (or remote tag) in the same UTC minute triggers one bounded wait until the next minute.
        self.change('rust/a.rs', 'fix(core): one')
        times = iter([datetime(2026, 11, 22, 23, 59, 40, tzinfo=timezone.utc),
                      datetime(2026, 11, 23, 0, 0, 1, tzinfo=timezone.utc)])
        waits, draft_reads = [], [0]

        def read_drafts():
            draft_reads[0] += 1
            return ['v0.2.1-dev.2611222359']

        version = self.stamp(now=lambda: next(times), drafts=read_drafts, sleep=waits.append)
        self.assertEqual(version, '0.2.1-dev.2611230000')
        self.assertEqual((len(waits), draft_reads[0]), (1, 2))
        self.assertAlmostEqual(waits[0], 21.0)
        # If origin/main moves on during the wait, nothing is committed or tagged.
        self.git('checkout', '-q', '--detach', 'origin/main')
        before_head = self.git('rev-parse', 'HEAD')
        times = iter([datetime(2026, 11, 23, 0, 0, 15, tzinfo=timezone.utc),
                      datetime(2026, 11, 23, 0, 1, 1, tzinfo=timezone.utc)])
        waits.clear()

        def move_main(delay):
            waits.append(delay)
            # Push a newer commit to origin/main without disturbing this checkout's HEAD.
            other = self.root.parent / 'other'
            subprocess.run(['git', 'clone', '-q', str(self.origin), str(other)], check=True)
            subprocess.run(['git', '-C', str(other), 'config', 'user.name', 'Other'], check=True)
            subprocess.run(['git', '-C', str(other), 'config', 'user.email', 'other@example.invalid'], check=True)
            (other / 'rust/moved.rs').parent.mkdir(parents=True, exist_ok=True)
            (other / 'rust/moved.rs').write_text('moved\n')
            subprocess.run(['git', '-C', str(other), 'add', '.'], check=True)
            subprocess.run(['git', '-C', str(other), 'commit', '-qm', 'fix(core): moved'], env=self.environment, check=True)
            subprocess.run(['git', '-C', str(other), 'push', '-q', 'origin', 'HEAD:refs/heads/main'], check=True)

        self.assertIsNone(self.stamp(now=lambda: next(times), sleep=move_main))
        self.assertEqual((len(waits), self.git('rev-parse', 'HEAD')), (1, before_head))
        # Reset checkout to the new origin/main; if the next minute is also taken (via re-read drafts or remote tag), refuse.
        self.git('fetch', '-q', 'origin')
        self.git('checkout', '-q', '--detach', 'origin/main')
        self.git('tag', '-d', 'v0.2.1-dev.2611230000')
        # Push a remote-only tag on origin for 00:05 and verify both initial remote read and post-wait refusal.
        self.git('push', '-q', 'origin', f'{before_head}:refs/tags/v0.2.1-dev.2611230005')
        times = iter([datetime(2026, 11, 23, 0, 5, 50, tzinfo=timezone.utc),
                      datetime(2026, 11, 23, 0, 6, 1, tzinfo=timezone.utc)])
        draft_seq = iter([[], ['v0.2.1-dev.2611230006']])
        waits.clear()
        with self.assertRaisesRegex(ValueError, 'collides with or precedes'):
            self.stamp(now=lambda: next(times), drafts=lambda: next(draft_seq), sleep=waits.append)
        self.assertEqual(len(waits), 1)
        # Clock behind an existing tag refuses immediately without waiting.
        waits.clear()
        with self.assertRaisesRegex(ValueError, 'collides with or precedes'):
            self.stamp(now=datetime(2026, 11, 23, 0, 4, 30, tzinfo=timezone.utc), sleep=waits.append)
        self.assertEqual(waits, [])

    def test_draft_tags_are_read_and_a_failed_read_raises(self):
        seen = []

        def read(returncode, stdout=b''):
            def answer(command):
                seen.append(command)
                return subprocess.CompletedProcess(command, returncode, stdout, b'gh: Error (HTTP 502)')
            return nightly.draft_tags(REPOSITORY, answer)
        self.assertEqual(read(0, b'v0.2.1-dev.2611222358\nv0.2.1-dev.2611222359\n'),
                         ['v0.2.1-dev.2611222358', 'v0.2.1-dev.2611222359'])
        self.assertEqual(seen[0], ['gh', 'api', '--paginate', f'repos/{REPOSITORY}/releases', '--jq',
                                   '.[] | select(.draft) | .tag_name'])
        self.assertEqual(read(0), [])
        with self.assertRaisesRegex(ValueError, 'failed'):
            read(1)

    def test_no_nightly_once_main_has_moved_on(self):
        self.change('rust/a.rs', 'fix(core): one')
        self.change('rust/b.rs', 'fix(core): two')
        self.git('checkout', '-q', '--detach', 'HEAD^')
        self.assertIsNone(self.stamp(force=True))
        # Nor is its tag pushed if main moves on after the stamp: the check comes right before the push.
        self.git('checkout', '-q', '--detach', 'origin/main')
        version, source = self.stamp(), self.git('rev-parse', 'origin/main')
        self.change('rust/c.rs', 'fix(core): three')
        self.assertFalse(nightly.push_tag(self.root, 'v' + version, source))
        self.assertEqual(self.git('ls-remote', '--tags', 'origin', '*-dev.*'), '')
        self.assertTrue(nightly.push_tag(self.root, 'v' + version, self.git('rev-parse', 'origin/main')))
        self.assertIn(f'refs/tags/v{version}', self.git('ls-remote', '--tags', 'origin'))

    def test_a_release_by_tag_is_read_by_its_http_status(self):
        tag, seen = 'v0.2.1-dev.2611222359', []

        def read(status, release=None):
            def answer(command):
                seen.append(command)
                return gh_answer(status, release)
            return nightly.published(REPOSITORY, tag, answer)
        published = {'id': 7, 'tag_name': tag, 'draft': False, 'prerelease': True}
        self.assertEqual(read(200, published), published)
        self.assertEqual(seen[0], ['gh', 'api', '--include', f'repos/{REPOSITORY}/releases/tags/{tag}'])
        # 404 is "none published": no release, or only a draft, which this endpoint never shows.
        self.assertIsNone(read(404))
        self.assertIsNone(read(200, {**published, 'draft': True}))
        # Anything else, or no answer, never passes for "not published".
        for status in (500, 403, None):
            with self.subTest(status), self.assertRaisesRegex(ValueError, f'HTTP {status}'):
                read(status)
        with self.assertRaisesRegex(ValueError, 'answered'):
            read(200, {**published, 'tag_name': 'v0.2.0'})

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
        self.assertEqual(nightly.stamp_source(self.root, self.stamp_of(self.main, '0.2.1-dev.2611222359')), self.main)
        branch = self.stamp_of(self.main, '0.2.0', 'rust/branch.rs')
        cases = {'another change': self.stamp_of(self.main, '0.2.1-dev.2611222359', 'rust/x.rs'),
                 'not the next release': self.stamp_of(self.main, '0.2.2-dev.2611222359'),
                 'legacy format': self.stamp_of(self.main, '0.2.1-dev.20261122.1'),
                 'a stable version': self.stamp_of(self.main, '0.2.1'),
                 'off main': self.stamp_of(branch, '0.2.1-dev.2611222359')}
        for name, commit in cases.items():
            with self.subTest(name):
                self.assertIsNone(nightly.stamp_source(self.root, commit))

    def test_nightly_tags_never_confuse_the_release_tools(self):
        self.stamp()
        self.change('rust/feature.rs', 'feat(reader): a feature')
        # The release gate's tag lookup and the tag tool read main's versions, never nightly tags.
        self.assertEqual(release.check_release_tagged(self.root), ['v0.2.0'])
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
        # Neither a fixed-width nor a legacy nightly version may land on a branch.
        for version in ('0.2.1-dev.2611222359', '0.2.1-dev.20261122.1'):
            with self.subTest(version=version):
                fixture_version(self.root, version)
                with self.assertRaisesRegex(ValueError, 'never commit it'):
                    release.gate(self.root)


# (tool, old, new, test): removing the guard must make its test fail.
CONTROLS = (
    ('nightly.py', "    version = f'{base}-dev.{stamp_id}'",
     "    version = f'{base}-dev.20{stamp_id[:6]}.{int(stamp_id[8:]) + 1}'",
     'test_github_text_order_puts_newest_nightly_first_and_matches_firmware_semver'),
    ('nightly.py', '    require(newest is None or int(stamp_id) > newest,', '    require(True,',
     'test_the_version_is_the_next_patch_stamped_in_utc'),
    ('nightly.py', '    require(len(version) < NEWT_VERSION_BYTES,', '    require(True,',
     'test_the_version_is_the_next_patch_stamped_in_utc'),
    ('nightly.py', "([0-9]+)(?:-[0-9A-Za-z.-]+)?', release)", "([0-9]+)', release)",
     'test_the_version_is_the_next_patch_stamped_in_utc'),
    ('nightly.py', "    require(found is not None, f'{release} is not", "    require(True, f'{release} is not",
     'test_the_version_is_the_next_patch_stamped_in_utc'),
    ('nightly.py', '    if since is not None and', '    if False and since is not None and',
     'test_only_a_change_the_image_is_built_from_gets_a_nightly'),
    ('nightly.py', 'if IMAGE_DOCS.fullmatch(path) or not (', 'if True or not (',
     'test_only_a_change_the_image_is_built_from_gets_a_nightly'),
    ('nightly.py', 'if IMAGE_DOCS.fullmatch(path) or not (', 'if not (',
     'test_only_a_change_the_image_is_built_from_gets_a_nightly'),
    ('nightly.py', '        if published(tag) is not None:', '        if True:',
     'test_only_a_published_nightly_counts_as_the_last_one'),
    ('nightly.py', '    if newest is not None and int(stamp_id) == newest:', '    if False:',
     'test_same_minute_collision_waits_once_rechecks_and_refuses_if_still_taken'),
    ('nightly.py', '    taken = [*tags, *origin_nightly_tags(root), *read_drafts()]', '    taken = [*tags]',
     'test_same_minute_collision_waits_once_rechecks_and_refuses_if_still_taken'),
    ('nightly.py', '        taken = [*git_output(root, \'tag\', \'--list\', \'v*-dev.*\').split(), *origin_nightly_tags(root), *read_drafts()]',
     '        taken = [*tags]', 'test_same_minute_collision_waits_once_rechecks_and_refuses_if_still_taken'),
    ('nightly.py', '        if live != head:\n            print(f\'No nightly: main has moved on to {live}; its own run builds the next one\', file=sys.stderr)\n            return None',
     '        if False:\n            return None',
     'test_same_minute_collision_waits_once_rechecks_and_refuses_if_still_taken'),
    ('nightly.py', "    require(reply.returncode == 0, f'{shlex.join(command)} failed: ",
     "    require(True, f'{shlex.join(command)} failed: ", 'test_draft_tags_are_read_and_a_failed_read_raises'),
    ('nightly.py', '    if live != head:\n        print(f\'No nightly:', '    if False:\n        print(f\'No nightly:',
     'test_no_nightly_once_main_has_moved_on'),
    ('nightly.py', "    if git_output(root, 'ls-remote', 'origin', 'refs/heads/main').split('\\t')[0] != source:",
     '    if False:', 'test_no_nightly_once_main_has_moved_on'),
    ('nightly.py', '    if status == 404:', '    if False:', 'test_a_release_by_tag_is_read_by_its_http_status'),
    ('nightly.py', '    require(status == 200 and reply.returncode == 0,', '    require(True,',
     'test_a_release_by_tag_is_read_by_its_http_status'),
    ('nightly.py', "    return None if release.get('draft') is not False else release", '    return release',
     'test_a_release_by_tag_is_read_by_its_http_status'),
    ('nightly.py', "    require(release.get('tag_name') == tag,", '    require(True,',
     'test_a_release_by_tag_is_read_by_its_http_status'),
    ('nightly.py', "sorted(changed) == sorted(VERSION_LINES) and same else None", 'True else None',
     'test_the_source_of_anything_but_an_exact_stamp_is_none'),
    ('nightly.py', "        nightly = re.fullmatch(pattern, version) and version.startswith(",
     "        nightly = True or version.startswith(", 'test_the_source_of_anything_but_an_exact_stamp_is_none'),
    ('nightly.py', "parents[0] not in git_output(root, 'rev-list', '--first-parent', DESTINATION).split():",
     'False:', 'test_the_source_of_anything_but_an_exact_stamp_is_none'),
    ('release_tools.py', '    if re.fullmatch(ANY_NIGHTLY, check_version(root)):', '    if check_version(root) and False:',
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
            (tools / 'cz_newt').mkdir(parents=True)
            shutil.copyfile(ROOT / release.PLUGIN / 'cz_newt.py', tools / 'cz_newt/cz_newt.py')
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
