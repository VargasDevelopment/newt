#!/usr/bin/env python3
"""Stamp a nightly test build on a main commit (docs/RELEASING.md, "Nightly test builds").

CI's nightly job runs `stamp` on the main commit whose gate just passed. It
writes the nightly version into the two version files, commits only that
change on top of the main commit, and tags that commit v<version>; no branch
ever carries it. The job then builds the stamped commit like any release, pushes
the tag and publishes the build as a GitHub pre-release with
release_publish.py --prerelease.

The version is <next>-dev.YYYYMMDD.N. <next> is the patch after the main
commit's own release, so the next release outranks every nightly built on it;
YYYYMMDD is the UTC day; N counts that day's nightly tags for <next>. Devices
hold at most 23 characters of a version, so a longer one is refused. A push
that changes nothing the image is built from since the last nightly gets none.

  nightly.py stamp            (CI's nightly job; prints version=VERSION, empty when skipped)
  nightly.py stamp --force    (a manual run: stamp even with no image change)
"""
import argparse
from datetime import datetime, timezone
import re
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
TAG = re.compile(r'v([0-9]+)\.([0-9]+)\.([0-9]+)-dev\.([0-9]{8})\.([1-9][0-9]*)')


def next_release(release):
    """The patch after a stable release, which every nightly built on it sorts below."""
    found = re.fullmatch(r'([0-9]+)\.([0-9]+)\.([0-9]+)', release)
    require(found is not None, f'Nightlies build on a stable release; this commit is {release}')
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


def image_changes(root, since, head):
    """Paths changed from `since` to `head` that the image may be built from."""
    changed = git_output(root, 'diff', '--name-only', since, head).splitlines()
    return [path for path in changed if not path.startswith(NOT_IMAGE) and not ('/' not in path and path.endswith('.md'))]


def last_source(root, tags):
    """The main commit the newest nightly tag stamps, or None."""
    newest = nightly_tags(tags)[-1:]
    if not newest:
        return None
    major, minor, patch, day, n = newest[0]
    return stamp_source(root, resolve_commit(root, f'refs/tags/v{major}.{minor}.{patch}-dev.{day:08d}.{n}'))


def stamp(root=ROOT, today=None, force=False):
    """Commits and tags the nightly of HEAD, main's tip; returns its version, or None when the image
    has not changed since the last nightly."""
    head = resolve_commit(root, 'HEAD')
    live = git_output(root, 'ls-remote', 'origin', 'refs/heads/main').split('\t')[0]
    if live != head:
        print(f'No nightly: main has moved on to {live}; its own run builds the next one', file=sys.stderr)
        return None
    require(resolve_commit(root, DESTINATION) == head, 'origin/main here is not origin\'s main: git fetch origin')
    tags = git_output(root, 'tag', '--list', 'v*-dev.*').split()
    since = last_source(root, tags)
    if not force and since is not None and is_ancestor(root, since, head) and not image_changes(root, since, head):
        print(f'No nightly: nothing the image is built from changed since the last one ({since})', file=sys.stderr)
        return None
    day = (today or datetime.now(timezone.utc)).strftime('%Y%m%d')
    version = nightly_version(committed_release(root, head)[0], day, tags)
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
    commands.add_parser('stamp', help="stamp main's tip").add_argument(
        '--force', action='store_true', help='stamp even when the image has not changed since the last nightly')
    args = parser.parse_args(argv)
    try:
        version = stamp(force=args.force)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f'Nightly stopped: {error}', file=sys.stderr)
        return 1
    print(f'version={version or ""}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
