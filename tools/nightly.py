#!/usr/bin/env python3
"""Stamp a nightly test build on a main commit (docs/RELEASING.md, "Nightly test builds").

CI's nightly job runs `stamp` on the main commit whose gate just passed. It
writes the nightly version into the two version files, commits only that
change on top of the main commit, and tags that commit v<version>; no branch
ever carries it. The job then builds the stamped commit like any release, pushes
the tag and publishes the build as a GitHub pre-release with
release_publish.py --prerelease.

The version is <next>-dev.YYYYMMDD.N. <next> is the patch after the core of the
main commit's own version (0.2.1-rc.1 gives 0.2.2); YYYYMMDD is the UTC day; N
follows every nightly tag of <next> that day, published or not, and every draft
release's tag: a run that declines to publish keeps its draft but never pushes
its tag, and the next run must not take that number again. Devices hold at
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

from bench_candidate import require
from release_tools import (DESTINATION, NIGHTLY, ROOT, VERSION_HEADER, committed_release, declared_release,
                           git_output, is_ancestor, resolve_commit)
from x3_slots import NEWT_VERSION_BYTES

CONFIG = '.cz.toml'
# Each version file's one version line, around the version.
VERSION_LINES = {CONFIG: ('version = "', '"'), VERSION_HEADER: ('#define NEWT_FIRMWARE_VERSION "', '"')}
# Paths the image is never built from; a change anywhere else builds a new nightly.
NOT_IMAGE = ('.github/', 'docs/', 'installer/', 'tests/', 'rust/tests/', 'rust/host/')
# The firmware toolchain locks live under docs/ but choose what builds the image.
IMAGE_DOCS = re.compile(r'docs/toolchain-[^/]+\.(?:json|lock)')
TAG = re.compile(r'v([0-9]+)\.([0-9]+)\.([0-9]+)-dev\.([0-9]{8})\.([1-9][0-9]*)')


def next_release(release):
    """The patch after a version's core, which every nightly built on it sorts below."""
    found = re.fullmatch(r'([0-9]+)\.([0-9]+)\.([0-9]+)(?:-[0-9A-Za-z.-]+)?', release)
    require(found is not None, f'{release} is not a release version')
    major, minor, patch = map(int, found.groups())
    return f'{major}.{minor}.{patch + 1}'


def nightly_tags(tags):
    """Each nightly tag's (major, minor, patch, YYYYMMDD, N), in SemVer order, newest last."""
    return sorted(tuple(map(int, found.groups())) for tag in tags if (found := TAG.fullmatch(tag)))


def nightly_version(release, day, tags):
    """The next nightly of `release` on `day` (YYYYMMDD), after every tag already taken."""
    base = next_release(release)
    core = tuple(map(int, base.split('.')))
    taken = [n for *other, date, n in nightly_tags(tags) if tuple(other) == core and date == int(day)]
    version = f'{base}-dev.{day}.{max(taken, default=0) + 1}'
    require(len(version) < NEWT_VERSION_BYTES,
            f'{version} is longer than the image identity holds ({NEWT_VERSION_BYTES - 1} characters)')
    return version


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


def stamp_source(root, commit):
    """The main commit that `commit` stamps, or None unless `commit` is exactly a stamp: one parent on
    origin/main's first-parent history, a nightly of that parent's next release, and no other change."""
    parents = git_output(root, 'rev-list', '--parents', '-n', '1', commit).split()[1:]
    if len(parents) != 1 or parents[0] not in git_output(root, 'rev-list', '--first-parent', DESTINATION).split():
        return None
    source, version = parents[0], committed_release(root, commit)[0]
    try:
        expected = stamped(root, source, version)
        nightly = re.fullmatch(NIGHTLY, version) and version.startswith(next_release(
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
    release_publish.py never deletes one, so a nightly that declined to publish keeps its number taken.
    A failed read raises: guessing "no drafts" would reuse a draft's number and fail its publish."""
    command = ['gh', 'api', '--paginate', f'repos/{repository}/releases', '--jq', '.[] | select(.draft) | .tag_name']
    reply = (answer or (lambda command: subprocess.run(command, capture_output=True)))(command)
    require(reply.returncode == 0, f'{shlex.join(command)} failed: {reply.stderr.decode().strip()}')
    return reply.stdout.decode().split()


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
    for major, minor, patch, day, n in reversed(nightly_tags(tags)):
        tag = f'v{major}.{minor}.{patch}-dev.{day:08d}.{n}'
        if published(tag) is not None:
            return stamp_source(root, resolve_commit(root, 'refs/tags/' + tag))
    return None


def stamp(published, root=ROOT, today=None, force=False, drafts=()):
    """Commits and tags the nightly of HEAD, main's tip; returns its version, or None when main has
    moved on or the image has not changed since the last published nightly. `published(tag)` reads
    GitHub; `drafts` are its draft releases' tags, whose numbers are taken too."""
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
    day = (today or datetime.now(timezone.utc)).strftime('%Y%m%d')
    version = nightly_version(committed_release(root, head)[0], day, [*tags, *drafts])
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
                        drafts=draft_tags(args.repository))
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f'Nightly stopped: {error}', file=sys.stderr)
        return 1
    print(f'version={version or ""}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
