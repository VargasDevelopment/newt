#!/usr/bin/env python3
"""Stamp a nightly test build on a main commit (docs/RELEASING.md, "Nightly test builds").

CI's nightly job runs `stamp` on the main commit whose gate just passed. It
writes the nightly version into the two version files, commits only that
change on top of the main commit, and tags that commit v<version>; no branch
ever carries it. The job then builds the stamped commit like any release, pushes
the tag and publishes the build as a GitHub pre-release with
release_publish.py --prerelease.

The version is <next>-dev.YYMMDDHHMM (UTC). <next> is the patch after the core
of the main commit's own version (0.5.0 gives 0.5.1, 0.5.1-rc.1 gives 0.5.2);
YYMMDDHHMM is a fixed-width 10-digit UTC timestamp so GitHub's descending text
order matches SemVer order and ranks above legacy <next>-dev.YYYYMMDD.N tags.
Every existing tag and draft release's tag counts as taken: a tag is never
reused, and a stamp behind an existing one is refused. On an exact same-minute
collision, `stamp` waits once until the next UTC minute, rechecks main's tip,
tags and drafts, and refuses if the new minute is still taken. Devices hold at
most 23 characters of a version, so a longer one is refused. A push that changes
nothing the image is built from since the last published nightly gets none; a
tag with no published release (none, or only a draft) never counts as one.

  nightly.py stamp --repository OWNER/NAME [--force]
      (CI's nightly job; prints version=VERSION, empty when skipped; --force for a manual run)
"""
import argparse
from datetime import datetime, timezone
import json
import re
import shlex
import subprocess
import sys
import time

from bench_candidate import require
from release_tools import (ANY_NIGHTLY, DESTINATION, NIGHTLY, NIGHTLY_STAMP, ROOT, VERSION_HEADER,
                           committed_release, declared_release, git_output, is_ancestor, resolve_commit)
from x3_slots import NEWT_VERSION_BYTES

CONFIG = '.cz.toml'
# Each version file's one version line, around the version.
VERSION_LINES = {CONFIG: ('version = "', '"'), VERSION_HEADER: ('#define NEWT_FIRMWARE_VERSION "', '"')}
# Paths the image is never built from; a change anywhere else builds a new nightly.
NOT_IMAGE = ('.github/', 'docs/', 'installer/', 'tests/', 'rust/tests/', 'rust/host/')
# The firmware toolchain locks live under docs/ but choose what builds the image.
IMAGE_DOCS = re.compile(r'docs/toolchain-[^/]+\.(?:json|lock)')
NEW_TAG = re.compile(r'v([0-9]+)\.([0-9]+)\.([0-9]+)-dev\.(' + NIGHTLY_STAMP + r')')
LEGACY_TAG = re.compile(r'v([0-9]+)\.([0-9]+)\.([0-9]+)-dev\.([0-9]{8})\.([1-9][0-9]*)')


def next_release(release):
    """The patch after a version's core, which every nightly built on it sorts below."""
    found = re.fullmatch(r'([0-9]+)\.([0-9]+)\.([0-9]+)(?:-[0-9A-Za-z.-]+)?', release)
    require(found is not None, f'{release} is not a release version')
    major, minor, patch = map(int, found.groups())
    return f'{major}.{minor}.{patch + 1}'


def parse_nightly_tag(tag):
    """(major, minor, patch, stamp_or_date, legacy_seq, tag) in SemVer order, or None.
    Fixed-width 10-digit stamps (>= 2101010000) have seq 0 and sort above all 8-digit legacy tags."""
    if (found := NEW_TAG.fullmatch(tag)) is not None:
        major, minor, patch, stamp_id = map(int, found.groups())
        return (major, minor, patch, stamp_id, 0, tag)
    if (found := LEGACY_TAG.fullmatch(tag)) is not None:
        major, minor, patch, day, seq = map(int, found.groups())
        return (major, minor, patch, day, seq, tag)
    return None


def nightly_tags(tags):
    """Each nightly tag's SemVer sort tuple, newest last."""
    return sorted(parsed for tag in tags if (parsed := parse_nightly_tag(tag)) is not None)


def format_stamp(moment):
    """A timezone-aware datetime as its UTC YYMMDDHHMM nightly identifier."""
    require(isinstance(moment, datetime) and moment.tzinfo is not None,
            'Nightly timestamp must be a timezone-aware datetime')
    utc = moment.astimezone(timezone.utc)
    require(2021 <= utc.year <= 2099, f'UTC year {utc.year} is outside the nightly range (2021..2099)')
    return utc.strftime('%y%m%d%H%M')


def latest_stamp(base, tags):
    """Highest taken 10-digit YYMMDDHHMM integer for `<base>-dev.*`, or None."""
    core = tuple(map(int, base.split('.')))
    taken = [stamp_id for *other, stamp_id, seq, _ in nightly_tags(tags) if tuple(other) == core and seq == 0]
    return max(taken, default=None)


def nightly_version(release, stamp_id, tags):
    """The nightly of `release` at UTC `stamp_id` (YYMMDDHHMM or datetime); refuses collisions and older stamps."""
    base = next_release(release)
    if isinstance(stamp_id, datetime):
        stamp_id = format_stamp(stamp_id)
    require(isinstance(stamp_id, str) and re.fullmatch(NIGHTLY_STAMP, stamp_id) is not None,
            f'{stamp_id!r} is not a UTC YYMMDDHHMM nightly identifier')
    newest = latest_stamp(base, tags)
    require(newest is None or int(stamp_id) > newest,
            f'Nightly {base}-dev.{stamp_id} collides with or precedes existing {base}-dev.{newest}')
    version = f'{base}-dev.{stamp_id}'
    require(len(version) < NEWT_VERSION_BYTES,
            f'{version} is longer than the image identity holds ({NEWT_VERSION_BYTES - 1} characters)')
    return version


def next_minute_wait(moment):
    """Seconds until just past the next UTC minute boundary, in (1.0, 61.0]."""
    utc = moment.astimezone(timezone.utc)
    return 61.0 - utc.second - utc.microsecond / 1_000_000


def show(root, commit, path):
    """A file's exact text at a commit (git_output would strip its last newline)."""
    return subprocess.run(['git', 'show', f'{commit}:{path}'], cwd=root, check=True, capture_output=True,
                          text=True).stdout


def stamped(root, commit, version):
    """The version files at `commit` with `version` in place of its release: exactly what a stamp commits."""
    release = committed_release(root, commit)[0]
    texts = {}
    for path, (before, after) in VERSION_LINES.items():
        line = re.compile('^' + re.escape(before + release + after) + '$', re.MULTILINE)
        text, count = line.subn(lambda _: before + version + after, show(root, commit, path))
        require(count == 1, f'{path} at {commit} has no single version line for {release}')
        texts[path] = text
    declared_release(texts[CONFIG], texts[VERSION_HEADER])
    return texts


def stamp_source(root, commit, legacy=False):
    """The main commit that `commit` stamps, or None unless `commit` is exactly a stamp: one parent on
    origin/main's first-parent history, a nightly of that parent's next release, and no other change."""
    parents = git_output(root, 'rev-list', '--parents', '-n', '1', commit).split()[1:]
    if len(parents) != 1 or parents[0] not in git_output(root, 'rev-list', '--first-parent', DESTINATION).split():
        return None
    source, version = parents[0], committed_release(root, commit)[0]
    pattern = ANY_NIGHTLY if legacy else NIGHTLY
    try:
        expected = stamped(root, source, version)
        nightly = re.fullmatch(pattern, version) and version.startswith(next_release(
            committed_release(root, source)[0]) + '-dev.')
    except ValueError:
        return None
    changed = git_output(root, 'diff', '--name-only', source, commit).splitlines()
    same = all(show(root, commit, path) == text for path, text in expected.items())
    return source if nightly and sorted(changed) == sorted(VERSION_LINES) and same else None


def published(repository, tag, answer=None):
    """The tag's published release, or None when GitHub answers 404: no release, or only a draft,
    which this endpoint never shows. Any other answer, or none at all, raises, so a failed read
    never passes for "not published". `answer` runs gh (tests give gh-shaped answers)."""
    command = ['gh', 'api', '--include', f'repos/{repository}/releases/tags/{tag}']
    reply = (answer or (lambda command: subprocess.run(command, capture_output=True)))(command)
    found = re.match(rb'HTTP/\S+ ([0-9]{3})[^\n]*\n.*?\r?\n\r?\n(.*)\Z', reply.stdout, re.DOTALL)
    status = int(found[1]) if found else None
    if status == 404:
        return None
    require(status == 200 and reply.returncode == 0,
            f'{shlex.join(command)} failed (HTTP {status}): {reply.stderr.decode().strip()}')
    release = json.loads(found[2])
    require(release.get('tag_name') == tag, f'GitHub answered {tag} with release {release.get("tag_name")}')
    return None if release.get('draft') is not False else release


def draft_tags(repository, answer=None):
    """The tags of the repository's draft releases. A draft has no git tag until it publishes, and
    release_publish.py never deletes one, so a nightly that declined to publish keeps its tag taken.
    A failed read raises: guessing "no drafts" would reuse a draft's tag and fail its publish."""
    command = ['gh', 'api', '--paginate', f'repos/{repository}/releases', '--jq', '.[] | select(.draft) | .tag_name']
    reply = (answer or (lambda command: subprocess.run(command, capture_output=True)))(command)
    require(reply.returncode == 0, f'{shlex.join(command)} failed: {reply.stderr.decode().strip()}')
    return reply.stdout.decode().split()


def origin_nightly_tags(root):
    """Origin's nightly tag names (git ls-remote, read only)."""
    lines = git_output(root, 'ls-remote', '--tags', 'origin', 'refs/tags/v*-dev.*').splitlines()
    return [line.split('\t', 1)[1].removeprefix('refs/tags/') for line in lines if not line.endswith('^{}')]


def push_tag(root, tag, source):
    """Pushes a nightly's tag, its one irreversible step, after every check; returns False instead
    when origin's main has moved on from `source`, the stamped commit (its own run publishes). Main's
    tip being `source` also means its declared version is the one stamped."""
    if git_output(root, 'ls-remote', 'origin', 'refs/heads/main').split('\t')[0] != source:
        return False
    git_output(root, 'push', '--no-follow-tags', 'origin', f'refs/tags/{tag}:refs/tags/{tag}')
    return True


def notes(repository, main, subject, release):
    """A nightly's notes above its identity: what it is and the main commit it builds."""
    return ('## Test build\n\n'
            'A nightly build of main for devices with Test builds on. It has had no bench or hardware check.\n\n'
            f'- Built from: {subject} ({main})\n'
            f'- Changes since {release}: https://github.com/{repository}/compare/v{release}...{main}\n')


def image_changes(root, since, head):
    """Paths changed from `since` to `head` that the image may be built from."""
    changed = git_output(root, 'diff', '--name-only', since, head).splitlines()
    return [path for path in changed if IMAGE_DOCS.fullmatch(path) or not (
        path.startswith(NOT_IMAGE) or ('/' not in path and path.endswith('.md')))]


def last_source(root, tags, published):
    """The main commit the newest published nightly stamps, or None. Only a release GitHub shows
    published for the tag counts: a tag whose publish failed must not suppress the next nightly."""
    for *_, tag in reversed(nightly_tags(tags)):
        if published(tag) is not None:
            return stamp_source(root, resolve_commit(root, 'refs/tags/' + tag), legacy=True)
    return None


def stamp(published, root=ROOT, now=None, force=False, drafts=(), sleep=time.sleep):
    """Commits and tags the nightly of HEAD, main's tip; returns its version, or None when main has
    moved on or the image has not changed since the last published nightly. `published(tag)` reads
    GitHub; `drafts` is a tag list or zero-arg reader for draft releases' tags. On a same-minute
    collision, waits once for the next UTC minute, rechecks main/tags/drafts, and refuses if still taken."""
    head = resolve_commit(root, 'HEAD')
    live = git_output(root, 'ls-remote', 'origin', 'refs/heads/main').split('\t')[0]
    if live != head:
        print(f'No nightly: main has moved on to {live}; its own run builds the next one', file=sys.stderr)
        return None
    require(resolve_commit(root, DESTINATION) == head, 'origin/main here is not origin\'s main: git fetch origin')
    tags = git_output(root, 'tag', '--list', 'v*-dev.*').split()
    since = None if force else last_source(root, tags, published)
    if since is not None and is_ancestor(root, since, head) and not image_changes(root, since, head):
        print(f'No nightly: nothing the image is built from changed since the last published one ({since})',
              file=sys.stderr)
        return None
    release = committed_release(root, head)[0]
    base = next_release(release)
    read_now = now if callable(now) else (lambda: now) if now is not None else (lambda: datetime.now(timezone.utc))
    read_drafts = drafts if callable(drafts) else (lambda: list(drafts))
    moment = read_now()
    stamp_id = format_stamp(moment)
    taken = [*tags, *origin_nightly_tags(root), *read_drafts()]
    newest = latest_stamp(base, taken)
    if newest is not None and int(stamp_id) == newest:
        delay = next_minute_wait(moment)
        print(f'Nightly {base}-dev.{stamp_id} is already taken; waiting {delay:.1f}s for the next UTC minute',
              file=sys.stderr)
        sleep(delay)
        live = git_output(root, 'ls-remote', 'origin', 'refs/heads/main').split('\t')[0]
        if live != head:
            print(f'No nightly: main has moved on to {live}; its own run builds the next one', file=sys.stderr)
            return None
        moment = read_now()
        stamp_id = format_stamp(moment)
        taken = [*git_output(root, 'tag', '--list', 'v*-dev.*').split(), *origin_nightly_tags(root), *read_drafts()]
    version = nightly_version(release, stamp_id, taken)
    for path, text in stamped(root, head, version).items():
        (root / path).write_text(text)
    git_output(root, 'commit', '-q', '-m', f'chore(release): nightly {version}\n\nSource: {head}', '--', *VERSION_LINES)
    git_output(root, 'tag', '-a', 'v' + version, '-m',
               f'Newt nightly {version}\n\nSource: {head}\nA test build: no bench or hardware check.\n')
    require(stamp_source(root, resolve_commit(root, 'HEAD')) == head, f'The stamp of {version} is not exact')
    print(f'Nightly {version}: {resolve_commit(root, "HEAD")}, stamped on {head}', file=sys.stderr)
    return version


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    commands = parser.add_subparsers(dest='command', required=True)
    command = commands.add_parser('stamp', help="stamp main's tip")
    command.add_argument('--repository', required=True, help='where nightlies are published (OWNER/NAME)')
    command.add_argument('--force', action='store_true',
                         help='stamp even when the image has not changed since the last published nightly')
    args = parser.parse_args(argv)
    try:
        version = stamp(lambda tag: published(args.repository, tag), force=args.force,
                        drafts=lambda: draft_tags(args.repository))
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f'Nightly stopped: {error}', file=sys.stderr)
        return 1
    print(f'version={version or ""}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
