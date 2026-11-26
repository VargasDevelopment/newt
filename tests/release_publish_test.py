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
import random
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

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
from release_publish import (ASSETS, DRAFT, HOP_REPOSITORY, NOTICES, SOURCE_REPOSITORY, STANDARD_NOTE, UPDATE_SOURCE, WHATS_NEW,
                             plan, publish, update_repository)
from bench_candidate import sha
from update_publish_test import BUILD, REVISION, VERSION, app, identity, newt_image, receipt
from x3_slots import NEWT_MARK
import update_publish

TAG = 'v' + VERSION
# A nightly: CI's stamp REVISION of the main commit MAIN, whose own release is 0.2.0.
MAIN, NIGHTLY = 'e' * 40, '0.2.1-dev.2611222359'
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
        self.tip, self.pushed = REVISION, []  # origin's main now; the tags pushed
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

    def push_nightly(self, tag, source):
        if self.tip != source:
            return False
        self.pushed.append(tag)
        return True

    def show(self, commit, path):
        return self.files.get((commit, path))


def stamped_git(**changes):
    """CI's nightly stamp of MAIN, main's tip, tagged here only (publish pushes the tag), its firmware
    reading the source repository."""
    return FakeGit(**{'tags': {'v' + NIGHTLY: REVISION}, 'origin': {}, 'mainline': {MAIN}, 'stamps': {REVISION: MAIN},
                      'versions': {REVISION: NIGHTLY, MAIN: '0.2.0'}, 'tip': MAIN,
                      'files': {**FakeGit().files, **moved(SOURCE_REPOSITORY)}, **changes})



class FakeGitHub:
    """One repository as GitHub keeps it. `answer` is what the publish PATCH does: 'ok', 'lost' (GitHub
    publishes, the answer never arrives) or 'failed' (nothing changes). It has no delete."""

    def __init__(self, answer='ok', corrupt=False, tags=None, previous=SHOWN, meanwhile=None):
        """`meanwhile` runs when the draft is made: another release published in between."""
        self.answer, self.corrupt, self.tags, self.previous = answer, corrupt, dict(tags or {}), previous
        self.meanwhile = meanwhile
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
        if self.meanwhile:
            self.meanwhile(self)

    def publish(self, release_id, latest=True):
        self.patched.append(release_id)
        self.made_latest = latest
        if self.answer != 'failed':
            self.releases[release_id]['draft'] = False
            if self.answer == 'unflagged':
                self.releases[release_id]['prerelease'] = False
            self.tags.setdefault(self.releases[release_id]['tag_name'], REVISION)
        if self.answer != 'ok':
            raise subprocess.CalledProcessError(1, 'gh api -X PATCH')

    def all_releases(self):
        """`previous` is the notes of an older published release this fake holds no assets for."""
        older = [{'tag_name': 'v0.2.0-alpha.0', 'draft': False, 'body': self.previous}] if self.previous else []
        return older + list(self.releases.values())

    def latest(self):
        published = [release for release in self.releases.values() if not release['draft'] and not release['prerelease']]
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

    def test_standard_note_belongs_to_a_current_entry(self):
        old = 'Update in Settings installs Newt from the SD card.'
        fresh = 'Reading is faster.'
        for entries, expected in (
                ([[fresh], [old, STANDARD_NOTE]], [fresh]),
                ([[STANDARD_NOTE], [old, STANDARD_NOTE]], [STANDARD_NOTE]),
                ([[fresh, STANDARD_NOTE], [old, STANDARD_NOTE]], [fresh, STANDARD_NOTE]),
                ([[old, STANDARD_NOTE]], []),
                ([[old], [fresh, STANDARD_NOTE]], [fresh, STANDARD_NOTE])):
            with self.subTest(entries=entries):
                git = FakeGit()
                git.files[REVISION, WHATS_NEW] = 'inline constexpr WhatsNewUpdate kWhatsNew[] = {' + ''.join(
                    '{"Oct 30, 2026", {' + ','.join(release_publish.json.dumps(line) for line in lines) + '}},'
                    for lines in entries) + '};'
                github = FakeGitHub(previous="## What's new\n\n- " + old + '\n- ' + STANDARD_NOTE + '\n')
                notes = self.plan(git=git, github=github).notes
                section = notes.split('## This build')[0]
                self.assertEqual([line[2:] for line in section.splitlines() if line.startswith('- ')], expected)

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
        github = options.pop('github', FakeGitHub())
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
        # Its tag goes out only after every check, so one already pushed is refused.
        pushed = stamped_git(origin={'v' + NIGHTLY: REVISION})
        self.assertIn("a nightly's tag is pushed only after every check", self.nightly(git=pushed)[0].refusals[0])
        github = FakeGitHub(tags={'v' + NIGHTLY: REVISION})
        self.assertIn(f'already has tag v{NIGHTLY}', self.nightly(github=github)[0].refusals[0])
        self.assertIn('only a nightly, is a pre-release', self.nightly(hop=True)[0].refusals[0])
        release, _ = self.nightly(git=stamped_git(stamps={}))
        self.assertIn("is not a nightly stamp of a commit on origin/main's first-parent history", release.refusals[0])
        self.assertNotIn('## Test build', release.notes)
        # A nightly version stamped by hand on a main commit is no stamp either, and legacy .N is not publishable.
        self.assertTrue(self.nightly(git=stamped_git(stamps={}, mainline={MAIN, REVISION}))[0].refusals)
        legacy = '0.2.1-dev.20261122.1'
        self.refused('only a nightly, is a pre-release', image=newt_image(legacy),
                     git=stamped_git(tags={'v' + legacy: REVISION}, versions={REVISION: legacy, MAIN: '0.2.0'}),
                     github=FakeGitHub(), bench_receipt=None, prerelease=True, firmware_version=legacy)

    def test_refuses_a_version_longer_than_devices_hold(self):
        version = '100.10.11-dev.2611222359'  # 24 characters: devices hold 23
        with self.assertRaisesRegex(ValueError, 'longer than 23 characters'):
            self.plan(newt_image(version), stamped_git(), FakeGitHub(),
                      bench_receipt=None, prerelease=True, firmware_version=version)

    def test_publishing_a_nightly_pushes_its_tag_last_and_verifies_it_by_id(self):
        # A stable release published in between changes nothing: only this release, by id, is checked.
        def stable(github):
            github.add('v0.2.1', pair(repository=SOURCE_REPOSITORY), draft=False)
        git = stamped_git()
        release, github = self.nightly(git=git, github=FakeGitHub(meanwhile=stable))
        self.assertTrue(publish(release, self.dir / 'out', github, git))
        self.assertEqual((git.pushed, github.patched, github.made_latest), (['v' + NIGHTLY], [100], False))
        self.assertTrue(github.release(100)['prerelease'])
        # GitHub showing it unflagged is reported, never deleted.
        git = stamped_git()
        release, github = self.nightly(git=git, github=FakeGitHub('unflagged'))
        with self.assertRaisesRegex(ValueError, 'does not show it flagged'), redirect_stderr(io.StringIO()):
            publish(release, self.dir / 'out2', github, git)
        self.assertFalse(github.release(100)['draft'])

    def test_a_nightly_publishes_nothing_once_main_has_moved_on_or_a_check_fails(self):
        git = stamped_git(tip='f' * 40)
        release, github = self.nightly(git=git)
        self.assertFalse(publish(release, self.dir / 'out', github, git))
        # Its verified draft stays private: no tag, nothing published.
        self.assertEqual((git.pushed, github.patched, github.release(100)['draft']), ([], [], True))
        # A local write that fails stops before the tag push.
        git, github = stamped_git(), FakeGitHub()
        (self.dir / 'taken').mkdir()
        (self.dir / 'taken' / 'notes.md').write_text('')
        with self.assertRaises(OSError):
            publish(self.nightly(git=git)[0], self.dir / 'taken', github, git)
        self.assertEqual((git.pushed, github.created), ([], []))
        # A draft whose upload does not verify stays private, and no tag goes out.
        git, github = stamped_git(), FakeGitHub(corrupt=True)
        with self.assertRaisesRegex(ValueError, 'newt.bin SHA-256 is not'):
            publish(self.nightly(git=git, github=github)[0], self.dir / 'out3', github, git)
        self.assertEqual((git.pushed, github.patched), ([], []))

    def test_refuses_a_host_path_in_the_image(self):
        self.refused('newt.bin contains the home folder', image=carrying(os.fsencode(Path.home()) + b'/x.cpp\0'))

    def test_refuses_a_host_detail_in_the_notes(self):
        files = {**FakeGit().files, (REVISION, WHATS_NEW): HEADER.replace(
            'Reading is faster.', 'Reading from ' + os.fsdecode(Path.home()))}
        self.refused('The notes contains the home folder', git=FakeGit(files=files))

    def test_allows_only_upstream_notice_addresses(self):
        self.assertEqual(self.plan(carrying(b'upstream@example.org\0')).refusals, [])
        self.refused('e-mail address someone@example.com', image=carrying(b'someone@example.com\0'))

    def test_email_guard_ignores_random_bytes_and_refuses_c_strings(self):
        # Seeded 4 MiB random binary plus the actual nightly 4b344fbc sequence (o@D.yq),
        # an isolated 7-byte run below PRINTABLE_RUN (x@w1.fi), random symbol-wrapped short bytes
        # ({[d@en.bi$y), and implausible noise must pass.
        noise = (random.Random(169).randbytes(4 * 1024 * 1024)
                 + b"\x0eo@D.yq\xb7'\x00x@w1.fi\x00\x85{[d@en.bi$y\xf0{[me@pm.me$y\x00.       a@b.de\x00"
                 + b'\xb5nyy@u.su\x94\xf7Q7@3Q.RS\x94\xd9, tHXO@Z.UK"o\x8b\xdd\nA@YL.Epf\x19\xbd3@T.Gvprfw\x83'
                 + b"  o@D.yq  \0  ab@cd.yq  \0  ab@cD.de  \0  e@wb1D.nn  \0"
                 + b"  a@b.cd  \0  ab@pm.ME  \0  ab@PM.me  \0"
                 + b"  foo..bar@example.com  \0  ab@bar.coM  \0  foo@bar.com@baz.org  \0"
                 + b"Build tag user@example.com123 is not an e-mail.\0"
                 + b"Build tag user@build.example.com123 is not an e-mail.\0"
                 + b"Build tag user@ci.build-tools.com123 is not an e-mail.\0"
                 + b"Build tag user@ci.build--tools.com123 is not an e-mail.\0"
                 + b"Build tag user@ci.build--tools.api.us.east.prod.com123 is not an e-mail.\0"
                 + b"Build tag user@ci.xn--p1ai.com123 is not an e-mail.\0")
        self.assertEqual(self.plan(carrying(noise)).refusals, [])
        # Real-looking addresses in C strings, printable notice lines and DER certs are still refused.
        der_cn_utf8 = (b'\x30\x2f\x31\x1c\x30\x1a\x06\x03\x55\x04\x03\x0c\x17'
                       + 'René alice@example.org'.encode() + b'\x31\x0f\x30\x0d\x06\x03\x55\x04\x0a\x0c\x06Issuer')
        der_cn_48 = (b'\x30\x48\x31\x35\x30\x33\x06\x03\x55\x04\x03\x0c\x30'
                     b'upstream@example.org (firmware support contacts)'
                     b'\x31\x0f\x30\x0d\x06\x03\x55\x04\x0a\x0c\x06Issuer')
        der_127 = 'a' * 62 + '@' + 'b' * 60 + '.org'
        der_128 = 'a' * 63 + '@' + 'b' * 60 + '.org'
        der_129 = 'alice@' + 'b' * 50 + '.' + 'c' * 33 + '.' + 'd' * 34 + '.org'
        der_260 = 'a' * 64 + '@' + '.'.join(['b' * 48, 'c' * 48, 'd' * 48, 'e' * 44, 'org'])
        der_278 = 'a' * 28 + '@' + '.'.join(['b' * 50, 'c' * 50, 'd' * 60, 'e' * 60, 'f' * 21, 'org'])
        der_290 = 'a' * 64 + '@' + '.'.join(['b' * 56, 'c' * 56, 'd' * 56, 'e' * 50, 'org'])
        for payload, address in (
                (b'someone@example.com\0', 'someone@example.com'),
                (b'support@example.COM\0', 'support@example.COM'),
                (b'support@example.Com\0', 'support@example.Com'),
                (b'Contact <support@example.Uk>\0', 'support@example.Uk'),
                (b'Contact <support@189.cn>\0', 'support@189.cn'),
                (b'it@x.com\0', 'it@x.com'),
                (b'it@189.cn\0', 'it@189.cn'),
                (b'IT@X.COM\0', 'IT@X.COM'),
                (b'IT@189.CN\0', 'IT@189.CN'),
                (b'Contact <it@189.cn>\0', 'it@189.cn'),
                (b'Email: it@189.cn\0', 'it@189.cn'),
                (b'Email: it@x.com\0', 'it@x.com'),
                (b'Email: it@x.com\xff', 'it@x.com'),
                (b'mailto:it@x.com\0', 'it@x.com'),
                (b'From: a@b.de\0', 'a@b.de'),
                (b'From: a@b.de\xff', 'a@b.de'),
                (b'To: x@w1.fi\0', 'x@w1.fi'),
                (b'To: x@w1.fi\xff', 'x@w1.fi'),
                (b'To: IT@x.com\0', 'IT@x.com'),
                (b'Cc: it@x.COM\0', 'it@x.COM'),
                (b'https://example.org/?to=me@pm.me&subject=Hello\0', 'me@pm.me'),
                (b'https://example.org/me@pm.me#top\0', 'me@pm.me'),
                (b'|me@pm.me|\0', 'me@pm.me'),
                (b'<it@x.com>\0', 'it@x.com'),
                (b'<it@x.com>\xff', 'it@x.com'),
                (b'Author: @me@pm.me\0', 'me@pm.me'),
                (b'Author: @me@pm.me\xff', 'me@pm.me'),
                (b'Contact [@me@pm.me]\0', 'me@pm.me'),
                (b'Contact <billing@example.FR>\0', 'billing@example.FR'),
                (b'Contact <billing@My.co.uk>\0', 'billing@My.co.uk'),
                (b'Contact <help+@example.com>\0', 'help+@example.com'),
                (b'support@x.com\0', 'support@x.com'),
                (b'Contact <support@x.ai>\0', 'support@x.ai'),
                (b'_team@example.com\0', '_team@example.com'),
                (b'123@example.com\0', '123@example.com'),
                (b'dev.user+tag@sub.example.co.uk\0', 'dev.user+tag@sub.example.co.uk'),
                (b'me@pm.me\0', 'me@pm.me'),
                (b'Contact [me@pm.me]\0', 'me@pm.me'),
                (b'me@foo.io\0', 'me@foo.io'),
                (b'j@example.com\0', 'j@example.com'),
                (b'm@proton.me\0', 'm@proton.me'),
                (b'Author <x@w1.fi>\0', 'x@w1.fi'),
                (b'ADMIN@EXAMPLE.COM\0', 'ADMIN@EXAMPLE.COM'),
                (b'Alice <alice@FreeBSD.org>\0', 'alice@FreeBSD.org'),
                (b'Contact <support@Example360.com>\0', 'support@Example360.com'),
                (b'j@FreeBSD.org\0', 'j@FreeBSD.org'),
                (b'jD@example.com\0', 'jD@example.com'),
                (b'user@iCloud.com\0', 'user@iCloud.com'),
                (b'contact@163.com\0', 'contact@163.com'),
                (b'mailto:owner@example.net\0', 'owner@example.net'),
                (b'Contact <support@example.xn--p1ai>\0', 'support@example.xn--p1ai'),
                (b'Contact <admin@mx.srv2--east.example.org>.\0', 'admin@mx.srv2--east.example.org'),
                (b'Contact admin@mx.srv2--east.example.org--thanks.\0', 'admin@mx.srv2--east.example.org'),
                (b'<!-- admin@mx.srv--east.example.org-->\0', 'admin@mx.srv--east.example.org'),
                (b'upstream@example.org--ops.com\0', 'upstream@example.org--ops.com'),
                (b'Contact upstream@example.org--backup@example.net for urgent requests.\0', 'backup@example.net'),
                (b'Contact upstream@example.org--jane.doe@example.net for urgent requests.\0', 'jane.doe@example.net'),
                (b'Contact upstream@example.org--jane.doe+tag@example.net\0', 'jane.doe+tag@example.net'),
                (b'Contact upstream@example.org--jane.doe1@example.net\0', 'jane.doe1@example.net'),
                (b'Contact upstream@example.org--jane.doe--ops@example.net for urgent requests.\0',
                 'jane.doe--ops@example.net'),
                (b'Contact upstream@example.org--it@x.com\0', 'it@x.com'),
                (b'Contact support@example.org--thanks.\0', 'support@example.org'),
                (b'Send reports to support@example.org--e.g., crash logs.\0', 'support@example.org'),
                (b'Report bugs to support@example.org--v0.6.0 users should attach logs.\0', 'support@example.org'),
                (b'<!-- Contact: owner@example.org-->\0', 'owner@example.org'),
                (b'Contact support@example.xn--p1ai--thanks.\0', 'support@example.xn--p1ai'),
                (b'<!-- Contact: owner@example.xn--p1ai-->\0', 'owner@example.xn--p1ai'),
                (b'Send reports to support@example.xn--p1ai--e.g., crash logs.\0', 'support@example.xn--p1ai'),
                (b'Report bugs to support@example.xn--p1ai--v0.6.0 users should attach logs.\0',
                 'support@example.xn--p1ai'),
                (b'<!-- Contact: it@x.com-->\0', 'it@x.com'),
                (b'alice@example.org->support\0', 'alice@example.org'),
                (b'it@x.com->support\0', 'it@x.com'),
                (b'Contact: ...alice@example.org...\0', 'alice@example.org'),
                (b'Contact: .alice@example.org\0', 'alice@example.org'),
                (b'Contact: hi...alice@example.org...\0', 'alice@example.org'),
                (b'Contact: ...it@x.com...\0', 'it@x.com'),
                (b'Contact: .it@x.com\0', 'it@x.com'),
                (b'Contact: it@x.com[1]\0', 'it@x.com'),
                (b'Contact: [1]it@x.com\0', 'it@x.com'),
                (b'Contact: it@x.com(1)\0', 'it@x.com'),
                (b'Contact @alice@example.org\0', 'alice@example.org'),
                (b'\x81\x07x@w1.fi\x82', 'x@w1.fi'),
                (b'\x16\x07x@w1.fi\xff', 'x@w1.fi'),
                (b'\x16\x06a@b.de\xff', 'a@b.de'),
                (b'\x16\x08it@x.com', 'it@x.com'),
                (b'\x81\x08it@x.com', 'it@x.com'),
                (b'\x16\x08it@x.com\xff', 'it@x.com'),
                (b'\x81\x08it@x.com\xff', 'it@x.com'),
                (b'\x16\x08it@x.com0\x82', 'it@x.com'),
                (b'\x81\x08it@x.com0\x82', 'it@x.com'),
                (b'\x16\x0fcert@example.hu0\x82', 'cert@example.hu'),
                (b'\x81\x09it@189.cn\0', 'it@189.cn'),
                (b'\x160a48-byte-cert-email-address@embedded-example.org0\x82',
                 'a48-byte-cert-email-address@embedded-example.org'),
                (b'\x16@firmware-release-coordinator@embedded-platform-tools.example.org',
                 'firmware-release-coordinator@embedded-platform-tools.example.org'),
                (b'\x16.the-46-byte-email-address@embedded-example.org',
                 'the-46-byte-email-address@embedded-example.org'),
                (b'\x81.the-46-byte-email-address@embedded-example.org',
                 'the-46-byte-email-address@embedded-example.org'),
                (b'\x16\x7f' + der_127.encode() + b'\x30\x00', der_127),
                (b'\x81\x7f' + der_127.encode() + b'\x30\x00', der_127),
                (b'\x16\x81\x80' + der_128.encode() + b'\x30\x00', der_128),
                (b'\x81\x81\x80' + der_128.encode() + b'\x30\x00', der_128),
                (b'\x16\x81\x81' + der_129.encode() + b'\x30\x00', der_129),
                (b'\x81\x81\x81' + der_129.encode() + b'\x30\x00', der_129),
                (b'\x16\x82\x01\x04' + der_260.encode() + b'\x30\x00', der_260),
                (b'\x16\x82\x01\x16' + der_278.encode() + b'\x30\x00', der_278),
                (b'\x16\x82\x01\x22' + der_290.encode() + b'\x30\x00', der_290),
                (bytes.fromhex(
                    '302d311a301806035504030c11616c696365406578616d706c652e6f7267310f300d060355040a0c06497373756572'),
                 'alice@example.org'),
                (der_cn_utf8, 'alice@example.org'),
                (bytes.fromhex(
                    '0c1652656ec3a9206140622e6465e28094737570706f7274310f300d060355040a0c06497373756572'),
                 'a@b.de'),
                (b'\x00' + 'René a@b.de—support'.encode() + b'\x00', 'a@b.de'),
                (b'\x00' + 'René ab@b.de—support'.encode() + b'\x00', 'ab@b.de'),
                (b'\x00' + 'send to it@x.COM—support'.encode() + b'\x00', 'it@x.COM'),
                (b'\x00' + ('send to it@x.COM—' + 'x' * 509).encode() + b'\x00', 'it@x.COM'),
                (b'\x00' + ('send to it@x.COM—' + 'x' * 510).encode() + b'\x00', 'it@x.COM'),
                (b'\x00' + ('it@x.COM—' + ' ' * 60 + 'help').encode() + b'\x00', 'it@x.COM'),
                (b'\x00' + ('it@x.COM—' + ' ' * 61 + 'help').encode() + b'\x00', 'it@x.COM'),
                (b'\x0c\x10' + 'a@b.de—support'.encode() + b'\x31\x0f', 'a@b.de'),
                (b'\x0c\x19Contact alice@example.org\x31\x0f', 'alice@example.org'),
                (b'\x0c\x06a@b.de\x31\x0f', 'a@b.de')):
            with self.subTest(address=address):
                self.assertEqual(
                    self.plan(carrying(payload)).refusals,
                    [f'newt.bin contains the e-mail address {address}'],
                )
        # Bracketed contact lists refuse every address inside the list.
        list_refusals = self.plan(carrying(b'Contacts: [me@pm.me, it@x.com]\0')).refusals
        self.assertEqual(
            list_refusals,
            ['newt.bin contains the e-mail address it@x.com', 'newt.bin contains the e-mail address me@pm.me'],
        )
        # Short, tag-colliding DER, and dash-separated addresses are allowed when credited in NOTICES.txt.
        git = FakeGit(files={**FakeGit().files,
                             (REVISION, NOTICES): (NOTICE_TEXT + f'Copyright <j@w1.fi>\n{der_129}\n{der_278}\n'
                                                   'jane.doe--ops@example.net\n'
                                                   'admin@mx.srv--east.example.org\nbackup@example.net\n')})
        for allowed_payload in (b'Author <j@w1.fi>\0',
                                b'\x16\x81\x81' + der_129.encode() + b'\x30\x00',
                                b'\x81\x81\x81' + der_129.encode() + b'\x30\x00',
                                b'\x16\x82\x01\x16' + der_278.encode() + b'\x30\x00',
                                der_cn_48,
                                b'Contact: ...upstream@example.org...\0',
                                b'Contact upstream@example.org--jane.doe--ops@example.net for urgent requests.\0',
                                b'admin@mx.srv--east.example.org--backup@example.net\0'):
            self.assertEqual(self.plan(carrying(allowed_payload), git=git).refusals, [])
        for allowed_note in ('Contact: ...upstream@example.org...',
                             'Contact upstream@example.org--jane.doe--ops@example.net for urgent requests.',
                             'admin@mx.srv--east.example.org--backup@example.net'):
            files = {**git.files, (REVISION, WHATS_NEW): HEADER.replace('Reading is faster.', allowed_note)}
            self.assertEqual(self.plan(git=FakeGit(files=files)).refusals, [])
        # Crediting only the first internal-`--` address still refuses the uncredited second address cleanly.
        one_git = FakeGit(files={**FakeGit().files,
                                 (REVISION, NOTICES): NOTICE_TEXT + 'admin@mx.srv--east.example.org\n'})
        self.assertEqual(
            self.plan(carrying(b'admin@mx.srv--east.example.org--backup@example.net\0'), git=one_git).refusals,
            ['newt.bin contains the e-mail address backup@example.net'],
        )
        one_files = {**one_git.files,
                     (REVISION, WHATS_NEW): HEADER.replace('Reading is faster.',
                                                           'admin@mx.srv--east.example.org--backup@example.net')}
        self.assertEqual(
            self.plan(git=FakeGit(files=one_files)).refusals,
            ['The notes contains the e-mail address backup@example.net'],
        )
        # Release notes are UTF-8 prose, so they skip binary-noise filtering and catch ordinary addresses directly.
        for note, address in (
                ('Contact <support@example.COM>.', 'support@example.COM'),
                ('Contact <support@example.xn--p1ai>.', 'support@example.xn--p1ai'),
                ('Contact <admin@mx.srv2--east.example.org>.', 'admin@mx.srv2--east.example.org'),
                ('Contact admin@mx.srv2--east.example.org--thanks.', 'admin@mx.srv2--east.example.org'),
                ('<!-- admin@mx.srv--east.example.org-->', 'admin@mx.srv--east.example.org'),
                ('Contact upstream@example.org--ops.com.', 'upstream@example.org--ops.com'),
                ('Contact upstream@example.org--backup@example.net for urgent requests.', 'backup@example.net'),
                ('Contact upstream@example.org--jane.doe@example.net for urgent requests.', 'jane.doe@example.net'),
                ('Contact upstream@example.org--jane.doe+tag@example.net', 'jane.doe+tag@example.net'),
                ('Contact upstream@example.org--jane.doe1@example.net', 'jane.doe1@example.net'),
                ('Contact upstream@example.org--jane.doe--ops@example.net for urgent requests.',
                 'jane.doe--ops@example.net'),
                ('Reference prefix@local@example.org here.', 'local@example.org'),
                ('Contact support@example.org--thanks.', 'support@example.org'),
                ('Send reports to support@example.org--e.g., crash logs.', 'support@example.org'),
                ('Report bugs to support@example.org--v0.6.0 users should attach logs.', 'support@example.org'),
                ('<!-- Contact: owner@example.org-->', 'owner@example.org'),
                ('Contact support@example.xn--p1ai--thanks.', 'support@example.xn--p1ai'),
                ('Send reports to support@example.xn--p1ai--e.g., crash logs.', 'support@example.xn--p1ai'),
                ('Report bugs to support@example.xn--p1ai--v0.6.0 users should attach logs.',
                 'support@example.xn--p1ai'),
                ('<!-- Contact: owner@example.xn--p1ai-->', 'owner@example.xn--p1ai'),
                ('alice@example.org->support', 'alice@example.org'),
                ('Contact: ...alice@example.org...', 'alice@example.org'),
                ('Contact: .alice@example.org', 'alice@example.org'),
                ('Contact: hi...alice@example.org...', 'alice@example.org'),
                ('Follow @alice@example.org for updates.', 'alice@example.org'),
                ('Contact <_team@example.com>.', '_team@example.com'),
                ('_Contact support@example.com_', 'support@example.com'),
                ('Mail mailto:123@example.com for help.', '123@example.com'),
                ('Contact <ab@x.com>.', 'ab@x.com'),
                ('a@b.cd', 'a@b.cd')):
            with self.subTest(note=note):
                files = {**FakeGit().files, (REVISION, WHATS_NEW): HEADER.replace('Reading is faster.', note)}
                self.assertEqual(
                    self.plan(git=FakeGit(files=files)).refusals,
                    [f'The notes contains the e-mail address {address}'],
                )
        for non_email in (
                'Build tag user@example.com123 is not an e-mail.',
                'Build tag user@build.example.com123 is not an e-mail.',
                'Build tag user@ci.build-tools.com123 is not an e-mail.',
                'Build tag user@ci.build--tools.com123 is not an e-mail.',
                'Build tag user@ci.build--tools.api.us.east.prod.com123 is not an e-mail.',
                'Build tag user@ci.xn--p1ai.com123 is not an e-mail.'):
            with self.subTest(note=non_email):
                files = {**FakeGit().files, (REVISION, WHATS_NEW): HEADER.replace('Reading is faster.', non_email)}
                self.assertEqual(self.plan(git=FakeGit(files=files)).refusals, [])

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
        # Devices read /releases/latest: a release that is not it is reported, never deleted.
        github = FakeGitHub()
        github.latest = lambda: {'id': 1}
        with self.assertRaisesRegex(ValueError, '/releases/latest does not show it'):
            publish(self.plan(github=github), self.dir / 'out2', github)
        # It must show this release's assets too, not only its id.
        github = FakeGitHub()
        github.latest = lambda: {**github.release(100), 'assets': []}
        with self.assertRaisesRegex(ValueError, '/releases/latest does not show it'):
            publish(self.plan(github=github), self.dir / 'out3', github)

    def test_new_draft_readback_retries_absence_with_a_bound(self):
        for misses in (0, 1, 3, 4):
            with self.subTest(misses=misses):
                github = FakeGitHub()
                release = self.plan(github=github)
                read = github.release_for
                answers = [None] * misses

                def delayed(tag):
                    return answers.pop() if answers else read(tag)

                github.release_for = delayed
                with mock.patch('release_publish.time.sleep') as sleep:
                    if misses < 4:
                        self.assertTrue(publish(release, self.dir / f'out{misses}', github))
                        self.assertEqual(github.patched, [100])
                    else:
                        with self.assertRaisesRegex(ValueError, 'shows no draft'):
                            publish(release, self.dir / f'out{misses}', github)
                        self.assertEqual(github.patched, [])
                    self.assertEqual([call.args[0] for call in sleep.call_args_list],
                                     [1, 2, 4][:min(misses, 3)])
                self.assertEqual(len(github.created), 1)

    def test_draft_readback_failures_never_push_a_nightly_tag(self):
        for answer, message, delays in (
                (ValueError('read failed'), 'read failed', []),
                ({'draft': False}, 'shows no draft', []),
                (None, 'shows no draft', [1, 2, 4])):
            with self.subTest(answer=answer):
                git = stamped_git()
                release, github = self.nightly(git=git)
                out = self.dir / f'out{len(delays)}{type(answer).__name__}'
                with mock.patch.object(github, 'release_for', return_value=answer) as read, \
                        mock.patch('release_publish.time.sleep') as sleep:
                    if isinstance(answer, ValueError):
                        read.side_effect = answer
                    with self.assertRaisesRegex(ValueError, message):
                        publish(release, out, github, git)
                    self.assertEqual([call.args[0] for call in sleep.call_args_list], delays)
                    self.assertEqual(read.call_count, len(delays) + 1)
                self.assertEqual((git.pushed, github.patched), ([], []))

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
    ('        for delay in (1, 2, 4):', '        for delay in ():',
     'test_new_draft_readback_retries_absence_with_a_bound'),
    ('line == STANDARD_NOTE and current', 'line == STANDARD_NOTE',
     'test_standard_note_belongs_to_a_current_entry'),
    ('(index == 0 and lines == [STANDARD_NOTE])', 'False',
     'test_standard_note_belongs_to_a_current_entry'),
    ('    if tagged != commit:', '    if False:', 'test_refuses_untagged_build'),
    ('    if tagged != commit:', '    if False:', 'test_refuses_mismatched_tag'),
    ('    manifest = update_publish.newt_manifest(image, receipt)',
     "    manifest = update_publish.manifest('newt', receipt['firmware_version'], 'build=' + receipt['build_id'], image)",
     'test_refuses_dirty_or_mismatched_build'),
    ('    elif (night or repository != SOURCE_REPOSITORY) and there is not None:', '    elif False:',
     'test_refuses_an_existing_tag_or_published_release'),
    ('(night or repository != SOURCE_REPOSITORY) and there', 'repository != SOURCE_REPOSITORY and there',
     'test_refuses_a_nightly_without_the_flag_off_a_stamp_or_hopping'),
    ('    if repository == SOURCE_REPOSITORY and not night and there != commit:', '    if False:',
     'test_the_source_repository_needs_its_ci_tag_on_the_build_commit'),
    ('!= (None if night else commit):', '!= commit and not night:',
     'test_refuses_a_nightly_without_the_flag_off_a_stamp_or_hopping'),
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
    ('    if draft is None:\n        github.create_draft', '    if True:\n        github.create_draft',
     'test_resumes_a_matching_draft_without_creating_one'),
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
    ("shown.get('prerelease') is release.prerelease and", '',
     'test_publishing_a_nightly_pushes_its_tag_last_and_verifies_it_by_id'),
    ("        github.publish(draft['id'], not release.prerelease)", "        github.publish(draft['id'])",
     'test_publishing_a_nightly_pushes_its_tag_last_and_verifies_it_by_id'),
    ('    if release.prerelease and not (git or Git()).push_nightly(', '    if False and not (git or Git()).push_nightly(',
     'test_a_nightly_publishes_nothing_once_main_has_moved_on_or_a_check_fails'),
    ("        require(latest.get('id') == draft['id'] and", '        require(True or',
     'test_publish_creates_a_draft_then_publishes_it'),
    ("latest.get('prerelease') is False and not asset_refusals(github, latest, release.assets()),",
     "latest.get('prerelease') is False,", 'test_publish_creates_a_draft_then_publishes_it'),
    ("    return {line for release in releases if not release['draft'] and release['tag_name'] != tag",
     '    return {line for release in releases', 'test_notes_list_what_this_repository_has_not_published'),
    ("            for section in re.findall(r\"^## What's new\\n(.*?)(?=^## |\\Z)\",",
     "            for section in re.findall(r'(.*)',", 'test_notes_list_what_this_repository_has_not_published'),
    ("leaks('newt.bin', image, patterns, allowed, binary=True) + ", '', 'test_refuses_a_host_path_in_the_image'),
    (" + leaks('The notes', notes.encode(), patterns, allowed)", '', 'test_refuses_a_host_detail_in_the_notes'),
    ('for email in sorted((binary_emails if binary else text_emails)(data, allowed_emails) - allowed_emails)]',
     'for email in []]', 'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ("allowed = text_emails((notices or '').encode())", 'allowed = set()',
     'test_allows_only_upstream_notice_addresses'),
    ('    if angle_wrapped or (nonspace >= 12 and (terminated or cased)) or labeled:',
     '    if False:',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('    return terminated and nonspace >= 8 and cased', '    return terminated and cased',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('    return terminated and nonspace >= 8 and cased', '    return nonspace >= 8 and cased',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('    return terminated and nonspace >= 8 and cased', '    return terminated and nonspace >= 8',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('    if not _short_boundaries_ok(run, start, end, brackets):', '    if False:',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('DER_TAGS = frozenset((0x0c, 0x13, 0x14, 0x16, 0x81))', 'DER_TAGS = frozenset((0x16, 0x81))',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('6 <= length <= 31', '8 <= length <= 31',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('    if pos >= 1 and data[pos - 1] in DER_TAGS and pos < n:', '    if False:',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('    if pos >= 2 and data[pos - 2] in DER_TAGS:', '    if False:',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('        if data[pos - 2] == 0x81 and data[pos - 1] >= 128:', '        if False:',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('        if data[pos - 2] == 0x82 and data[pos - 1] == 1 and pos < n:', '        if False:',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('    if pos >= 4 and data[pos - 4] in DER_TAGS and data[pos - 3] == 0x82:', '    if False:',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ("        if b'@' in inner and _der_text_ok(inner, tag):", '        if False:',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('        if val_end is None:\n            u_start = _utf8_run_start(data, start, stop=no_der_end)',
     '        if False:\n            u_start = _utf8_run_start(data, start, stop=no_der_end)',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ("    trimmed = raw_domain.rstrip(b'.-')", '    trimmed = raw_domain',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ("        while start < at and text[start] == ord('.'):", '        while False:',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('                u_end = _utf8_run_end(data, end)',
     '                u_end = end',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('            min_nonspace = cached_u_nonspace',
     '            min_nonspace = 0',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('        if followed_by_at:', '        if False:',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('            if len(split_ends) >= 2 and allowed_emails:', '            if False:',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('        if len(domain) < len(raw_domain):', '        if False:',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('        if _plausible_binary_email(local, domain, text, start, end, nonspace, terminated, brackets):',
     '        if True:',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
    ('if binary else text_emails', 'if binary else binary_emails',
     'test_email_guard_ignores_random_bytes_and_refuses_c_strings'),
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
