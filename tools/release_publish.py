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
never as Latest, with no bench check and no What's new. Every check runs before
its tag push (nightly.push_tag), and only the release created is then verified,
by its id; nothing reads list positions or /releases/latest for it.

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
import time

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
# Fixes-only updates reuse this line; its text alone cannot identify an old entry.
STANDARD_NOTE = 'Various smaller enhancements.'
# Scan binary images via `@`-anchored printable/UTF-8 runs and DER string fields; text scans skip binary heuristics.
AT_ANCHOR = re.compile(rb'[A-Za-z0-9._%+-]@[A-Za-z0-9.-]{4}')
NON_PRINTABLE = re.compile(rb'[^\t\x20-\x7e]')
DOMAIN_RAW = re.compile(rb'[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+')
TLD_PUNYCODE = rb'[Xx][Nn]--[A-Za-z0-9](?:(?:[A-Za-z0-9]|-(?!-)){0,57}[A-Za-z0-9])?'
TLD_TEXT = re.compile(rb'(?:' + TLD_PUNYCODE + rb'|[A-Za-z]{2,})\Z')
TLD_BINARY = re.compile(rb'(?:' + TLD_PUNYCODE + rb'|[A-Za-z]{2,24})\Z')
BINARY_LOCAL = re.compile(rb'[A-Za-z0-9_%+-](?:[A-Za-z0-9._%+-]{0,62}[A-Za-z0-9_%+-])?\Z')
BINARY_LABEL = re.compile(rb'[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?\Z')
BUILD_TAG_SUFFIX = re.compile(rb'[A-Za-z]+[0-9]+[A-Za-z0-9-]*\Z')
# Delegated IANA two-letter ccTLDs, used only to filter random binary two-letter suffixes like `.yq`.
CCTLDS = frozenset((
    'ac ad ae af ag ai al am ao aq ar as at au aw ax az ba bb bd be bf bg bh bi bj bm bn bo br bs bt bv bw by bz '
    'ca cc cd cf cg ch ci ck cl cm cn co cr cu cv cw cx cy cz de dj dk dm do dz ec ee eg er es et eu fi fj fk fm fo fr '
    'ga gb gd ge gf gg gh gi gl gm gn gp gq gr gs gt gu gw gy hk hm hn hr ht hu id ie il im in io iq ir is it je jm jo jp '
    'ke kg kh ki km kn kp kr kw ky kz la lb lc li lk lr ls lt lu lv ly ma mc md me mg mh mk ml mm mn mo mp mq mr ms mt mu mv mw mx my mz '
    'na nc ne nf ng ni nl no np nr nu nz om pa pe pf pg ph pk pl pm pn pr ps pt pw py qa '
    're ro rs ru rw sa sb sc sd se sg sh si sj sk sl sm sn so sr ss st su sv sx sy sz '
    'tc td tf tg th tj tk tl tm tn to tr tt tv tw tz ua ug uk us uy uz va vc ve vg vi vn vu wf ws ye yt za zm zw'
).split())
# Clean boundaries (plus matching brackets/braces and text terminators) used for short (< 12 byte) binary candidates.
LOCAL_BYTES = frozenset(b'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._%+-')
BEFORE_SHORT = frozenset(b'\t <("\':,;=/_*`?|&#\\~')
AFTER_SHORT = frozenset(b'\t >)"\',.;:/?!_*`&#|\\~')
TEXT_END = frozenset((0x00, 0x0a, 0x0d))
LABEL_PREFIX = re.compile(rb'(?:^|[\s,(;[<{|])(?:[a-z]{2,}|[A-Z]{2,}|[A-Z][a-z]+)[:=]\s*$')
DER_TAGS = frozenset((0x0c, 0x13, 0x14, 0x16, 0x81))
DER_ASCII_RUN = re.compile(rb'[\t\x20-\x7e]{6,}')


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

    def push_nightly(self, tag, source):
        return nightly.push_tag(self.root, tag, source)

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
    # The main commit a nightly stamps.
    source: str | None = None

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


def host_patterns(root=ROOT):
    """This host's details, found at run time so no private string is ever committed: (label, bytes, any case)."""
    found = [('home folder', os.fsencode(Path.home()), False), ('checkout', os.fsencode(root.resolve()), False),
             ('user name', getpass.getuser().encode(), True),
             ('private build folder', b'.build/private', False), ('private books folder', b'private-books', False)]
    host = socket.gethostname()
    found += [('host name', name.encode(), True) for name in {host, host.split('.')[0]} if name]
    return found


def _domain_splits(raw_domain, tld_pattern, *, binary=False):
    """Return `(trimmed, full_valid, split_ends, labels)` for `raw_domain`; `split_ends` are `(label_idx, byte_end)` before `--`."""
    trimmed = raw_domain.rstrip(b'.-')
    labels = trimmed.split(b'.')
    if len(labels) < 2:
        return trimmed, False, (), labels
    if binary:
        valid_prefix = 0
        while valid_prefix < len(labels) - 1 and BINARY_LABEL.match(labels[valid_prefix]):
            valid_prefix += 1
    else:
        valid_prefix = len(labels) - 1
    full_valid = valid_prefix == len(labels) - 1 and bool(tld_pattern.match(labels[-1]))
    split_ends = []
    offset = len(labels[0]) + 1
    for i in range(1, valid_prefix + 1):
        label = labels[i]
        at = label.find(b'--', 4 if label[:4].lower() == b'xn--' else 0)
        if at != -1 and tld_pattern.match(label[:at]):
            split_ends.append((i, offset + at))
        offset += len(label) + 1
    return trimmed, full_valid, split_ends, labels


def _extract_domain(raw_domain, tld_pattern, *, binary=False, followed_by_at=False):
    """Validate greedy `raw_domain`, splitting at non-Punycode `--` when not a complete standalone domain."""
    trimmed, full_valid, split_ends, labels = _domain_splits(raw_domain, tld_pattern, binary=binary)
    if full_valid and not followed_by_at:
        return trimmed
    if split_ends:
        i, end = split_ends[0] if followed_by_at else split_ends[-1]
        if not followed_by_at and i < len(labels) - 1 and BUILD_TAG_SUFFIX.match(labels[-1]):
            return None
        return trimmed[:end]
    return trimmed if full_valid else None


def _allowed_index(allowed_emails):
    """Build `--`-segment tries keyed by local part (forward) and domain (reverse) for O(N + S) scoring."""
    by_local, by_domain = {}, {}
    for addr in allowed_emails:
        if b'@' not in addr:
            continue
        al, ad = addr.split(b'@', 1)
        node = by_local.setdefault(al, {})
        for seg in ad.split(b'--'):
            node = node.setdefault(seg, {})
        node[None] = True
        node = by_domain.setdefault(ad, {})
        for seg in reversed(al.split(b'--')):
            node = node.setdefault(seg, {})
        node[None] = True
    return by_local, by_domain


def _iter_emails(text, tld_pattern, *, binary=False, allowed_emails=(), allowed_cache=None):
    """Yield `(start, end, local, domain)` for each address in `text`, in O(len(text) + sum(|allowed|)) time."""
    if b'@' not in text:
        return
    if allowed_cache is None:
        allowed_cache = [None]
    min_start = 0
    at = -1
    while (at := text.find(b'@', at + 1)) != -1:
        start = at
        while start > min_start and text[start - 1] in LOCAL_BYTES:
            if text[start - 1] == ord('.') and start >= min_start + 3 and text[start - 3:start] == b'...':
                break
            start -= 1
        while start < at and text[start] == ord('.'):
            start += 1
        if start == at:
            continue
        dm = DOMAIN_RAW.match(text, at + 1)
        if not dm:
            continue
        raw_domain = dm.group(0)
        tail = dm.end()
        while tail < len(text) and text[tail] in LOCAL_BYTES:
            tail += 1
        followed_by_at = tail < len(text) and text[tail] == ord('@')
        local = text[start:at]
        if followed_by_at:
            trimmed, full_valid, split_ends, _ = _domain_splits(raw_domain, tld_pattern, binary=binary)
            if len(split_ends) >= 2 and allowed_emails:
                if allowed_cache[0] is None:
                    allowed_cache[0] = _allowed_index(allowed_emails)
                by_local, by_domain = allowed_cache[0]
                next_at = tail
                next_domain = None
                if (next_dm := DOMAIN_RAW.match(text, next_at + 1)) is not None:
                    next_tail = next_dm.end()
                    while next_tail < len(text) and text[next_tail] in LOCAL_BYTES:
                        next_tail += 1
                    next_domain = _extract_domain(
                        next_dm.group(0), tld_pattern, binary=binary,
                        followed_by_at=next_tail < len(text) and text[next_tail] == ord('@'),
                    )
                matched_1 = set()
                if (node := by_local.get(local)) is not None:
                    pos = 0
                    for seg in trimmed.split(b'--'):
                        node = node.get(seg)
                        if node is None:
                            break
                        pos += len(seg)
                        if None in node:
                            matched_1.add(pos)
                        pos += 2
                matched_2 = set()
                if next_domain is not None and (node := by_domain.get(next_domain)) is not None:
                    rsegs = text[at + 1:next_at].split(b'--')
                    pos = next_at - (at + 1)
                    for idx in range(len(rsegs) - 1, 0, -1):
                        seg = rsegs[idx]
                        pos -= len(seg)
                        stripped = seg.lstrip(b'-')
                        if stripped != seg and (alt := node.get(stripped)) is not None and None in alt:
                            matched_2.add(pos - 2)
                        node = node.get(seg)
                        if node is None:
                            break
                        if None in node:
                            matched_2.add(pos - 2)
                        pos -= 2
                best_end, best_score = split_ends[0][1], -1
                for _, split_end in split_ends:
                    score = (split_end in matched_1) + (split_end in matched_2)
                    if score > best_score:
                        best_end, best_score = split_end, score
                        if best_score == 2:
                            break
                domain = trimmed[:best_end]
            elif split_ends:
                domain = trimmed[:split_ends[0][1]]
            else:
                domain = trimmed if full_valid else None
        else:
            domain = _extract_domain(raw_domain, tld_pattern, binary=binary)
        if domain is None:
            continue
        end = at + 1 + len(domain)
        if len(domain) < len(raw_domain):
            next_start = end + 1
            while next_start < len(text) and text[next_start] in (ord('-'), ord('.')):
                next_start += 1
            min_start = max(min_start, next_start)
        else:
            min_start = max(min_start, end)
        yield start, end, local, domain


def text_emails(data, allowed_emails=()):
    """E-mail addresses in UTF-8 text (notices and release notes), without binary-noise filtering."""
    return {local + b'@' + domain
            for _, _, local, domain in _iter_emails(data, TLD_TEXT, allowed_emails=allowed_emails)}


def _cased_token(token):
    return (token == token.lower() or token == token.upper()
            or (len(token) >= 2 and token[:1].isupper() and token[1:] == token[1:].lower()))


def _cased_short_email(local, labels, tld):
    tld_lower = tld.islower() or tld.lower().startswith(b'xn--')
    tld_upper = tld.isupper()
    tld_title = len(tld) >= 3 and tld[:1].isupper() and tld[1:].islower()
    if not (tld_lower or tld_upper or tld_title):
        return False
    if not _cased_token(local) or not all(_cased_token(label) for label in labels[:-1]):
        return False
    if tld_upper:
        return local == local.upper() and all(label == label.upper() for label in labels[:-1])
    if tld_title:
        return all(not (token == token.upper() and any(65 <= byte <= 90 for byte in token))
                   for token in (local, *labels[:-1]))
    return all(not (len(token) >= 2 and token == token.upper() and any(65 <= byte <= 90 for byte in token))
               for token in (local, *labels[:-1]))


def _bracket_bounds(run):
    """First open and last close index for `[...]`, `{...}`, and `(...)` in `run`, computed once per run."""
    return (run.find(b'['), run.rfind(b']'), run.find(b'{'), run.rfind(b'}'), run.find(b'('), run.rfind(b')'))


def _short_boundaries_ok(run, start, end, brackets):
    first_sq, last_sq, first_cu, last_cu, first_pa, last_pa = brackets
    if start == 0:
        before_ok = True
    else:
        before = run[start - 1]
        if before in BEFORE_SHORT or (start >= 2 and run[start - 2:start] == b'--'):
            before_ok = True
        elif before == ord('.'):
            prev = run[start - 2] if start >= 2 else None
            before_ok = (start == 1 or prev in BEFORE_SHORT or prev == ord('.')
                         or (prev == ord('[') and last_sq >= end)
                         or (prev == ord('{') and last_cu >= end))
        elif before == ord('['):
            before_ok = last_sq >= end
        elif before == ord('{'):
            before_ok = last_cu >= end
        elif before == ord(']'):
            before_ok = 0 <= first_sq < start - 1
        elif before == ord('}'):
            before_ok = 0 <= first_cu < start - 1
        elif before == ord(')'):
            before_ok = 0 <= first_pa < start - 1
        elif before == ord('@'):
            prev = run[start - 2] if start >= 2 else None
            before_ok = (start == 1 or prev in BEFORE_SHORT
                         or (start >= 3 and run[start - 3:start - 1] == b'--')
                         or (prev == ord('[') and last_sq >= end)
                         or (prev == ord('{') and last_cu >= end))
        else:
            before_ok = False
    if end == len(run):
        after_ok = True
    else:
        after = run[end]
        after_ok = (after in AFTER_SHORT or run[end:end + 2] in (b'--', b'->')
                    or (after == ord(']') and 0 <= first_sq < start)
                    or (after == ord('}') and 0 <= first_cu < start)
                    or (after == ord('[') and last_sq > end)
                    or (after == ord('{') and last_cu > end)
                    or (after == ord('(') and last_pa > end))
    return before_ok and after_ok


def _plausible_binary_email(local, domain, run, start, end, nonspace, terminated, brackets):
    """True when local@domain looks like a real address rather than stray bytes in a binary."""
    if b'..' in local or len(domain) > 253:
        return False
    # Reject chained `@` tokens like `foo@bar.com@baz.org` without blocking a leading `@` (e.g. `@me@pm.me`).
    if (start >= 2 and run[start - 1] == ord('@') and run[start - 2] in LOCAL_BYTES
            and not (start >= 3 and run[start - 3:start - 1] == b'--')):
        return False
    labels = domain.split(b'.')
    tld = labels[-1]
    if len(tld) == 2 and tld.decode('ascii').lower() not in CCTLDS:
        return False
    # Long (>= 12 byte) addresses carry enough entropy on their own.
    if end - start >= 12:
        return True
    if not _short_boundaries_ok(run, start, end, brackets):
        return False
    cased = _cased_short_email(local, labels, tld)
    angle_wrapped = start > 0 and end < len(run) and run[start - 1] == ord('<') and run[end] == ord('>')
    labeled = bool(LABEL_PREFIX.search(run, max(0, start - 32), start))
    # Strong textual context: `<...>`, a longer (>= 12 non-space byte) string, or a labeled prefix (`From: `, `To: `).
    if angle_wrapped or (nonspace >= 12 and (terminated or cased)) or labeled:
        return True
    # Standalone short candidates must sit in NUL/line-terminated text with >= 8 non-space bytes and sensible casing.
    return terminated and nonspace >= 8 and cased


def _der_email(inner, no_brackets):
    """Return `inner` if it is a complete, plausible binary e-mail address from a DER field."""
    if inner.count(b'@') != 1:
        return None
    local, raw_domain = inner.split(b'@', 1)
    if not BINARY_LOCAL.match(local) or not DOMAIN_RAW.fullmatch(raw_domain):
        return None
    domain = _extract_domain(raw_domain, TLD_BINARY, binary=True)
    if domain != raw_domain:
        return None
    if _plausible_binary_email(local, domain, inner, 0, len(inner), max(len(inner), 12), True, no_brackets):
        return inner
    return None


def _der_after_ok(data, end):
    return end == len(data) or data[end] in (0x30, 0x31) or not (0x20 <= data[end] <= 0x7e)


def _der_text_ok(inner, tag):
    if tag == 0x0c:
        if any((b < 0x20 and b not in (9, 10, 13)) or b == 0x7f for b in inner):
            return False
        try:
            inner.decode('utf-8')
            return True
        except UnicodeDecodeError:
            return False
    return all(b == 9 or 0x20 <= b <= 0x7e for b in inner)


def _utf8_run_start(data, pos, stop=-1):
    """Walk backward from `pos` across printable ASCII and valid multibyte UTF-8, stopping at `stop`."""
    cur = pos
    while cur > 0:
        if cur == stop:
            return None
        byte = data[cur - 1]
        if byte == 9 or 0x20 <= byte <= 0x7e:
            cur -= 1
            continue
        if 0x80 <= byte <= 0xbf:
            lead = cur - 2
            while lead >= 0 and cur - lead <= 4 and 0x80 <= data[lead] <= 0xbf:
                lead -= 1
            if lead >= 0 and cur - lead <= 4 and 0xc2 <= data[lead] <= 0xf4:
                try:
                    data[lead:cur].decode('utf-8')
                except UnicodeDecodeError:
                    break
                cur = lead
                continue
        break
    return cur


def _utf8_run_end(data, pos):
    """Walk forward from `pos` across printable ASCII and valid multibyte UTF-8 to the run's true end."""
    n = len(data)
    cur = pos
    while cur < n:
        byte = data[cur]
        if byte == 9 or 0x20 <= byte <= 0x7e:
            cur += 1
            continue
        if 0xc2 <= byte <= 0xf4:
            size = 2 if byte < 0xe0 else (3 if byte < 0xf0 else 4)
            if cur + size <= n:
                try:
                    data[cur:cur + size].decode('utf-8')
                except UnicodeDecodeError:
                    break
                cur += size
                continue
        break
    return cur


def _der_tlv_slices_at(data, pos):
    """Yield `(tag, val_start, val_end)` for DER string TLVs anchored at `pos`, longest header first."""
    n = len(data)
    if pos >= 4 and data[pos - 4] in DER_TAGS and data[pos - 3] == 0x82:
        length = (data[pos - 2] << 8) | data[pos - 1]
        if 256 <= length <= 511 and pos + length <= n and _der_after_ok(data, pos + length):
            yield data[pos - 4], pos, pos + length
    if pos >= 3 and data[pos - 3] in DER_TAGS:
        if data[pos - 2] == 0x81 and data[pos - 1] >= 128:
            length = data[pos - 1]
            if pos + length <= n and _der_after_ok(data, pos + length):
                yield data[pos - 3], pos, pos + length
        if data[pos - 2] == 0x82 and data[pos - 1] == 1 and pos < n:
            length = 256 | data[pos]
            if length <= 511 and pos + 1 + length <= n and _der_after_ok(data, pos + 1 + length):
                yield data[pos - 3], pos + 1, pos + 1 + length
    if pos >= 2 and data[pos - 2] in DER_TAGS:
        length = data[pos - 1]
        if (6 <= length <= 31 or length == 127) and pos + length <= n and _der_after_ok(data, pos + length):
            yield data[pos - 2], pos, pos + length
    if pos >= 1 and data[pos - 1] in DER_TAGS and pos < n:
        length = data[pos]
        if (length == 9 or 0x20 <= length <= 0x7e) and pos + 1 + length <= n and _der_after_ok(data, pos + 1 + length):
            yield data[pos - 1], pos + 1, pos + 1 + length


def _scan_binary_run(text, terminated, found, *, min_nonspace=0, allowed_emails=(), allowed_cache=None):
    if b'@' not in text:
        return
    nonspace = max(len(text) - text.count(b' ') - text.count(b'\t'), min_nonspace)
    brackets = _bracket_bounds(text)
    for start, end, local, domain in _iter_emails(text, TLD_BINARY, binary=True,
                                                  allowed_emails=allowed_emails, allowed_cache=allowed_cache):
        if not BINARY_LOCAL.match(local):
            continue
        if end < len(text) and text[end] == ord('@'):
            continue
        if _plausible_binary_email(local, domain, text, start, end, nonspace, terminated, brackets):
            found.add(local + b'@' + domain)


def _unwrap_der_at(data, pos, found, no_brackets, allowed_emails=(), allowed_cache=None):
    """If a DER string TLV at `pos` carries an e-mail address, record its addresses and return `val_end`."""
    slices = tuple(_der_tlv_slices_at(data, pos))
    for _, val_start, val_end in slices:
        if (email := _der_email(data[val_start:val_end], no_brackets)) is not None:
            found.add(email)
            return val_end
    for tag, val_start, val_end in slices:
        inner = data[val_start:val_end]
        if b'@' in inner and _der_text_ok(inner, tag):
            min_nonspace = len(inner) - inner.count(b' ') - inner.count(b'\t')
            for part in DER_ASCII_RUN.finditer(inner):
                _scan_binary_run(part.group(0), True, found, min_nonspace=min_nonspace,
                                 allowed_emails=allowed_emails, allowed_cache=allowed_cache)
            return val_end
    return None


def binary_emails(data, allowed_emails=()):
    """Plausible e-mail addresses inside printable/UTF-8 runs and DER string fields of binary `data`."""
    found = set()
    no_brackets = (-1, -1, -1, -1, -1, -1)
    allowed_cache = [None]
    n = len(data)
    covered_until = 0
    no_der_start, no_der_end = -1, -1
    cached_u_start, cached_u_end, cached_u_nonspace = -1, -1, 0
    for anchor in AT_ANCHOR.finditer(data):
        if anchor.start() < covered_until:
            continue
        start = anchor.start()
        while start > covered_until and (data[start - 1] == 9 or 0x20 <= data[start - 1] <= 0x7e):
            start -= 1
        right = NON_PRINTABLE.search(data, anchor.end())
        end = right.start() if right else n
        val_end = _unwrap_der_at(data, start, found, no_brackets, allowed_emails, allowed_cache)
        u_start = start
        if val_end is None:
            u_start = _utf8_run_start(data, start, stop=no_der_end)
            if u_start is None:
                u_start = no_der_start
                no_der_end = start
            elif u_start < start:
                val_end = _unwrap_der_at(data, u_start, found, no_brackets, allowed_emails, allowed_cache)
                if val_end is None:
                    no_der_start, no_der_end = u_start, start
        if val_end is not None:
            covered_until = max(covered_until, val_end)
            if val_end >= end:
                continue
            start = val_end
            u_start = start
        if end - start < 12 or u_start < start or (end < n and 0xc2 <= data[end] <= 0xf4):
            if end <= cached_u_end:
                u_end = cached_u_end
            else:
                u_end = _utf8_run_end(data, end)
        else:
            u_end = end
        if u_end - u_start < 8:
            covered_until = max(covered_until, end)
            continue
        terminated = u_end == n or data[u_end] in TEXT_END
        if u_start < start or u_end > end:
            if u_start != cached_u_start or u_end != cached_u_end:
                u_span = data[u_start:u_end]
                cached_u_start, cached_u_end = u_start, u_end
                cached_u_nonspace = len(u_span) - u_span.count(b' ') - u_span.count(b'\t')
            min_nonspace = cached_u_nonspace
        else:
            min_nonspace = 0
        _scan_binary_run(data[start:end], terminated, found, min_nonspace=min_nonspace,
                         allowed_emails=allowed_emails, allowed_cache=allowed_cache)
        covered_until = max(covered_until, end)
    return found


def leaks(name, data, patterns, allowed_emails, *, binary=False):
    """Each host detail or non-notice e-mail address in data, as a refusal."""
    found = [f'{name} contains the {label}' for label, pattern, fold in patterns
             if (pattern.lower() in data.lower() if fold else pattern in data)]
    found += [f'{name} contains the e-mail address {email.decode()}'
              for email in sorted((binary_emails if binary else text_emails)(data, allowed_emails) - allowed_emails)]
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
    elif (origin := git.origin_tag_commit(tag)) != (None if night else commit):
        refusals.append(f"Origin's tag {tag} names {origin}, not " +
                        ('nothing: a nightly\'s tag is pushed only after every check' if night else commit))
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
    news = []
    if not night:
        for index, (_, lines) in enumerate(whats_new(header)):
            current = any(line not in shown for line in lines) or (index == 0 and lines == [STANDARD_NOTE])
            news.extend(line for line in lines if line not in shown or (line == STANDARD_NOTE and current))
        news = list(dict.fromkeys(news))
    notes = render_notes(version, commit, build, image, news)
    if main:
        notes = nightly.notes(source, main, git.subject(main), git.version(main)) + '\n' + notes
    # Until its firmware reads it, the source repository is private and its links 404.
    if source != SOURCE_REPOSITORY and re.search(r'VargasDevelopment/newt(?!-releases)', notes):
        refusals.append('The notes link into the private source repository')
    patterns = host_patterns(root)
    allowed = text_emails((notices or '').encode())
    refusals += leaks('newt.bin', image, patterns, allowed, binary=True) + leaks('The notes', notes.encode(), patterns, allowed)
    release = Plan(repository, version, commit, build, image, manifest, notes, notices, refusals, prerelease=night,
                   source=main)
    if hop and source == SOURCE_REPOSITORY:
        public = public or GitHub(SOURCE_REPOSITORY)
        shown = public.release_for(tag)
        if shown is None or shown.get('draft') is not False:
            refusals.append(f'{SOURCE_REPOSITORY} has not published {tag}: the hop follows that release')
        else:
            refusals += [f"{SOURCE_REPOSITORY}'s {tag}: {difference}"
                         for difference in asset_refusals(public, shown, release.assets())]
    there = github.tag_commit(tag)
    if repository == SOURCE_REPOSITORY and not night and there != commit:
        refusals.append(f"{repository}'s tag {tag} names {there}, not the build commit {commit}")
    elif (night or repository != SOURCE_REPOSITORY) and there is not None:
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


def publish(release, out, github, git=None):
    """Creates (or resumes) the draft, verifies its assets, publishes it with one PATCH, then reads it
    back by id (and a stable release as devices will). Never deletes. A nightly pushes its tag only after
    its draft verifies, so a failed check or a newer main (returns False) leaves at most a private draft."""
    draft = release.draft
    if draft is None:
        digests = dict(update_publish.publish(out, 'newt', release.image, release.manifest))
        require(digests == {'newt.bin': sha(release.image), 'newt.manifest': sha(release.manifest)},
                'The written pair differs from the checked pair')
        for name, text in (('NOTICES.txt', release.notices), ('notes.md', release.notes)):
            with (out / name).open('x', encoding='utf-8') as file:
                file.write(text)
    if draft is None:
        github.create_draft(release.command(out))
        draft = github.release_for(release.tag)
        # A newly created draft can lag in GitHub's listing. Retry absence only;
        # API errors and a visible but mismatched release still stop publication.
        for delay in (1, 2, 4):
            if draft is not None:
                break
            time.sleep(delay)
            draft = github.release_for(release.tag)
        require(draft is not None and draft.get('draft') is True,
                f'{release.repository} shows no draft for {release.tag} after creating it')
        mismatch = draft_mismatch(github, draft, release)
        require(not mismatch, draft_refusals(release, mismatch)[0])
    # The one irreversible step for a nightly: GitHub publishes the draft at this pushed tag.
    if release.prerelease and not (git or Git()).push_nightly(release.tag, release.source):
        return False
    try:
        github.publish(draft['id'], not release.prerelease)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        # GitHub may have published it and only the answer was lost; the read-back decides.
        print(f'The publish request failed ({error}); reading draft {draft["id"]} back', file=sys.stderr)
    shown = github.release(draft['id'])
    require(shown.get('draft') is False,
            f'{release.tag} is still a draft ({release.repository} release {draft["id"]}), so not public; '
            'rerun --publish to resume it')
    # By id: this release's tag, flag and assets. GitHub never makes a pre-release Latest.
    require(shown.get('tag_name') == release.tag and shown.get('prerelease') is release.prerelease and
            not asset_refusals(github, shown, release.assets()),
            f'{release.repository} published {release.tag}, but release {draft["id"]} does not show it '
            f'{"flagged" if release.prerelease else "unflagged"} with exactly {", ".join(ASSETS)}; never delete it '
            '(docs/RELEASING.md)')
    # Devices read /releases/latest, so a stable release must be it, with its assets; a nightly never is.
    if not release.prerelease:
        latest = github.latest()
        require(latest.get('id') == draft['id'] and latest.get('tag_name') == release.tag and
                latest.get('prerelease') is False and not asset_refusals(github, latest, release.assets()),
                f'{release.repository} published {release.tag}, but /releases/latest does not show it with exactly '
                f'{", ".join(ASSETS)}; never delete it (docs/RELEASING.md)')
    return True


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
    git = git or Git()
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
        if not publish(release, out, github, git):
            print(f'Not published: origin\'s main has moved on from {release.source}; its own run publishes')
            return 0
    except (OSError, KeyError, TypeError, ValueError, subprocess.CalledProcessError) as error:
        print(f'Release publish stopped: {error}', file=sys.stderr)
        return 1
    shows = ' as a pre-release, never Latest,' if release.prerelease else '; /releases/latest shows it'
    print(f'Published {release.tag} to {release.repository}{shows} with {", ".join(ASSETS)}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
