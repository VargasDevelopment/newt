"""The gate's tiers, and which tests' negative controls a run includes.

full:  every phase and every negative control (CI on release merges, nightly
       and manual runs; in CI also fresh objects and fresh toolchains).
push:  positive tests except declared full-only suites, no negative controls.
local: the push tier plus the negative controls of the tests this branch
       changes or whose changed tests/ inputs they use (a worker's
       per-candidate gate, tools/check.py's default, and CI on a pull request).

A negative control proves a test can fail: it breaks the code on purpose and
expects the test to catch it. It is evidence about the test, so it needs to run
when the test or its anchors change, and at least once per release. A test
that mixes checks and controls asks negative_controls(__file__); run by hand,
outside the gate, it runs both.
"""
import argparse
import ast
import json
import math
import os
from pathlib import Path
import re
import statistics
import subprocess
import sys

from gate_runner import GateFailure
from native_output import write_text
from release_tag import config_version

from rust_gate_families import (SHARED_INPUTS, FULL_POSITIVE_SOURCES, positive_owners,
                           test_inputs as rust_test_inputs)

CONTROLS = 'NEWT_GATE_CONTROLS'  # 'all', or the comma-separated test scripts whose controls run
BASE = 'origin/main'
SECONDS = '.build/gate-phase-seconds.json'  # the last run's wall seconds per phase
# Each phase's CPU seconds in a measured push-tier run; they only balance CI's shards.
SHARD_SECONDS = 'tools/gate-shard-seconds.json'
# Rust test packets a tests/ script runs, by that script (#360).
RUST_PACKETS = {'slot_vectors_test.py': 'rust/tests/slot_vectors/',
                'app_store_test.py': 'rust/tests/app_store/',
                'dev_put_test.py': 'rust/tests/dev_put/'}
# tests/ files whose text can name another tests/ file
TEXT = frozenset(('.py', '.h', '.hpp', '.inc', '.cpp', '.c', '.json', '.txt', '.md'))
# Product owners select moved positives only; test/harness changes select
# mutation controls too. Deleted inputs follow the same ownership rules.
TEST_PATHS = ('tests', 'rust/tests', ':(glob)rust/*.py', ':(glob)rust/*.json', 'rust-health', 'tools/rust-toolchain.py', 'tools/rust_gate_families.py',
              *(path for roots in FULL_POSITIVE_SOURCES.values() for path in roots))


def negative_controls(script) -> bool:
    value = os.environ.get(CONTROLS, 'all')
    return value == 'all' or Path(script).name in value.split(',')


def controls(tier, root, *, announce=True) -> str:
    """The NEWT_GATE_CONTROLS value for a check.py tier."""
    if tier == 'full':
        return 'all'
    if tier == 'push':
        return ''
    base = subprocess.run(['git', 'merge-base', 'HEAD', BASE], cwd=root, capture_output=True, text=True)
    if base.returncode:  # Without the base, run them all: more work, never less proof.
        if announce:
            print(f'TIER local: no {BASE} to diff against, so every negative control runs (git fetch origin)', flush=True)
        return 'all'
    changed = subprocess.run(['git', 'diff', '--name-only', base.stdout.strip(), '--', *TEST_PATHS], cwd=root,
                             check=True, capture_output=True, text=True).stdout.split()
    changed += subprocess.run(['git', 'ls-files', '--others', '--exclude-standard', '--', *TEST_PATHS], cwd=root,
                              check=True, capture_output=True, text=True).stdout.split()
    selected = changed_tests(root, changed)
    if selected is None:
        if announce:
            print('TIER local: a changed test/harness input has no owner, so every negative control runs',
                  flush=True)
        return 'all'
    return ','.join(selected)


def changed_tests(root, changed):
    """The test scripts whose controls the changed tests/ files need: each test
    a changed file's name starts with (a fixture, header or C++ half named after
    its test), and each test whose sources name it, directly or through other
    tests/ files (an include, path or module). None, meaning all, when a changed
    file has no such test: more work, never less proof."""
    root = Path(root)
    positives = {key for name in changed for key in positive_owners(name)}
    changed = [name for name in changed if not positive_owners(name)
               and (name.startswith(('tests/', 'rust/tests/', 'rust-health/'))
                    or name in SHARED_INPUTS
                    or (Path(name).parent.as_posix() == 'rust' and Path(name).suffix in ('.py', '.json')))]
    native = [name for name in changed if Path(name).parts[0] == 'tests']
    rust = [name for name in changed if name not in native]
    rust_selected = changed_rust_tests(root, rust)
    if rust_selected is None:
        return None
    tests = root / 'tests'
    names = {}  # each name a reference can use, to its files
    files = [path for path in tests.rglob('*') if path.is_file() and '__pycache__' not in path.parts]
    for path in files:
        for name in (path.name, *((path.stem,) if path.suffix == '.py' else ())):
            names.setdefault(name, set()).add(path)
    named = {path: {other for token in set(re.findall(r'[\w.-]+', path.read_text(errors='ignore')))
                    for other in names.get(token.strip('.'), ())}
             for path in files if path.suffix in TEXT}
    sources = {}
    for script in tests.glob('*_test.py'):
        reached, pending = set(), [script]
        while pending:
            for other in named.get(pending.pop(), ()):
                if other not in reached:
                    reached.add(other)
                    pending.append(other)
        sources[script.name] = {path.name for path in reached | {script}}
    selected = set(rust_selected) | positives
    for name in map(lambda path: Path(path).name, native):
        users = {script for script, used in sources.items()
                 if name in used or name.split('.')[0].startswith(script.removesuffix('_test.py'))}
        if not users:
            return None
        selected |= users
    return tuple(sorted(selected))


def rust_references(root, path, files):
    """Resolve Python imports and explicit file paths without executing a harness."""
    tree = ast.parse(path.read_text())
    references = set()
    bases = (path.parent, root, root / 'rust', root / 'rust/tests',
             root / 'rust/tests/inflate', root / 'rust-health')
    def parts(node):
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            return [node.value]
        if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Div):
            return parts(node.left) + parts(node.right)
        return []
    for node in ast.walk(tree):
        modules = ([alias.name for alias in node.names] if isinstance(node, ast.Import) else
                   [node.module] if isinstance(node, ast.ImportFrom) and node.module else [])
        for module in modules:
            for base in bases:
                candidate = base / (module.replace('.', '/') + '.py')
                if candidate in files:
                    references.add(candidate)
                    break
        if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Div):
            suffix = parts(node)
            if suffix:
                explicit = len(suffix) > 1 or '/' in suffix[0]
                path_bases = bases if explicit and not suffix[0].startswith('.') else (path.parent,)
                for base in path_bases:
                    candidate = Path(os.path.normpath(base.joinpath(*suffix)))
                    if candidate in files:
                        references.add(candidate)
                        break
    return references


def changed_rust_tests(root, changed):
    """Resolve full changed paths against family roots and shared named inputs.

    Packets are path-owned. Shared Python dependencies follow resolved imports
    and explicit paths, never bare filenames. Unowned inputs fail closed.
    """
    if not changed:
        return ()
    inputs = rust_test_inputs(root)
    for script, seeds in inputs.items():
        if script.startswith('rust:'):
            packet = 'rust/tests/' + script.removeprefix('rust:') + '/'
            seeds.update(root / name for name in changed if name.startswith(packet))
            seeds.update(root / name for name in SHARED_INPUTS)
    health = root / 'rust-health'
    inputs['rust_health_test.py'] = {path for path in health.rglob('*') if path.is_file()}
    for script, packet in RUST_PACKETS.items():
        inputs[script] = {path for path in (root / packet).rglob('*') if path.is_file()}
        inputs[script].update(root / name for name in changed if name.startswith(packet))
    files = {path for folder in (root / 'rust', health, root / 'tools') for path in folder.rglob('*')
             if path.is_file() and path.suffix in TEXT
             and 'target' not in path.parts and '__pycache__' not in path.parts}
    files.update(root / name for name in changed)
    named = {path: rust_references(root, path, files) for path in files
             if path.suffix == '.py' and path.is_file()}
    sources = {}
    for script, seeds in inputs.items():
        reached, pending = set(seeds), list(seeds)
        while pending:
            for other in named.get(pending.pop(), ()):
                if other not in reached:
                    reached.add(other)
                    pending.append(other)
        sources[script] = reached
    selected = set()
    for name in changed:
        users = {script for script, used in sources.items() if root / name in used}
        if not users:
            return None
        selected.update(users)
    return tuple(sorted(selected))

def release_merge(root, commit):
    """Its version differs from its first parent's (a root commit counts)."""
    parent = subprocess.run(['git', 'rev-parse', '--verify', '--quiet', commit + '^1'], cwd=root,
                            capture_output=True, text=True).stdout.strip()
    return not parent or config_version(root, commit) != config_version(root, parent)


def ci_tier(root, commit):
    """local for a pull request, a candidate like any other; full for a release
    merge, a scheduled (nightly) or manual run; push for every other main push.

    Nothing can lower a push run's tier, so the release-tag job, which tags only
    the release merge its own run checked, never tags without a full-tier pass."""
    event = os.environ.get('GITHUB_EVENT_NAME')
    if event == 'pull_request':
        return 'local'
    return 'full' if release_merge(root, commit) or event != 'push' else 'push'


def release_push(root, commit):
    """A release merge's push run: the one run whose gate job CI never cancels,
    so it always finishes and tags that release (.github/workflows/fresh-gates.yml).
    Every other push or nightly run may be cancelled by a newer one."""
    return os.environ.get('GITHUB_EVENT_NAME') == 'push' and release_merge(root, commit)


def time_budget(tier, part, path=None):
    """A tier's whole wall-time budget in seconds, from the hand-edited policy:
    part 'check' for tools/check.py, 'ci' for the CI job (tools/ci_gate.py).
    The local tier shares the push tier's budget."""
    policy = json.loads(Path(path or Path(__file__).with_name('gate-time-budgets.json')).read_text())
    entry = policy['tiers']['push' if tier == 'local' else tier]
    seconds = entry.get(part + '_seconds')
    if (policy.get('schema') != 1 or type(seconds) not in (int, float) or not math.isfinite(seconds)
            or seconds <= 0 or not entry.get('reason', '').strip()):
        raise ValueError(f'Gate time budget for {tier} {part} needs positive seconds and a reason')
    return seconds


def check_time_budget(name, elapsed, budget, timings, top=10):
    """Fail loudly once a whole run outgrew its tier's wall-time budget, naming
    the phases that cost most. The answer is to cut or move work, never to
    shard it: sharding spreads CPU, it never removes it. On a host with more
    runnable work than threads (5-minute load above its thread count, as when
    two gates share it) the wall time measures the host, not the gate: the
    overrun is reported just as loudly, but does not fail."""
    loads, threads = os.getloadavg(), os.cpu_count() or 1
    load = ' '.join(f'{value:.1f}' for value in loads)
    if elapsed <= budget:
        print(f'TIME {name}: {elapsed:.0f}s of its {budget:.0f}s budget (load {load})', flush=True)
        return
    rows = sorted(timings.items(), key=lambda item: -item[1].get('cpu_seconds', item[1].get('wall_seconds', 0)))
    offenders = '\n'.join(f"  {row.get('cpu_seconds', 0):8.1f}s CPU {row.get('wall_seconds', 0):8.1f}s wall  {phase}"
                          for phase, row in rows[:top])
    total = sum(row.get('cpu_seconds', 0) for row in timings.values())
    cpu = f'; {total:.0f}s CPU in its phases' if total else ''
    message = (f'{name} took {elapsed:.0f}s, over its {budget:.0f}s budget '
               f'(load {load} on {threads} threads{cpu}). Top offenders:\n{offenders}\n'
               'Cut the work or move it to the full tier (docs/CI.md, "Tiers"); do not shard to fit.')
    if loads[1] > threads:
        print(f'TIME OVER BUDGET on a busy host, so not failing; rerun on an idle host to time it. {message}',
              flush=True)
        return
    raise GateFailure(message)


def phase_seconds(root, path=SECONDS) -> dict:
    """The last runs' wall seconds per phase (or SHARD_SECONDS' CPU seconds).
    They only order the phases' starts (gate_runner.supervise) or balance
    shards, so a missing or malformed file, or entry, is none."""
    try:
        data = json.loads((Path(root) / path).read_text())
    except (OSError, ValueError):
        return {}
    return {name: value for name, value in data.items() if type(value) in (int, float)
            and math.isfinite(value) and value >= 0} if isinstance(data, dict) else {}


def save_phase_seconds(root, timings):
    seconds = phase_seconds(root) | {name: round(row['wall_seconds'], 1) for name, row in timings.items()
                                     if 'wall_seconds' in row}
    write_text(Path(root), Path(root) / SECONDS, json.dumps(seconds, indent=1, sort_keys=True) + '\n')


def shard(plan, index, count, seconds):
    """Shard index (from 0) of count: the plan phases one CI job runs, in plan
    order, so the shards together run every phase exactly once.

    Phases joined by `after` (one reads another's output) stay in one shard, so
    each shard is a complete plan of its own. Whole groups go, heaviest first,
    to the lightest shard so far, weighed by seconds (a phase without one
    weighs the median). `uses` only serializes phases on one machine, so a
    group goes to a shard holding none of its `uses` paths when there is one,
    and such phases run in parallel on separate jobs. Every job computes the
    same split from the same plan and seconds."""
    if not 0 <= index < count:
        raise ValueError(f'No shard {index} of {count}')
    group = {phase.name: phase.name for phase in plan}

    def root(name):
        while group[name] != name:
            name = group[name]
        return name
    for phase in plan:
        for other in phase.after:
            group[root(other)] = root(phase.name)
    groups = {}
    for phase in plan:
        groups.setdefault(root(phase.name), []).append(phase.name)
    default = statistics.median(seconds.values()) if seconds else 1

    def weight(names):
        return sum(seconds.get(name, default) for name in names)
    uses = {phase.name: set(phase.uses) for phase in plan}
    loads, held, chosen = [0] * count, [set() for _ in range(count)], set()
    for names in sorted(groups.values(), key=lambda names: -weight(names)):  # stable: ties keep plan order
        paths = set().union(*(uses[name] for name in names))
        free = [shard for shard in range(count) if not held[shard] & paths] or range(count)
        lightest = min(free, key=loads.__getitem__)  # ties: the lowest shard
        loads[lightest] += weight(names)
        held[lightest] |= paths
        if lightest == index:
            chosen.update(names)
    return [phase for phase in plan if phase.name in chosen]


def main():
    parser = argparse.ArgumentParser(description='What CI runs for a commit.')
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument('--release-push', metavar='COMMIT', help="print true for a release merge's push run, else false")
    action.add_argument('--ci-tier', metavar='COMMIT', help='print the tier this CI run takes')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    if args.ci_tier:
        print(ci_tier(root, args.ci_tier))
    else:
        print('true' if release_push(root, args.release_push) else 'false')


if __name__ == '__main__':
    main()
