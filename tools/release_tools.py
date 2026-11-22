"""Small release gate and source identity helpers; never installs, tags or flashes.

The gate also refuses anything built on an untagged release; tools/release_tag.py
is the one tool that makes release tags."""
import argparse
import hashlib
import io
from pathlib import Path
import re
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
ADOPTION_BASE = "fcca87ad1cb792a3ca2bfafb7c7e5bae3ff14b0d"
# A repository started from tools/clean_export.py has no ADOPTION_BASE. Its one
# root commit carries this trailer instead: the exported revision and the digest
# of the exported tree, which that root's own tree must reproduce.
SNAPSHOT_TRAILER = "Newt-Snapshot"
SNAPSHOT_VALUE = re.compile(r"([0-9a-f]{40}) sha256:([0-9a-f]{64})")
FILE_MODES = ("100644", "100755")
# The supported firmware release vocabulary; SemVer build metadata stays in receipts.
CORE = r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
SEMVER = CORE + r"(?:-(?:alpha|beta|rc)\.(?:0|[1-9][0-9]*))?"
# A nightly test build (tools/nightly.py): <next>-dev.YYYYMMDD.N. CI stamps it on
# its own commit off main, so no branch ever carries it.
NIGHTLY = CORE + r"-dev\.[0-9]{8}\.[1-9][0-9]*"
VERSION_HEADER = "rust/firmware/native/firmware_version.h"
# Its earlier home: commits before the C++ removal (NEWT-90) bind this path.
HISTORICAL_VERSION_HEADER = "include/newt/firmware_version.h"
# Every release merges here. CI checks it out with full history and tags.
DESTINATION = "refs/remotes/origin/main"
TAG_COMMAND = "python3 -B tools/release_tag.py"


def header_version(text):
    versions = re.findall(r'^#define NEWT_FIRMWARE_VERSION "([^"]+)"$', text, re.MULTILINE)
    if len(versions) != 1 or not (re.fullmatch(SEMVER, versions[0]) or re.fullmatch(NIGHTLY, versions[0])):
        raise ValueError("Invalid or ambiguous firmware SemVer")
    return versions[0]


def firmware_version(root=ROOT):
    return header_version((Path(root) / VERSION_HEADER).read_text())


def declared_release(config_text, header_text, header=VERSION_HEADER):
    """One source's version and release tag name, once its Commitizen config and
    About header (at the path that source binds) agree."""
    version = header_version(header_text)
    settings = tomllib.loads(config_text)["tool"]["commitizen"]
    if settings["version"] != version:
        raise ValueError("Firmware version drift between Commitizen and About header")
    if settings["version_scheme"] != "semver2" or settings["version_files"] != [header + ":NEWT_FIRMWARE_VERSION"]:
        raise ValueError("Firmware version binding differs from the reviewed SemVer configuration")
    if "$version" not in settings["tag_format"]:
        raise ValueError("Release tag_format must name $version")
    return version, settings["tag_format"].replace("$version", version)


def check_version(root=ROOT):
    root = Path(root)
    return declared_release((root / ".cz.toml").read_text(), (root / VERSION_HEADER).read_text())[0]


def git_output(root, *arguments):
    return subprocess.run(["git", *arguments], cwd=root, check=True,
                          capture_output=True, text=True).stdout.strip()


def committed_release(root, commit):
    """A commit's own version and tag name, read from that commit, not the checkout."""
    config = git_output(root, "show", f"{commit}:.cz.toml")
    files = tomllib.loads(config)["tool"]["commitizen"].get("version_files")
    header = next((path for path in (VERSION_HEADER, HISTORICAL_VERSION_HEADER)
                   if files == [path + ":NEWT_FIRMWARE_VERSION"]), None)
    if header is None:
        raise ValueError("Firmware version binding differs from the reviewed SemVer configuration")
    return declared_release(config, git_output(root, "show", f"{commit}:{header}"), header)


def resolve_commit(root, ref):
    """The commit a ref names, or None when this checkout lacks the ref."""
    result = subprocess.run(["git", "rev-parse", "-q", "--verify", ref + "^{commit}"],
                            cwd=root, capture_output=True, text=True)
    return result.stdout.strip() if result.returncode == 0 else None


def is_ancestor(root, commit, descendant):
    code = subprocess.run(["git", "merge-base", "--is-ancestor", commit, descendant], cwd=root,
                          capture_output=True).returncode
    if code not in (0, 1):
        raise ValueError(f"Cannot compare {commit} with {descendant}")
    return code == 0


def tree_entries(root, rev):
    """(mode, kind, object, path) for every tracked entry at REV, in path byte order."""
    listing = subprocess.run(["git", "ls-tree", "-r", "-z", "--full-tree", rev], cwd=root,
                             check=True, capture_output=True).stdout
    rows = []
    for record in filter(None, listing.split(b"\0")):
        meta, path = record.split(b"\t", 1)
        mode, kind, obj = meta.decode().split()
        rows.append((mode, kind, obj, path.decode("utf-8", "surrogateescape")))
    return sorted(rows, key=lambda row: row[3].encode("utf-8", "surrogateescape"))


def read_blobs(root, objects):
    batch = subprocess.run(["git", "cat-file", "--batch"], cwd=root, check=True, capture_output=True,
                           input="".join(f"{obj}\n" for obj in objects).encode()).stdout
    stream, blobs = io.BytesIO(batch), {}
    for obj in objects:
        name, _kind, size = stream.readline().split()
        blobs[name.decode()] = stream.read(int(size))
        stream.read(1)
    return blobs


def tree_digest(files, gitlinks):
    """A clean export's identity: (mode, path, bytes) files, then (path, object)
    submodule pins, each in path byte order."""
    digest = hashlib.sha256()
    for mode, path, data in files:
        digest.update(f"{mode} {path}\0{hashlib.sha256(data).hexdigest()}\n".encode("utf-8", "surrogateescape"))
    for path, obj in gitlinks:
        digest.update(f"160000 {path}\0{obj}\n".encode("utf-8", "surrogateescape"))
    return digest.hexdigest()


def snapshot_digest(root, rev):
    """tree_digest of every entry at REV, or None when REV holds an entry no export writes."""
    entries = tree_entries(root, rev)
    if any(kind != "commit" and mode not in FILE_MODES for mode, kind, _obj, _path in entries):
        return None
    blobs = read_blobs(root, sorted({obj for _mode, kind, obj, _path in entries if kind == "blob"}))
    return tree_digest([(mode, path, blobs[obj]) for mode, kind, obj, path in entries if kind == "blob"],
                       [(path, obj) for _mode, kind, obj, path in entries if kind == "commit"])


def snapshot_root(root):
    """The root commit of a history started from a clean export, else None.

    Only a complete history counts whose single root carries exactly one
    trailer in the form clean_export.py prints, and whose own tree reproduces
    that trailer's digest. A shallow clone, a malformed or repeated trailer, a
    trailer on any later commit, or a digest of a different tree does not."""
    if git_output(root, "rev-parse", "--is-shallow-repository") != "false":
        return None
    roots = git_output(root, "rev-list", "--max-parents=0", "HEAD").split()
    if len(roots) != 1:
        return None
    values = git_output(root, "log", "-1", f"--format=%(trailers:key={SNAPSHOT_TRAILER},valueonly)",
                        roots[0]).splitlines()
    match = SNAPSHOT_VALUE.fullmatch(values[0]) if len(values) == 1 else None
    if match is None or snapshot_digest(root, roots[0]) != match[2]:
        return None
    return roots[0]


def adoption_base(root=ROOT):
    """The commit after which every commit follows the release rules."""
    if resolve_commit(root, ADOPTION_BASE):
        return ADOPTION_BASE
    snapshot = snapshot_root(root)
    if snapshot is None:
        raise ValueError(f"History lacks the adoption commit {ADOPTION_BASE[:12]} and has no "
                         f"{SNAPSHOT_TRAILER} root: fetch the complete history")
    return snapshot


def destination_bases(root, head, planned_merge=False):
    """The origin/main commits head builds on.

    Work not yet on origin/main builds on the newest origin/main history it
    contains through any parent, so merging main into a branch counts. A commit
    already on origin/main (main's own CI) builds on its first parent, so a
    release merge needs no tag of its own. A planned merge's first parent is its
    destination, which a stale origin/main must not silently replace."""
    if resolve_commit(root, DESTINATION) is None:
        raise ValueError("The release-tag guard needs origin/main and its tags: git fetch origin")
    parents = git_output(root, "rev-list", "--parents", "-n", "1", head).split()[1:]
    if not parents:
        # A clean export's root is where this history starts, not a release built on.
        if resolve_commit(root, ADOPTION_BASE) is None and snapshot_root(root) == resolve_commit(root, head):
            return []
        raise ValueError(f"{head} has no parent, so it builds on no release")
    if planned_merge and len(parents) > 1 and not is_ancestor(root, parents[0], DESTINATION):
        raise ValueError(f"origin/main is behind the planned merge's destination {parents[0][:12]}: git fetch origin")
    if is_ancestor(root, head, DESTINATION):
        return [parents[0]]
    # Criss-cross history can have several bases; each is a release head builds on.
    bases = subprocess.run(["git", "merge-base", "--all", head, DESTINATION], cwd=root,
                           capture_output=True, text=True).stdout.split()
    if not bases:
        raise ValueError(f"{head} shares no history with origin/main")
    return bases


def check_release_tagged(root=ROOT, head="HEAD", planned_merge=False):
    """Refuse anything built or merged on top of an untagged release.

    Each release is the version at one of head's destination bases. Head's own
    version is not checked, so a release branch and its release merge pass before
    the tag can exist. The tag must name a commit with that version on
    origin/main's first-parent history: a reviewed merge, never a merged branch's
    commit (where a bare `cz bump` would put it)."""
    bases = destination_bases(root, head, planned_merge)
    mainline = set(git_output(root, "rev-list", "--first-parent", DESTINATION).split())
    tags = set()
    for base in bases:
        version, tag = committed_release(root, base)
        tagged = resolve_commit(root, "refs/tags/" + tag)
        if tagged is None:
            raise ValueError(f"Release {version} ({base[:12]} on origin/main) has no tag {tag} in this checkout; "
                             "nothing may build on an untagged release. "
                             f"If origin has it, fetch it: git fetch origin tag {tag}. "
                             f"Otherwise tag the release once its exact-source CI passed: {TAG_COMMAND}")
        if committed_release(root, tagged)[0] != version:
            raise ValueError(f"Tag {tag} names {tagged[:12]}, whose version is not {version}")
        if tagged not in mainline:
            raise ValueError(f"Tag {tag} names {tagged[:12]}, which is not on origin/main's first-parent history")
        tags.add(tag)
    return ", ".join(sorted(tags))


def source_identity(root=ROOT):
    # Match the existing DEV build's dirty scope: generated artifacts aren't source.
    return {"firmware_version": firmware_version(root),
            "git_revision": git_output(root, "rev-parse", "HEAD"),
            "dirty": bool(git_output(root, "status", "--porcelain", "--", ".", ":(exclude)artifacts"))}


def release_python(root=ROOT):
    interpreter = Path(root) / ".release-venv/bin/python"
    if not interpreter.is_file():
        raise ValueError("Missing project release venv; follow docs/RELEASING.md")
    probe = "from importlib.metadata import version; print(version('commitizen'))"
    actual = subprocess.run([str(interpreter), "-I", "-B", "-c", probe],
                            check=True, capture_output=True, text=True).stdout.strip()
    pin = re.findall(r"^commitizen==(.+)$", (Path(root) / "requirements-release.txt").read_text(), re.MULTILINE)
    if pin != [actual]:
        raise ValueError("Installed Commitizen differs from requirements-release.txt")
    return interpreter


def check_commits(root=ROOT, rev_range=None):
    # Validate new commits only. Legacy candidate history is deliberately exempt.
    if rev_range is None:
        base = adoption_base(root)
        subprocess.run(["git", "merge-base", "--is-ancestor", base, "HEAD"], cwd=root, check=True)
        rev_range = base + "..HEAD"
    if not git_output(root, "rev-list", rev_range):
        return
    subprocess.run([str(release_python(root)), "-I", "-B", "-m", "commitizen", "check",
                    "--rev-range", rev_range], cwd=root, check=True)


def gate(root=ROOT, rev_range=None):
    """The release gate. A review range's end is the work it guards; if that end
    is a merge, it is a planned merge onto origin/main."""
    if re.fullmatch(NIGHTLY, check_version(root)):
        raise ValueError("A nightly version is CI's stamp on its own commit; never commit it to a branch")
    tag = check_release_tagged(root, (rev_range or "").rpartition("..")[2] or "HEAD", rev_range is not None)
    release_python(root)
    check_commits(root, rev_range)
    return tag


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rev-range", help="CI base..HEAD range; full adoption history by default")
    args = parser.parse_args()
    tag = gate(rev_range=args.rev_range)
    built_on = f"built on tagged release {tag}" if tag else "the snapshot root builds on no earlier release"
    print(f"PASS: firmware SemVer consistency, {built_on}, pinned Commitizen and Conventional Commits")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(f"Release gate stopped: {error}", file=sys.stderr)
        sys.exit(1)
