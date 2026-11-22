"""The docs lane: a pull request that changes only docs skips the toolchain and
the gate's shards (docs/CI.md, "Docs lane").

classify prints lane=docs when every path a pull request changes is in the lane
(in_lane), and lane=full for anything else, including a change set it cannot
work out. check runs what a docs change can break: the release gate as
tools/check.py runs it, the links of the Markdown it touches, and DOCS_CHECKS,
every gate check that reads docs text (tests/docs_lane_test.py keeps that list
complete). Standard library only, so the runner's own python3 can classify.
"""
import argparse
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parents[1]

# Every gate check that reads a lane path, run as the gate runs it.
DOCS_CHECKS = (
    ('tests/repo_docs_allowlist_test.py',),
    ('tools/make-install-wordmark.py', '--check'),  # tests/installer_test.py runs it
)
# Sources that name a lane path or the docs folder but are no check of this
# repository's docs text, and why.
NOT_DOCS_CHECKS = {
    'tools/docs_lane.py': 'the lane itself; its own run checks the links',
    'tests/docs_lane_test.py': "fixture repositories and the lane's own names",
    'tests/public_ci_test.py': "the docs job's name",
    'tests/ci_toolchain_test.py': 'a fixture docs/ folder holding toolchain locks',
    'tests/clean_export_test.py': 'files in a fixture repository',
    'tests/code_health_resources_test.py': 'a file in a fixture checkout',
    'tests/freeink_patch_destination_test.py': "a fixture SDK copy's README",
    'tests/nightly_test.py': 'path names it classifies, never their text',
    'tools/ci_toolchain.py': 'the toolchain locks in docs/, which are not in the lane',
    'tools/nightly.py': 'a path prefix it classifies, never file text',
    'tools/software_gate_shadow.py': 'hashes docs/ as one observed input; no doc text decides a result',
}
# A path's mode after the change: a regular file, or deleted.
LANE_MODES = ('100644', '000000')
SHA = re.compile('[0-9a-f]{40}')
INLINE = re.compile(r'!?\[[^\]]*\]\(\s*<?([^)\s>]+)')
REFERENCE = re.compile(r'^ {0,3}\[[^\]]+\]:\s*<?([^\s>]+)', re.MULTILINE)
HTML = re.compile(r'\b(?:src|href)="([^"]+)"')
CODE = re.compile(r'^(```|~~~).*?^\1|`[^`\n]*`', re.MULTILINE | re.DOTALL)
SCHEME = re.compile(r'[a-z][a-z0-9+.-]*:', re.IGNORECASE)


def in_lane(path):
    """README.md, or a Markdown file or image directly in docs/. CHANGELOG.md
    is release-managed and docs/toolchain-* choose the firmware toolchain, so
    neither is in it."""
    parts = PurePosixPath(path).parts
    return path == 'README.md' or (
        len(parts) == 2 and parts[0] == 'docs' and PurePosixPath(path).suffix in ('.md', '.png', '.jpg')
        and not parts[1].startswith('toolchain-'))


def changes(root, base, head):
    """(mode after, path) for each path changed since head's merge base with
    base. A rename counts as its deletion and its addition."""
    output = subprocess.run(['git', 'diff', '--raw', '-z', '--no-renames', '--no-abbrev', f'{base}...{head}'],
                            cwd=root, check=True, capture_output=True).stdout.decode('utf-8', 'surrogateescape')
    fields = output.split('\0')
    if fields[-1] != '' or len(fields) % 2 == 0:
        raise ValueError('unexpected git diff output')
    result = []
    for meta, path in zip(fields[:-1:2], fields[1::2]):
        status = meta.split()
        if not meta.startswith(':') or len(status) != 5 or not path:
            raise ValueError(f'unexpected git diff record {meta!r}')
        result.append((status[1], path))
    return result


def lane(root, event, base, head):
    """('docs' or 'full', why). Anything not proven docs-only is full."""
    if event != 'pull_request':
        return 'full', f'a {event} run takes the full gate'
    if not (SHA.fullmatch(base) and SHA.fullmatch(head)):
        return 'full', 'the pull request has no exact base and head commits'
    try:
        changed = changes(root, base, head)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        return 'full', f'its changed paths could not be listed: {error}'
    outside = [path for mode, path in changed if not (in_lane(path) and mode in LANE_MODES)]
    if not changed or outside:
        return 'full', f'it changes {outside[0] if outside else "nothing"}, outside the docs lane'
    return 'docs', f'its {len(changed)} changed paths are all in the docs lane'


def links(text):
    """The local targets a Markdown text links to or shows, without anchors;
    code is not a link."""
    text = CODE.sub('', text)
    for pattern in (INLINE, REFERENCE, HTML):
        for match in pattern.finditer(text):
            target = unquote(re.split('[#?]', match[1])[0])
            if target and not SCHEME.match(target):
                yield target


def broken_links(root, touched):
    """Links that name no tracked file or folder, from a touched Markdown file,
    or to a touched path, so a removed or renamed doc breaks no link to it."""
    listed = subprocess.run(['git', 'ls-files', '-z'], cwd=root, check=True, capture_output=True).stdout
    tracked = set(listed.decode('utf-8', 'surrogateescape').split('\0')) - {''}
    folders = {parent.as_posix() for path in tracked for parent in PurePosixPath(path).parents}
    broken = []
    for source in sorted(path for path in tracked if path.endswith('.md')):
        for target in links((root / source).read_text(errors='replace')):
            resolved = os.path.normpath(target[1:] if target.startswith('/')
                                        else PurePosixPath(source).parent / target)
            if (source in touched or resolved in touched) and resolved not in tracked | folders:
                broken.append(f'{source}: {target}')
    return broken


def check(root, base):
    if not SHA.fullmatch(base):
        sys.exit('The docs lane needs the exact base commit')
    touched = {path for _, path in changes(root, base, 'HEAD')}
    broken = broken_links(root, touched)
    if broken:
        sys.exit('FAIL links to no tracked file:\n  ' + '\n  '.join(broken))
    print(f'PASS links of and to the {len(touched)} changed paths', flush=True)
    for script, *arguments in (('tools/release_tools.py',), *DOCS_CHECKS):
        subprocess.run([sys.executable, '-B', str(root / script), *arguments], cwd=root, check=True)
    print('PASS docs lane: release gate, links and every check that reads docs', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    commands = parser.add_subparsers(dest='command', required=True)
    classify = commands.add_parser('classify', help='print lane=docs or lane=full, for $GITHUB_OUTPUT')
    classify.add_argument('--event', required=True)
    classify.add_argument('--base', default='')
    classify.add_argument('--head', default='')
    checks = commands.add_parser('check', help="a docs pull request's checks, on its checked-out head")
    checks.add_argument('--base', required=True)
    args = parser.parse_args()
    if args.command == 'classify':
        # CI runs the base's copy from outside the checkout, so the checkout is the
        # working directory, not this file's parent.
        chosen, why = lane(Path.cwd(), args.event, args.base, args.head)
        print(f'LANE {chosen}: {why}', file=sys.stderr)
        print(f'lane={chosen}')
    else:
        check(ROOT, args.base)


if __name__ == '__main__':
    main()
