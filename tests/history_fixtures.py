"""Historic sources the tests compare against, stored once by content.

Every file was copied from a named commit of Newt's private development
history, which the public repository does not carry. The tests read them only
here, never through git, so they behave the same in either repository.
fixtures/history/manifest.tsv maps commit + path to the sha256 of the bytes,
stored as fixtures/history/blobs/<sha256> without a source extension so no
source glob, build or code-health scan treats them as live code.
"""
from functools import cache
from hashlib import sha256
from pathlib import Path
import re

FIXTURES = Path(__file__).resolve().parent / "fixtures/history"
MANIFEST = FIXTURES / "manifest.tsv"
BLOBS = FIXTURES / "blobs"
# The reviewed manifest. Every read checks it, so a dropped or repointed row
# fails loudly instead of reading as a file the old commit lacked. Any manifest
# edit must move this pin in the same commit.
MANIFEST_SHA256 = "cfea65bf58d54226d22a13d247add9cefa57b6fef3d06ac78efdfaae60d20fd8"


class HistoryFixtureError(Exception):
    """A historic source is missing from the manifest or its stored bytes changed."""


@cache
def manifest() -> dict[tuple[str, str], str]:
    """(full commit, path) -> sha256 of the stored bytes."""
    raw = MANIFEST.read_bytes()
    if sha256(raw).hexdigest() != MANIFEST_SHA256:
        raise HistoryFixtureError(f"{MANIFEST.name} is not the reviewed manifest (MANIFEST_SHA256)")
    entries: dict[tuple[str, str], str] = {}
    for number, line in enumerate(raw.decode().splitlines(), 1):
        if line.startswith("#"):
            continue
        commit, path, digest, _tests = line.split("\t")
        if not re.fullmatch(r"[0-9a-f]{40}", commit) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise HistoryFixtureError(f"{MANIFEST.name}:{number}: malformed commit or sha256")
        if (commit, path) in entries:
            raise HistoryFixtureError(f"{MANIFEST.name}:{number}: repeats {commit[:9]}:{path}")
        entries[commit, path] = digest
    return entries


def commit_of(revision: str) -> str:
    """The one manifest commit that `revision` abbreviates."""
    matches = {commit for commit, _path in manifest() if commit.startswith(revision)}
    if len(matches) != 1:
        raise HistoryFixtureError(f"{revision} names {len(matches)} fixture commits, not one")
    return matches.pop()


def blob(digest: str) -> Path:
    """The stored file for `digest`, refused if it is a link or leaves the fixture root."""
    path = BLOBS / digest
    if path.is_symlink() or path.resolve() != FIXTURES.resolve() / "blobs" / digest:
        raise HistoryFixtureError(f"blobs/{digest} is a link or leaves the fixture root")
    return path


def read(revision: str, path: str) -> bytes:
    """`git show revision:path`, from the stored copy, verified against its digest."""
    commit = commit_of(revision)
    digest = manifest().get((commit, path))
    if digest is None:
        raise HistoryFixtureError(f"no fixture for {commit[:9]}:{path}")
    data = blob(digest).read_bytes()
    if sha256(data).hexdigest() != digest:
        raise HistoryFixtureError(f"blobs/{digest} no longer matches its sha256 ({commit[:9]}:{path})")
    return data


def text(revision: str, path: str) -> str:
    return read(revision, path).decode()
