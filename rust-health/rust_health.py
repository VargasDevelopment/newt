#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Read-only code-health ratchet for the Rust port's production sources.

Production Rust is every .rs file under rust/ outside any tests/ or target/
directory, minus files declared only as #[cfg(test)] modules. Code inside
#[cfg(test)] items is not production either. baseline.json records today's
values per file; only growth fails, except size, which only warns.

- unsafe: per-file ceilings per kind. A new file may contain unsafe only in a
  crate that already does. A crate here is a first-level source directory
  (core/src, reader-ui/host, salamander/board), so a package's host-probe binary
  does not license unsafe in its library. Every unsafe needs a `SAFETY:`
  comment (or a `# Safety` doc section) directly above it; attributes and other
  comment lines may sit between. Today's gaps have per-file ceilings.
- suppressions: per-file ceilings on #[allow( and #![allow(.
- panic surface: per-file ceilings on .unwrap(), .expect(, panic! and
  unreachable!. todo! and unimplemented! always fail.
- size (advisory): a file over 500 lines and over its recorded size prints a
  warning; it never fails. --update only lowers a recorded size, so a grown
  file keeps warning. Generated data files are exempt by exact path.
- crates: a shared crate may not gain a Cargo path dependency on a platform
  (firmware, host or native) crate; today's such edges are listed exceptions.
  The external package set in rust/Cargo.lock is pinned.
- natives: only the native owners declare natives (extern ABIs, #[link],
  no_mangle, export_name): Salamander, the device layer (rust/salamander), and
  target-probe, whose exports are the size probes and never reach the image.
  Other files keep today's counts as named exceptions that may only shrink.

--update rewrites the baseline from a passing tree. A pass means every value is
within its ceiling, so an update can only lower ceilings, drop deleted files
and record new files at their current values.

Run: python3 rust-health/rust_health.py [--update]. Stdlib only; no cargo.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
import tomllib
from collections import Counter
from collections.abc import Callable, Iterable
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import TypedDict

sys.dont_write_bytecode = True  # read-only: no __pycache__ beside the tool
from rust_source import lex, module_declarations, test_spans, without  # noqa: E402

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
BASELINE = HERE / 'baseline.json'

UNSAFE_KINDS = ('unsafe_block', 'unsafe_fn', 'unsafe_impl', 'unsafe_trait',
                'unsafe_extern', 'unsafe_attr', 'unsafe_other')
UNDOCUMENTED = 'unsafe_undocumented'
ALLOW = 'allow'
PANICS = {'unwrap': re.compile(r'\.\s*unwrap\s*\(\s*\)'), 'expect': re.compile(r'\.\s*expect\s*\('),
          'panic': re.compile(r'\bpanic\s*!'), 'unreachable': re.compile(r'\bunreachable\s*!')}
BANNED = 'banned'
# A native boundary: any extern ABI (blocks, exports, fn-pointer types; the
# ABI string is blank in the code view), #[link] and exported symbol names.
NATIVE = 'native'
_NATIVE = re.compile(r'\bextern\b(?!\s*crate\b)|#\s*!?\s*\[\s*link\s*\(|\b(?:no_mangle|export_name)\b')
TOKENS = {ALLOW: re.compile(r'#\s*!?\s*\[\s*allow\s*\('),
          BANNED: re.compile(r'\b(?:todo|unimplemented)\s*!'), **PANICS}


class Baseline(TypedDict):
    line_limit: int
    generated: list[str]
    platform_crates: list[str]
    platform_edge_exceptions: list[str]
    native_owners: list[str]  # crates (or crate prefixes) that may declare natives
    native_exceptions: dict[str, int]  # path -> native sites, outside the owners
    external_packages: list[str]
    files: dict[str, dict[str, int]]


@dataclass(frozen=True)
class Source:
    path: str  # repo-relative POSIX path
    crate: str
    lines: int
    metrics: Counter[str]
    where: dict[str, list[int]]  # metric -> line numbers, for messages


@dataclass(frozen=True)
class Tree:
    sources: dict[str, Source]
    test_only: list[str]
    edges: set[tuple[str, str]]  # Cargo package -> package, by member path
    externals: set[str]
    forbid_gaps: list[str]  # roots of zero-unsafe crates without #![forbid(unsafe_code)]


# ---- measurement ----

_UNSAFE = re.compile(r'\bunsafe\b')
_SAFETY = re.compile(r'SAFETY:|#\s*Safety\b')
_FORBID = re.compile(r'#!\s*\[\s*forbid\s*\([^\]]*\bunsafe_code\b')


def crate_of(path: str) -> str:
    parts = PurePosixPath(path).parts[1:]  # drop 'rust'
    return '/'.join(parts[:2]) if len(parts) > 2 else parts[0]


def _unsafe_kind(code: str, end: int) -> str:
    rest = code[end:end + 200].lstrip()
    if rest.startswith('{'):
        return 'unsafe_block'
    if rest.startswith('('):
        return 'unsafe_attr'
    word = re.match(r'\w+', rest)
    keyword = word.group() if word else ''
    if keyword == 'extern':  # the ABI string is blank in the code view
        return 'unsafe_fn' if re.match(r'extern\s+fn\b', rest) else 'unsafe_extern'
    return {'fn': 'unsafe_fn', 'impl': 'unsafe_impl', 'trait': 'unsafe_trait'}.get(keyword, 'unsafe_other')


def _documented(code_lines: list[str], comment_lines: list[str], line: int) -> bool:
    for index in range(line - 2, -1, -1):
        code, comment = code_lines[index].strip(), comment_lines[index]
        if _SAFETY.search(comment):
            return code == '' or code.startswith('#')
        if code.startswith('#'):
            continue
        if code or not comment.strip():
            return False
    return False


def _measure_file(path: str, text: str, code: str, comments: str, spans: list[tuple[int, int]]) -> Source:
    production = without(code, spans)
    code_lines, comment_lines = code.split('\n'), comments.split('\n')
    metrics: Counter[str] = Counter()
    where: dict[str, list[int]] = {}

    def record(metric: str, offset: int) -> None:
        metrics[metric] += 1
        where.setdefault(metric, []).append(code.count('\n', 0, offset) + 1)

    for match in _UNSAFE.finditer(production):
        record(_unsafe_kind(production, match.end()), match.start())
        if not _documented(code_lines, comment_lines, code.count('\n', 0, match.start()) + 1):
            record(UNDOCUMENTED, match.start())
    for metric, pattern in (*TOKENS.items(), (NATIVE, _NATIVE)):
        for match in pattern.finditer(production):
            record(metric, match.start())
    return Source(path, crate_of(path), len(text.splitlines()), metrics, where)


def _production_files(rust: Path) -> list[Path]:
    found = []
    for directory, subdirectories, files in os.walk(rust):
        here = Path(directory)
        for name in [*subdirectories, *files]:
            if (here / name).is_symlink():
                raise ValueError(f'symlink in Rust sources cannot be audited: {here / name}')
        subdirectories[:] = sorted(name for name in subdirectories if name not in ('tests', 'target'))
        found += [here / name for name in sorted(files) if name.endswith('.rs')]
    return found


def _member_path(rust: Path, base: Path, relative: str, members: list[str]) -> str:
    target = os.path.relpath(os.path.normpath(base / relative), rust)
    return next((member for member in members if target == member), PurePosixPath(target).as_posix())


def _cargo(rust: Path, sources: dict[str, Source]) -> tuple[set[tuple[str, str]], set[str], list[str]]:
    workspace = tomllib.loads((rust / 'Cargo.toml').read_text())['workspace']
    members: list[str] = workspace['members']
    if any('*' in member for member in members):
        raise ValueError('workspace member globs are not supported; list members explicitly')
    shared_dependencies = workspace.get('dependencies', {})
    edges: set[tuple[str, str]] = set()
    names: set[str] = set()
    roots: list[str] = []
    for member in members:
        package = rust / member
        manifest = tomllib.loads((package / 'Cargo.toml').read_text())
        names.add(manifest['package']['name'])
        tables = [manifest.get('dependencies', {}), manifest.get('build-dependencies', {})]
        for target in manifest.get('target', {}).values():
            tables += [target.get('dependencies', {}), target.get('build-dependencies', {})]
        for table in tables:
            for name, specification in table.items():
                if isinstance(specification, dict) and specification.get('workspace'):
                    shared = shared_dependencies.get(name, {})
                    if isinstance(shared, dict) and 'path' in shared:
                        edges.add((member, _member_path(rust, rust, shared['path'], members)))
                elif isinstance(specification, dict) and 'path' in specification:
                    edges.add((member, _member_path(rust, package, specification['path'], members)))
        declared = [manifest.get('lib', {}).get('path', 'src/lib.rs'), 'src/main.rs',
                    *(binary['path'] for binary in manifest.get('bin', []) if 'path' in binary)]
        declared += [f'src/bin/{path.name}' for path in sorted((package / 'src/bin').glob('*.rs'))]
        roots += [PurePosixPath(os.path.relpath(os.path.normpath(package / path), rust.parent)).as_posix()
                  for path in declared]
    unsafe_crates = {source.crate for source in sources.values() if any(source.metrics[kind] for kind in UNSAFE_KINDS)}
    gaps = sorted({root for root in roots if root in sources and sources[root].crate not in unsafe_crates
                   and not _FORBID.search(lex((rust.parent / root).read_text())[0])})
    lock = tomllib.loads((rust / 'Cargo.lock').read_text())
    externals = {f"{package['name']} {package['version']} {package.get('source', 'path')}"
                 for package in lock.get('package', []) if 'source' in package or package['name'] not in names}
    return edges, externals, gaps


def measure(root: Path) -> Tree:
    rust = root / 'rust'
    parsed = {}
    declarations: dict[Path, list[tuple[Path, bool]]] = {}
    for path in _production_files(rust):
        text = path.read_text()
        code, comments = lex(text)
        spans = test_spans(code)
        parsed[path] = text, code, comments, spans
        declarations[path] = list(module_declarations(path, text, code, spans))
    targets = {target for rows in declarations.values() for target, _ in rows}
    reached: set[tuple[Path, bool]] = set()
    # Test-only scope propagates through child modules. A production reference
    # wins; unreferenced files (including cycles) stay conservatively production.
    for entry in sorted(set(parsed) - targets) + sorted(parsed):
        if (entry, False) in reached or (entry, True) in reached:
            continue
        pending = [(entry, False)]
        while pending:
            path, in_test = pending.pop()
            if (path, in_test) in reached:
                continue
            reached.add((path, in_test))
            pending.extend((target, in_test or local_test)
                           for target, local_test in declarations.get(path, ()))
    test_only = {path for path, in_test in reached if in_test and (path, False) not in reached}
    sources = {}
    for path, (text, code, comments, spans) in parsed.items():
        if path not in test_only:
            relative = path.relative_to(root).as_posix()
            sources[relative] = _measure_file(relative, text, code, comments, spans)
    edges, externals, gaps = _cargo(rust, sources)
    return Tree(sources, sorted(path.relative_to(root).as_posix() for path in test_only if path in parsed),
                edges, externals, gaps)


# ---- checks: each returns failure messages; the registry lets tests remove one ----

def _ceilings(tree: Tree, baseline: Baseline, metrics: Iterable[str], new_file: int | None = 0) -> list[str]:
    failures = []
    for source in tree.sources.values():
        recorded = baseline['files'].get(source.path)
        for metric in metrics:
            ceiling = new_file if recorded is None else recorded.get(metric, 0)
            if ceiling is not None and source.metrics[metric] > ceiling:
                failures.append(f'{source.path}: {metric} {source.metrics[metric]} exceeds ceiling {ceiling} '
                                f'(lines {", ".join(map(str, source.where[metric]))})')
    return failures


def check_unsafe_crates(tree: Tree, baseline: Baseline) -> list[str]:
    allowed = {crate_of(path) for path, recorded in baseline['files'].items()
               if any(recorded.get(kind) for kind in UNSAFE_KINDS)}
    return [f'{source.path}: unsafe in {source.crate}, a crate without unsafe; keep unsafe in a boundary crate'
            for path, source in tree.sources.items()
            if path not in baseline['files'] and source.crate not in allowed
            and any(source.metrics[kind] for kind in UNSAFE_KINDS)]


def check_banned(tree: Tree, _baseline: Baseline) -> list[str]:
    return [f'{source.path}: todo!/unimplemented! at lines {", ".join(map(str, source.where[BANNED]))}'
            for source in tree.sources.values() if source.metrics[BANNED]]


def check_generated(tree: Tree, baseline: Baseline) -> list[str]:
    # A stale exclusion would silently exempt whatever file later takes its path.
    return [f'{path}: generated exclusion matches no production file' for path in baseline['generated']
            if path not in tree.sources]


def size_warnings(tree: Tree, baseline: Baseline) -> list[tuple[str, str]]:
    """(path, message) for each file over the line limit and its recorded size.
    Length is a review signal, not a gate (docs/CODE_HEALTH.md)."""
    warnings = []
    for source in tree.sources.values():
        if source.path in baseline['generated']:
            continue
        recorded = baseline['files'].get(source.path, {}).get('lines', 0)
        if source.lines > max(baseline['line_limit'], recorded):
            size = f', recorded size {recorded}' if recorded else ''
            warnings.append((source.path, f"{source.lines} lines, over the {baseline['line_limit']}-line guide"
                                          f'{size}; split it where a cohesive boundary makes the design simpler'))
    return warnings


def print_warning(path: str, message: str) -> None:
    # On GitHub Actions this workflow command annotates the file in the pull request.
    prefix = f'::warning file={path}::' if os.environ.get('GITHUB_ACTIONS') == 'true' else 'WARN '
    print(f'{prefix}{path}: {message}')


def _platform(crate: str, platform: list[str]) -> bool:
    return any(crate == name or crate.startswith(name + '/') for name in platform)


def _forbidden_edges(tree: Tree, platform: list[str]) -> set[str]:
    return {f'{source} -> {target}' for source, target in tree.edges
            if not _platform(source, platform) and _platform(target, platform)}


def check_crate_edges(tree: Tree, baseline: Baseline) -> list[str]:
    return [f'crate edge {edge}: a shared crate may not depend on a firmware, host or native crate'
            for edge in sorted(_forbidden_edges(tree, baseline['platform_crates'])
                               - set(baseline['platform_edge_exceptions']))]


def _native_exceptions(tree: Tree, owners: list[str]) -> dict[str, int]:
    return {path: source.metrics[NATIVE] for path, source in sorted(tree.sources.items())
            if source.metrics[NATIVE] and not _platform(source.crate, owners)}


def check_natives(tree: Tree, baseline: Baseline) -> list[str]:
    allowed = baseline['native_exceptions']
    return [f'{path}: {count} native sites exceed {allowed.get(path, 0)} (lines '
            f'{", ".join(map(str, tree.sources[path].where[NATIVE]))}); declare natives in Salamander (rust/salamander)'
            for path, count in _native_exceptions(tree, baseline['native_owners']).items()
            if count > allowed.get(path, 0)]


def check_externals(tree: Tree, baseline: Baseline) -> list[str]:
    return [f'rust/Cargo.lock: external package {package} is not pinned'
            for package in sorted(tree.externals - set(baseline['external_packages']))]


Check = Callable[[Tree, Baseline], list[str]]
CHECKS: dict[str, Check] = {
    # New files have no unsafe ceiling here; check_unsafe_crates confines them.
    'unsafe': lambda tree, baseline: _ceilings(tree, baseline, UNSAFE_KINDS, new_file=None),
    'unsafe_crates': check_unsafe_crates,
    'safety': lambda tree, baseline: _ceilings(tree, baseline, [UNDOCUMENTED]),
    'suppressions': lambda tree, baseline: _ceilings(tree, baseline, [ALLOW]),
    'panic_surface': lambda tree, baseline: _ceilings(tree, baseline, PANICS),
    'banned': check_banned, 'crate_edges': check_crate_edges, 'externals': check_externals,
    'natives': check_natives, 'generated': check_generated,
}


def run_checks(tree: Tree, baseline: Baseline, checks: dict[str, Check] = CHECKS) -> dict[str, list[str]]:
    return {name: failures for name, check in checks.items() if (failures := check(tree, baseline))}


# ---- baseline ----

def baseline_from(tree: Tree, policy: Baseline) -> Baseline:
    """Today's values under an existing policy (limit, generated paths, platform and native owners)
    and its recorded sizes."""
    files = {}
    for path, source in sorted(tree.sources.items()):
        entry = {metric: count for metric, count in sorted(source.metrics.items())
                 if count and metric not in (BANNED, NATIVE)}
        # A recorded size only shrinks; a new or grown file keeps its warning.
        recorded = policy['files'].get(path, {}).get('lines')
        if recorded and path not in policy['generated'] and source.lines > policy['line_limit']:
            entry['lines'] = min(source.lines, recorded)
        files[path] = entry
    return Baseline(line_limit=policy['line_limit'], generated=policy['generated'],
                    platform_crates=policy['platform_crates'],
                    platform_edge_exceptions=sorted(_forbidden_edges(tree, policy['platform_crates'])),
                    native_owners=policy['native_owners'],
                    native_exceptions=_native_exceptions(tree, policy['native_owners']),
                    external_packages=sorted(tree.externals), files=files)


def _raised(old: Baseline, new: Baseline) -> list[str]:
    raised = [f'{path}: {metric}' for path, entry in new['files'].items() if path in old['files']
              for metric, value in entry.items()
              if value > old['files'][path].get(metric, old['line_limit'] if metric == 'lines' else 0)]
    for key in ('platform_edge_exceptions', 'external_packages'):
        raised += [f'{key}: {value}' for value in set(new[key]) - set(old[key])]
    raised += [f'native_exceptions: {path}' for path, count in new['native_exceptions'].items()
               if count > old['native_exceptions'].get(path, 0)]
    return raised


def write_baseline(path: Path, baseline: Baseline) -> None:
    path.write_text(json.dumps(baseline, indent=2, sort_keys=True) + '\n')


def _summary(tree: Tree) -> str:
    totals: Counter[str] = Counter()
    for source in tree.sources.values():
        totals.update(source.metrics)
    unsafe = sum(totals[kind] for kind in UNSAFE_KINDS)
    panics = ', '.join(f'{name} {totals[name]}' for name in PANICS)
    return (f'{len(tree.sources)} files ({len(tree.test_only)} test-only skipped); unsafe {unsafe} '
            f'({totals[UNDOCUMENTED]} without SAFETY); allow {totals[ALLOW]}; {panics}; '
            f'{len(tree.edges)} crate edges; {len(tree.externals)} external packages')


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--update', action='store_true', help='tighten the baseline to a passing tree')
    parser.add_argument('--root', type=Path, default=ROOT, help='repository root (default: this checkout)')
    parser.add_argument('--baseline', type=Path, default=BASELINE)
    args = parser.parse_args(argv)
    started = time.monotonic()
    tree = measure(args.root)
    baseline: Baseline = json.loads(args.baseline.read_text())
    failures = [message for messages in run_checks(tree, baseline).values() for message in messages]
    if failures:
        print('FAIL rust-health:\n  ' + '\n  '.join(failures), file=sys.stderr)
        return 1
    current = baseline_from(tree, baseline)
    if args.update:
        if raised := _raised(baseline, current):  # unreachable after a pass; guards the tighten-only contract
            raise ValueError('update would raise: ' + ', '.join(raised))
        write_baseline(args.baseline, current)
    for path, message in size_warnings(tree, baseline):
        print_warning(path, message)
    print(f'PASS rust-health: {_summary(tree)} ({time.monotonic() - started:.1f}s)')
    if tree.forbid_gaps:
        print('NOTE zero-unsafe crate roots without #![forbid(unsafe_code)]: ' + ', '.join(tree.forbid_gaps))
    if current != baseline and not args.update:
        print('NOTE the baseline can tighten: run with --update')
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, tomllib.TOMLDecodeError) as error:
        print(f'rust-health stopped: {error}', file=sys.stderr)
        sys.exit(2)
