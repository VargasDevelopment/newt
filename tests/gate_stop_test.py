"""Gate stop path: a stopped gate ends every running phase, then itself, within
a bound and with its exact status, whatever its output streams do."""
from contextlib import ExitStack, suppress
import itertools
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest

sys.dont_write_bytecode = True
TOOLS = Path(__file__).resolve().parents[1] / 'tools'
sys.path.insert(0, str(TOOLS))
import gate_runner

# A long phase's process. It starts a grandchild that ignores SIGTERM, so only
# SIGKILL to the phase's process group ends it, records both pids in the file
# it names, then sleeps. A stubborn phase ignores SIGTERM itself as well, only
# recording it (".term"); a filling one first fills the pipes it shares as
# stdout and stderr.
LONG = ('import os, signal, subprocess, sys, time\n'
        'record, mode = sys.argv[1:3]\n'
        'sleeper = "import signal, time\\nsignal.signal(signal.SIGTERM, signal.SIG_IGN)\\nprint(flush=True)\\n'
        'time.sleep(600)\\n"\n'
        'grandchild = subprocess.Popen([sys.executable, "-c", sleeper], stdout=subprocess.PIPE)\n'
        'grandchild.stdout.readline()\n'
        'if mode == "stubborn":\n'
        '    signal.signal(signal.SIGTERM, lambda number, frame: open(record + ".term", "w").close())\n'
        'for fd in (1, 2) if mode == "fill" else ():\n'
        '    os.set_blocking(fd, False)\n'
        '    for size in (4096, 1):\n'
        '        try:\n'
        '            while True:\n'
        '                os.write(fd, b"x" * size)\n'
        '        except BlockingIOError:\n'
        '            pass\n'
        '    os.set_blocking(fd, True)\n'
        'open(record + ".part", "w").write(f"{os.getpid()} {grandchild.pid}")\n'
        'os.replace(record + ".part", record)\n'
        'time.sleep(600)\n')
# A phase that passes as soon as the first long phase has recorded itself.
QUICK = ('import os, time\n'
         'deadline = time.monotonic() + 60\n'
         'while not os.path.exists("a.pids") and time.monotonic() < deadline:\n'
         '    time.sleep(0.01)\n'
         'open("quick.done", "w").close()\n')
# A gate run as check.py runs it (with job slots, then a final line), with a
# one second GRACE and EPILOGUE. Long phase "a" runs first; "b" (long) runs
# beside it with two jobs. In the reporting cases a quick phase finishes beside
# "a", and reporting it fails: the test closed the read end of the gate's
# stdout ("broken"), or the gate's stdout is closed. In the full cases nobody
# reads the gate's stdout or stderr pipe, and both are full: with one job "a"
# fills them, as a phase streaming output can; with two the test filled them,
# and the quick phase's report then blocks the gate.
SUPERVISOR = ('import sys\n'
              'from pathlib import Path\n'
              'sys.path.insert(0, sys.argv[1])\n'
              'import gate_runner\n'
              'long, quick, jobs, case = sys.argv[2:6]\n'
              'gate_runner.GRACE = gate_runner.EPILOGUE = 1.0\n'
              'full, reporting = case.startswith("full-"), case in ("broken", "closed")\n'
              'def phase(name, *arguments):\n'
              '    return gate_runner.Phase(name, (sys.executable, "-B", "-c", *arguments))\n'
              'mode = "stubborn" if case in ("stubborn", "further") else "fill" if full and jobs == "1" else "plain"\n'
              'plan = [phase("a", long, "a.pids", mode)]\n'
              'second = phase("b", long, "b.pids", "plain")\n'
              'plan.append(phase("quick", quick) if reporting or full and jobs == "2" else second)\n'
              'if case == "closed":\n'
              '    sys.stdout.close()\n'
              'try:\n'
              '    with gate_runner.job_environment(Path.cwd(), int(jobs)):\n'
              '        gate_runner.run(plan, Path.cwd(), int(jobs))\n'
              'finally:\n'
              '    print("DONE gate invocation", flush=True)\n')
# The signal that stops each signal case's gate, and the exit status it must
# report: SIGINT ends the gate by SIGINT, as Python ends on an unhandled
# KeyboardInterrupt (130 to a shell); the others exit 128 + signal. A further
# case is stubborn, and also gets SIGINT once its stubborn phase got SIGTERM.
STOPS = {'SIGINT': (signal.SIGINT, -signal.SIGINT), 'SIGTERM': (signal.SIGTERM, 128 + signal.SIGTERM),
         'SIGHUP': (signal.SIGHUP, 128 + signal.SIGHUP), 'stubborn': (signal.SIGTERM, 128 + signal.SIGTERM),
         'further': (signal.SIGTERM, 128 + signal.SIGTERM),
         'full-SIGINT': (signal.SIGINT, -signal.SIGINT), 'full-SIGTERM': (signal.SIGTERM, 128 + signal.SIGTERM),
         'full-SIGHUP': (signal.SIGHUP, 128 + signal.SIGHUP)}
FULL = ('full-SIGINT', 'full-SIGTERM', 'full-SIGHUP')
# Reporting cases, with the error that must stop their gate (exit 1).
REPORTING = {'broken': 'BrokenPipeError', 'closed': 'ValueError: I/O operation on closed file'}
STOP_CASES = [*itertools.product(STOPS, (1, 2)), *((case, 2) for case in REPORTING)]
# The test's bound, 2 * GRACE + EPILOGUE = 3 seconds, with time for a loaded host.
STOPPED_WITHIN = 10.0
# A gate as check.py runs it, of twenty phases that each print 10,000 bytes;
# the last fails.
NOISY = ('import sys\n'
         'from pathlib import Path\n'
         'sys.path.insert(0, sys.argv[1])\n'
         'from gate_runner import GateFailure, Phase, run\n'
         'speak = "import sys\\nprint(sys.argv[1] * 10000, flush=True)\\nsys.exit(sys.argv[1] == \\"t\\")\\n"\n'
         'try:\n'
         '    run([Phase(name, (sys.executable, "-B", "-c", speak, name)) for name in sys.argv[3]],\n'
         '        Path.cwd(), int(sys.argv[2]))\n'
         'except GateFailure as failure:\n'
         '    print(f"GateFailure: {failure}", flush=True)\n'
         '    raise SystemExit(1)\n'
         'finally:\n'
         '    print("DONE gate invocation", flush=True)\n')
# A phase's process that leaves another running in its group and records its
# pid. With one job (argument "1") that process shares the gate's stdout and
# stderr; with two it has neither, as one holding the phase's output pipe
# would keep the phase running.
LEAVE = ('import subprocess, sys\n'
         'quiet = None if sys.argv[1] == "1" else subprocess.DEVNULL\n'
         'left = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(600)"],\n'
         '                        stdout=quiet, stderr=quiet)\n'
         'open("left.pid", "w").write(str(left.pid))\n')
# A passing gate of that one phase.
LEAVING = ('import sys\n'
           'from pathlib import Path\n'
           'sys.path.insert(0, sys.argv[1])\n'
           'import gate_runner\n'
           'leave, jobs = sys.argv[2:4]\n'
           'phase = gate_runner.Phase("leaving", (sys.executable, "-B", "-c", leave, jobs))\n'
           'gate_runner.run([phase], Path.cwd(), int(jobs))\n'
           'print("gate passed", flush=True)\n')
# A passing gate that sends itself the signal it names as it ends its
# (finished) phases ("ending"), or as it restores the previous handlers
# ("restoring").
LATE = ('import os, signal, sys\n'
        'from pathlib import Path\n'
        'sys.path.insert(0, sys.argv[1])\n'
        'import gate_runner\n'
        'if sys.argv[2] == "ending":\n'
        '    owner, name = gate_runner.Children, "stop"\n'
        'else:\n'
        '    owner, name = gate_runner.Interrupts, "restore"\n'
        'original = getattr(owner, name)\n'
        'def late(self):\n'
        '    os.kill(os.getpid(), signal.Signals[sys.argv[3]])\n'
        '    return original(self)\n'
        'setattr(owner, name, late)\n'
        'gate_runner.run([gate_runner.Phase("only", (sys.executable, "-c", "pass"))], Path.cwd(), 1)\n'
        'print("gate passed", flush=True)\n')


def fill(fd):
    """Write to a pipe until it is full."""
    os.set_blocking(fd, False)
    for size in (4096, 1):
        with suppress(BlockingIOError):
            while True:
                os.write(fd, b'x' * size)
    os.set_blocking(fd, True)


def writing(pid, children=False):
    """Whether a process's main thread, or with children any child's, waits to
    write a pipe; None where /proc cannot tell."""
    if not Path(f'/proc/{os.getpid()}/task/{os.getpid()}/wchan').exists():
        return None
    pids, tasks = [pid], []
    with suppress(OSError):  # it ended
        tasks = list(Path(f'/proc/{pid}/task').iterdir()) if children else []
    for task in tasks:
        with suppress(OSError):  # that thread ended
            pids += map(int, (task / 'children').read_text().split())
    for each in pids:
        with suppress(OSError):  # that process ended
            if 'pipe_write' in Path(f'/proc/{each}/task/{each}/wchan').read_text():
                return True
    return False


def recorded(directory):
    """The pids the long phases of a stop case's gate recorded."""
    return [int(pid) for name in ('a.pids', 'b.pids') if (directory / name).exists()
            for pid in (directory / name).read_text().split()]


def alive(pid):
    """True while a process exists and has not yet ended (a zombie has)."""
    try:
        stat = Path(f'/proc/{pid}/stat')
        if stat.parent.parent.is_dir():
            return stat.read_text().rsplit(')', 1)[1].split()[0] != 'Z'
        os.kill(pid, 0)
        return True
    except (FileNotFoundError, ProcessLookupError):
        return False


class StopTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def rebuilt_tools(self, name, replacements):
        """A copy of the runner's tools with each (old, new) text replaced once."""
        tools = self.root / name
        tools.mkdir()
        (tools / 'host_jobs.py').write_text((TOOLS / 'host_jobs.py').read_text())
        (tools / 'gate_child.py').write_text((TOOLS / 'gate_child.py').read_text())
        source = (TOOLS / 'gate_runner.py').read_text()
        for old, new in replacements:
            self.assertEqual(source.count(old), 1, old)
            source = source.replace(old, new)
        (tools / 'gate_runner.py').write_text(source)
        return tools

    def start_gate(self, files, tag, tools, case, jobs):
        directory = self.root / f'{tag}-{case}-{jobs}'
        directory.mkdir()
        if case in FULL:  # pipes nobody reads, full from the start with two jobs
            pipes = [os.pipe() for _ in ('stdout', 'stderr')]
            for read, write in pipes:
                files.callback(os.close, read)
                files.callback(os.close, write)
                if jobs == 2:
                    fill(write)
            streams = {'stdout': pipes[0][1], 'stderr': pipes[1][1]}
        else:
            streams = {'stdout': subprocess.PIPE if case == 'broken' else
                       files.enter_context(open(directory / 'stdout', 'wb')),
                       'stderr': files.enter_context(open(directory / 'stderr', 'wb'))}
        gate = subprocess.Popen([sys.executable, '-B', '-c', SUPERVISOR, str(tools), LONG, QUICK, str(jobs), case],
                                cwd=directory, **streams)
        if case == 'broken':
            gate.stdout.close()  # every report now fails
        return directory, gate

    def ready_to_stop(self, case, jobs, directory, gate):
        """Whether a gate's long phases have recorded themselves, and in the
        full cases with two jobs, the gate waits to write its first report."""
        records = ['a.pids', 'b.pids'] if jobs == 2 and case in STOPS and case not in FULL else ['a.pids']
        if case in FULL and jobs == 2:
            records.append('quick.done')
        if not all((directory / name).exists() for name in records):
            return False
        return case not in FULL or jobs == 1 or writing(gate.pid) is not False

    def stop_problems(self, case, jobs, directory, gate, stopped):
        """How a stopped gate failed to end promptly, exactly and completely, if it did."""
        problems, until = [], stopped + STOPPED_WITHIN
        try:
            code = gate.wait(max(0.0, until - time.monotonic()))
        except subprocess.TimeoutExpired:
            code = None
            problems.append(f'still running {STOPPED_WITHIN}s after stopping')
        expected = STOPS[case][1] if case in STOPS else 1
        if code is not None and code != expected:
            problems.append(f'exited {code}, not {expected}')
        if code is not None and case not in FULL:  # nothing can write a full stderr
            error = (directory / 'stderr').read_text(errors='replace')
            cause = STOPS[case][0].name if case in STOPS else REPORTING[case].split(':')[0]
            line = f'GATE STOPPED by {cause}; running phases ended: {2 if jobs == 2 and case in STOPS else 1}\n'
            if not error.endswith(line) or case in REPORTING and REPORTING[case] not in error:
                problems.append(f'stderr does not end {line!r}: {error[-300:]!r}')
        if jobs == 1 and (directory / 'b.pids').exists():
            problems.append('a phase started after the gate stopped')
        if code is not None and list((directory / '.build').glob('.gate-slots-*')):
            problems.append('its job slots remain')
        pids = recorded(directory)
        if not pids:
            problems.append('no long phase recorded itself')
        while any(alive(pid) for pid in pids) and time.monotonic() < until:
            time.sleep(0.01)
        return problems + [f'process {pid} outlived the gate' for pid in pids if alive(pid)]

    def stopping_problems(self, runs):
        """Start a gate for every (tag, tools, case, jobs) run at once, stop each
        while its long phases run, and return each run's problems by (tag, case, jobs)."""
        gates, stopped, problems = {}, {}, {}
        with ExitStack() as files:
            try:
                for tag, tools, case, jobs in runs:
                    gates[tag, case, jobs] = self.start_gate(files, tag, tools, case, jobs)
                deadline = time.monotonic() + 60
                for (tag, case, jobs), (directory, gate) in gates.items():
                    while not self.ready_to_stop(case, jobs, directory, gate) and time.monotonic() < deadline:
                        time.sleep(0.01)
                    problems[tag, case, jobs] = [] if self.ready_to_stop(case, jobs, directory, gate) else [
                        'never ready to stop']
                    if case in STOPS:
                        gate.send_signal(STOPS[case][0])
                    stopped[tag, case, jobs] = time.monotonic()
                for key, (directory, gate) in gates.items():
                    while (key[1] == 'further' and not (directory / 'a.pids.term').exists() and gate.poll() is None
                           and time.monotonic() < stopped[key] + STOPPED_WITHIN):
                        time.sleep(0.01)
                    if key[1] == 'further':
                        gate.send_signal(signal.SIGINT)
                for key, (directory, gate) in gates.items():
                    problems[key] += self.stop_problems(*key[1:], directory, gate, stopped[key])
                return problems
            finally:
                for directory, gate in gates.values():
                    for pid in recorded(directory):
                        with suppress(OSError):
                            os.kill(pid, signal.SIGKILL)
                    gate.kill()
                    gate.wait()

    def test_a_stopped_gate_ends_every_running_phase_promptly(self):
        # SIGINT, SIGTERM or SIGHUP to the gate, or a report it cannot write,
        # ends each running phase's process group (with a grandchild that only
        # SIGKILL ends, and a stubborn phase) and the gate within the bound,
        # with its exact status and status line; no further phase starts; and a
        # further SIGINT as it stops changes nothing. So do SIGINT, SIGTERM and
        # SIGHUP while nobody reads its full stdout and stderr.
        problems = self.stopping_problems([('stop', TOOLS, case, jobs) for case, jobs in STOP_CASES])
        self.assertEqual(problems, {('stop', case, jobs): [] for case, jobs in STOP_CASES})

    def test_earlier_stop_paths_fail_the_stop_controls(self):
        # Negative controls, run together. "waiting": the runner rebuilt to
        # handle no signal and, once supervision fails, to wait for running
        # phases to finish, as 87fd603's executor did. SIGINT and reporting
        # failures then hang until killed; SIGTERM and SIGHUP end only the gate,
        # leaving its phases running. "raising": the runner rebuilt to end its
        # phases and raise, as ee1fdc3 did. With full output pipes it then
        # waits, after its phases ended, to write a traceback or flush output.
        # "handling": the runner rebuilt so that a further signal raises while
        # it stops. The further SIGINT then cuts the stop short, before its
        # status line (and typically before SIGKILL to the stubborn phase).
        waiting = self.rebuilt_tools('waiting-tools', [
            ('            interrupts.arm()\n            passed, failed =',
             '            pass\n            passed, failed ='),
            ('                    os.killpg(process.pid, number)\n', '                    process.wait()\n')])
        raising = self.rebuilt_tools('raising-tools', [
            ('        stop(children, interrupts.received or error)\n', '        children.stop()\n        raise\n')])
        handling = self.rebuilt_tools('handling-tools', [
            ('                    signal.signal(number, lambda number, frame: None)\n',
             '                    signal.signal(number, signal.default_int_handler)\n')])
        problems = self.stopping_problems([*(('waiting', waiting, *run) for run in STOP_CASES),
                                           *(('raising', raising, case, jobs) for case, jobs in STOP_CASES
                                             if case in FULL),
                                           *(('handling', handling, 'further', jobs) for jobs in (1, 2))])
        for case, jobs in STOP_CASES:
            with self.subTest(control='waiting', case=case, jobs=jobs):
                found = problems['waiting', case, jobs]
                if case in REPORTING or STOPS[case][0] == signal.SIGINT:
                    self.assertIn(f'still running {STOPPED_WITHIN}s after stopping', found)
                else:
                    self.assertIn(f'exited {-STOPS[case][0]}, not {STOPS[case][1]}', found)
                    self.assertTrue(any(problem.endswith('outlived the gate') for problem in found), found)
            if case in FULL:
                with self.subTest(control='raising', case=case, jobs=jobs):
                    found = problems['raising', case, jobs]
                    self.assertIn(f'still running {STOPPED_WITHIN}s after stopping', found)
                    self.assertFalse(any(problem.endswith('outlived the gate') for problem in found), found)
            if case == 'further':
                with self.subTest(control='handling', jobs=jobs):
                    found = problems['handling', case, jobs]
                    self.assertTrue(any(problem.startswith('stderr does not end') for problem in found), found)

    def test_a_signal_as_the_gate_finishes_stops_it_or_reaches_the_restored_handler(self):
        # SIGTERM or SIGINT recorded after supervision, before the handlers are
        # restored, still stops the gate, with its exact status. SIGTERM sent
        # as they are restored is held until then, and reaches the restored
        # (default) handler. Negative controls: without the late check the
        # gate passed, as ee1fdc3's did; without the hold, the gate's own
        # handler took the signal while being replaced, where CPython can drop
        # one ("ignored due to race condition").
        def line(name):
            return f'GATE STOPPED by {name}; running phases ended: 0\n'
        late = '    if interrupts.received:  # It arrived as the gate finished: its handlers are still in place.\n'
        ignoring = self.rebuilt_tools('ignoring-tools', [(late, '    if False:\n')])
        hold = '        held = signal.pthread_sigmask(signal.SIG_BLOCK, SIGNALS)\n'
        unheld = self.rebuilt_tools('unheld-tools', [(hold, hold.replace('SIGNALS', '[]'))])
        for tools, when, name, code, error in (
                (TOOLS, 'ending', 'SIGTERM', 128 + signal.SIGTERM, line('SIGTERM')),
                (TOOLS, 'ending', 'SIGINT', -signal.SIGINT, line('SIGINT')),
                (ignoring, 'ending', 'SIGTERM', 0, ''), (TOOLS, 'restoring', 'SIGTERM', -signal.SIGTERM, ''),
                (unheld, 'restoring', 'SIGTERM', 128 + signal.SIGTERM, line('SIGTERM'))):
            with self.subTest(tools=tools.name, when=when, signal=name):
                result = subprocess.run([sys.executable, '-B', '-c', LATE, str(tools), when, name], cwd=self.root,
                                        capture_output=True, text=True, timeout=60)
                self.assertEqual((result.returncode, result.stderr), (code, error), result)
                self.assertEqual(result.stdout.splitlines()[-1:] == ['gate passed'], code == 0, result)

    def test_a_passed_phase_leaves_nothing_running(self):
        # What a passed phase's process left running in its group ends with it,
        # with one job even sharing the gate's stdout. Negative control: without
        # that SIGKILL it outlived the gate.
        leaving = self.rebuilt_tools('leaving-tools', [
            ('            os.killpg(process.pid, signal.SIGKILL)\n', '            pass\n')])
        for tools, jobs in itertools.product((TOOLS, leaving), (1, 2)):
            with self.subTest(tools=tools.name, jobs=jobs):
                directory = self.root / f'{tools.name}-{jobs}'
                directory.mkdir()
                with open(directory / 'output', 'w+') as output:  # not a pipe a left process could hold open
                    code = subprocess.run([sys.executable, '-B', '-c', LEAVING, str(tools), LEAVE, str(jobs)],
                                          cwd=directory, stdout=output, stderr=subprocess.STDOUT, timeout=60).returncode
                    output.seek(0)
                    lines = output.read().splitlines()
                left = int((directory / 'left.pid').read_text())
                try:
                    deadline = time.monotonic() + 10
                    while tools == TOOLS and alive(left) and time.monotonic() < deadline:
                        time.sleep(0.01)
                    self.assertEqual((code, lines[-1:], alive(left)), (0, ['gate passed'], tools != TOOLS))
                finally:
                    with suppress(OSError):
                        os.kill(left, signal.SIGKILL)

    def test_a_gate_runs_only_in_the_main_thread(self):
        # Only the main thread receives signals, so a gate elsewhere could not
        # be stopped as promised: it refuses to start.
        errors = []

        def attempt():
            try:
                gate_runner.run([gate_runner.Phase('ran', (sys.executable, '-c', 'open("ran", "w")'))], self.root)
            except RuntimeError as error:
                errors.append(str(error))
        thread = threading.Thread(target=attempt)
        thread.start()
        thread.join(60)
        self.assertEqual(errors, ['A gate runs in the main thread, which receives its signals'])
        self.assertFalse((self.root / 'ran').exists())

    def test_a_failing_phase_reports_everything_to_a_paused_reader(self):
        # Not a stop: the gate waits for a reader that pauses with its stdout
        # full, then reports every phase's whole output and the failure.
        names = 'abcdefghijklmnopqrst'
        for jobs in (1, 2):
            with self.subTest(jobs=jobs):
                gate = subprocess.Popen([sys.executable, '-B', '-c', NOISY, str(TOOLS), str(jobs), names],
                                        cwd=self.root, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                with gate:  # closes its pipes
                    try:
                        deadline = time.monotonic() + 60
                        while writing(gate.pid, children=True) is False and time.monotonic() < deadline:
                            time.sleep(0.01)
                        self.assertIsNot(writing(gate.pid, children=True), False)
                        time.sleep(0.5)
                        stdout, stderr = gate.communicate(timeout=60)
                    finally:
                        gate.kill()
                lines = [re.sub(r' \([0-9.]+s\)', '', line) for line in stdout.decode().splitlines()]
                self.assertEqual((gate.returncode, stderr), (1, b''))
                self.assertEqual(len(lines), 3 * len(names) + 2)
                for name in names:
                    start = lines.index(f'START {name}')
                    self.assertEqual(lines[start + 1:start + 3],
                                     [name * 10000, f'FAIL {name}, exit 1' if name == 't' else f'PASS {name}'])
                self.assertEqual(lines[-2:], ['GateFailure: Gate phases failed: t', 'DONE gate invocation'])

    def test_ignored_signals_stay_ignored_and_handlers_are_restored(self):
        previous = signal.signal(signal.SIGHUP, signal.SIG_IGN)
        try:
            before = {number: signal.getsignal(number) for number in gate_runner.SIGNALS}
            interrupts = gate_runner.Interrupts()
            interrupts.arm()
            try:
                self.assertIs(signal.getsignal(signal.SIGHUP), signal.SIG_IGN)
                self.assertEqual(signal.getsignal(signal.SIGTERM), interrupts.interrupt)
                with self.assertRaises(SystemExit) as stop:
                    interrupts.interrupt(signal.SIGTERM, None)
                self.assertEqual(stop.exception.code, 128 + signal.SIGTERM)
                interrupts.interrupt(signal.SIGINT, None)  # once stopping, a later signal does not raise
            finally:
                interrupts.restore()
            self.assertEqual({number: signal.getsignal(number) for number in gate_runner.SIGNALS}, before)
        finally:
            signal.signal(signal.SIGHUP, previous)

    def test_children_do_not_inherit_ignored_stop_signals(self):
        # A launcher (nohup, background job, IDE terminal) may ignore SIGHUP or
        # SIGINT. The outer gate keeps its own ignored disposition, while every
        # phase launched through gate_child.py gets SIG_DFL for stop signals.
        # Negative control: reverting gate_child.py to reset only SIGPIPE and
        # SIGXFSZ leaks SIG_IGN into both direct and measured children.
        unfixed = self.rebuilt_tools('unfixed-child-tools', [])
        (unfixed / 'gate_cpu.py').write_text((TOOLS / 'gate_cpu.py').read_text())
        child_source = (TOOLS / 'gate_child.py').read_text()
        fixed_loop = 'for number in (signal.SIGPIPE, signal.SIGXFSZ, signal.SIGINT, signal.SIGTERM, signal.SIGHUP):\n'
        self.assertEqual(child_source.count(fixed_loop), 1)
        (unfixed / 'gate_child.py').write_text(
            child_source.replace(fixed_loop, 'for number in (signal.SIGPIPE, signal.SIGXFSZ):\n'))
        probe = (
            'import json, signal, subprocess, sys\n'
            'sys.path.insert(0, sys.argv[1])\n'
            'import gate_runner\n'
            'for number in gate_runner.SIGNALS:\n'
            '    signal.signal(number, signal.SIG_IGN)\n'
            'children = gate_runner.Children()\n'
            'try:\n'
            '    direct = {number.name: children.run(["/bin/sh", "-c", f"kill -{number.value} $$; exit 99"],\n'
            '              stdout=subprocess.PIPE, stderr=subprocess.PIPE).returncode\n'
            '              for number in gate_runner.SIGNALS}\n'
            'finally:\n'
            '    children.stop()\n'
            'inspect = ("import json, signal; print(json.dumps("\n'
            '           "{n.name: signal.getsignal(n) is signal.SIG_IGN "\n'
            '           "for n in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)}))")\n'
            'measured = gate_runner.phase_run([sys.executable, "-B", "-c", inspect],\n'
            '    cpu_budgets={"default_seconds": 30, "critical_path_seconds": 30, "phases": {}},\n'
            '    stdout=subprocess.PIPE, stderr=subprocess.PIPE)\n'
            'outer = {number.name: signal.getsignal(number) is signal.SIG_IGN for number in gate_runner.SIGNALS}\n'
            'print(json.dumps({"direct": direct, "measured": json.loads(measured.stdout.decode()), "outer": outer}))\n'
        )
        for label, tools in (('fixed', TOOLS), ('unfixed', unfixed)):
            with self.subTest(tools=label):
                result = subprocess.run([sys.executable, '-B', '-c', probe, str(tools)],
                                        cwd=self.root, capture_output=True, text=True, timeout=60, check=True)
                data = json.loads(result.stdout.splitlines()[-1])
                self.assertEqual(data['outer'], {'SIGINT': True, 'SIGTERM': True, 'SIGHUP': True})
                if label == 'fixed':
                    self.assertEqual(data['direct'], {n.name: -n for n in gate_runner.SIGNALS})
                    self.assertEqual(data['measured'], {'SIGINT': False, 'SIGTERM': False, 'SIGHUP': False})
                else:
                    self.assertEqual(data['direct'], {n.name: 99 for n in gate_runner.SIGNALS})
                    self.assertEqual(data['measured'], {'SIGINT': True, 'SIGTERM': True, 'SIGHUP': True})


if __name__ == '__main__':
    unittest.main()
