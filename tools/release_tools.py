"""Small release gate and source identity helpers; never installs, tags or flashes.

The gate also refuses anything built on an untagged release; tools/release_tag.py
is the one tool that makes release tags. --no-tags is the routine local check:
it reads no tag and checks only the commits this work adds to origin/main. CI
and release validation never pass it. --prepare is the stricter check before
`cz bump`: it starts where Commitizen will read."""
import argparse
import hashlib
import io
import json
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
# A nightly test build (tools/nightly.py): <next>-dev.YYMMDDHHMM (UTC, 2021..2099).
# CI stamps it on its own commit off main, so no branch ever carries it.
NIGHTLY_STAMP = r"(?:2[1-9]|[3-9][0-9])(?:0[1-9]|1[0-2])(?:0[1-9]|[12][0-9]|3[01])(?:[01][0-9]|2[0-3])[0-5][0-9]"
NIGHTLY = CORE + r"-dev\." + NIGHTLY_STAMP
# Legacy nightlies used <next>-dev.YYYYMMDD.N; kept to read historical tags and
# refuse nightly versions on branches.
LEGACY_NIGHTLY = CORE + r"-dev\.[0-9]{8}\.[1-9][0-9]*"
ANY_NIGHTLY = f"(?:{NIGHTLY}|{LEGACY_NIGHTLY})"
VERSION_HEADER = "rust/firmware/native/firmware_version.h"
# Its earlier home: commits before the C++ removal (NEWT-90) bind this path.
HISTORICAL_VERSION_HEADER = "include/newt/firmware_version.h"
# Every release merges here. CI checks it out with full history and tags.
DESTINATION = "refs/remotes/origin/main"
TAG_COMMAND = "python3 -B tools/release_tag.py"
# The start of every merge subject Git and GitHub write ("Merge pull request #N
# from ...", "Merge branch ...").
MERGE_PREFIX = "Merge "
# Newt's Commitizen plugin (.cz.toml `name`), installed into the release venv.
PLUGIN = "tools/cz_newt"
PLUGIN_INSTALL = (".release-venv/bin/python -I -B -m pip --isolated install --no-index --no-deps --no-compile "
                  "--force-reinstall " + PLUGIN)
# Runs in the release venv, so the gate applies the pinned Commitizen's own
# rules, never a copy that can drift: the pattern bump classifies every message
# line with, and the changelog heading its incremental changelog starts after.
COMMITIZEN_RULES = """
import json
from pathlib import Path
from commitizen import factory
from commitizen.changelog_formats import get_changelog_format
from commitizen.config import read_cfg
config = read_cfg()
changelog = config.settings["changelog_file"]
path = Path(config.path.parent, changelog).as_posix()
print(json.dumps({"bump_pattern": factory.committer_factory(config).bump_pattern, "changelog": changelog,
                  "changelog_tag": get_changelog_format(config, path).get_metadata(path).latest_version_tag}))
"""


def header_version(text):
    versions = re.findall(r'^#define NEWT_FIRMWARE_VERSION "([^"]+)"$', text, re.MULTILINE)
    if len(versions) != 1 or not (re.fullmatch(SEMVER, versions[0]) or re.fullmatch(ANY_NIGHTLY, versions[0])):
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


def configured_release(root=ROOT):
    """The checkout's version and release tag name."""
    root = Path(root)
    return declared_release((root / ".cz.toml").read_text(), (root / VERSION_HEADER).read_text())


def check_version(root=ROOT):
    return configured_release(root)[0]


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


def destination_bases(root, head, planned_merge=False):
    """The origin/main commits head builds on.

    Work not yet on origin/main builds on the newest origin/main history it
    contains through any parent, so merging main into a branch counts. A commit
    already on origin/main (main's own CI) builds on its first parent, so a
    release merge needs no tag of its own. A planned merge's first parent is its
    destination, which a stale origin/main must not silently replace."""
    if resolve_commit(root, DESTINATION) is None:
        raise ValueError("The release gate needs origin/main: git fetch origin")
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
    commit (where a bare `cz bump` would put it). Returns those tags' names."""
    bases = destination_bases(root, head, planned_merge)
    mainline = first_parents(root)
    tags = set()
    for base in bases:
        version, tag = committed_release(root, base)
        tagged = resolve_commit(root, "refs/tags/" + tag)
        if tagged is None:
            raise ValueError(f"Release {version} ({base[:12]} on origin/main) has no tag {tag} in this checkout; "
                             "nothing may build on an untagged release. "
                             f"If origin has it, fetch it: git fetch origin tag {tag}. "
                             f"Otherwise tag the release once its exact-source CI passed: {TAG_COMMAND}")
        check_tag(root, tag, tagged, version, mainline)
        tags.add(tag)
    return sorted(tags)


def first_parents(root):
    return set(git_output(root, "rev-list", "--first-parent", DESTINATION).split())


def check_tag(root, tag, tagged, version, mainline):
    """TAG, naming commit TAGGED, must name a commit with VERSION on origin/main's
    first-parent history (MAINLINE)."""
    if committed_release(root, tagged)[0] != version:
        raise ValueError(f"Tag {tag} names {tagged[:12]}, whose version is not {version}")
    if tagged not in mainline:
        raise ValueError(f"Tag {tag} names {tagged[:12]}, which is not on origin/main's first-parent history")


def release_start(root=ROOT, head="HEAD"):
    """The tag `cz bump` starts at; returns its name.

    Bump reads the commits since the configured version's own tag; without
    that tag it reads the whole history as an initial release. The incremental
    changelog adds entries after its latest release heading. So before a bump,
    the tag must exist on origin/main, head must contain it, and the changelog's
    latest heading must name it. A parent release's tag never stands in: just
    after a release merges, main's version is untagged until CI tags it.

    Bump classifies every commit since the tag, so a shallow clone, which may
    lack some of them, picks a wrong version. The changelog lists head's
    first-parent history (tools/cz_newt), so head must branch from main and
    never merge it later: main's merges would sit behind a second parent."""
    complete = git_output(root, "rev-parse", "--is-shallow-repository") == "false"
    if not complete:
        raise ValueError("This clone is shallow, so cz bump would classify only part of the history since the "
                         "release tag; fetch complete history first: git fetch --unshallow origin")
    version, tag = configured_release(root)
    tagged = resolve_commit(root, "refs/tags/" + tag)
    if tagged is None:
        raise ValueError(f"Release {version} has no tag {tag} in this checkout, so cz bump would read the "
                         "whole history. A merged release is tagged once its CI passes; then fetch it: "
                         f"git fetch origin tag {tag}. Otherwise tag it: {TAG_COMMAND}")
    check_tag(root, tag, tagged, version, first_parents(root))
    if not is_ancestor(root, tagged, head):
        raise ValueError(f"{head} does not contain {tag}; prepare a release on a branch from main")
    own_history = set(git_output(root, "rev-list", "--first-parent", head).split())
    if not own_history.issuperset(git_output(root, "merge-base", "--all", DESTINATION, head).split()):
        raise ValueError(f"{head} merged origin/main after it branched, so the changelog, which lists only {head}'s "
                         "first-parent history, would leave out main's changes; prepare the release on a fresh "
                         "branch from main")
    rules = commitizen_rules(root)
    if rules["changelog_tag"] != tag:
        raise ValueError(f"{rules['changelog']}'s latest release heading is {rules['changelog_tag'] or 'missing'}, "
                         f"not {tag}: its incremental changelog would start somewhere other than cz bump")
    return tag


def source_identity(root=ROOT):
    # Match the existing DEV build's dirty scope: generated artifacts aren't source.
    return {"firmware_version": firmware_version(root),
            "git_revision": git_output(root, "rev-parse", "HEAD"),
            "dirty": bool(git_output(root, "status", "--porcelain", "--", ".", ":(exclude)artifacts"))}


def release_python(root=ROOT):
    interpreter = Path(root) / ".release-venv/bin/python"
    if not interpreter.is_file():
        raise ValueError("Missing project release venv; follow docs/RELEASING.md")
    probe = ("import hashlib, importlib.util; from importlib.metadata import version; "
             "plugin = importlib.util.find_spec('cz_newt'); "
             "print(version('commitizen'), plugin and hashlib.sha256(open(plugin.origin, 'rb').read()).hexdigest())")
    actual, plugin = subprocess.run([str(interpreter), "-I", "-B", "-c", probe],
                                    check=True, capture_output=True, text=True).stdout.split()
    pin = re.findall(r"^commitizen==(.+)$", (Path(root) / "requirements-release.txt").read_text(), re.MULTILINE)
    if pin != [actual]:
        raise ValueError("Installed Commitizen differs from requirements-release.txt")
    if plugin != hashlib.sha256((Path(root) / PLUGIN / "cz_newt.py").read_bytes()).hexdigest():
        raise ValueError(f"The release venv's Commitizen plugin is missing or differs from {PLUGIN}; "
                         f"install it: {PLUGIN_INSTALL}")
    return interpreter


def commitizen_rules(root=ROOT):
    """COMMITIZEN_RULES, as the pinned Commitizen reads the checkout's config."""
    return json.loads(subprocess.run([str(release_python(root)), "-I", "-B", "-c", COMMITIZEN_RULES], cwd=root,
                                     check=True, capture_output=True, text=True).stdout)


def check_commits(root, ranges):
    """Every commit in the ranges except a merge is a Conventional Commit.

    Commitizen classifies each change by its own commit. A merge only joins
    commits that are already classified, so it may keep Git's or GitHub's merge
    subject; a commit with that subject that is not a merge would go unclassified.
    Bump classifies every line of every message, so such a merge's body is empty
    or plain text: no line there may match Commitizen's bump pattern."""
    bump_line = re.compile(commitizen_rules(root)["bump_pattern"])
    for rev_range in ranges:
        if not git_output(root, "rev-list", rev_range):
            continue
        unmerged = [line for line in git_output(root, "log", "--no-merges", "--format=%h %s", rev_range).splitlines()
                    if line.partition(" ")[2].startswith(MERGE_PREFIX)]
        if unmerged:
            raise ValueError("Only a merge may skip Conventional Commits with a merge subject: " + "; ".join(unmerged))
        classified = []
        for record in filter(None, git_output(root, "log", "--merges", "-z", "--format=%h %s%n%b", rev_range).split("\0")):
            title, _, body = record.partition("\n")
            commit, _, subject = title.partition(" ")
            if subject.startswith(MERGE_PREFIX):
                classified += [f"{commit} {line}" for line in body.splitlines() if bump_line.search(line)]
        if classified:
            raise ValueError("A merge with a merge subject keeps an empty or plain-text body; cz bump would "
                             "classify these lines: " + "; ".join(classified))
        subprocess.run([str(release_python(root)), "-I", "-B", "-m", "commitizen", "check",
                        "--rev-range", rev_range, "--allowed-prefixes", MERGE_PREFIX], cwd=root, check=True)


def check_title(root, title):
    """A pull request's title is a Conventional Commit: GitHub's merge commit
    takes it as its subject (with " (#N)" added), and no merge subject is typed."""
    subprocess.run([str(release_python(root)), "-I", "-B", "-m", "commitizen", "check", "--message", title],
                   cwd=root, check=True)


def gate(root=ROOT, rev_range=None, tags=True, prepare=False):
    """The release gate; returns the release tags head builds on, or None without tags.

    A review range's end is the work it guards; if that end is a merge, it is a
    planned merge onto origin/main. Commit messages are checked where versioning
    reads them: Commitizen classifies the commits since the release tag, so that
    is the default range. Without tags, the range is what head adds to origin/main,
    whose own commits CI checked when they landed. Release preparation
    (`prepare`, never with a range or without tags) checks from release_start,
    where cz bump reads, and returns that tag."""
    if re.fullmatch(ANY_NIGHTLY, check_version(root)):
        raise ValueError("A nightly version is CI's stamp on its own commit; never commit it to a branch")
    head = (rev_range or "").rpartition("..")[2] or "HEAD"
    built_on = check_release_tagged(root, head, rev_range is not None) if tags else None
    if prepare:
        built_on = [release_start(root, head)]
        ranges = [f"refs/tags/{built_on[0]}..{head}"]
    elif rev_range:
        ranges = [rev_range]
    elif tags:
        ranges = [f"refs/tags/{tag}..{head}" for tag in built_on]
    else:
        ranges = [f"{base}..{head}" for base in destination_bases(root, head)]
    release_python(root)
    check_commits(root, ranges)
    return built_on


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--rev-range", help="an explicit review range; by default, since the release tag")
    mode.add_argument("--no-tags", action="store_true",
                      help="routine local check: no release tags; CI and release validation never pass it")
    mode.add_argument("--prepare", action="store_true",
                      help="before cz bump: start at the configured version's own tag and changelog heading")
    mode.add_argument("--pr-title", help="check only this pull request title, which becomes its merge subject")
    args = parser.parse_args()
    if args.pr_title is not None:
        check_title(ROOT, args.pr_title)
        print("PASS: the pull request title, its merge subject, is a Conventional Commit")
        return
    tags = gate(rev_range=args.rev_range, tags=not args.no_tags, prepare=args.prepare)
    if args.prepare:
        built_on = "release preparation starts at tagged release " + tags[0]
    elif tags is None:
        built_on = "release tags left to CI and release validation"
    elif tags:
        built_on = "built on tagged release " + ", ".join(tags)
    else:
        built_on = "the snapshot root builds on no earlier release"
    print(f"PASS: firmware SemVer consistency, {built_on}, pinned Commitizen and Conventional Commits")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(f"Release gate stopped: {error}", file=sys.stderr)
        sys.exit(1)
