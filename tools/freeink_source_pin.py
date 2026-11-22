"""Side-effect-free authority for the official FreeInk checkout."""
import hashlib
from pathlib import Path
import subprocess
import tempfile

SDK_PIN = 'cd6f5b831c8fad899723b6d406586f00c575f021'
SDK_ORIGIN = 'https://github.com/Free-Ink/freeink-sdk.git'


def git_environment():
    return {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C', 'TMPDIR': tempfile.gettempdir(),
            'GIT_NO_REPLACE_OBJECTS': '1'}


def source_git(source: Path, *arguments, text=False):
    """One replacement-proof Git read boundary for the FreeInk checkout."""
    return subprocess.run(['git', '-C', str(source), *arguments], check=True, capture_output=True,
                          text=text, env=git_environment()).stdout


def reject_replace_refs(source: Path) -> None:
    refs = source_git(source, 'for-each-ref', '--format=%(refname)', 'refs/replace', text=True)
    if refs.strip():
        raise RuntimeError('FreeInk checkout contains replacement object refs')


def git_executable(mode: int) -> bool:
    """Git records only the owner execute bit in regular-file modes."""
    return bool(mode & 0o100)


def verify_source_origin(source: Path) -> None:
    """Bind both the literal local origin and Git's effective transport URL."""
    expected = SDK_ORIGIN.encode('utf-8') + b'\n'
    raw = source_git(source, 'config', '--local', '--no-includes', '--get-all',
                     'remote.origin.url')
    if raw != expected:
        raise RuntimeError('FreeInk source raw local origin differs from official repository')
    effective = source_git(source, 'remote', 'get-url', '--all', 'origin')
    if effective != expected:
        raise RuntimeError('FreeInk source effective origin differs from official repository')


def verify_source_pin(source: Path) -> str:
    """Establish the actual checkout, including injected untracked inputs."""
    if source.is_symlink() or not (source / '.git').exists():
        raise RuntimeError('Missing real pinned FreeInk checkout')
    reject_replace_refs(source)

    def git(*arguments):
        return source_git(source, *arguments, text=True).strip()
    if git('rev-parse', 'HEAD') != SDK_PIN:
        raise RuntimeError('FreeInk source commit differs from reviewed pin')
    verify_source_origin(source)
    if git('status', '--porcelain', '--untracked-files=all', '--ignored'):
        raise RuntimeError('FreeInk checkout contains modified or additional inputs')
    verify_committed_bytes(source)
    return SDK_PIN


def verify_committed_bytes(source: Path) -> None:
    """Compare actual tracked bytes to HEAD, bypassing mutable index flags."""
    listing = source_git(source, 'ls-tree', '-r', '--full-tree', '-z', 'HEAD')
    for entry in filter(None, listing.split(b'\0')):
        metadata, relative = entry.split(b'\t', 1)
        mode, kind, expected = metadata.split()
        if kind == b'commit':
            continue
        path = source / relative.decode('utf-8')
        if mode not in (b'100644', b'100755') or path.is_symlink() or not path.is_file():
            raise RuntimeError('FreeInk tracked input has an unsupported file type')
        if git_executable(path.stat().st_mode) != (mode == b'100755'):
            raise RuntimeError('FreeInk tracked executable mode differs from the reviewed commit')
        digestor = hashlib.sha1(b'blob ' + str(path.stat().st_size).encode() + b'\0')
        with path.open('rb') as contents:
            for chunk in iter(lambda: contents.read(1024 * 1024), b''):
                digestor.update(chunk)
        if digestor.hexdigest().encode() != expected:
            raise RuntimeError('FreeInk tracked bytes differ from the reviewed commit')
