#!/usr/bin/env python3
"""Publish a tagged release as one public GitHub Release (docs/RELEASING.md).

A dry run by default: it makes every check, prints the plan and the notes, and
writes nothing. Only --publish, run by the Lead, writes the update pair through
update_publish.py into a new folder, creates a draft release with the three
assets, verifies the draft's assets, then publishes it with one PATCH. A rerun
resumes a draft whose title, notes, flag and assets match. It never deletes a release: `gh release
create` without --draft deletes the release when its publish response is lost,
even if GitHub published it. Devices read GET /repos/<repository>/releases/latest,
then the assets newt.manifest and newt.bin.

The repository is the one the release's own firmware reads (LATEST_RELEASE_URL
in UPDATE_SOURCE at the release commit), so moving releases is that one line.
--hop publishes the same checked release to HOP_REPOSITORY instead, once, and
only for firmware that reads SOURCE_REPOSITORY and that SOURCE_REPOSITORY has
already published with the same assets: devices still reading the old
repository find it there and from then on read the new one. CI tags each
release in SOURCE_REPOSITORY, so there the tag must exist and name the build
commit; anywhere else an existing tag is refused.

A nightly (tools/nightly.py) is the one pre-release: --prerelease publishes it,
never as Latest, from CI's stamp of a main commit, with no bench check and no
What's new. Only devices with Test builds on, which read the newest release,
see it.

  release_publish.py --bench-receipt JSON                 (dry run)
  release_publish.py --bench-receipt JSON --publish       (the Lead only)
  release_publish.py --bench-receipt JSON --hop --publish (the Lead, once, at launch)
  release_publish.py --prerelease --publish               (CI's nightly job)
"""
from __future__ import annotations
import argparse
from dataclasses import dataclass, field
import getpass
import json
import os
from pathlib import Path
import re
import shlex
import socket
import subprocess
import sys

from bench_candidate import VARIANT, json_bytes, require, sha
from daily_firmware import BUILD_DIRECTORY, IDENTITY_RECEIPT
from release_tools import DESTINATION, NIGHTLY, committed_release, git_output, resolve_commit
import nightly
import update_publish

ROOT = Path(__file__).resolve().parents[1]
UPDATE_SOURCE = 'rust/core/src/update_online.rs'
SOURCE_REPOSITORY = 'VargasDevelopment/newt'
HOP_REPOSITORY = 'VargasDevelopment/newt-releases'
ASSETS = ('newt.bin', 'newt.manifest', 'NOTICES.txt')
NOTICES = 'docs/NOTICES.txt'
# NOTICES.txt marks each item that still needs the owner's decision with this.
DRAFT = 'DRAFT: needs owner approval'
WHATS_NEW = 'rust/device-ui/whats_new_notes.h'
EMAIL = re.compile(rb'[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}')


class Git:
    """The source repository, read only: tags, origin/main and files at a commit."""

    def __init__(self, root=ROOT):
        self.root = root

    def tag_commit(self, tag):
        return resolve_commit(self.root, 'refs/tags/' + tag)

    def origin_tag_commit(self, tag):
        """The commit origin's tag names, or None (git ls-remote, a read)."""
        ref = 'refs/tags/' + tag
        refs = dict(reversed(line.split('\t')) for line in
                    git_output(self.root, 'ls-remote', 'origin', ref, ref + '^{}').splitlines())
        return refs.get(ref + '^{}', refs.get(ref))

    def on_mainline(self, commit):
        return commit in git_output(self.root, 'rev-list', '--first-parent', DESTINATION).split()

    def version(self, commit):
        return committed_release(self.root, commit)[0]

    def stamp_source(self, commit):
        return nightly.stamp_source(self.root, commit)

    def subject(self, commit):
        return git_output(self.root, 'log', '-1', '--format=%s', commit)

    def show(self, commit, path):
        """A file's text at a commit, or None where it does not exist."""
        shown = subprocess.run(['git', 'show', f'{commit}:{path}'], cwd=self.root, capture_output=True, text=True)
        return None if shown.returncode else shown.stdout


def update_repository(source):
    """Read the shipped Rust URL; duplicate or changed declarations fail closed."""
    declared = re.findall(r'^.*\bLATEST_RELEASE_URL\b\s*:.*$', source, re.MULTILINE)
    found = re.fullmatch(r'pub const LATEST_RELEASE_URL: &CStr = c"https://api.github.com/repos/([A-Za-z0-9-]+/[A-Za-z0-9._-]+)/releases/latest";',
                         declared[0]) if len(declared) == 1 else None
    require(found is not None, f'{UPDATE_SOURCE} names no single LATEST_RELEASE_URL')
    return found[1]


def publish_command(repository, release_id, latest=True):
    """Publishes a verified draft, marked Latest unless it is a nightly; the only change ever made to an
    existing release."""
    return ['gh', 'api', '-X', 'PATCH', f'repos/{repository}/releases/{release_id}',
            '-F', 'draft=false', '-f', f'make_latest={str(latest).lower()}']


class GitHub:
    """One repository through gh: reads by `gh api`; writes are the draft create and the publish PATCH."""

    def __init__(self, repository):
        self.repository = repository

    def api(self, path):
        return json.loads(self.run(['gh', 'api', path]))

    @staticmethod
    def run(command):
        answer = subprocess.run(command, capture_output=True)
        require(answer.returncode == 0, f'{shlex.join(command)} failed: {answer.stderr.decode().strip()}')
        return answer.stdout

    def tag_commit(self, tag):
        """The commit the repository's tag names (an annotated tag peeled), or None."""
        target = next((ref['object'] for ref in self.api(f'repos/{self.repository}/git/matching-refs/tags/{tag}')
                       if ref['ref'] == 'refs/tags/' + tag), None)
        while target is not None and target['type'] == 'tag':
            target = self.api(f'repos/{self.repository}/git/tags/{target["sha"]}')['object']
        return target and target['sha']

    def release_for(self, tag):
        """The tag's release, drafts included (only an authorized list shows them), or None."""
        ids = self.run(['gh', 'api', '--paginate', f'repos/{self.repository}/releases?per_page=100',
                        '--jq', f'.[] | select(.tag_name == "{tag}") | .id']).split()
        require(len(ids) <= 1, f'{self.repository} has {len(ids)} releases for {tag}')
        return self.release(ids[0].decode()) if ids else None

    def release(self, release_id):
        return self.api(f'repos/{self.repository}/releases/{release_id}')

    def download(self, asset_id):
        return self.run(['gh', 'api', f'repos/{self.repository}/releases/assets/{asset_id}',
                         '-H', 'Accept: application/octet-stream'])

    def create_draft(self, command):
        self.run(command)

    def publish(self, release_id, latest=True):
        self.run(publish_command(self.repository, release_id, latest))

    def latest(self):
        return self.api(f'repos/{self.repository}/releases/latest')

    def newest(self):
        """The newest published release, which devices with Test builds on read."""
        return next((release for release in self.api(f'repos/{self.repository}/releases?per_page=20')
                     if not release['draft']), {})

    def all_releases(self):
        """Every release here, drafts included, as {tag_name, draft, prerelease, body}."""
        rows = self.run(['gh', 'api', '--paginate', f'repos/{self.repository}/releases?per_page=100',
                         '--jq', '.[] | {tag_name, draft, prerelease, body} | tojson'])
        return [json.loads(row) for row in rows.splitlines()]


@dataclass
class Plan:
    repository: str
    version: str
    commit: str
    build: str
    image: bytes
    manifest: bytes
    notes: str
    notices: str | None
    refusals: list[str] = field(default_factory=list)
    # The matching draft a rerun resumes instead of creating one.
    draft: dict | None = None
    prerelease: bool = False

    @property
    def tag(self):
        return 'v' + self.version

    @property
    def title(self):
        return f'Newt {self.version}'

    def command(self, out):
        """The draft, never public, that publish_command later publishes; --prerelease only for a nightly."""
        return ['gh', 'release', 'create', self.tag, '--repo', self.repository, '--title', self.title,
                '--notes-file', str(out / 'notes.md'), *(['--prerelease'] if self.prerelease else []),
                '--draft', *(str(out / name) for name in ASSETS)]

    def assets(self):
        """Each asset's expected (size, SHA-256)."""
        return {name: (len(data), sha(data))
                for name, data in zip(ASSETS, (self.image, self.manifest, (self.notices or '').encode()))}


def whats_new(header):
    """The What's new header's updates, newest first: [(date, [lines])]."""
    def strings(text):
        found = re.findall(r'"((?:[^"\\]|\\.)*)"', text)
        require(all(re.fullmatch(r'(?:[^\\]|\\["\\])*', item) for item in found),
                f'{WHATS_NEW} uses an escape other than \\" or \\\\')
        return [re.sub(r'\\(.)', r'\1', item) for item in found]
    return [(strings(date)[0], strings(lines))
            for date, lines in re.findall(r'\{\s*("[^"]*")\s*,\s*\{(.*?)\}\s*\}', header, re.DOTALL)]


def announced(releases, tag):
    """Every What's new line that the published notes here, other than `tag`'s and nightlies', already
    showed readers."""
    return {line for release in releases if not release['draft'] and release['tag_name'] != tag
            and not release.get('prerelease')
            for section in re.findall(r"^## What's new\n(.*?)(?=^## |\Z)",
                                      (release['body'] or '').replace('\r\n', '\n'), re.MULTILINE | re.DOTALL)
            for line in re.findall(r'^- (.+)$', section, re.MULTILINE)}


def render_notes(version, commit, build, image, news):
    """The release body: What's new, then the build's identity. Plain words, deterministic. The
    CHANGELOG stays out: it describes internal work in the private source repository."""
    parts = []
    if news:
        parts.append("## What's new\n\n" + ''.join(f'- {line}\n' for line in news))
    parts.append('## This build\n\n'
                 f'- Version: {version}\n'
                 f'- Source commit: {commit}\n'
                 f'- Build ID: {build}\n'
                 f'- newt.bin: {len(image):,} bytes, SHA-256 {sha(image)}\n')
    return '\n'.join(parts)


def nightly_notes(repository, main, subject, release):
    """A nightly's notes above its identity: what it is and the main commit it builds."""
    return ('## Test build\n\n'
            'A nightly build of main for devices with Test builds on. It has had no bench or hardware check.\n\n'
            f'- Built from: {subject} ({main})\n'
            f'- Changes since {release}: https://github.com/{repository}/compare/v{release}...{main}\n')


def host_patterns(root=ROOT):
    """This host's details, found at run time so no private string is ever committed: (label, bytes, any case)."""
    found = [('home folder', os.fsencode(Path.home()), False), ('checkout', os.fsencode(root.resolve()), False),
             ('user name', getpass.getuser().encode(), True),
             ('private build folder', b'.build/private', False), ('private books folder', b'private-books', False)]
    host = socket.gethostname()
    found += [('host name', name.encode(), True) for name in {host, host.split('.')[0]} if name]
    return found


def leaks(name, data, patterns, allowed_emails):
    """Each host detail or non-notice e-mail address in data, as a refusal."""
    found = [f'{name} contains the {label}' for label, pattern, fold in patterns
             if (pattern.lower() in data.lower() if fold else pattern in data)]
    found += [f'{name} contains the e-mail address {email.decode()}'
              for email in sorted(set(EMAIL.findall(data)) - allowed_emails)]
    return found


def bench_refusals(path, version, build, image, manifest):
    """Why the bench install check does not cover exactly this pair: version, build and both SHA-256s,
    then a VALID boot that reached Home."""
    if path is None:
        return ['No bench install receipt (--bench-receipt): install this exact pair on the bench X3 first']
    try:
        record = json_bytes(Path(path).read_bytes())
    except (OSError, ValueError) as error:
        return [f'The bench install receipt cannot be read: {error}']
    expected = {'version': version, 'build': build, 'newt_bin_sha256': sha(image),
                'newt_manifest_sha256': sha(manifest), 'boot': 'VALID', 'home': True}
    return [f'The bench install receipt has {key}={record.get(key)!r}, not {value!r}'
            for key, value in expected.items() if record.get(key) != value]


def asset_refusals(github, release, expected):
    """How a release's assets differ from exactly `expected`: names, sizes, then SHA-256s (the API's
    digest when it gives one, otherwise the downloaded bytes)."""
    assets = {asset['name']: asset for asset in release.get('assets', ())}
    if set(assets) != set(expected):
        return [f'its assets are {sorted(assets)}, not {sorted(expected)}']
    found = []
    for name, (size, digest) in expected.items():
        asset = assets[name]
        if asset.get('state', 'uploaded') != 'uploaded' or asset['size'] != size:
            found.append(f"its {name} is {asset['size']:,} bytes ({asset.get('state')}), not {size:,}")
        elif (asset.get('digest') or 'sha256:' + sha(github.download(asset['id']))) != 'sha256:' + digest:
            found.append(f'its {name} SHA-256 is not {digest}')
    return found


def draft_mismatch(github, draft, release):
    """How a draft differs from the one this run creates. Publishing keeps the draft's own title, notes
    and flag, so matching assets alone never resume it."""
    body = (draft.get('body') or '').replace('\r\n', '\n').strip()
    found = [message for same, message in (
        (draft.get('name') == release.title, f'its title is not {release.title!r}'),
        (body == release.notes.strip(), 'its notes are not the notes this run renders'),
        (draft.get('prerelease') is release.prerelease,
         f'it is {"not " if release.prerelease else ""}flagged pre-release')) if not same]
    return found + asset_refusals(github, draft, release.assets())


def draft_refusals(release, mismatch):
    tag, repository = release.tag, release.repository
    url = f'https://github.com/{repository}/releases'
    return [f'{repository} has a draft for {tag} that is not this pair: {"; ".join(mismatch)}. A draft is '
            f'never public: the Lead deletes it by hand (gh release delete {tag} --repo {repository}, or {url}), '
            'then reruns']


def plan(image, receipt, bench_receipt=None, prerelease=False, git=None, github=None, root=ROOT, hop=False,
         public=None):
    """Every check, and what --publish would publish. Identity failures raise; the rest are refusals.
    `public` reads SOURCE_REPOSITORY for the hop."""
    git = git or Git(root)
    manifest = update_publish.newt_manifest(image, receipt)
    version, commit, build = receipt['firmware_version'], receipt['git_revision'], receipt['build_id']
    night = re.fullmatch(NIGHTLY, version) is not None
    main = git.stamp_source(commit) if night else None  # the main commit a nightly stamps
    refusals = []
    source = update_repository(git.show(commit, UPDATE_SOURCE) or '')
    repository = HOP_REPOSITORY if hop else source
    if hop and source != SOURCE_REPOSITORY:
        refusals.append(f'The hop publishes to {HOP_REPOSITORY} only firmware that reads {SOURCE_REPOSITORY}; '
                        f'this build reads {source}')
    github = github or GitHub(repository)
    tag = 'v' + version
    tagged = git.tag_commit(tag)
    if tagged != commit:
        refusals.append(f'Tag {tag} here names {tagged}, not the build commit {commit}: '
                        'publish only the release merge CI tagged (git fetch origin)')
    elif git.origin_tag_commit(tag) != commit:
        refusals.append(f"Origin's tag {tag} does not name {commit}")
    elif not (main if night else git.on_mainline(commit)):
        refusals.append(f"{commit} is not " + ('a nightly stamp of a commit on' if night else 'on') +
                        " origin/main's first-parent history")
    elif git.version(commit) != version:
        refusals.append(f"{commit}'s own version is not {version}")
    if prerelease != night or (hop and night):
        refusals.append("A nightly, and only a nightly, is a pre-release, published here: /releases/latest, which "
                        'devices read, skips pre-releases; Test builds read the newest release')
    notices = git.show(commit, NOTICES)
    if notices is None:
        refusals.append(f'{commit} has no {NOTICES}')
    elif DRAFT in notices:
        refusals.append(f'{NOTICES} still contains "{DRAFT}"')
    elif not re.search(rf'(?<![^\s(<])https://github\.com/{re.escape(source)}/issues(?=$|[\s,;)>]|\.(?!\S))',
                       notices, re.MULTILINE):
        # The written source offer must stay answerable: exactly this repository's issues, never a private one.
        refusals.append(f"{NOTICES}'s written offer does not take requests at https://github.com/{source}/issues, "
                        "where this firmware's releases are")
    if not night:  # CI publishes nightlies unchecked; only devices with Test builds on see them.
        refusals += bench_refusals(bench_receipt, version, build, image, manifest)
    header = git.show(commit, WHATS_NEW)
    require(header is not None, f'{commit} has no {WHATS_NEW}')
    # News is what this repository's published notes have not yet shown. Git tags are no guide: the public
    # export put every older tag on one commit, and a first release here (or the hop) has other readers.
    shown = set() if night else announced(github.all_releases(), tag)
    news = [] if night else list(dict.fromkeys(line for _, lines in whats_new(header) for line in lines
                                               if line not in shown))
    notes = render_notes(version, commit, build, image, news)
    if main:
        notes = nightly_notes(source, main, git.subject(main), git.version(main)) + '\n' + notes
    # Until its firmware reads it, the source repository is private and its links 404.
    if source != SOURCE_REPOSITORY and re.search(r'VargasDevelopment/newt(?!-releases)', notes):
        refusals.append('The notes link into the private source repository')
    patterns = host_patterns(root)
    allowed = set(EMAIL.findall((notices or '').encode()))
    refusals += leaks('newt.bin', image, patterns, allowed) + leaks('The notes', notes.encode(), patterns, allowed)
    release = Plan(repository, version, commit, build, image, manifest, notes, notices, refusals, prerelease=night)
    if hop and source == SOURCE_REPOSITORY:
        public = public or GitHub(SOURCE_REPOSITORY)
        shown = public.release_for(tag)
        if shown is None or shown.get('draft') is not False:
            refusals.append(f'{SOURCE_REPOSITORY} has not published {tag}: the hop follows that release')
        else:
            refusals += [f"{SOURCE_REPOSITORY}'s {tag}: {difference}"
                         for difference in asset_refusals(public, shown, release.assets())]
    there = github.tag_commit(tag)
    if repository == SOURCE_REPOSITORY and there != commit:
        refusals.append(f"{repository}'s tag {tag} names {there}, not the build commit {commit}")
    elif repository != SOURCE_REPOSITORY and there is not None:
        refusals.append(f'{repository} already has tag {tag}; a published version is never replaced')
    elif (existing := github.release_for(tag)) is not None:
        if not existing.get('draft'):
            refusals.append(f'{repository} already published {tag}; a published version is never replaced')
        elif mismatch := draft_mismatch(github, existing, release):
            refusals += draft_refusals(release, mismatch)
        else:
            release.draft = existing
    return release


def describe(release, out):
    print(f'Release {release.tag} to {release.repository}, ' +
          ('a pre-release, never Latest' if release.prerelease else 'marked Latest, no pre-release flag'))
    print(f'Title: Newt {release.version}')
    print(f'Source commit: {release.commit} (build {release.build}, {VARIANT})')
    for name, data in zip(ASSETS, (release.image, release.manifest, release.notices)):
        data = data.encode() if isinstance(data, str) else data
        print(f'Asset {name}: ' + (f'{len(data):,} bytes, SHA-256 {sha(data)}' if data is not None else 'missing'))
    if release.draft is None:
        print('Draft: ' + shlex.join(release.command(out)))
    else:
        print(f"Draft: resume {release.repository} draft {release.draft['id']}, whose assets match")
    print('Publish: ' + shlex.join(publish_command(release.repository, release.draft['id'] if release.draft
                                                   else '<draft id>', not release.prerelease)))
    print('--- Notes ---')
    print(release.notes, end='')
    print('--- End of notes ---')


def publish(release, out, github):
    """Creates (or resumes) the draft, verifies its assets, publishes it with one PATCH, then reads it
    back by id and as devices will. Never deletes: a draft that stays a draft is reported."""
    draft = release.draft
    if draft is None:
        digests = dict(update_publish.publish(out, 'newt', release.image, release.manifest))
        require(digests == {'newt.bin': sha(release.image), 'newt.manifest': sha(release.manifest)},
                'The written pair differs from the checked pair')
        for name, text in (('NOTICES.txt', release.notices), ('notes.md', release.notes)):
            with (out / name).open('x', encoding='utf-8') as file:
                file.write(text)
        github.create_draft(release.command(out))
        draft = github.release_for(release.tag)
        require(draft is not None and draft.get('draft') is True,
                f'{release.repository} shows no draft for {release.tag} after creating it')
        mismatch = draft_mismatch(github, draft, release)
        require(not mismatch, draft_refusals(release, mismatch)[0])
    try:
        github.publish(draft['id'], not release.prerelease)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        # GitHub may have published it and only the answer was lost; the read-back decides.
        print(f'The publish request failed ({error}); reading draft {draft["id"]} back', file=sys.stderr)
    shown = github.release(draft['id'])
    require(shown.get('draft') is False,
            f'{release.tag} is still a draft ({release.repository} release {draft["id"]}), so not public; '
            'rerun --publish to resume it')
    latest = github.latest()
    if release.prerelease:
        newest = github.newest()
        require(shown.get('prerelease') is True and latest.get('id') != draft['id'] and
                newest.get('id') == draft['id'] and not asset_refusals(github, newest, release.assets()),
                f'{release.repository} published {release.tag}, but it is Latest, or not the newest release with '
                f'exactly {", ".join(ASSETS)}; never delete it (docs/RELEASING.md)')
        return
    require(latest.get('id') == draft['id'] and latest.get('tag_name') == release.tag and
            latest.get('prerelease') is False and not asset_refusals(github, latest, release.assets()),
            f'{release.repository} published {release.tag}, but /releases/latest does not show it with exactly '
            f'{", ".join(ASSETS)}; never delete it (docs/RELEASING.md)')


def main(argv=None, git=None, github=None, public=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--image', type=Path, default=ROOT / BUILD_DIRECTORY / 'firmware.bin')
    parser.add_argument('--receipt', type=Path, default=ROOT / IDENTITY_RECEIPT)
    parser.add_argument('--bench-receipt', type=Path, help="the bench install check's receipt (docs/RELEASING.md)")
    parser.add_argument('--out', type=Path, help='a new folder for the published files (default .build/release/TAG)')
    parser.add_argument('--prerelease', action='store_true', help="a nightly, CI's test build; refused for others")
    parser.add_argument('--publish', action='store_true', help='create the release (the Lead only)')
    parser.add_argument('--hop', action='store_true',
                        help=f'publish to {HOP_REPOSITORY} instead: once, at launch (docs/RELEASING.md)')
    args = parser.parse_args(argv)
    try:
        release = plan(args.image.read_bytes(), json_bytes(args.receipt.read_bytes()), args.bench_receipt,
                       args.prerelease, git, github, hop=args.hop, public=public)
        github = github or GitHub(release.repository)
        out = args.out or ROOT / '.build/release' / (release.tag + ('-hop' if args.hop else ''))
        describe(release, out)
        for refusal in release.refusals:
            print(f'Refused: {refusal}', file=sys.stderr)
        if release.refusals:
            return 1
        if not args.publish:
            print('DRY RUN: nothing written or published')
            return 0
        publish(release, out, github)
    except (OSError, KeyError, TypeError, ValueError, subprocess.CalledProcessError) as error:
        print(f'Release publish stopped: {error}', file=sys.stderr)
        return 1
    shows = 'the newest release, not /releases/latest,' if release.prerelease else '/releases/latest'
    print(f'Published {release.tag} to {release.repository}; {shows} shows it with {", ".join(ASSETS)}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
