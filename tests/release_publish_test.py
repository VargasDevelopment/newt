#!/usr/bin/env python3
"""tools/release_publish.py with fake git and gh (no network): each refusal,
the deterministic notes, and the draft, verify, publish flow with its lost or
failed publish answers and resumed drafts. --negative-controls removes each
guard in a copy of the tool and requires its test to fail."""
import argparse
from contextlib import redirect_stderr, redirect_stdout
import io
import os
from pathlib import Path
import re
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
import release_publish
from release_publish import (ASSETS, DRAFT, HOP_REPOSITORY, NOTICES, SOURCE_REPOSITORY, UPDATE_SOURCE, WHATS_NEW,
                             plan, publish, update_repository)
from bench_candidate import sha
from update_publish_test import BUILD, REVISION, VERSION, app, identity, newt_image, receipt
from x3_slots import NEWT_MARK
import update_publish

TAG = 'v' + VERSION
# A nightly: CI's stamp REVISION of the main commit MAIN, whose own release is 0.2.0.
MAIN, NIGHTLY = 'e' * 40, '0.2.1-dev.20261122.1'
HEADER = '''inline constexpr WhatsNewUpdate kWhatsNew[] = {
    {"Oct 30, 2026",
     {"Updates arrive over \\"Wi-Fi\\".",
      "Reading is faster."}},
    {"Oct 28, 2026",
     {"Update in Settings installs Newt from the SD card."}},
};'''
SHOWN = '''## What's new\r
\r
- Update in Settings installs Newt from the SD card.\r
\r
## This build\r
\r
- Reading is faster.\r
'''
NOTES = f'''## What's new

- Updates arrive over "Wi-Fi".
- Reading is faster.

## This build

- Version: {VERSION}
- Source commit: {REVISION}
- Build ID: {BUILD}
'''
NOTICE_TEXT = ('Newt firmware: third-party notices\n\nCopyright upstream <upstream@example.org>\n'
               f'To ask, open an issue at\nhttps://github.com/{HOP_REPOSITORY}/issues, naming the release\n')


def moved(repository):
    """The shipped URL and notices' offer move together."""
    return {(REVISION, UPDATE_SOURCE): online(repository),
            (REVISION, NOTICES): NOTICE_TEXT.replace(HOP_REPOSITORY, repository)}


def online(repository):
    """The shipped Rust update URL, not the retired C++ repository constant."""
    return f'pub const LATEST_RELEASE_URL: &CStr = c"https://api.github.com/repos/{repository}/releases/latest";\n'


class FakeGit:
    """A tagged release merge on origin/main, with its files and the previous release's."""

    def __init__(self, **changes):
        self.tags = {TAG: REVISION}
        self.origin = {TAG: REVISION}
        self.mainline = {REVISION}
        self.files = {(REVISION, NOTICES): NOTICE_TEXT, (REVISION, WHATS_NEW): HEADER,
                      (REVISION, UPDATE_SOURCE): online(HOP_REPOSITORY)}
        self.versions, self.stamps = {}, {}  # each commit's own version (VERSION otherwise); stamp -> main
        self.__dict__.update(changes)

    def tag_commit(self, tag):
        return self.tags.get(tag)

    def origin_tag_commit(self, tag):
        return self.origin.get(tag)

    def on_mainline(self, commit):
        return commit in self.mainline

    def version(self, commit):
        return self.versions.get(commit, VERSION)

    def stamp_source(self, commit):
        return self.stamps.get(commit)

    def subject(self, commit):
        return 'fix(reader): keep the page'

    def show(self, commit, path):
        return self.files.get((commit, path))


def stamped_git(**changes):
    """CI's nightly stamp of MAIN, tagged here and on origin, its firmware reading the source repository."""
    tag = 'v' + NIGHTLY
    return FakeGit(**{'tags': {tag: REVISION}, 'origin': {tag: REVISION}, 'mainline': {MAIN}, 'stamps': {REVISION: MAIN},
                      'versions': {REVISION: NIGHTLY, MAIN: '0.2.0'},
                      'files': {**FakeGit().files, **moved(SOURCE_REPOSITORY)}, **changes})



class FakeGitHub:
    """One repository as GitHub keeps it. `answer` is what the publish PATCH does: 'ok', 'lost' (GitHub
    publishes, the answer never arrives) or 'failed' (nothing changes). It has no delete."""

    def __init__(self, answer='ok', corrupt=False, tags=None, previous=SHOWN):
        self.answer, self.corrupt, self.tags, self.previous = answer, corrupt, dict(tags or {}), previous
        self.releases, self.blobs, self.created, self.patched = {}, {}, [], []

    def add(self, tag, files, draft=True, name=None, body=None, prerelease=False):
        """A release holding `files` ({name: bytes}); its asset records carry no digest, so reads download."""
        release_id = 100 + len(self.releases)
        assets = []
        for asset, data in files.items():
            self.blobs[len(self.blobs) + 1] = data
            assets.append({'id': len(self.blobs), 'name': asset, 'size': len(data), 'state': 'uploaded'})
        self.releases[release_id] = {'id': release_id, 'tag_name': tag, 'draft': draft, 'prerelease': prerelease,
                                     'name': name, 'body': body, 'assets': assets}
        return release_id

    def tag_commit(self, tag):
        return self.tags.get(tag)

    def release_for(self, tag):
        return next((dict(release) for release in self.releases.values() if release['tag_name'] == tag), None)

    def release(self, release_id):
        return dict(self.releases[release_id])

    def download(self, asset_id):
        return self.blobs[asset_id]

    def create_draft(self, command):
        self.created.append(command)
        files = {Path(path).name: Path(path).read_bytes() for path in command[command.index('--draft') + 1:]}
        if self.corrupt:
            files['newt.bin'] = files['newt.bin'][:-1] + b'\xff'
        self.add(command[3], files, name=command[command.index('--title') + 1],
                 body=Path(command[command.index('--notes-file') + 1]).read_text(), prerelease='--prerelease' in command)

    def publish(self, release_id, latest=True):
        self.patched.append(release_id)
        self.made_latest = latest
        if self.answer != 'failed':
            self.releases[release_id]['draft'] = False
            self.tags.setdefault(self.releases[release_id]['tag_name'], REVISION)
        if self.answer != 'ok':
            raise subprocess.CalledProcessError(1, 'gh api -X PATCH')

    def all_releases(self):
        """`previous` is the notes of an older published release this fake holds no assets for."""
        older = [{'tag_name': 'v0.2.0-alpha.0', 'draft': False, 'body': self.previous}] if self.previous else []
        return older + list(self.releases.values())

    def latest(self):
        published = [release for release in self.releases.values()
                     if not release['draft'] and (not release['prerelease'] or self.answer == 'latest')]
        return dict(published[-1]) if published else {}

    def newest(self):
        published = [release for release in self.releases.values() if not release['draft']]
        return dict(published[-1]) if published else {}


IMAGE = newt_image()
MANIFEST = update_publish.newt_manifest(IMAGE, receipt(IMAGE))


def bench(directory, image=IMAGE, **changes):
    """The Lead's bench install receipt for `image`'s exact pair."""
    path = Path(directory) / 'bench.json'
    record = {'version': VERSION, 'build': BUILD, 'newt_bin_sha256': sha(image),
              'newt_manifest_sha256': sha(update_publish.newt_manifest(image, receipt(image))), 'boot': 'VALID', 'home': True,
              'checked_at': '2026-10-30', 'installed_by': 'card', **changes}
    path.write_text(release_publish.json.dumps(record))
    return path


def pair(image=IMAGE, repository=HOP_REPOSITORY):
    """The three assets of `image`'s exact release, its firmware reading `repository`."""
    return {'newt.bin': image, 'newt.manifest': update_publish.newt_manifest(image, receipt(image)),
            'NOTICES.txt': NOTICE_TEXT.replace(HOP_REPOSITORY, repository).encode()}


def public_source():
    """The source repository once its firmware reads it: CI's tag on the release merge."""
    return FakeGit(files={**FakeGit().files, **moved(SOURCE_REPOSITORY)}), FakeGitHub(tags={TAG: REVISION})


def carrying(extra):
    """A valid Newt image whose bytes also hold `extra`."""
    return app('arduino-lib-builder', '8cabf2c', NEWT_MARK + identity(VERSION, BUILD) + BUILD.encode() + b'\0' + extra)


class ReleasePublishTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='release-publish-')
        self.addCleanup(self.scratch.cleanup)
        self.dir = Path(self.scratch.name)

    def plan(self, image=IMAGE, git=None, github=None, bench_receipt=True, prerelease=False, hop=False, public=None,
             **changes):
        bench_path = bench(self.dir, image) if bench_receipt is True else bench_receipt
        return plan(image, receipt(image, **changes), bench_path, prerelease, git or FakeGit(), github or FakeGitHub(),
                    hop=hop, public=public or FakeGitHub())

    def refused(self, message, **options):
        refusals = self.plan(**options).refusals
        self.assertTrue(any(message in refusal for refusal in refusals), refusals)

    def test_clean_release_passes_with_exact_notes(self):
        release = self.plan()
        self.assertEqual(release.refusals, [])
        size = f'- newt.bin: {len(IMAGE):,} bytes, SHA-256 {sha(IMAGE)}\n'
        self.assertEqual(release.notes, NOTES + size)
        self.assertEqual(self.plan().notes, release.notes)

    def test_notes_list_what_this_repository_has_not_published(self):
        # A first release here lists every current note, even if git history carried them already.
        first = self.plan(github=FakeGitHub(previous=None)).notes
        shown = '- Update in Settings installs Newt from the SD card.\n'
        self.assertIn(shown, first)
        self.assertNotIn(shown, self.plan().notes)
        # A line under "This build" was never a What's new line.
        self.assertIn('- Reading is faster.\n', self.plan().notes)
        # Neither a private draft nor this release's own published notes count as shown.
        github = FakeGitHub()
        github.add('v0.2.0-alpha.1', {}, body=first)
        github.add(TAG, {}, draft=False, body=first)
        self.assertEqual(self.plan(github=github).notes, self.plan().notes)

    def test_cpp_repository_cannot_override_the_shipped_rust_url(self):
        git = FakeGit()
        git.files[REVISION, 'include/newt/update_online.h'] = (
            'inline constexpr char kUpdateRepository[] = "' + SOURCE_REPOSITORY + '";\n')
        self.assertEqual(self.plan(git=git).repository, HOP_REPOSITORY)
        self.assertEqual(UPDATE_SOURCE, 'rust/core/src/update_online.rs')
        del git.files[REVISION, UPDATE_SOURCE]
        with self.assertRaisesRegex(ValueError, 'no single LATEST_RELEASE_URL'):
            self.plan(git=git)

    def test_refuses_untagged_build(self):
        self.refused(f'Tag {TAG} here names None', git=FakeGit(tags={}))

    def test_refuses_mismatched_tag(self):
        self.refused(f'Tag {TAG} here names {"f" * 40}', git=FakeGit(tags={TAG: 'f' * 40}))

    def test_refuses_dirty_or_mismatched_build(self):
        for name, image, changes, message in (('dirty', IMAGE, {'dirty': True}, 'dirty'),
                                              ('image', IMAGE, {'binary_sha256': '0' * 64}, 'differs')):
            with self.subTest(name), self.assertRaisesRegex(ValueError, message):
                self.plan(image, **changes)

    def test_refuses_an_existing_tag_or_published_release(self):
        self.refused(f'already has tag {TAG}', github=FakeGitHub(tags={TAG: REVISION}))
        github = FakeGitHub()
        github.add(TAG, pair(), draft=False)
        self.refused(f'already published {TAG}', github=github)

    def test_the_source_repository_needs_its_ci_tag_on_the_build_commit(self):
        # After launch CI tags the release merge in the repository it is published to.
        git, github = public_source()
        self.assertEqual(self.plan(git=git, github=github).refusals, [])
        for tags in ({}, {TAG: 'f' * 40}):
            with self.subTest(tags=tags):
                self.refused(f"{SOURCE_REPOSITORY}'s tag {TAG} names", git=git, github=FakeGitHub(tags=tags))
        github.add(TAG, pair(repository=SOURCE_REPOSITORY), draft=False)
        self.refused(f'already published {TAG}', git=git, github=github)

    def test_reads_ci_annotated_tag_as_its_commit(self):
        refs = f'repos/{SOURCE_REPOSITORY}/git/matching-refs/tags/'
        answers = {refs + TAG: [{'ref': f'refs/tags/{TAG}.1', 'object': {'type': 'commit', 'sha': 'e' * 40}},
                                {'ref': f'refs/tags/{TAG}', 'object': {'type': 'tag', 'sha': 'a' * 40}}],
                   f'repos/{SOURCE_REPOSITORY}/git/tags/{"a" * 40}': {'object': {'type': 'commit', 'sha': REVISION}},
                   refs + 'v9.9.9': []}
        github = release_publish.GitHub(SOURCE_REPOSITORY)
        github.api = answers.__getitem__
        self.assertEqual((github.tag_commit(TAG), github.tag_commit('v9.9.9')), (REVISION, None))

    def draft(self, github, files, **fields):
        """A draft that matches this run's title, notes and flag unless `fields` says otherwise."""
        expected = self.plan()
        return github.add(TAG, files, **{'name': expected.title, 'body': expected.notes, **fields})

    def test_refuses_a_mismatched_draft(self):
        github = FakeGitHub()
        self.draft(github, {**pair(), 'newt.bin': IMAGE[:-1] + b'\xff'})
        self.refused('newt.bin SHA-256 is not', github=github)
        self.refused(f'gh release delete {TAG} --repo {HOP_REPOSITORY}', github=github)

    def test_refuses_a_draft_whose_title_notes_or_flag_differ(self):
        notes = self.plan().notes
        for fields, message in (({'body': notes + 'https://github.com/VargasDevelopment/newt/pull/1\n'}, 'its notes'),
                                ({'name': 'Newt'}, 'its title'), ({'prerelease': True}, 'flagged pre-release')):
            github = FakeGitHub()
            self.draft(github, pair(), **fields)
            self.refused(message, github=github)

    def test_refuses_the_prerelease_flag(self):
        self.refused('only a nightly, is a pre-release', prerelease=True)

    def nightly(self, **options):
        image = newt_image(NIGHTLY)
        github = options.pop('github', FakeGitHub(tags={'v' + NIGHTLY: REVISION}))
        return self.plan(image, options.pop('git', stamped_git()), github, bench_receipt=None,
                         prerelease=options.pop('prerelease', True), firmware_version=NIGHTLY, **options), github

    def test_a_nightly_is_a_prerelease_with_test_build_notes_and_no_bench_check(self):
        release, _ = self.nightly()
        self.assertEqual((release.refusals, release.prerelease, release.tag), ([], True, 'v' + NIGHTLY))
        self.assertIn('--prerelease', release.command(self.dir))
        self.assertTrue(release.notes.startswith('## Test build\n'), release.notes)
        self.assertIn(f'- Built from: fix(reader): keep the page ({MAIN})\n', release.notes)
        self.assertIn(f'https://github.com/{SOURCE_REPOSITORY}/compare/v0.2.0...{MAIN}\n', release.notes)
        self.assertIn(f'- Version: {NIGHTLY}\n', release.notes)
        self.assertNotIn("What's new", release.notes)
        # A stable release's What's new ignores whatever a nightly's notes said.
        github = FakeGitHub()
        github.add('v' + NIGHTLY, {}, draft=False, prerelease=True, body=self.plan(github=FakeGitHub(previous=None)).notes)
        self.assertEqual(self.plan(github=github).notes, self.plan().notes)

    def test_refuses_a_nightly_without_the_flag_off_a_stamp_or_hopping(self):
        self.assertIn('only a nightly, is a pre-release', self.nightly(prerelease=False)[0].refusals[0])
        self.assertIn('only a nightly, is a pre-release', self.nightly(hop=True)[0].refusals[0])
        release, _ = self.nightly(git=stamped_git(stamps={}))
        self.assertIn("is not a nightly stamp of a commit on origin/main's first-parent history", release.refusals[0])
        self.assertNotIn('## Test build', release.notes)
        # A nightly version stamped by hand on a main commit is no stamp either.
        self.assertTrue(self.nightly(git=stamped_git(stamps={}, mainline={MAIN, REVISION}))[0].refusals)

    def test_refuses_a_version_longer_than_devices_hold(self):
        version = '10.10.11-dev.20261122.10'  # 24 characters: devices hold 23
        with self.assertRaisesRegex(ValueError, 'longer than 23 characters'):
            self.plan(newt_image(version), stamped_git(), FakeGitHub(tags={'v' + version: REVISION}),
                      bench_receipt=None, prerelease=True, firmware_version=version)

    def test_publishing_a_nightly_never_makes_it_latest(self):
        release, github = self.nightly()
        publish(release, self.dir / 'out', github)
        self.assertEqual((github.patched, github.made_latest, github.latest(), github.newest()['id']), ([100], False, {}, 100))
        self.assertTrue(github.release(100)['prerelease'])
        # GitHub showing it as Latest anyway is reported, never deleted.
        release, github = self.nightly(github=FakeGitHub('latest', tags={'v' + NIGHTLY: REVISION}))
        with self.assertRaisesRegex(ValueError, 'it is Latest, or not the newest release'):
            publish(release, self.dir / 'out2', github)
        self.assertFalse(github.release(100)['draft'])

    def test_refuses_a_host_path_in_the_image(self):
        self.refused('newt.bin contains the home folder', image=carrying(os.fsencode(Path.home()) + b'/x.cpp\0'))

    def test_refuses_a_host_detail_in_the_notes(self):
        files = {**FakeGit().files, (REVISION, WHATS_NEW): HEADER.replace(
            'Reading is faster.', 'Reading from ' + os.fsdecode(Path.home()))}
        self.refused('The notes contains the home folder', git=FakeGit(files=files))

    def test_allows_only_upstream_notice_addresses(self):
        self.assertEqual(self.plan(carrying(b'upstream@example.org\0')).refusals, [])
        self.refused('e-mail address someone@example.com', image=carrying(b'someone@example.com\0'))

    def test_committed_notices_credit_the_pinned_certifi(self):
        # newt.bin embeds certifi's CA list (tools/crt_bundle.py pins its version); NOTICES must credit it.
        def credits(notices, pin):
            return f"certifi {re.search(r'SOURCE_SHA256 = .*# certifi (\S+)', pin)[1]}" in notices
        notices, pin = (ROOT / 'docs/NOTICES.txt').read_text(), (ROOT / 'tools/crt_bundle.py').read_text()
        self.assertTrue(credits(notices, pin))
        self.assertFalse(credits(notices.replace('certifi 2', 'certifi 1'), pin), 'negative control')

    def test_committed_notices_credit_the_pinned_rust_runtime(self):
        version = release_publish.json.loads((ROOT / 'rust/toolchain.json').read_text())['version']
        notices = (ROOT / 'docs/NOTICES.txt').read_text()
        credit = f'Rust {version} core and alloc'
        self.assertIn(credit, notices)
        self.assertIn('MIT AND Apache-2.0 WITH LLVM-exception', notices)
        self.assertIn('As an exception, if, as a result of your compiling', notices)

    def test_refuses_without_a_matching_bench_receipt(self):
        self.refused('No bench install receipt', bench_receipt=None)
        for key, value in (('newt_bin_sha256', '0' * 64), ('boot', 'PENDING_VERIFY'), ('home', False)):
            with self.subTest(key):
                self.refused(f'has {key}=', bench_receipt=bench(self.dir, **{key: value}))

    def test_refuses_the_draft_marker(self):
        files = {**FakeGit().files, (REVISION, NOTICES): NOTICE_TEXT + DRAFT + '\n'}
        self.refused('still contains', git=FakeGit(files=files))

    def test_refuses_private_links(self):
        files = {**FakeGit().files, (REVISION, WHATS_NEW): HEADER.replace(
            'Reading is faster.', 'See github.com/VargasDevelopment/newt/pull/3')}
        self.refused('private source repository', git=FakeGit(files=files))
        # Once the firmware reads the source repository, that repository is public.
        public = FakeGit(files={**files, **moved(SOURCE_REPOSITORY)})
        self.assertEqual(self.plan(git=public, github=public_source()[1]).refusals, [])

    def test_publishes_where_the_release_firmware_reads_updates(self):
        # The shipped Rust source names one of the two homes, and the
        # committed notices' offer moves with it.
        committed = update_repository((ROOT / UPDATE_SOURCE).read_text())
        self.assertIn(committed, (HOP_REPOSITORY, SOURCE_REPOSITORY))
        self.assertIn(f'https://github.com/{committed}/issues', (ROOT / NOTICES).read_text())
        for repository in (SOURCE_REPOSITORY, 'Someone/newt-fork'):
            with self.subTest(repository):
                git = FakeGit(files={**FakeGit().files, **moved(repository)})
                release = self.plan(git=git, github=public_source()[1] if repository == SOURCE_REPOSITORY else None)
                self.assertEqual((release.refusals, release.repository), ([], repository))
                command = release.command(self.dir)
                self.assertEqual(command[command.index('--repo') + 1], repository)
        real = 'pub const LATEST_RELEASE_URL: &CStr =\n    c"https://api.github.com/repos/a/b/releases/latest";\n'
        for header in ('', online('a/b') + online('c/d'), online('not a repository'), real,
                       '/*\n' + online(HOP_REPOSITORY) + '*/\n' + real, '// ' + online('a/b') + online('c/d')):
            git = FakeGit(files={**FakeGit().files, (REVISION, UPDATE_SOURCE): header})
            with self.subTest(header), self.assertRaisesRegex(ValueError, 'no single LATEST_RELEASE_URL'):
                self.plan(git=git)

    def test_the_hop_needs_firmware_that_reads_the_source_repository(self):
        self.refused(f'only firmware that reads {SOURCE_REPOSITORY}', hop=True)

    def test_the_hop_follows_the_source_repositorys_published_release(self):
        git, public = public_source()
        public.add(TAG, pair(repository=SOURCE_REPOSITORY), draft=False)
        release = self.plan(git=git, hop=True, public=public)
        self.assertEqual((release.refusals, release.repository), ([], HOP_REPOSITORY))
        self.refused(f'{SOURCE_REPOSITORY} has not published {TAG}', git=git, hop=True, public=FakeGitHub())
        draft = FakeGitHub()
        draft.add(TAG, pair(repository=SOURCE_REPOSITORY))
        self.refused(f'{SOURCE_REPOSITORY} has not published {TAG}', git=git, hop=True, public=draft)
        # The same version and build with other bytes is another build.
        other = FakeGitHub()
        other.add(TAG, pair(carrying(b'another build\0'), SOURCE_REPOSITORY), draft=False)
        self.refused(f"{SOURCE_REPOSITORY}'s {TAG}: its newt.bin", git=git, hop=True, public=other)

    def test_the_source_offer_takes_requests_where_the_firmware_reads_releases(self):
        # The launch flip moves both lines; a release with only the header moved is refused.
        git = FakeGit(files={**FakeGit().files, (REVISION, UPDATE_SOURCE): online(SOURCE_REPOSITORY)})
        disabled = FakeGit(files={**FakeGit().files, **moved(SOURCE_REPOSITORY), (REVISION, NOTICES): NOTICE_TEXT.replace(
            HOP_REPOSITORY + '/issues', SOURCE_REPOSITORY + '/issues-disabled')})
        for hop in (False, True):
            for case in (git, disabled):
                with self.subTest(hop=hop):
                    self.refused(f'does not take requests at https://github.com/{SOURCE_REPOSITORY}/issues', git=case,
                                 hop=hop)

    def test_publish_creates_a_draft_then_publishes_it(self):
        github, out = FakeGitHub(), self.dir / 'out'
        publish(self.plan(github=github), out, github)
        self.assertEqual(github.created, [[
            'gh', 'release', 'create', TAG, '--repo', HOP_REPOSITORY, '--title', f'Newt {VERSION}',
            '--notes-file', str(out / 'notes.md'), '--draft', *(str(out / name) for name in ASSETS)]])
        self.assertEqual((github.patched, github.made_latest, github.latest()['tag_name']), ([100], True, TAG))
        self.assertEqual({name: (out / name).read_bytes() for name in ASSETS}, pair())

    def test_lost_publish_answer_that_published_succeeds(self):
        github = FakeGitHub('lost')
        with redirect_stderr(io.StringIO()) as printed:
            publish(self.plan(github=github), self.dir / 'out', github)
        self.assertIn('reading draft 100 back', printed.getvalue())
        self.assertEqual((github.patched, github.release(100)['draft']), ([100], False))

    def test_failed_publish_keeps_the_draft_and_stops(self):
        github = FakeGitHub('failed')
        with self.assertRaisesRegex(ValueError, 'still a draft'), redirect_stderr(io.StringIO()):
            publish(self.plan(github=github), self.dir / 'out', github)
        self.assertEqual((github.patched, github.release(100)['draft']), ([100], True))

    def test_resumes_a_matching_draft_without_creating_one(self):
        github = FakeGitHub()
        draft = self.draft(github, pair())
        release = self.plan(github=github)
        self.assertEqual((release.refusals, release.draft['id']), ([], draft))
        publish(release, self.dir / 'out', github)
        self.assertEqual((github.created, github.patched, github.latest()['id']), ([], [draft], draft))

    def test_refuses_to_publish_a_corrupt_upload(self):
        github = FakeGitHub(corrupt=True)
        with self.assertRaisesRegex(ValueError, 'newt.bin SHA-256 is not'):
            publish(self.plan(github=github), self.dir / 'out', github)
        self.assertEqual((github.patched, github.release(100)['draft']), ([], True))

    def test_dry_run_writes_and_creates_nothing(self):
        image, record = self.dir / 'firmware.bin', self.dir / 'identity.json'
        image.write_bytes(IMAGE)
        record.write_text(release_publish.json.dumps(receipt(IMAGE)))
        github, out = FakeGitHub(), self.dir / 'out'
        with redirect_stdout(io.StringIO()) as printed:
            code = release_publish.main(['--image', str(image), '--receipt', str(record), '--out', str(out),
                                         '--bench-receipt', str(bench(self.dir))], FakeGit(), github)
        self.assertEqual((code, github.created, out.exists()), (0, [], False))
        self.assertIn(NOTES, printed.getvalue())


# (old, new, test): removing the guard must make its test fail.
CONTROLS = (
    ('    if tagged != commit:', '    if False:', 'test_refuses_untagged_build'),
    ('    if tagged != commit:', '    if False:', 'test_refuses_mismatched_tag'),
    ('    manifest = update_publish.newt_manifest(image, receipt)',
     "    manifest = update_publish.manifest('newt', receipt['firmware_version'], 'build=' + receipt['build_id'], image)",
     'test_refuses_dirty_or_mismatched_build'),
    ('    elif repository != SOURCE_REPOSITORY and there is not None:', '    elif False:',
     'test_refuses_an_existing_tag_or_published_release'),
    ('    if repository == SOURCE_REPOSITORY and there != commit:', '    if False:',
     'test_the_source_repository_needs_its_ci_tag_on_the_build_commit'),
    ("        if not existing.get('draft'):", '        if False:', 'test_refuses_an_existing_tag_or_published_release'),
    ('        elif mismatch := draft_mismatch(github, existing, release):', '        elif False:',
     'test_refuses_a_mismatched_draft'),
    ('    return found + asset_refusals(github, draft, release.assets())',
     '    return asset_refusals(github, draft, release.assets())', 'test_refuses_a_draft_whose_title_notes_or_flag_differ'),
    ('        require(not mismatch, draft_refusals(release, mismatch)[0])', '',
     'test_refuses_to_publish_a_corrupt_upload'),
    ('    except (OSError, ValueError, subprocess.CalledProcessError) as error:', '    except ZeroDivisionError as error:',
     'test_lost_publish_answer_that_published_succeeds'),
    ("    require(shown.get('draft') is False,", '    require(True,', 'test_failed_publish_keeps_the_draft_and_stops'),
    ('    if draft is None:', '    if True:', 'test_resumes_a_matching_draft_without_creating_one'),
    ('    if prerelease != night or (hop and night):', '    if False:', 'test_refuses_the_prerelease_flag'),
    ('    if prerelease != night or (hop and night):', '    if prerelease and not night:',
     'test_refuses_a_nightly_without_the_flag_off_a_stamp_or_hopping'),
    ('    elif not (main if night else git.on_mainline(commit)):', '    elif not git.on_mainline(commit) and not night:',
     'test_refuses_a_nightly_without_the_flag_off_a_stamp_or_hopping'),
    ('    if not night:  # CI publishes', '    if True:  # CI publishes',
     'test_a_nightly_is_a_prerelease_with_test_build_notes_and_no_bench_check'),
    ('    if main:\n', '    if False:\n', 'test_a_nightly_is_a_prerelease_with_test_build_notes_and_no_bench_check'),
    ("            and not release.get('prerelease')", '',
     'test_a_nightly_is_a_prerelease_with_test_build_notes_and_no_bench_check'),
    ("    if release.prerelease:\n        newest = github.newest()", "    if release.prerelease:\n        return",
     'test_publishing_a_nightly_never_makes_it_latest'),
    ("        github.publish(draft['id'], not release.prerelease)", "        github.publish(draft['id'])",
     'test_publishing_a_nightly_never_makes_it_latest'),
    ("    return {line for release in releases if not release['draft'] and release['tag_name'] != tag",
     '    return {line for release in releases', 'test_notes_list_what_this_repository_has_not_published'),
    ("            for section in re.findall(r\"^## What's new\\n(.*?)(?=^## |\\Z)\",",
     "            for section in re.findall(r'(.*)',", 'test_notes_list_what_this_repository_has_not_published'),
    ("leaks('newt.bin', image, patterns, allowed) + ", '', 'test_refuses_a_host_path_in_the_image'),
    (" + leaks('The notes', notes.encode(), patterns, allowed)", '', 'test_refuses_a_host_detail_in_the_notes'),
    ('        refusals += bench_refusals(bench_receipt, version, build, image, manifest)', '        pass',
     'test_refuses_without_a_matching_bench_receipt'),
    ('    elif DRAFT in notices:', '    elif False:', 'test_refuses_the_draft_marker'),
    ('    repository = HOP_REPOSITORY if hop else source', '    repository = HOP_REPOSITORY',
     'test_publishes_where_the_release_firmware_reads_updates'),
    ('    if hop and source != SOURCE_REPOSITORY:', '    if False:',
     'test_the_hop_needs_firmware_that_reads_the_source_repository'),
    ('    elif not re.search(', '    elif False and re.search(',
     'test_the_source_offer_takes_requests_where_the_firmware_reads_releases'),
    (r'/issues(?=$|[\s,;)>]|\.(?!\S))', '/issues', 'test_the_source_offer_takes_requests_where_the_firmware_reads_releases'),
    ("    declared = re.findall(r'^.*\\bLATEST_RELEASE_URL\\b\\s*:.*$'",
     """    declared = re.findall(r'^pub const LATEST_RELEASE_URL: &CStr = c".*";$'""",
     'test_publishes_where_the_release_firmware_reads_updates'),
    ("UPDATE_SOURCE = 'rust/core/src/update_online.rs'", "UPDATE_SOURCE = 'include/newt/update_online.h'",
     'test_cpp_repository_cannot_override_the_shipped_rust_url'),
    ("        while target is not None and target['type'] == 'tag':", '        while False:',
     'test_reads_ci_annotated_tag_as_its_commit'),
    ("        if shown is None or shown.get('draft') is not False:", '        if shown is None:',
     'test_the_hop_follows_the_source_repositorys_published_release'),
    ('for difference in asset_refusals(public, shown, release.assets())]', 'for difference in []]',
     'test_the_hop_follows_the_source_repositorys_published_release'),
)


def negative_controls():
    for old, new, test in CONTROLS:
        with tempfile.TemporaryDirectory(prefix='release-publish-control-') as scratch:
            path = Path(scratch) / 'release_publish.py'
            source = (args.tool_root / 'release_publish.py').read_text()
            if source.count(old) != 1:
                raise RuntimeError('Negative control anchor drift: ' + test)
            path.write_text(source.replace(old, new))
            result = subprocess.run([sys.executable, '-B', __file__, '--tool-root', scratch, '--single', test],
                                    capture_output=True, text=True)
            if result.returncode == 0 or ('FAIL:' not in result.stderr and 'ERROR:' not in result.stderr):
                raise RuntimeError('Guard removal was not caught: ' + test + '\n' + result.stderr)
            print('PASS guard-removal control: ' + test, flush=True)


if __name__ == '__main__':
    loader = unittest.defaultTestLoader
    suite = (loader.loadTestsFromName(args.single, ReleasePublishTests) if args.single
             else loader.loadTestsFromTestCase(ReleasePublishTests))
    if not unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful():
        sys.exit(1)
    if args.negative_controls:
        negative_controls()
