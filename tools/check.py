#!/usr/bin/env python3
"""Complete host gate. This does not compile or flash ESP32 firmware."""
import os
import sys

sys.dont_write_bytecode = True
sys.pycache_prefix = None
os.environ.pop("PYTHONPYCACHEPREFIX", None)
# The gate's tests check with assert; -O or PYTHONOPTIMIZE would remove every check.
if sys.flags.optimize:
    sys.exit("The host gate needs its assertions: run it without -O or PYTHONOPTIMIZE")
# Sanitized test binaries fail on any report, as in CI (tools/ci_gate.py): a
# caller's ASAN_OPTIONS=exitcode=0 or UBSAN_OPTIONS=halt_on_error=0 would pass them.
os.environ.update(UBSAN_OPTIONS="halt_on_error=1", ASAN_OPTIONS="halt_on_error=1:abort_on_error=1")

HANDOFF_GUARD = "NEWT_CHECK_PYTHON_HANDOFF"


def handoff_to_pinned_python(root: str) -> None:
    """Re-exec under the checkout's pinned Python when another interpreter started the gate.

    A distro python3 fails the strict runtime check (tools/python_runtime.py),
    and may be too old for the gate's imports, so this runs before any of them
    and uses only os and sys. No pinned Python is an error, never a fallback.
    """
    pinned = os.path.join(root, ".toolchain/platformio/penv/bin/python")
    target = os.path.realpath(pinned)
    attempted = os.environ.pop(HANDOFF_GUARD, None)
    if os.path.realpath(sys.executable) == target:
        return
    if attempted is not None:
        sys.exit(f"The host gate handed off to {target} but is running under {sys.executable}")
    if not (os.path.isfile(target) and os.access(target, os.X_OK)):
        sys.exit(f"The host gate runs under the checkout's pinned Python, {pinned}, which is missing. "
                 "Set up the toolchain first (README.md, Build from source).")
    os.environ[HANDOFF_GUARD] = target
    os.execv(target, [target, "-B", os.path.abspath(__file__), *sys.argv[1:]])


if __name__ == "__main__":
    handoff_to_pinned_python(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import argparse
import time
import subprocess

import gate_plan
import gate_runner
import gate_tier
import gate_scratch
import check_scratch
import host_jobs
from gate_runner import phase_run
from host_common import ROOT
import rust_preview
from trusted_host import SDK_PIN as FREEINK_COMMIT, verify_source_pin


def phase_compile(name, build):
    started = time.perf_counter()
    print(f"START compile {name}", flush=True)
    try:
        binary = build()
    except (OSError, RuntimeError, subprocess.CalledProcessError):
        print(f"FAIL compile {name} ({time.perf_counter() - started:.3f}s)", flush=True)
        raise
    print(f"PASS compile {name} ({time.perf_counter() - started:.3f}s)", flush=True)
    return binary


def focused(suite):
    if suite == 'cache':
        phase_run([sys.executable, '-B', str(ROOT / 'tests/host_objects_test.py')], check=True, cwd=ROOT,
                  cpu_budgets=gate_runner.cpu_budgets())
    else:
        binary = phase_compile('newt-preview', lambda: rust_preview.cli(rust_preview.build()))
        for script in ('library_preview_test.py', 'library_cover_prepare_test.py', 'system_ui_contract_test.py',
                       'system_ui_rule_test.py', 'preview_owners_test.py', 'preview_update_test.py'):
            command = [sys.executable, '-B', str(ROOT / 'tests' / script)]
            command.append('--portable-only' if script == 'library_cover_prepare_test.py' else str(binary))
            phase_run(command, check=True, cwd=ROOT,
                      cpu_budgets=gate_runner.cpu_budgets())
    print(f"PASS development suite {suite}; complete host/native gates and physical proof remain required", flush=True)


def shard_argument(text):
    index, _, count = text.partition('/')
    if not (index.isdigit() and count.isdigit() and int(index) < int(count)):
        raise argparse.ArgumentTypeError(f'needs I/N with 0 <= I < N, not {text!r}')
    return int(index), int(count)


def configured_main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--suite', choices=('all', 'quick', 'ui', 'cache'), default='all',
                        help='Focused suites are development feedback; all is the required host gate')
    parser.add_argument('--tier', choices=('local', 'push', 'full'), default='local',
                        help="local: a candidate's gate, the push tier plus the negative controls of the tests "
                             'this branch changes; push: every positive test (CI per push); full: every '
                             'negative control too (CI on release merges and nightly). See tools/gate_tier.py')
    parser.add_argument('--release-tags', action='store_true',
                        help='Also check the release tags (tools/release_tools.py): CI and release validation. '
                             'A routine run reads no tag, so a missing tag never fails it')
    parser.add_argument('--no-time-budget', action='store_true',
                        help="Skip the tier's wall-time budget: the caller budgets the whole run (tools/ci_gate.py)")
    parser.add_argument('--fresh-objects', action='store_true', help='Recompile reusable objects for cache comparison')
    parser.add_argument('--profile', choices=tuple(gate_plan.PROFILES), default='linux',
                        help='Host gate profile (Linux x86_64 only; uses portable cover checks)')
    parser.add_argument('-j', '--jobs', type=int, default=gate_runner.default_jobs(),
                        help='Phases and compiler jobs at once (default: the usable CPUs); 1 is the serial gate')
    parser.add_argument('--host-jobs', type=int, help='Per-user phase/compiler slots across checkouts (default usable CPUs; NEWT_HOST_GLOBAL_JOBS)')
    parser.add_argument('--keep-going', action='store_true',
                        help='After a failure, still run every phase whose prerequisites passed, then report all failures')
    parser.add_argument('--shard', metavar='I/N', type=shard_argument,
                        help="Run only shard I (from 0) of N of the tier's plan, as one CI job does (tools/gate_tier.py)")
    args = parser.parse_args()
    if args.shard and args.suite != 'all':
        parser.error('--shard splits the complete gate (--suite all)')
    args.started = STARTED
    if args.jobs < 1:
        parser.error('--jobs must be at least 1')
    if args.host_jobs is not None and args.host_jobs < 1:
        parser.error('--host-jobs must be at least 1')
    try:
        gate_plan.require_host(args.profile)
    except (RuntimeError, ValueError) as error:
        parser.error(str(error))
    if args.fresh_objects:
        os.environ['NEWT_HOST_OBJECT_CACHE'] = '0'
    with gate_runner.job_environment(ROOT, args.jobs, args.host_jobs):
        source = ('--host-jobs' if args.host_jobs is not None else
                  host_jobs.GLOBAL_JOBS if os.environ.get(host_jobs.GLOBAL_JOBS) else
                  host_jobs.available_cpus()[1])
        print(f"HOST slots capacity {host_jobs.global_limit()} ({source}) shared across this user's checkouts; local jobs {args.jobs}", flush=True)
        gate(args)


def gate(args) -> None:
    if args.suite == 'all':
        # Before the release tests, which run their negative controls by this selection too.
        included = gate_tier.controls(args.tier, ROOT)
        os.environ[gate_tier.CONTROLS] = included
    phase_run([sys.executable, "-B", str(ROOT / "tools/release_tools.py"), *([] if args.release_tags else ["--no-tags"])],
              check=True, cwd=ROOT, cpu_budgets=gate_runner.cpu_budgets())
    phase_run([sys.executable, "-B", str(ROOT / "tests/release_tools_test.py")], check=True, cwd=ROOT,
              cpu_budgets=gate_runner.cpu_budgets())
    verify_source_pin()
    if args.suite == 'quick':
        gate_runner.run(gate_plan.quick(args.profile, args.fresh_objects), ROOT, args.jobs, args.keep_going,
                        cpu_budgets=gate_runner.cpu_budgets())
        print("PASS development suite quick; complete host/native gates and physical proof remain required",
              flush=True)
        return
    if args.suite != 'all':
        focused(args.suite)
        return
    print(f"PASS: official FreeInk source {FREEINK_COMMIT}, checkout inputs clean", flush=True)
    print(f"TIER {args.tier}: negative controls of {included or 'no test'}", flush=True)
    phases, complete, weights = gate_plan.host_phases(ROOT, args.profile, args.fresh_objects, included, args.shard)
    # A fresh CI runner has no last run to order its starts.
    seconds = weights | gate_tier.phase_seconds(ROOT)
    if args.shard:
        print(f"SHARD {args.shard[0]}/{args.shard[1]}: {len(phases)} of the tier's {complete} phases", flush=True)
    timings = gate_runner.run(phases, ROOT, args.jobs, args.keep_going, cpu_budgets=gate_runner.cpu_budgets(),
                              seconds=seconds)
    gate_tier.save_phase_seconds(ROOT, timings)
    if not args.no_time_budget:
        gate_tier.check_time_budget(f'{args.tier} tier host gate', time.perf_counter() - args.started,
                                    gate_tier.time_budget(args.tier, 'check'), timings)
    print("PASS: host compile (ASan/UBSan), Rust checks and CLI validation")
    print('Linux software coverage: portable cover/cache controls; '
          'for Mac ImageIO coverage see docs/CI.md#explicit-cover-coverage-boundary')
    print("Hardware firmware compilation and physical bring-up are separate gates.")


def main() -> None:
    with gate_scratch.environment(ROOT):
        check_scratch.check(ROOT)
        configured_main()


STARTED = time.perf_counter()

if __name__ == "__main__":
    try:
        main()
    finally:
        print(f"DONE host gate invocation ({time.perf_counter() - STARTED:.3f}s)", flush=True)
