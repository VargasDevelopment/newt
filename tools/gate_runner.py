"""Host gate phases in a declared serial order, run serially or in parallel.

Standard library only. A plan lists phases in the order the serial gate runs
them. With one job they run in exactly that order and stream their output, as
a plain loop would. With more jobs a phase starts once every phase it depends
on has passed: the phases it names in `after` and every earlier phase sharing
one of its `uses`. An `alone` phase never overlaps another phase: it waits
until nothing else can run, and nothing starts while it runs. Every phase is a
command in its own process; with more jobs its output is buffered and printed
whole, between its START and result lines, when it finishes.

No phase code runs in this process, so no phase can end or bypass the gate: a
phase's result is the exit status its own process returns, and a phase whose
process ends without one (killed by a signal) fails. After a failure no new
phase starts; phases already running finish and print first. With keep_going,
every phase whose prerequisites passed still runs, and all failures are
reported together. The gate passes only when every phase reported PASS.

Each phase's process leads its own session, so the gate can end it with
everything it started: when that process exits, anything it left running in
its group gets SIGKILL. A gate runs in the main thread, which receives its
signals. If supervision fails (SIGINT, SIGTERM or SIGHUP to this process
before run() has restored the previous handlers, or any error while
scheduling or reporting), the gate stops, and a further signal changes
nothing: no new phase starts; each running phase's process group gets
SIGTERM, then SIGKILL, each followed by at most GRACE seconds to reap them;
a thread given at most EPILOGUE seconds tries the status line on a ready stderr
before removing the job slot directories; then the
process ends at once, never flushing or waiting on stdout or stderr: by
SIGINT itself for SIGINT (130 to a shell), with 128 + signal for the others
and 1 for an error. A stopped gate thus ends within 2 * GRACE + EPILOGUE
seconds, whatever its output streams do. A signal ignored when the gate
started (nohup, a background job) stays ignored. While run() restores the
previous handlers, it holds these signals, so none is lost: one held then
reaches the restored handler. Only the main thread writes the gate's stdout,
and only while it supervises.
"""
import _thread
from contextlib import contextmanager, suppress
from dataclasses import dataclass
import os
import json
import math
from pathlib import Path
import queue
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import traceback

import host_jobs


def label(command) -> str:
    """The serial gate's phase name: the script or executable, then its -- flags."""
    executable = Path(command[0]).name
    name = next((Path(arg).name for arg in command[1:] if str(arg).endswith('.py')), executable)
    name += ' ' + ' '.join(str(arg) for arg in command[2:] if str(arg).startswith('--'))
    return name.strip()


def phase_run(command, *, cpu_budgets=None, **kwargs):
    name = label(command)
    started = time.perf_counter()
    print(f"START {name}", flush=True)
    try:
        if cpu_budgets is None:
            result = subprocess.run(command, **kwargs)
        else:
            checked = kwargs.pop('check', False)
            children, interrupts = Children(cpu_budgets), Interrupts()
            try:
                interrupts.arm()
                result = children.run(list(map(str, command)), phase_name=name, **kwargs)
            except BaseException:
                children.stop()
                raise
            finally:
                interrupts.armed = False
                interrupts.restore()
            if interrupts.received:
                if interrupts.received == signal.SIGINT:
                    raise KeyboardInterrupt
                raise SystemExit(128 + interrupts.received)
            timing = children.timings[name]
            print(f"CPU {name}: {timing['cpu_seconds']:.3f}s / {timing['budget_seconds']:.3f}s", flush=True)
            if checked:
                result.check_returncode()
    except subprocess.CalledProcessError as error:
        print(f"FAIL {name} ({time.perf_counter() - started:.3f}s), exit {error.returncode}", flush=True)
        raise
    status = 'PASS' if result.returncode == 0 else f'DONE exit {result.returncode}'
    print(f"{status} {name} ({time.perf_counter() - started:.3f}s)", flush=True)
    return result


@dataclass(frozen=True)
class Phase:
    """A command run in its own process from the gate's directory."""
    name: str
    command: tuple = ()
    after: tuple = ()
    uses: tuple = ()
    alone: bool = False
    first: bool = False  # starts before every other ready phase, so its failure stops the gate early


class GateFailure(RuntimeError):
    pass


GRACE = 5.0  # Seconds to reap stopped phases after SIGTERM, and again after SIGKILL.
EPILOGUE = 1.0  # Seconds a stopping gate gives its status line and slot cleanup.
SIGNALS = (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)
STOP_REMOVES = []  # job_environment slot directories: a stop skips its cleanup, so removes them.


class Children:
    """The gate's running phase processes; none starts once the gate stops."""
    def __init__(self, cpu_budgets=None):
        self.cpu_budgets = cpu_budgets
        self.timings = {}
        self.lock = threading.Lock()
        self.live = []
        self.stopped = False

    def run(self, command, phase_name=None, **options):
        """Run and measure in a private process; concurrent phases cannot mix CPU."""
        if self.cpu_budgets is None:
            return self._run(command, **options)
        budget = min(self.cpu_budgets['critical_path_seconds'],
                     self.cpu_budgets['phases'].get(phase_name, {}).get(
                         'seconds', self.cpu_budgets['default_seconds']))
        with tempfile.TemporaryDirectory(prefix='newt-gate-cpu-') as directory:
            receipt = Path(directory) / 'cpu.json'
            result = self._run([sys.executable, '-B', str(Path(__file__).with_name('gate_cpu.py')),
                                str(receipt), str(budget), *command], **options)
            cpu = json.loads(receipt.read_text())['cpu_seconds']
            if not isinstance(cpu, (int, float)) or not math.isfinite(cpu) or cpu < 0:
                raise ValueError(f'Invalid CPU receipt for {phase_name}')
            with self.lock:
                self.timings.setdefault(phase_name, {}).update(cpu_seconds=cpu, budget_seconds=budget)
            return result

    def _run(self, command, **options):
        """subprocess.run for one phase, its process leading its own session."""
        read_error, write_error = os.pipe()
        try:
            launch_mask = signal.pthread_sigmask(signal.SIG_BLOCK, SIGNALS)
            try:
                with self.lock:
                    if self.stopped:
                        raise RuntimeError('The gate stopped before this phase started')
                    # Restore the child's inherited mask without preexec_fn
                    # (unsafe in threads). Keep Popen's exec-error diagnostics.
                    launch = [sys.executable, '-B', str(Path(__file__).with_name('gate_child.py')),
                              ','.join(str(int(number)) for number in launch_mask), str(write_error), *command]
                    inherited = (*options.pop('pass_fds', ()), write_error)
                    process = subprocess.Popen(launch, start_new_session=True, pass_fds=inherited, **options)
                    self.live.append(process)
            finally:
                os.close(write_error)
                # Release the lock before a pending handler can stop us.
                signal.pthread_sigmask(signal.SIG_SETMASK, launch_mask)
            with os.fdopen(read_error, 'rb', closefd=False) as stream:
                startup_error = stream.read()
        finally:
            os.close(read_error)
        stdout = process.communicate()[0]
        with suppress(OSError):  # whatever it left running in its group (none: ESRCH)
            os.killpg(process.pid, signal.SIGKILL)
        with self.lock:
            self.live.remove(process)
        if startup_error:
            number, message, filename = json.loads(startup_error)
            raise OSError(number, message, filename)
        return subprocess.CompletedProcess(command, process.returncode, stdout)

    def stop(self):
        """Start no phase; end each running phase's process group and reap it.
        Return how many phases were running."""
        with self.lock:
            self.stopped = True
            live = list(self.live)
        for number in (signal.SIGTERM, signal.SIGKILL):
            for process in live:
                with suppress(OSError):  # its group already ended
                    os.killpg(process.pid, number)
            deadline = time.monotonic() + GRACE
            for process in live:
                with suppress(subprocess.TimeoutExpired):
                    process.wait(max(0.0, deadline - time.monotonic()))
        return len(live)


class Interrupts:
    """While armed, SIGINT, SIGTERM and SIGHUP raise in the main thread, once.
    Armed or not, the first one received is recorded."""
    def __init__(self):
        self.armed, self.previous, self.received = False, {}, None

    def arm(self):
        if threading.current_thread() is not threading.main_thread():
            return
        self.armed = True
        for number in SIGNALS:
            handler = signal.getsignal(number)
            if handler not in (None, signal.SIG_IGN):
                self.previous[number] = handler
                signal.signal(number, self.interrupt)

    def interrupt(self, number, frame):
        self.received = self.received or number
        if self.armed:
            self.armed = False
            raise KeyboardInterrupt if number == signal.SIGINT else SystemExit(128 + number)

    def restore(self):
        for number, handler in self.previous.items():
            signal.signal(number, handler)


def prerequisites(plan) -> dict:
    """Each phase's complete prerequisite names, after validating the plan."""
    names = [phase.name for phase in plan]
    if len(set(names)) != len(names):
        raise ValueError('Gate phase names must be unique')
    result = {}
    for index, phase in enumerate(plan):
        if not phase.command:
            raise ValueError(f'Gate phase needs a command: {phase.name}')
        earlier = plan[:index]
        unknown = set(phase.after) - {other.name for other in earlier}
        if unknown:
            raise ValueError(f'Gate phase {phase.name} must follow earlier phases: {sorted(unknown)}')
        result[phase.name] = {*phase.after, *(other.name for other in earlier if set(other.uses) & set(phase.uses))}
    return result


def execute(phase, root, buffered, children):
    """Run one phase's process; return (passed, output bytes, result text)."""
    started = time.perf_counter()
    output, code = bytearray(), None
    try:
        streams = {'stdout': subprocess.PIPE, 'stderr': subprocess.STDOUT} if buffered else {}
        result = children.run(list(map(str, phase.command)), phase_name=phase.name, cwd=root, **streams)
        output.extend(result.stdout or b'')
        code = result.returncode
    except BaseException as error:  # reported with the phase, even if it never launched; the gate then fails
        output.extend(f'{type(error).__name__}: {error}\n'.encode())
    elapsed = time.perf_counter() - started
    with children.lock:
        timing = children.timings.setdefault(phase.name, {})
        timing['wall_seconds'] = elapsed
    if 'cpu_seconds' in timing:
        output.extend(f"CPU {phase.name}: {timing['cpu_seconds']:.3f}s / {timing['budget_seconds']:.3f}s\n".encode())
    if code == 0:
        return True, bytes(output), f'PASS {phase.name} ({elapsed:.3f}s)'
    # A negative code is the signal that ended the process without an exit status.
    failure = '' if code is None else f', exit {code}'
    return False, bytes(output), f'FAIL {phase.name} ({elapsed:.3f}s){failure}'


def chain_seconds(plan, needs, seconds):
    """Each phase's last-run seconds plus its longest chain of dependents."""
    rank = {}
    for phase in reversed(plan):  # Every dependent comes later in the plan.
        rank[phase.name] = seconds.get(phase.name, 0) + max(
            (rank[other.name] for other in plan if phase.name in needs[other.name]), default=0)
    return rank


def supervise(plan, needs, root, jobs, keep_going, children, seconds):
    """Run the plan's phases; return the names that passed and the phases that failed.
    Ready phases start first ones first, then longest chain first, so a long
    phase never starts last and runs alone; with no seconds they start in plan order."""
    buffered, results, workers = jobs > 1, queue.SimpleQueue(), []
    rank = chain_seconds(plan, needs, seconds)
    pending = sorted(plan, key=lambda phase: (not phase.first, -rank[phase.name]))
    running, passed, failed, unrun = [], set(), [], []

    def work(phase):
        result = False, b'', f'FAIL {phase.name}'  # if execute itself fails
        try:
            result = execute(phase, root, buffered, children)
        finally:
            results.put((phase, result))

    while True:
        for phase in list(pending) if keep_going or not failed else ():
            missing = needs[phase.name] - passed
            if missing & {*unrun, *(item.name for item in failed)}:
                pending.remove(phase)
                unrun.append(phase.name)
            elif (not missing and len(running) < jobs and not (phase.alone and running)
                  and not any(other.alone for other in running)):
                pending.remove(phase)
                if not buffered:
                    print(f'START {phase.name}', flush=True)
                running.append(phase)
                workers.append(threading.Thread(target=work, args=(phase,), daemon=True))
                workers[-1].start()
        if not running:
            for worker in workers:  # Each has posted its result; none remains to take a signal.
                worker.join(GRACE)
            return passed, failed
        # Wait for one phase, then report every finished phase in start order.
        finished = [results.get()]
        while not results.empty():
            finished.append(results.get())
        finished = dict(finished)
        for phase in [phase for phase in running if phase in finished]:
            running.remove(phase)
            ok, output, status = finished[phase]
            report(phase, buffered, output, status)
            if ok:
                passed.add(phase.name)
            else:
                failed.append(phase)


def report(phase, buffered, output, status):
    """Print a finished phase: with more jobs its whole output under its START
    line; with one job, whose phase streamed its own, only the runner's message."""
    if buffered:
        output = f'START {phase.name}\n'.encode() + output
    block = output + (b'' if output.endswith(b'\n') or not output else b'\n')
    if block:
        sys.stdout.flush()
        if hasattr(sys.stdout, 'buffer'):
            sys.stdout.buffer.write(block)
        else:
            sys.stdout.write(block.decode(errors='replace'))
    print(status, flush=True)


def default_jobs() -> int:
    """Every usable CPU: the per-user host slots (tools/host_jobs.py) already
    share them when two gates run at once."""
    return host_jobs.available_cpus()[0]


@contextmanager
def job_environment(root, jobs, global_jobs=None):
    """Bound every compiler job of this gate and its phases to `jobs`.

    One job keeps every phase's compiles serial, as the serial gate always ran
    them. More jobs share that many gate-wide compiler slots across phases."""
    build = Path(root) / '.build'
    build.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.gate-slots-', dir=build) as slots:
        saved = {name: os.environ.get(name) for name in (host_jobs.JOBS, host_jobs.SLOTS, host_jobs.GLOBAL_JOBS)}
        STOP_REMOVES.append(slots)
        try:
            if global_jobs is not None:
                os.environ[host_jobs.GLOBAL_JOBS] = str(global_jobs)
            host_jobs.global_limit()  # malformed limits fail before any phase starts
            os.environ[host_jobs.JOBS] = str(jobs)
            os.environ.pop(host_jobs.SLOTS, None)
            if jobs > 1:
                os.environ[host_jobs.SLOTS] = str(host_jobs.create_slots(Path(slots), jobs))
            yield
        finally:
            STOP_REMOVES.remove(slots)
            for name, value in saved.items():
                if value is None:
                    os.environ.pop(name, None)
                else:
                    os.environ[name] = value


def conclude(cause, ended, finished):
    """A stopping gate's last work, any of which may block, so the stopping
    thread waits for it (finished) at most EPILOGUE seconds: say why the gate
    stopped before removing scratch, writing stderr's file directly."""
    try:
        if isinstance(cause, int):
            text = f'GATE STOPPED by {signal.Signals(cause).name}'
        else:
            text = ''.join(traceback.format_exception(cause)) + f'GATE STOPPED by {type(cause).__name__}'
        text = f'{text}; running phases ended: {ended}\n'.encode(errors='replace')
        with suppress(OSError):
            # A full pipe cannot report, but must not prevent scratch cleanup.
            # Bound each write to the amount a writable pipe promises to take.
            while text and select.select([], [2], [], 0)[1]:
                text = text[os.write(2, text[:select.PIPE_BUF]):]
        for directory in list(STOP_REMOVES):
            shutil.rmtree(directory, ignore_errors=True)
    finally:
        finished.release()


def stop(children, cause):
    """End this process after supervision failed, given the signal number or
    the error: end every running phase, try to say why, and exit without
    flushing or waiting on stdout or stderr."""
    cause = signal.SIGINT if isinstance(cause, KeyboardInterrupt) else cause
    try:
        if threading.current_thread() is threading.main_thread():
            for number in SIGNALS:  # A further signal changes nothing; unlike SIG_IGN, no phase inherits this.
                if signal.getsignal(number) not in (None, signal.SIG_IGN):
                    signal.signal(number, lambda number, frame: None)
        finished = _thread.allocate_lock()
        finished.acquire()
        _thread.start_new_thread(conclude, (cause, children.stop(), finished))  # nothing waits for it to start
        finished.acquire(timeout=EPILOGUE)
    finally:
        if cause == signal.SIGINT:
            with suppress(BaseException):  # end as an unhandled SIGINT ends Python, for the shell
                signal.signal(signal.SIGINT, signal.SIG_DFL)
                signal.pthread_sigmask(signal.SIG_UNBLOCK, [signal.SIGINT])
                os.kill(os.getpid(), signal.SIGINT)
        os._exit(128 + cause if isinstance(cause, int) else 1)


def run(plan, root, jobs=1, keep_going=False, *, cpu_budgets=None, seconds=None):
    """Run a plan; raise GateFailure naming each phase that failed or never ran.
    If supervision fails, or a signal arrives before the previous handlers are
    back, end the process instead (stop)."""
    if threading.current_thread() is not threading.main_thread():
        raise RuntimeError('A gate runs in the main thread, which receives its signals')
    plan = list(plan)
    needs = prerequisites(plan)
    children, interrupts = Children(cpu_budgets), Interrupts()
    try:
        try:
            interrupts.arm()
            passed, failed = supervise(plan, needs, root, jobs, keep_going, children, seconds or {})
        finally:
            interrupts.armed = False  # From here a signal is only recorded.
        children.stop()
        held = signal.pthread_sigmask(signal.SIG_BLOCK, SIGNALS)
        if not interrupts.received:
            interrupts.restore()
    except BaseException as error:
        stop(children, interrupts.received or error)
    if interrupts.received:  # It arrived as the gate finished: its handlers are still in place.
        stop(children, interrupts.received)
    signal.pthread_sigmask(signal.SIG_SETMASK, held)  # One held meanwhile reaches the restored handler.
    measured = {name: row for name, row in children.timings.items() if 'cpu_seconds' in row}
    if measured:
        total = sum(row['cpu_seconds'] for row in measured.values())
        longest = max(measured, key=lambda name: measured[name]['cpu_seconds'])
        print(f"GATE CPU total {total:.3f}s; critical path (longest single phase) "
              f"{measured[longest]['cpu_seconds']:.3f}s: {longest}", flush=True)
    # Only a reported PASS passes: a phase that failed or never ran fails the gate.
    unrun = [phase.name for phase in plan if phase.name not in passed and phase not in failed]
    if failed or unrun:
        raise GateFailure(f"Gate phases failed: {', '.join(phase.name for phase in failed) or 'none'}"
                          + (f"; {len(unrun)} not run: {', '.join(unrun)}" if unrun else ''))
    return children.timings


def cpu_budgets(path=None, *, fresh_objects=None):
    """Read the hand-edited policy; malformed limits must never disable the guard."""
    path = path or Path(__file__).with_name('gate-cpu-budgets.json')
    policy = json.loads(Path(path).read_text())
    if policy.get('schema') != 1 or not isinstance(policy.get('phases'), dict):
        raise ValueError('Invalid gate CPU budget schema')
    def positive(value):
        return type(value) in (int, float) and math.isfinite(value) and value > 0
    for name in ('default', 'critical_path'):
        if not positive(policy.get(name + '_seconds')) or not policy.get(name + '_reason', '').strip():
            raise ValueError(f'Gate CPU {name} budget needs positive seconds and a reason')
    if (not positive(policy.get('fresh_objects_multiplier'))
            or not policy.get('fresh_objects_reason', '').strip()):
        raise ValueError('Fresh-object CPU multiplier needs positive value and a reason')
    for name, entry in policy['phases'].items():
        if (not name or not isinstance(entry, dict) or not positive(entry.get('seconds'))
                or not entry.get('reason', '').strip()):
            raise ValueError(f'Gate CPU budget needs positive seconds and a reason: {name}')
    # Validate against the complete plan, not a selected quick/focused subset.
    # Prerequisites/focused commands run outside that plan but share this policy.
    import gate_plan
    names = {phase.name for profile in gate_plan.PROFILES for phase in gate_plan.plan(profile, False)}
    names.update(('release_tools.py', 'release_tools.py --no-tags', 'release_tools_test.py'))
    unknown = set(policy['phases']) - names
    if unknown:
        raise ValueError(f'Unknown gate CPU phase names: {sorted(unknown)}')
    if fresh_objects is None:
        fresh_objects = os.environ.get('NEWT_HOST_OBJECT_CACHE') == '0'
    if fresh_objects:
        multiplier = policy['fresh_objects_multiplier']
        policy['default_seconds'] *= multiplier
        policy['critical_path_seconds'] *= multiplier
        for entry in policy['phases'].values():
            entry['seconds'] *= multiplier
    return policy
