"""Gate phase runner: serial order, declared orderings, failure and output controls."""
from contextlib import redirect_stdout
import dataclasses
import io
import itertools
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
import gate_plan
import gate_runner
import gate_tier
from gate_runner import GateFailure, Phase
import host_jobs

# Each step appends "start NAME" and "end NAME" to a journal, holding for a
# short time in between, so overlaps and order are observable. A step that
# names journal events also holds until they appear (for at most a minute), so
# the overlaps and orders a test asserts never depend on host load.
STEP = ('import sys, time\n'
        'journal, name, hold, code = sys.argv[1], sys.argv[2], float(sys.argv[3]), int(sys.argv[4])\n'
        'awaited = [event for event in sys.argv[5].split(",") if event]\n'
        'open(journal, "a").write(f"start {name}\\n")\n'
        'print(f"output of {name}", flush=True)\n'
        'time.sleep(hold)\n'
        'deadline = time.monotonic() + 60\n'
        'while time.monotonic() < deadline and not set(awaited) <= set(open(journal).read().splitlines()):\n'
        '    time.sleep(0.01)\n'
        'open(journal, "a").write(f"end {name}\\n")\n'
        'raise SystemExit(code)\n')

# Like STEP, but holds until the runner has printed `line` (e.g. 'PASS fast')
# to the gate's output file, so it finishes strictly after that report.
REPORTED = ('import sys, time\n'
            'journal, name, output, line = sys.argv[1:5]\n'
            'open(journal, "a").write(f"start {name}\\n")\n'
            'print(f"output of {name}", flush=True)\n'
            'deadline = time.monotonic() + 60\n'
            'while time.monotonic() < deadline and not any(\n'
            '        printed.startswith(line + " (") for printed in open(output).read().splitlines()):\n'
            '    time.sleep(0.01)\n'
            'open(journal, "a").write(f"end {name}\\n")\n')

# A gate whose first phase's process prints, then runs its ending statement.
# That phase runs alone, so the independent phase after it starts only after its
# result, whatever the job count, and "required" fails whenever it runs. Run as
# a process, as check.py runs the gate: its exit status is the gate's.
ENDS = 'import os, signal, sys\nprint("output before ending", flush=True)\nexec(sys.argv[1])\n'
RAN = 'import sys\nprint(sys.argv[1], "ran", flush=True)\nif sys.argv[1] == "required":\n    sys.exit(1)\n'
GATE = ('import sys\n'
        'from pathlib import Path\n'
        'sys.path.insert(0, sys.argv[1])\n'
        'from gate_runner import GateFailure, Phase, run\n'
        'ends, ran, ending, jobs, keep_going = sys.argv[2:7]\n'
        'def step(name, **options):\n'
        '    return Phase(name, (sys.executable, "-B", "-c", ran, name), **options)\n'
        'plan = [Phase("ending", (sys.executable, "-B", "-c", ends, ending), alone=True), step("independent"),\n'
        '        step("dependent", after=("ending",)), step("required", after=("dependent",))]\n'
        'try:\n'
        '    run(plan, Path.cwd(), int(jobs), keep_going == "keep-going")\n'
        'except GateFailure as failure:\n'
        '    print(f"GateFailure: {failure}", flush=True)\n'
        '    raise SystemExit(1)\n'
        'print("gate passed", flush=True)\n')
# Each ending, with its phase's result detail; None passes. A process's exit
# status is its result: SystemExit(0) and os._exit(0) pass, as in a serial
# gate; a signal ends a process without one.
ENDINGS = (('raise SystemExit(0)', None), ('raise SystemExit(None)', None), ('raise SystemExit(1)', ', exit 1'),
           ("raise SystemExit('msg')", ', exit 1'), ('raise KeyboardInterrupt', ', exit -2'),
           ('raise GeneratorExit', ', exit 1'), ('os._exit(0)', None), ('os._exit(3)', ', exit 3'),
           ('os.kill(os.getpid(), signal.SIGKILL)', ', exit -9'))

class RunnerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / '.build').mkdir()
        pool = patch.dict(os.environ, {host_jobs.GLOBAL_JOBS: '8', host_jobs.GLOBAL_ROOT: str(self.root / 'host-pool')})
        pool.start()
        self.addCleanup(pool.stop)
        self.journal = self.root / 'journal'
        self.output = self.root / 'output'

    def tearDown(self):
        self.temp.cleanup()

    def phase(self, name, hold=0.0, code=0, awaits=(), **options):
        return Phase(name, (sys.executable, '-B', '-c', STEP, str(self.journal), name, str(hold), str(code),
                            ','.join(awaits)), **options)

    def reported(self, name, line):
        """A phase that journals like a step but ends only once the runner has
        printed the result `line` (e.g. 'PASS fast'), so it finishes strictly
        after that report."""
        return Phase(name, (sys.executable, '-B', '-c', REPORTED, str(self.journal), name, str(self.output), line))

    def run_plan(self, plan, jobs, keep_going=False):
        """Run a plan, its runner output in a file that phases can read."""
        failure = None
        with open(self.output, 'w') as output, redirect_stdout(output):
            try:
                gate_runner.run(plan, self.root, jobs, keep_going)
            except GateFailure as error:
                failure = error
        events = self.journal.read_text().split('\n')[:-1] if self.journal.exists() else []
        return events, self.output.read_text(), failure

    def overlapping(self, events, first, second):
        """True when one phase started while the other was still running."""
        active, seen = set(), False
        for event in events:
            kind, name = event.split(' ', 1)
            if kind == 'start':
                seen |= (name == first and second in active) or (name == second and first in active)
                active.add(name)
            else:
                active.discard(name)
        return seen

    def test_one_job_runs_the_declared_order_and_streams(self):
        plan = [self.phase(name, 0.05) for name in 'abcdef']
        events, output, failure = self.run_plan(plan, 1)
        self.assertIsNone(failure)
        self.assertEqual(events, [f'{kind} {name}' for name in 'abcdef' for kind in ('start', 'end')])
        self.assertEqual([line.split(' (')[0] for line in output.splitlines() if line.startswith(('START', 'PASS'))],
                         [f'{kind} {name}' for name in 'abcdef' for kind in ('START', 'PASS')])

    def test_parallel_phases_overlap_only_where_nothing_orders_them(self):
        plan = [self.phase('producer', awaits=('start free',)),
                self.phase('free', awaits=('start producer', 'start shared-a')),
                self.phase('consumer', 0.1, after=('producer',)),
                self.phase('shared-a', 0.3, uses=('path',)), self.phase('shared-b', 0.1, uses=('path',)),
                self.phase('alone', 0.1, alone=True), self.phase('later', 0.1), self.phase('quiet', 0.1, alone=True)]
        events, output, failure = self.run_plan(plan, 4)
        self.assertIsNone(failure)
        self.assertTrue(self.overlapping(events, 'producer', 'free'))
        self.assertTrue(self.overlapping(events, 'free', 'shared-a'))
        self.assertLess(events.index('end producer'), events.index('start consumer'))
        self.assertLess(events.index('end shared-a'), events.index('start shared-b'))
        for single in ('alone', 'quiet'):
            self.assertIn(f'end {single}', events)
            for name in ('producer', 'free', 'consumer', 'shared-a', 'shared-b', 'later', 'alone', 'quiet'):
                if name != single:
                    self.assertFalse(self.overlapping(events, single, name), (single, name))
        # A running alone phase also holds back phases that become ready after it.
        self.journal.unlink()
        events, _, failure = self.run_plan([self.phase('first', 0.2, alone=True), self.phase('next'),
                                            self.phase('last')], 3)
        self.assertIsNone(failure)
        self.assertEqual(events[:2], ['start first', 'end first'])

    def test_buffered_output_is_whole_labelled_and_in_completion_order(self):
        plan = [self.reported('slow', 'PASS fast'), self.phase('fast', 0.0)]
        events, output, failure = self.run_plan(plan, 2)
        self.assertIsNone(failure)
        lines = [line.split(' (')[0] for line in output.splitlines()]
        self.assertEqual(lines, ['START fast', 'output of fast', 'PASS fast',
                                 'START slow', 'output of slow', 'PASS slow'])

    def test_first_failure_stops_scheduling_and_waits_for_running_phases(self):
        plan = [self.reported('long', 'FAIL broken'), self.phase('broken', 0.0, code=3),
                self.phase('queued', 0.0), self.phase('dependent', 0.0, after=('broken',))]
        events, output, failure = self.run_plan(plan, 2)
        self.assertIn('broken', str(failure))
        self.assertIn('end long', events)
        self.assertNotIn('start queued', events)
        self.assertNotIn('start dependent', events)
        self.assertIn('FAIL broken', output)
        self.assertIn('exit 3', output)
        self.assertIn('PASS long', output)
        self.journal.unlink()
        events, _, failure = self.run_plan([self.phase('first', code=1), self.phase('second')], 1)
        self.assertIsNotNone(failure)
        self.assertEqual(events, ['start first', 'end first'])

    def test_keep_going_runs_independent_phases_and_reports_every_failure(self):
        for jobs in (1, 3):
            with self.subTest(jobs=jobs):
                self.journal.unlink(missing_ok=True)
                plan = [self.phase('one', code=1), self.phase('two', code=2),
                        self.phase('independent'), self.phase('dependent', after=('one',)),
                        self.phase('transitive', after=('dependent',))]
                events, output, failure = self.run_plan(plan, jobs, keep_going=True)
                self.assertIn('one', str(failure))
                self.assertIn('two', str(failure))
                self.assertIn('2 not run', str(failure))
                self.assertIn('end independent', events)
                self.assertNotIn('start dependent', events)
                self.assertNotIn('start transitive', events)

    def test_unlaunchable_commands_fail_their_phase_and_others_still_report(self):
        missing = self.root / 'missing-interpreter'
        for jobs in (1, 2):
            with self.subTest(jobs=jobs):
                self.journal.unlink(missing_ok=True)
                plan = [self.phase('running', 0.2), Phase('unlaunchable', (missing,)), self.phase('started')]
                events, output, failure = self.run_plan(plan, jobs, keep_going=True)
                self.assertEqual(str(failure), 'Gate phases failed: unlaunchable')
                self.assertIn('FileNotFoundError', output)
                self.assertIn('FAIL unlaunchable', output)
                self.assertIn('PASS running', output)
                self.assertIn('PASS started', output)

    def ending_problems(self, tools, ending, jobs, keep_going):
        """How a gate misreports, if it does, when its first phase prints and
        then ends with `ending` (a statement and its result detail)."""
        statement, detail = ending
        result = subprocess.run([sys.executable, '-B', '-c', GATE, str(tools), ENDS, RAN, statement, str(jobs),
                                 'keep-going' if keep_going else 'stop'],
                                cwd=self.root, capture_output=True, text=True, timeout=60)
        lines, problems, passes = result.stdout.splitlines(), [], detail is None
        if result.returncode != 1:
            problems.append(f'the gate exited {result.returncode}')
        status = 'PASS ending' if passes else 'FAIL ending'
        reported = [index for index, line in enumerate(lines)
                    if re.fullmatch(re.escape(status) + r' \([0-9.]+s\)' + re.escape(detail or ''), line)]
        printed = [index for index, line in enumerate(lines) if line == 'output before ending']
        if not reported:
            problems.append(f'no {status}{detail or ""} result')
        if not printed:
            problems.append('output lost')
        if reported and printed and printed[0] > reported[0]:
            problems.append('output after the result')
        # A passing ending lets every phase run, so the required failure fails the
        # gate. A failing one leaves its dependents unrun; the independent phase
        # runs only with keep_going.
        expected = ['independent', 'dependent', 'required'] if passes else ['independent'] if keep_going else []
        ran = [line.split()[0] for line in lines if line in ('independent ran', 'dependent ran', 'required ran')]
        if sorted(ran) != sorted(expected):
            problems.append(f'ran {ran}, not {expected}')
        unrun = [name for name in ('independent', 'dependent', 'required') if name not in expected]
        failure = 'GateFailure: Gate phases failed: ' + (
            'required' if passes else f"ending; {len(unrun)} not run: {', '.join(unrun)}")
        if failure not in lines:
            problems.append(f'missing {failure!r}')
        return problems, result

    def test_no_phase_ending_ends_the_gate_or_bypasses_it(self):
        # Every phase is its own process: however it ends (SystemExit, an
        # interrupt, a hard exit or a signal), the runner reports its result with
        # its output, and the gate still runs every other phase it can.
        tools = Path(__file__).resolve().parents[1] / 'tools'
        for ending, jobs, keep_going in itertools.product(ENDINGS, (1, 2), (False, True)):
            with self.subTest(ending=ending[0], jobs=jobs, keep_going=keep_going):
                problems, result = self.ending_problems(tools, ending, jobs, keep_going)
                self.assertEqual(problems, [], result)

    def test_in_process_phases_fail_the_ending_controls(self):
        # Negative control: the runner rebuilt to run each phase's code in its
        # own process, as in-process actions did. A hard exit then ended the gate
        # with status 0 before any result, so the required failure never ran.
        tools = self.root / 'in-process-tools'
        tools.mkdir()
        current = Path(__file__).resolve().parents[1] / 'tools'
        (tools / 'host_jobs.py').write_text((current / 'host_jobs.py').read_text())
        (tools / 'gate_child.py').write_text((current / 'gate_child.py').read_text())
        source = (current / 'gate_runner.py').read_text()
        launch = '        result = children.run(list(map(str, phase.command)), phase_name=phase.name, cwd=root, **streams)\n'
        self.assertEqual(source.count(launch), 1)
        (tools / 'gate_runner.py').write_text(source.replace(launch, (
            "        sys.argv = ['-c', *map(str, phase.command[4:])]; exec(phase.command[3], {'__name__': '__main__'})\n"
            "        result = subprocess.CompletedProcess(phase.command, 0, b'')\n")))
        for ending, jobs in itertools.product(ENDINGS, (1, 2)):
            with self.subTest(ending=ending[0], jobs=jobs):
                problems, result = self.ending_problems(tools, ending, jobs, False)
                self.assertNotEqual(problems, [], result)
                if ending[0] == 'os._exit(0)':
                    self.assertIn('the gate exited 0', problems, result)
                    self.assertIn("ran [], not ['independent', 'dependent', 'required']", problems, result)

    def test_phases_are_only_commands(self):
        with self.assertRaises(TypeError):
            Phase('action', action=print)
        self.assertEqual([field.name for field in dataclasses.fields(Phase)],
                         ['name', 'command', 'after', 'uses', 'alone', 'first'])

    def test_invalid_plans_are_rejected(self):
        for plan in ([self.phase('same'), self.phase('same')],
                     [self.phase('early', after=('late',)), self.phase('late')],
                     [Phase('nothing')]):
            with self.assertRaises(ValueError):
                gate_runner.run(plan, self.root, 2)

    def test_job_environment_bounds_compiler_jobs_and_restores(self):
        saved = {name: os.environ.get(name) for name in (host_jobs.JOBS, host_jobs.SLOTS)}
        with gate_runner.job_environment(self.root, 1):
            self.assertEqual(os.environ[host_jobs.JOBS], '1')
            self.assertNotIn(host_jobs.SLOTS, os.environ)
            self.assertEqual(host_jobs.job_limit(), 1)
        with gate_runner.job_environment(self.root, 3):
            slots = Path(os.environ[host_jobs.SLOTS])
            self.assertEqual(sorted(path.name for path in slots.iterdir()), ['slot-0', 'slot-1', 'slot-2'])
            holding = []
            with host_jobs.slot(), host_jobs.slot(), host_jobs.slot():
                started = time.monotonic()
                # A fourth holder must wait for a slot; a thread proves it blocks.
                waiter = threading.Thread(target=lambda: holding.append(host_jobs.slot().__enter__()))
                waiter.start()
                waiter.join(0.3)
                self.assertTrue(waiter.is_alive())
            waiter.join(5)
            self.assertFalse(waiter.is_alive())
            self.assertGreater(time.monotonic() - started, 0.25)
        self.assertEqual({name: os.environ.get(name) for name in saved}, saved)
        self.assertFalse(slots.exists())

    def test_job_environment_creates_a_missing_build_directory(self):
        (self.root / '.build').rmdir()
        with gate_runner.job_environment(self.root, 2):
            self.assertTrue(Path(os.environ[host_jobs.SLOTS]).is_dir())
        self.assertEqual(list((self.root / '.build').iterdir()), [])

    def test_without_gate_slots_one_process_bounds_nested_compiler_jobs(self):
        saved = {name: os.environ.get(name) for name in (host_jobs.JOBS, host_jobs.SLOTS)}
        os.environ.pop(host_jobs.SLOTS, None)
        os.environ[host_jobs.JOBS] = '2'
        lock, active, peak = threading.Lock(), [0], [0]
        def compiler():
            with host_jobs.slot():
                with lock:
                    active[0] += 1
                    peak[0] = max(peak[0], active[0])
                time.sleep(0.05)
                with lock:
                    active[0] -= 1
        try:
            # Three outer tasks each start three inner jobs: nine threads, two slots.
            host_jobs.run_all([lambda: host_jobs.run_all([compiler] * 3, jobs=3)] * 3, jobs=3)
        finally:
            for name, value in saved.items():
                if value is None:
                    os.environ.pop(name, None)
                else:
                    os.environ[name] = value
        self.assertEqual(peak[0], 2)


class PlanTests(unittest.TestCase):
    """The real gate keeps each output, shared path and timing hazard ordered."""
    def test_bounded_rust_landing_is_two_required_phases(self):
        # Landing's contract test runs beside it, not inside it (#337); every tier keeps both.
        landing = (gate_plan.PYTHON, '-B', str(gate_plan.ROOT / 'rust/check.py'), '--landing')
        contract = (gate_plan.PYTHON, '-B', str(gate_plan.ROOT / 'rust/tests/landing_test.py'))
        for controls in (None, lambda _: False):
            plan = gate_plan.plan('linux', False, controls)
            phases = [phase for phase in plan if '--landing' in phase.command
                      or any(Path(arg).name == 'landing_test.py' for arg in phase.command)]
            self.assertEqual([phase.command for phase in phases], [landing, contract])
            self.assertEqual([phase.uses for phase in phases], [(gate_plan.ROOT / '.build/rust/target',), ()])

    def test_rust_family_controls_follow_the_same_tier_selection(self):
        for chosen in (None, set(), {'rust:foundation', 'rust:xml_input'}, {'rust-positive:inflate'}):
            controls = None if chosen is None else chosen.__contains__
            plan = gate_plan.plan('linux', False, controls)
            phases = [phase for phase in plan if str(gate_plan.ROOT / 'rust/check.py') in phase.command]
            families = [phase for phase in phases if any(arg.startswith('--family=') for arg in phase.command)]
            wanted = {name for name in gate_plan.ADAPTER_FAMILIES
                      if chosen is None or name not in gate_plan.FULL_POSITIVE_SOURCES
                      or 'rust:' + name in chosen or 'rust-positive:' + name in chosen}
            self.assertEqual({arg.removeprefix('--family=') for phase in families for arg in phase.command
                              if arg.startswith('--family=')}, wanted)
            if chosen is None:
                self.assertEqual(len(families) + 1, 20)  # landing owns foundation positives
            for phase in families:
                name = next(arg.removeprefix('--family=') for arg in phase.command if arg.startswith('--family='))
                selected = chosen is None or 'rust:' + name in chosen
                self.assertIn('--control-family=' + (name if selected else 'none'), phase.command)
            campaigns = {arg.split('=', 1)[1] for phase in phases for arg in phase.command
                         if arg.startswith('--foundation-controls=')}
            self.assertEqual(campaigns, set(gate_plan.FOUNDATION_CAMPAIGNS)
                             if chosen is None or 'rust:foundation' in chosen else set())
            self.assertEqual(sum('--target-probes' in phase.command for phase in phases), 1)
            for script in ('rust_health.py', 'rust_health_test.py', 'family_dispatch_test.py'):
                self.assertEqual(sum(any(Path(arg).name == script for arg in phase.command) for phase in plan), 1)
            gate_runner.prerequisites(plan)

    def test_rust_checks_overlap_except_on_the_shared_target(self):
        # Each Rust check owns its receipt and temporary folder (rust/check.py, #345):
        # only landing and the target probes share a Cargo target.
        plan = gate_plan.plan('linux', False)
        needs, closure = gate_runner.prerequisites(plan), {}
        def before(name):
            if name not in closure:
                closure[name] = set(needs[name]).union(*(before(item) for item in needs[name]))
            return closure[name]
        rust = [phase for phase in plan if str(gate_plan.ROOT / 'rust/check.py') in phase.command
                or phase.name == 'legacy_nvs_test.py']
        self.assertEqual(len(rust), 26)
        serial = {'check.py --landing', 'check.py --target-probes'}
        self.assertIn('check.py --landing', before('check.py --target-probes'))
        for phase in rust:
            for other in rust:
                if phase is not other and not {phase.name, other.name} <= serial:
                    self.assertNotIn(other.name, before(phase.name), (phase.name, other.name))

    def test_shared_outputs_and_timing_hazards_are_declared(self):
        for profile in gate_plan.PROFILES:
            for fresh in (False, True):
                with self.subTest(profile=profile, fresh=fresh):
                    plan = gate_plan.plan(profile, fresh)
                    needs, closure = gate_runner.prerequisites(plan), {}
                    def before(name):
                        if name not in closure:
                            closure[name] = set(needs[name]).union(*(before(item) for item in needs[name]))
                        return closure[name]
                    pairs = [('compile newt-preview', name) for name in
                              ('library_preview_test.py', 'system_ui_contract_test.py', 'system_ui_rule_test.py',
                               'preview_owners_test.py', 'preview_update_test.py', 'preview_reader_wake_test.py',
                               'preview_reader_finish_test.py', 'preview_reader_reimport_test.py',
                               'preview_reader_reflow_test.py',
                               'preview_holiday_outfits_test.py',
                               'preview_card_app_test.py', 'preview_cover_test.py', 'preview_cover_status_test.py',
                               'gate_previews.py')]
                    pairs += [(f'compile {name}', name) for _, name in gate_plan.UNIT_TESTS]
                    for first, second in pairs:
                        self.assertIn(first, before(second), (first, second))
                    alone = {phase.name for phase in plan if phase.alone}
                    self.assertEqual(alone, set())

    def test_quick_plan_is_the_complete_plans_phases_with_their_prerequisites(self):
        for profile in gate_plan.PROFILES:
            for fresh in (False, True):
                with self.subTest(profile=profile, fresh=fresh):
                    complete, quick = gate_plan.plan(profile, fresh), gate_plan.quick(profile, fresh)
                    names = [phase.name for phase in quick]
                    self.assertEqual(quick, [phase for phase in complete if phase.name in names])
                    self.assertEqual(set(names) - set(gate_plan.QUICK),
                                     {'compile newt-preview', *(f'compile {unit}' for _, unit in gate_plan.UNIT_TESTS)})
                    gate_runner.prerequisites(quick)
        with patch.object(gate_plan, 'QUICK', (*gate_plan.QUICK, 'retired_test.py')), \
                self.assertRaisesRegex(ValueError, 'retired_test.py'):
            gate_plan.quick('linux', False)

    def test_profiles_match_supported_host_build_platform_and_reject_unsupported(self):
        import check as host_check
        import firmware_host_build

        toolchain = json.loads((gate_plan.ROOT / 'rust/toolchain.json').read_text())
        rust_host_targets = {item['target'] for item in toolchain['components']
                             if item['name'] in ('rustc', 'cargo')}
        self.assertEqual(rust_host_targets, {firmware_host_build.HOST})
        self.assertEqual(tuple(gate_plan.PROFILES), ('linux',))
        for profile in gate_plan.PROFILES:
            _, _, triple = gate_plan.validate_profile(profile)
            self.assertEqual(triple, firmware_host_build.HOST)
            self.assertIn(triple, rust_host_targets)
        cover_phases = [phase for phase in gate_plan.plan('linux', False)
                        if any(Path(arg).name == 'library_cover_prepare_test.py' for arg in phase.command)]
        self.assertEqual(len(cover_phases), 1)
        self.assertIn('--portable-only', cover_phases[0].command)

        # Negative control: a profile naming a platform the firmware host build cannot run on fails.
        bad_profiles = {**gate_plan.PROFILES, 'canonical': ('darwin', 'arm64', 'aarch64-apple-darwin')}
        with patch.object(gate_plan, 'PROFILES', bad_profiles):
            with self.assertRaisesRegex(ValueError, 'aarch64-apple-darwin'):
                gate_plan.validate_profile('canonical')
            with self.assertRaisesRegex(ValueError, 'aarch64-apple-darwin'):
                gate_plan.plan('canonical', False)

        # The removed canonical profile is rejected by both gate_plan and check.py.
        with self.assertRaisesRegex(ValueError, 'Unsupported host gate profile'):
            gate_plan.plan('canonical', False)
        stderr = io.StringIO()
        with patch.object(sys, 'argv', ['check.py', '--profile', 'canonical']), \
                patch('sys.stderr', stderr), self.assertRaises(SystemExit) as exited:
            host_check.configured_main()
        self.assertNotEqual(exited.exception.code, 0)
        self.assertIn('canonical', stderr.getvalue())

        # Non-Linux and non-x86_64 hosts fail clearly with guidance to the Mac cover gate.
        for host_platform, machine in (('darwin', 'arm64'), ('linux', 'aarch64')):
            with self.subTest(host_platform=host_platform, machine=machine), \
                    patch.object(gate_plan.sys, 'platform', host_platform), \
                    patch.object(gate_plan.platform, 'machine', return_value=machine):
                with self.assertRaisesRegex(RuntimeError, r'Linux x86_64.*check-mac-covers\.py'):
                    gate_plan.require_host('linux')
                stderr = io.StringIO()
                with patch.object(sys, 'argv', ['check.py']), \
                        patch('sys.stderr', stderr), self.assertRaises(SystemExit) as exited:
                    host_check.configured_main()
                self.assertNotEqual(exited.exception.code, 0)
                self.assertIn('Linux x86_64', stderr.getvalue())
                self.assertIn('check-mac-covers.py', stderr.getvalue())

    def test_check_entrypoint_rejects_unsupported_hosts_before_pinned_python_handoff(self):
        import check as host_check

        self.assertEqual(host_check.SUPPORTED_HOST, gate_plan.PROFILES['linux'][:2])
        self.assertEqual(host_check.MAC_COVER_NOTE, gate_plan.MAC_COVER_NOTE)
        source = (gate_plan.ROOT / 'tools/check.py').read_text()
        guard = '    require_supported_host()\n'
        self.assertEqual(source.count(guard), 1)

        def run_entry(script_text, host_platform, machine):
            with tempfile.TemporaryDirectory() as directory:
                gate = Path(directory) / 'tools/check.py'
                gate.parent.mkdir()
                gate.write_text(script_text)
                driver = (
                    f"import platform, runpy, sys\n"
                    f"sys.platform = {host_platform!r}\n"
                    f"platform.machine = lambda: {machine!r}\n"
                    f"sys.argv = [{str(gate)!r}]\n"
                    f"runpy.run_path({str(gate)!r}, run_name='__main__')\n"
                )
                return subprocess.run([sys.executable, '-I', '-B', '-c', driver],
                                      capture_output=True, text=True, timeout=60)

        for host_platform, machine in (('darwin', 'arm64'), ('linux', 'aarch64')):
            with self.subTest(host_platform=host_platform, machine=machine):
                result = run_entry(source, host_platform, machine)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('Linux x86_64', result.stderr)
                self.assertIn('check-mac-covers.py', result.stderr)
                self.assertNotIn('pinned Python', result.stderr)

        # Negative control: removing the early host guard falls through to the missing pinned-Python error.
        without_guard = run_entry(source.replace(guard, '', 1), 'darwin', 'arm64')
        self.assertNotEqual(without_guard.returncode, 0)
        self.assertIn('pinned Python', without_guard.stderr)
        self.assertNotIn('check-mac-covers.py', without_guard.stderr)


    def test_push_plan_drops_only_negative_controls(self):
        complete = [phase for phase in gate_plan.plan('linux', False)
                    if str(gate_plan.ROOT / 'rust/check.py') not in phase.command]
        control_renamed = {name for name in (phase.name for phase in complete)
                           if gate_plan.CONTROL_FLAGS.intersection(name.split())}
        full_renamed = {name for name in (phase.name for phase in complete)
                        if gate_plan.FULL_FLAGS.intersection(name.split())}
        tier_flags = gate_plan.CONTROL_FLAGS | gate_plan.FULL_FLAGS
        for chosen in ((), ('bench_flash_test.py', 'installer_test.py')):
            with self.subTest(chosen=chosen):
                push = [phase for phase in gate_plan.plan('linux', False, lambda script: script in chosen)
                        if str(gate_plan.ROOT / 'rust/check.py') not in phase.command]
                gate_runner.prerequisites(push)  # Every ordering still names a phase that runs.
                dropped = {phase.name for phase in complete} - {phase.name for phase in push}
                kept = {name for name in control_renamed if name.split()[0] in chosen}
                expected_dropped = (control_renamed - kept) | full_renamed
                self.assertEqual(dropped, expected_dropped)
                # A renamed phase still runs, without its dropped tier flags.
                for name in expected_dropped:
                    self.assertIn(' '.join(part for part in name.split() if part not in tier_flags),
                                  {phase.name for phase in push})

    def test_large_epub_corpus_runs_full_only_in_full_tier(self):
        """The corpus cases always run; its controls are a second phase that runs
        whenever the test's controls are selected."""
        for controls, want_full, want_controls in (
            (None, True, True),
            (lambda _: False, False, False),
            (lambda script: script == 'large_epub_corpus_test.py', False, True),
        ):
            phases = [phase for phase in gate_plan.plan('linux', False, controls)
                      if any(Path(arg).name == 'large_epub_corpus_test.py' for arg in phase.command)]
            cases = [phase for phase in phases if '--negative-controls' not in phase.command]
            self.assertEqual(len(cases), 1)
            self.assertEqual('--full' in cases[0].command, want_full)
            self.assertEqual(len(phases) - len(cases), int(want_controls))


class StartOrderTests(unittest.TestCase):
    def test_the_longest_remaining_chain_starts_first(self):
        """Last-run seconds reorder ready phases; a prerequisite still runs first."""
        plan = [Phase(name, (sys.executable, '-B', '-c', 'pass'), after=after)
                for name, after in (('short', ()), ('long', ()), ('after-long', ('long',)))]
        for seconds, expected in (({}, ['short', 'long', 'after-long']),
                                  ({'short': 1, 'long': 5, 'after-long': 5}, ['long', 'after-long', 'short'])):
            with self.subTest(seconds=seconds), tempfile.TemporaryDirectory() as directory:
                output = io.StringIO()
                with redirect_stdout(output):
                    gate_runner.run(plan, Path(directory), 1, seconds=seconds)
                self.assertEqual(re.findall(r'^START (\S+)$', output.getvalue(), re.M), expected)

    def test_a_first_phase_starts_before_longer_chains_and_its_failure_stops_the_gate(self):
        plan = [Phase('long', (sys.executable, '-B', '-c', 'pass')),
                Phase('quick-check', (sys.executable, '-B', '-c', 'raise SystemExit(1)'), first=True)]
        with tempfile.TemporaryDirectory() as directory:
            output = io.StringIO()
            with redirect_stdout(output), self.assertRaises(GateFailure):
                gate_runner.run(plan, Path(directory), 1, seconds={'long': 300, 'quick-check': 60})
        self.assertEqual(re.findall(r'^START (\S+)$', output.getvalue(), re.M), ['quick-check'])


class TimeBudgetTests(unittest.TestCase):
    """The whole run, not each phase, has a wall-time budget per tier."""
    def test_policy_has_both_tiers_and_local_shares_push(self):
        self.assertEqual(gate_tier.time_budget('push', 'check'), 720)
        self.assertEqual(gate_tier.time_budget('push', 'ci'), 1440)
        self.assertEqual(gate_tier.time_budget('local', 'check'), gate_tier.time_budget('push', 'check'))
        self.assertGreater(gate_tier.time_budget('full', 'ci'), gate_tier.time_budget('push', 'ci'))
        with tempfile.TemporaryDirectory() as directory:
            for entry in ({'check_seconds': 0, 'reason': 'r'}, {'check_seconds': 9}, {'check_seconds': 'x', 'reason': 'r'}):
                path = Path(directory, 'budgets.json')
                path.write_text(json.dumps({'schema': 1, 'tiers': {'push': entry}}))
                with self.subTest(entry=entry), self.assertRaises(ValueError):
                    gate_tier.time_budget('push', 'check', path)

    def setUp(self):
        # An idle 16-thread host, whatever the host running this test is doing.
        for target, value in (('getloadavg', (2.0, 2.0, 2.0)), ('cpu_count', 16)):
            stub = patch.object(gate_tier.os, target, return_value=value)
            stub.start(); self.addCleanup(stub.stop)

    def test_over_budget_fails_naming_the_costliest_phases(self):
        timings = {f'phase{index}': {'cpu_seconds': float(index), 'wall_seconds': 1.0} for index in range(20)}
        output = io.StringIO()
        with redirect_stdout(output):
            gate_tier.check_time_budget('push tier host gate', 479.0, 480, timings)
        self.assertIn('TIME push tier host gate: 479s of its 480s budget', output.getvalue())
        with self.assertRaises(GateFailure) as failure:
            gate_tier.check_time_budget('push tier host gate', 481.0, 480, timings)
        message = str(failure.exception)
        self.assertIn('took 481s, over its 480s budget', message)
        named = re.findall(r'wall  (phase\d+)', message)
        self.assertEqual(named, [f'phase{index}' for index in range(19, 9, -1)])
        self.assertIn('do not shard to fit', message)

    def test_a_busy_host_reports_the_overrun_without_failing(self):
        output = io.StringIO()
        with patch.object(gate_tier.os, 'getloadavg', return_value=(30.0, 17.0, 12.0)), redirect_stdout(output):
            gate_tier.check_time_budget('push tier host gate', 900.0, 480, {'slow_test.py': {'cpu_seconds': 9.0}})
        self.assertIn('TIME OVER BUDGET on a busy host', output.getvalue())
        self.assertIn('took 900s, over its 480s budget (load 30.0 17.0 12.0 on 16 threads', output.getvalue())
        self.assertIn('slow_test.py', output.getvalue())

    def test_a_slow_phase_fails_the_real_gate_command(self):
        """Negative control: a real gate run whose one phase sleeps past the budget fails."""
        with tempfile.TemporaryDirectory() as directory:
            sleep = Phase('slow_test.py', (sys.executable, '-B', '-c', 'import time; time.sleep(1.2)'))
            started = time.perf_counter()
            output = io.StringIO()
            with redirect_stdout(output):
                timings = gate_runner.run([sleep], Path(directory), 1)
            self.assertGreaterEqual(timings['slow_test.py']['wall_seconds'], 1.2)
            with self.assertRaisesRegex(GateFailure, r'over its 1s budget[\s\S]*slow_test.py'):
                gate_tier.check_time_budget('push tier host gate', time.perf_counter() - started, 1, timings)


if __name__ == '__main__':
    unittest.main()
