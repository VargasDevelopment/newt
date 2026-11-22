#!/usr/bin/env python3
"""Export a clean snapshot of one revision and fail on any private-data leak.

`--rev REV --out DIR` writes exactly the tracked files at REV, minus the
committed omit list, prints one digest of the exported tree, the trailer a
fresh repository's root commit carries (docs/RELEASING.md) and the pinned
submodules it must restore, then scans every exported path and byte and every
archive member's name and content. `--commit "NAME <EMAIL>"` then makes OUT that
fresh repository: one root commit of exactly the export, with the trailer. Content it cannot read in full (encrypted,
corrupt, trailing data, nested past MAX_DEPTH or inflating past MAX_INFLATED)
fails the scan rather than passing unread.
The public repository starts from this snapshot without history, so the scan
guards the repository itself: allowlist entries are exact (file + pattern +
the matched value's digest + reason), never a directory, a whole pattern or a
whole file's matches, and a stale entry fails. The external terms file carries
its own exceptions (`allow<TAB>PATH<TAB>TERM`: that listed term, in that file
only), so the repository never learns which terms are scanned; they go stale
the same way.

Output names file:line and a pattern id (or "term #N") only, never the matched
text, so a run never copies a private term into a log. A path, archive member
or submodule whose own name is flagged is printed only by its position, and a
submodule URL never with its userinfo. Patterns are built at runtime so neither
this tool nor its tests contain a string they flag.
"""
import sys

sys.dont_write_bytecode = True

import argparse
from dataclasses import dataclass
import hashlib
import io
from pathlib import Path, PurePosixPath
import re
import subprocess
import zipfile
import zlib

from release_tools import FILE_MODES, SNAPSHOT_TRAILER, read_blobs, snapshot_root, tree_digest, tree_entries

ROOT = Path(__file__).resolve().parents[1]
OMIT_LIST = "tools/clean-export-omit.txt"
ALLOW_LIST = "tools/clean-export-allow.txt"
EMAIL_LIST = "tools/clean-export-emails.txt"
# Nested containers are opened at most this deep and this large, so a hostile
# archive cannot exhaust the host; content past either limit fails the scan.
MAX_DEPTH = 3
MAX_INFLATED = 256 << 20
WORD = rb"A-Za-z0-9_"


def _rx(*parts, flags=0):
    return re.compile(b"".join(parts), flags)


SEP = rb"[\\/]+"
USER = rb"[A-Za-z0-9_][A-Za-z0-9._-]*"
OCTETS = rb"(?:\.(?:25[0-5]|2[0-4][0-9]|1?[0-9]{1,2}))"
PATTERNS = {
    # /home/<user>/, /Users/<user>/ and C:\Users\<user>\ (JSON doubles the slashes).
    "home-path": _rx(rb"(?<![" + WORD + rb"])(?:/" + b"home" + rb"/|/" + b"Users" + rb"/|[A-Za-z]:" + SEP
                     + b"Users" + SEP + rb")" + USER + SEP),
    "email": _rx(rb"(?<![" + WORD + rb".%+-])([" + WORD + rb".%+-]+)" + b"@"
                 + rb"((?:[A-Za-z0-9-]+\.)+[A-Za-z]{2,})(?![" + WORD + rb"-])"),
    "mac": _rx(rb"(?<![0-9A-Fa-f:-])[0-9A-Fa-f]{2}([:-])(?:[0-9A-Fa-f]{2}\1){4}[0-9A-Fa-f]{2}(?![0-9A-Fa-f:-])"),
    # A concrete serial in a udev match; placeholders (<...>, $VAR, globs, code) are generic.
    "udev-serial": _rx(rb"ATTRS?\{" + b"serial" + rb"\}\s*==\s*\"([^\"<>$*?%{}'+ ]+)\""),
    "private-ipv4": _rx(rb"(?<![0-9.])(?:10" + OCTETS * 3 + rb"|(?:192\.168|172\.(?:1[6-9]|2[0-9]|3[01])"
                        rb"|100\.(?:6[4-9]|[7-9][0-9]|1[01][0-9]|12[0-7]))" + OCTETS * 2 + rb")(?![0-9]|\.[0-9])"),
    "local-host": _rx(rb"(?<![" + WORD + rb".-])[A-Za-z0-9][A-Za-z0-9-]*(?:\.[A-Za-z0-9-]+)*\.(?:"
                      + b"local" + rb"|ts\.net)(?![" + WORD + rb"([-]|\.[" + WORD + rb"])"),
    "secret": _rx(b"-" * 5 + rb"BEGIN [A-Z ]*" + b"PRIVATE" + rb" KEY" + b"-" * 5
                  + rb"|(?<![" + WORD + rb"])(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{40,}"
                  rb"|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{35}|xox[abprs]-[A-Za-z0-9-]{10,}|sk-(?:ant-)?[A-Za-z0-9_-]{32,})"),
}
# RFC 2606/6761 reserved names can never name a real mailbox or host.
RESERVED_DOMAIN = re.compile(rb"(?:^|\.)(?:example\.(?:com|net|org)|example|invalid|test|localhost)$", re.I)


@dataclass(frozen=True)
class Hit:
    where: str
    line: int | None  # 0 is the name itself; None is content the scan could not read
    rule: str

    def __str__(self):
        return f"{self.where}: {self.rule}" if self.line is None else f"{self.where}:{self.line}: {self.rule}"


def git(repo, *args, data=None):
    return subprocess.run(["git", "-C", str(repo), *args], input=data, capture_output=True, check=True).stdout


def list_rows(text, fields, label):
    """Tab-separated committed rows; '#' comments; every row ends with a reason."""
    rows = []
    for number, line in enumerate(text.splitlines(), 1):
        if not line.strip() or line.startswith("#"):
            continue
        cells = line.split("\t")
        if len(cells) != fields or not all(cell.strip() for cell in cells):
            raise ValueError(f"{label}:{number}: expected {fields} tab-separated fields ending in a reason")
        rows.append((number, *cells[:-1]))
    return rows


def committed_lists(repo, rev, entries):
    by_path = {row[3]: row[2] for row in entries}
    names = [name for name in (OMIT_LIST, ALLOW_LIST, EMAIL_LIST) if name in by_path]
    blobs = read_blobs(repo, [by_path[name] for name in names])
    text = {name: blobs[by_path[name]].decode() for name in names}
    omit = list_rows(text.get(OMIT_LIST, ""), 2, OMIT_LIST)
    allow = list_rows(text.get(ALLOW_LIST, ""), 4, ALLOW_LIST)
    emails = list_rows(text.get(EMAIL_LIST, ""), 2, EMAIL_LIST)
    for number, _path, rule, value in allow:
        if rule not in PATTERNS:
            raise ValueError(f"{ALLOW_LIST}:{number}: unknown pattern {rule}")
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", value):
            raise ValueError(f"{ALLOW_LIST}:{number}: the value must be sha256:<hex> of the exact match")
    allow = [(f"{ALLOW_LIST}:{number}", *entry) for number, *entry in allow]
    return omit, allow, {address.lower().encode() for _number, address in emails}


def omitted(path, omit, used):
    for number, entry in omit:
        if path == entry or (entry.endswith("/") and path.startswith(entry)):
            used.add(number)
            return True
    return False


def export(repo, rev, out):
    """Write the tracked files at REV into OUT; return (files, submodules, digest, lists, index), where
    index is each exported entry as (mode, object, path)."""
    entries = tree_entries(repo, rev)
    omit, allow, emails = committed_lists(repo, rev, entries)
    used, files, submodules = set(), [], []
    for mode, kind, obj, path in entries:
        if omitted(path, omit, used):
            continue
        if kind == "commit":
            submodules.append((path, obj))
        elif mode in FILE_MODES:
            files.append((mode, obj, path))
        else:
            raise ValueError(f"tree entry #{len(files) + len(submodules) + 1}: unsupported mode {mode}")
    stale = [number for number, _entry in omit if number not in used]
    if stale:
        raise ValueError(f"{OMIT_LIST}: entries match nothing at {rev}: lines {stale}")
    blobs = read_blobs(repo, sorted({obj for _mode, obj, _path in files}))
    out.mkdir(parents=True, exist_ok=True)
    if any(out.iterdir()):
        raise ValueError(f"{out}: output directory must be empty")
    exported = []
    for mode, obj, path in files:
        target = out / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(blobs[obj])
        target.chmod(0o755 if mode == "100755" else 0o644)
        exported.append((path, blobs[obj]))
    urls = submodule_urls(out / ".gitmodules")
    pinned = []
    for number, (path, obj) in enumerate(submodules, 1):
        url = urls.get(path)
        if url is None:
            raise ValueError(f"submodule #{number}: pinned without a URL in .gitmodules")
        pinned.append((path, url, obj))
    digest = tree_digest([(mode, path, blobs[obj]) for mode, obj, path in files], submodules)
    return exported, pinned, digest, (allow, emails), files + [("160000", obj, path) for path, obj in submodules]


def commit_export(out, index, trailer, author):
    """Make OUT a repository whose one root commit, by AUTHOR, holds exactly INDEX, with TRAILER.

    The index is written entry by entry, never by `git add`, which skips every
    tracked path the exported .gitignore matches and cannot add a submodule pin.
    The release gate's own check then confirms the root reproduces the digest."""
    named = re.fullmatch(r"([^<>]+?) <([^<>\s]+)>", author)
    if named is None:
        raise ValueError("--commit needs an author as NAME <EMAIL>")
    git(out, "init", "-q", "-b", "main")
    files = [path for mode, _obj, path in index if mode != "160000"]
    git(out, "hash-object", "-w", "--no-filters", "--stdin-paths", data="".join(f"{path}\n" for path in files).encode())
    for path in {path for mode, _obj, path in index if mode == "160000"}:
        (out / path).mkdir(parents=True, exist_ok=True)
    git(out, "update-index", "-z", "--index-info",
        data=b"".join(f"{mode} {obj}\t{path}\0".encode("utf-8", "surrogateescape") for mode, obj, path in index))
    tree = git(out, "write-tree").decode().strip()
    commit = git(out, "-c", f"user.name={named[1]}", "-c", f"user.email={named[2]}", "commit-tree", tree,
                 "-m", "chore: start from a clean export", "-m", trailer).decode().strip()
    git(out, "update-ref", "refs/heads/main", commit)
    if snapshot_root(out) != commit:
        raise ValueError("the root commit does not reproduce the export's digest")
    return commit


def submodule_urls(gitmodules):
    if not gitmodules.exists():
        return {}
    paths, urls, section = {}, {}, None
    for raw in gitmodules.read_text().splitlines():
        line = raw.strip()
        if line.startswith("[submodule"):
            section = line
        elif "=" in line and section:
            key, value = (part.strip() for part in line.split("=", 1))
            (paths if key == "path" else urls if key == "url" else {})[section] = value
    return {paths[name]: urls[name] for name in paths if name in urls}


class Unreadable(Exception):
    """Content the scan cannot read in full: it fails the scan, never passes unread."""


class Corrupt(Unreadable):
    """Bytes that do not decode at all in the format their first bytes suggest."""


NAME, TEXT, UNREAD = "name", "text", "unread"
PNG = b"\x89PNG\r\n\x1a\n"


# A location is a tuple of (text, redacted) components from the exported file
# down to an archive member or a view. A view label is fixed text (redacted None).
UTF16, URL = ("utf16", None), ("url", None)


def subjects(number, path, data):
    """Everything exported file #NUMBER carries: (where, kind, payload) for its
    path, its bytes, their UTF-16 readings, and each container member's name and
    content, recursively. What cannot be read in full is an UNREAD reason."""
    where = ((path, f"#{number}"),)
    yield where, NAME, path.encode("utf-8", "surrogateescape")
    yield from walk(where, data, 0, [MAX_INFLATED])


def submodule_subjects(number, path, url):
    where = submodule_where(number, path)
    yield where, NAME, path.encode("utf-8", "surrogateescape")
    yield where + (URL,), NAME, url.encode("utf-8", "surrogateescape")


def submodule_where(number, path):
    return ((f"submodule {path}", f"submodule #{number}"),)


def display(where, flagged):
    """A location with every flagged name shown only by its position."""
    return "!".join(redacted if redacted and where[:depth + 1] in flagged else text
                    for depth, (text, redacted) in enumerate(where))


def without_userinfo(url):
    """A URL as printable: credentials and user names never leave the scan."""
    if "://" in url:
        return re.sub(r"^([^:/]+://)[^/?#]*@", r"\1", url)
    return re.sub(r"^[^/:]*@", "", url)


def walk(where, data, depth, budget):
    yield where, TEXT, data
    for text in utf16_texts(data):
        yield where + (UTF16,), TEXT, text
    try:
        members = container_members(data, budget)
    except Unreadable as error:
        yield where, UNREAD, str(error)
        return
    if members and depth >= MAX_DEPTH:
        yield where, UNREAD, f"containers nest deeper than {MAX_DEPTH}"
        return
    for number, (label, name, inner) in enumerate(members, 1):
        inner_where = where + ((label, None if name is None else f"member #{number}"),)
        if name is not None:
            yield inner_where, NAME, name
        yield from walk(inner_where, inner, depth + 1, budget)


def utf16_texts(data):
    """UTF-16 text (font name tables, office documents) at both byte orders and alignments."""
    if b"\0" in data or data[:2] in (b"\xff\xfe", b"\xfe\xff"):
        for codec in ("utf-16-le", "utf-16-be"):
            for offset in (0, 1):
                yield data[offset:].decode(codec, "replace").encode("utf-8")


def inflate(raw, budget, wbits=zlib.MAX_WBITS):
    """One complete deflate stream, charged to the file's budget, and the bytes after it."""
    engine = zlib.decompressobj(wbits)
    try:
        data = engine.decompress(raw, budget[0] + 1)
    except zlib.error as error:
        raise Corrupt("corrupt compressed stream") from error
    if len(data) > budget[0] or engine.unconsumed_tail:
        raise Unreadable(f"inflates past the {MAX_INFLATED}-byte budget")
    if not engine.eof:
        raise Unreadable("truncated compressed stream")
    budget[0] -= len(data)
    return data, engine.unused_data


def container_members(data, budget):
    """(label, name or None, bytes) for each member of a container, or []."""
    if data.startswith(b"PK\x03\x04"):
        return zip_members(data, budget)
    if data.startswith(b"\x1f\x8b"):
        return [("gunzip", None, gunzip(data, budget))]
    if data.startswith(PNG):
        return png_text(data, budget)
    if data[:1] == b"\x78" and len(data) > 2 and int.from_bytes(data[:2], "big") % 31 == 0:
        try:
            text, rest = inflate(data, budget)
        except Corrupt:
            return []  # a weak signature only; its bytes were scanned as they are
        if rest:
            raise Unreadable("data after the zlib stream")
        return [("zlib", None, text)]
    return []


def zip_members(data, budget):
    members = []
    try:
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            for info in archive.infolist():
                if info.flag_bits & 0x1:
                    raise Unreadable("encrypted zip member")
                if info.file_size > budget[0]:
                    raise Unreadable(f"inflates past the {MAX_INFLATED}-byte budget")
                content = archive.read(info)
                budget[0] -= len(content)
                members.append((info.filename, info.filename.encode("utf-8", "surrogateescape"), content))
    except (zipfile.BadZipFile, zipfile.LargeZipFile, NotImplementedError, RuntimeError,
            zlib.error, EOFError, OSError) as error:
        raise Unreadable("unreadable zip archive") from error
    return members


def gunzip(data, budget):
    """Every member of a gzip file, concatenated; anything but padding after them fails."""
    texts, rest = [], data
    while rest.strip(b"\0"):
        if not rest.startswith(b"\x1f\x8b"):
            raise Unreadable("data after the last gzip member")
        text, rest = inflate(rest, budget, 16 + zlib.MAX_WBITS)
        texts.append(text)
    return b"".join(texts)


def png_text(data, budget):
    """Inflate compressed PNG text chunks; plain tEXt is already in the raw bytes."""
    members, offset = [], len(PNG)
    while offset + 8 <= len(data):
        size = int.from_bytes(data[offset:offset + 4], "big")
        kind, body = data[offset + 4:offset + 8], data[offset + 8:offset + 8 + size]
        offset += 12 + size
        if kind == b"zTXt":
            fields = body.split(b"\0", 1)
            if len(fields) != 2 or fields[1][:1] != b"\0":
                raise Unreadable("unsupported zTXt chunk")
            members.append(("zTXt", None, png_stream(fields[1][1:], budget)))
        elif kind == b"iTXt":
            fields = body.split(b"\0", 1)
            if len(fields) != 2 or fields[1][:1] not in (b"\0", b"\1"):
                raise Unreadable("unsupported iTXt chunk")
            if fields[1][:1] == b"\1":
                text = fields[1][2:].split(b"\0", 2)
                if fields[1][1:2] != b"\0" or len(text) != 3:
                    raise Unreadable("unsupported iTXt chunk")
                members.append(("iTXt", None, png_stream(text[2], budget)))
    return members


def png_stream(raw, budget):
    try:
        text, rest = inflate(raw, budget)
    except Corrupt as error:
        raise Unreadable("corrupt PNG text chunk") from error
    if rest:
        raise Unreadable("data after a PNG text stream")
    return text


def term_patterns(path):
    """The terms' rules and their exceptions. An exception is labelled by line
    only, and one naming no listed term can never match, so it goes stale."""
    rows, rules, exceptions = [], {}, []
    for number, line in enumerate(Path(path).read_text(encoding="utf-8").splitlines(), 1):
        if line.startswith("allow\t"):
            cells = line.split("\t")
            if len(cells) != 3 or not all(cells):
                raise ValueError(f"terms file line {number}: expected allow<TAB>PATH<TAB>TERM")
            exceptions.append((number, cells[1], cells[2].strip()))
        elif term := line.strip():
            rules[term] = f"term #{number}"
            rows.append((rules[term], _rx(rb"(?<![" + WORD + rb"])" + re.escape(term.encode()) + rb"(?![" + WORD + rb"])")))
    # A term is a literal, so its exact matched value is the term itself.
    allow = [(f"terms file line {number}", path, rules.get(term, "unlisted"),
              "sha256:" + hashlib.sha256(term.encode()).hexdigest()) for number, path, term in exceptions]
    return rows, allow


def synthetic_mac(value):
    """A burned-in device address is universally administered unicast and non-zero."""
    return bool(int(value[:2], 16) & 0b11) or not value.strip(b"0:-")


def generic(rule, match, emails):
    if rule == "udev-serial":
        # An X3's USB serial is its MAC; an UPPER_CASE name is a placeholder.
        value = match.group(1)
        return re.fullmatch(rb"[A-Z][A-Z_]*", value) is not None or (
            PATTERNS["mac"].fullmatch(value) is not None and synthetic_mac(value))
    if rule == "mac":
        return synthetic_mac(match.group(0))
    if rule == "email":
        address, domain = match.group(0).lower(), match.group(2)
        return address in emails or bool(RESERVED_DOMAIN.search(domain))
    return False


def value_digest(match):
    return "sha256:" + hashlib.sha256(match.group(0)).hexdigest()


def scan_text(data, rules, emails, name=False):
    """(line, rule, digest of the exact matched value) for each match no generic rule excuses."""
    found = set()
    for rule, pattern in rules:
        for match in pattern.finditer(data):
            if not generic(rule, match, emails):
                found.add((0 if name else data.count(b"\n", 0, match.start()) + 1, rule, value_digest(match)))
    return found


def scan(exported, pinned, terms, allow, emails):
    """Hits no exact allowlist entry covers, stale entries, and the flagged names."""
    rules = [*PATTERNS.items(), *terms]
    sources = [(path, subjects(number, path, data)) for number, (path, data) in enumerate(exported, 1)]
    sources += [(path, submodule_subjects(number, path, url)) for number, (path, url, _obj) in enumerate(pinned, 1)]
    found, flagged = set(), set()
    for path, items in sources:
        for where, kind, payload in items:
            if kind == UNREAD:
                found.add((path, where, None, f"unscanned: {payload}", None))
                continue
            for line, rule, value in scan_text(payload, rules, emails, name=kind == NAME):
                found.add((path, where, line, rule, value))
                if kind == NAME:
                    flagged.add(where)
    allowed = {(path, rule, value): label for label, path, rule, value in allow}
    hits, used = set(), set()
    for path, where, line, rule, value in found:
        label = allowed.get((path, rule, value))
        if label is None:
            hits.add(Hit(display(where, flagged), line, rule))
        else:
            used.add(label)
    stale = [f"{label}: allows nothing" for label, *_entry in allow if label not in used]
    order = sorted(hits, key=lambda hit: (hit.where, -1 if hit.line is None else hit.line, hit.rule))
    return order, stale, flagged


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--rev", required=True, help="revision to export")
    parser.add_argument("--out", required=True, type=Path, help="new or empty output directory")
    parser.add_argument("--terms", type=Path, help="private literals kept outside the repository, one per line; allow<TAB>PATH<TAB>TERM excepts one")
    parser.add_argument("--commit", metavar="'NAME <EMAIL>'", help="after a clean scan, commit OUT as a new repository's root")
    parser.add_argument("--repo", type=Path, default=ROOT, help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    try:
        commit = git(args.repo, "rev-parse", "--verify", f"{args.rev}^{{commit}}").decode().strip()
        exported, pinned, digest, (allow, emails), index = export(args.repo, commit, args.out)
        terms, term_allow = term_patterns(args.terms) if args.terms else ([], [])
        hits, stale, flagged = scan(exported, pinned, terms, allow + term_allow, emails)
        print(f"export {commit}: {len(exported)} files, tree sha256:{digest}")
        trailer = f"{SNAPSHOT_TRAILER}: {commit} sha256:{digest}"
        print(f"root commit trailer: {trailer}")
        for number, (path, url, obj) in enumerate(pinned, 1):
            where = submodule_where(number, path)
            link = "<flagged url>" if where + (URL,) in flagged else without_userinfo(url)
            print(f"{display(where, flagged)} {link} {obj}")
            if where not in flagged:
                print(f"  restore: git update-index --add --cacheinfo 160000,{obj},{path} && git submodule update --init {path}")
        for line in [*map(str, hits), *stale]:
            print(line)
        print(f"scan: {len(hits)} hits, {len(stale)} stale allowlist entries, {len(terms)} private terms")
        if hits or stale:
            return 1
        if args.commit:
            print(f"root commit {commit_export(args.out, index, trailer, args.commit)} in {args.out}")
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"clean-export: {error}", file=sys.stderr)
        return 2
    return 0

if __name__ == "__main__":
    sys.exit(main())
