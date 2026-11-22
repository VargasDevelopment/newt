"""Bounded concurrent host compiler jobs; Python standard library only.

The same commands run at every job limit; concurrency changes only when they
run. A slot bounds one compiler, linker or compiled fixture run. A gate may
share one set of slot files with all its phases (NEWT_HOST_JOB_SLOTS), so
nested test processes together never hold more slots than the gate allows.
Without them, one process never holds more than NEWT_HOST_JOBS at once.
The per-user pool also bounds phases/compilers across that user's checkouts. One nested
worker may borrow its phase reservation; additional workers need host capacity.
Never acquire nested compiler slots while already holding a compiler slot.
"""
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
import fcntl
import os
from pathlib import Path
import subprocess
import stat
import sys
import threading
import time

SLOTS = 'NEWT_HOST_JOB_SLOTS'
JOBS = 'NEWT_HOST_JOBS'
GLOBAL_JOBS = 'NEWT_HOST_GLOBAL_JOBS'
GLOBAL_ROOT = 'NEWT_HOST_GLOBAL_SLOT_DIR'
CREDIT = 'NEWT_HOST_PHASE_CREDIT'
CREDIT_POOL = 'NEWT_HOST_PHASE_POOL'
WAIT_LOG = 'NEWT_HOST_SLOT_WAIT_LOG'
NATIVE_ROOTS = 'NEWT_GATE_NATIVE_ROOTS'  # Per-gate, private short native mount names.


CGROUP_MEMBERSHIP = Path('/proc/self/cgroup')
CGROUP_ROOT = Path('/sys/fs/cgroup')


def available_cpus() -> tuple[int, str]:
    """Affinity capped by this process's tightest cgroup v2 CPU quota.

    Unsupported affinity falls back to cpu_count. Missing/unreadable cgroup
    files leave affinity authoritative; still inspect every visible ancestor.
    """
    try:
        affinity = max(1, len(os.sched_getaffinity(0)))
        source = f'affinity={affinity}'
    except (AttributeError, OSError):
        affinity = os.cpu_count() or 1
        source = f'cpu_count={affinity} (affinity unavailable)'
    quota, quota_source, readable = None, '', False
    try:
        membership = next(line[3:] for line in CGROUP_MEMBERSHIP.read_text().splitlines()
                          if line.startswith('0::'))
        relative = Path(membership)
        if not relative.is_absolute() or '..' in relative.parts:
            raise ValueError('invalid cgroup path')
        directory = CGROUP_ROOT / membership.lstrip('/')
        while True:
            try:
                amount, period = (directory / 'cpu.max').read_text().split()
                period = int(period)
                if period <= 0 or (amount != 'max' and int(amount) <= 0):
                    raise ValueError('invalid CPU quota')
                readable = True
                if amount != 'max':
                    limit = max(1, (int(amount) + period - 1) // period)
                    if quota is None or limit < quota:
                        quota, quota_source = limit, f'{directory / "cpu.max"}={amount}/{period}'
            except (OSError, ValueError):
                pass
            if directory == CGROUP_ROOT:
                break
            directory = directory.parent
    except (OSError, ValueError, StopIteration):
        pass
    detail = f'{quota} ({quota_source})' if quota is not None else ('unlimited' if readable else 'unavailable')
    return min(affinity, quota) if quota is not None else affinity, f'{source}; cgroup v2 cpu.max={detail}'


def global_limit() -> int:
    value = os.environ.get(GLOBAL_JOBS, '')
    if value and (not value.isdigit() or int(value) < 1):
        raise RuntimeError(f'{GLOBAL_JOBS} must be a positive integer')
    return int(value) if value else available_cpus()[0]


def global_root() -> Path:
    return Path(os.environ.get(GLOBAL_ROOT, f'/var/tmp/newt-host-job-slots-v2-{os.getuid()}')).absolute()


@contextmanager
def global_slot():
    """One host-wide lease, or the phase's reserved credit. Never unlink pool files.

    Retry credit as well as pool slots: waiting only on the pool would deadlock
    when every phase held a reservation and its compilers needed that credit.
    flock and close-on-exec release each lease if its owning process crashes.
    """
    root = global_root()
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    metadata = root.lstat()
    if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid() or metadata.st_mode & 0o022:
        raise RuntimeError(f'Host slot pool must be a user-owned private directory: {root}')
    paths = [root / f'slot-{index}' for index in range(global_limit())]
    if os.environ.get(CREDIT_POOL) == str(root) and os.environ.get(CREDIT):
        paths.insert(0, Path(os.environ[CREDIT]))
    started, waited, delay = time.monotonic(), False, 0.005
    while True:
        for path in paths:
            descriptor = os.open(path, os.O_RDWR | os.O_CREAT | os.O_CLOEXEC | os.O_NOFOLLOW, 0o600)
            try:
                fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                os.close(descriptor)
                continue
            except BaseException:
                os.close(descriptor)
                raise
            try:
                # A reused inode must not age out under the host's temporary-file
                # cleanup policy while gates are actively acquiring leases.
                os.utime(descriptor, None)
                yield time.monotonic() - started if waited else 0.0
            finally:
                os.close(descriptor)
            return
        waited = True
        time.sleep(delay)
        delay = min(delay * 2, 0.1)


def record_wait(seconds):
    if not seconds:
        return
    log = os.environ.get(WAIT_LOG)
    if log:
        descriptor = os.open(log, os.O_WRONLY | os.O_APPEND | os.O_CREAT | os.O_CLOEXEC, 0o600)
        try:
            os.write(descriptor, f'{seconds:.6f}\n'.encode())
        finally:
            os.close(descriptor)
    else:
        print(f'HOST slots waited {seconds:.3f}s', file=sys.stderr, flush=True)


def job_limit() -> int:
    """Compiler jobs one process may start at once; NEWT_HOST_JOBS=1 is serial."""
    value = os.environ.get(JOBS, '')
    if value and (not value.isdigit() or int(value) < 1):
        raise RuntimeError(f'{JOBS} must be a positive integer')
    return int(value) if value else available_cpus()[0]


def create_slots(directory: Path, count: int) -> Path:
    """Prepare the shared compiler slot files for one gate invocation."""
    directory.mkdir(parents=True, exist_ok=True)
    for stale in directory.glob('slot-*'):
        stale.unlink()
    for index in range(count):
        (directory / f'slot-{index}').touch()
    return directory


_process = {'limit': None, 'semaphore': None}
_process_lock = threading.Lock()


def process_slots() -> threading.BoundedSemaphore:
    """This process's compiler slots, used when the gate shares none."""
    limit = job_limit()
    with _process_lock:
        if _process['limit'] != limit:
            _process.update(limit=limit, semaphore=threading.BoundedSemaphore(limit))
        return _process['semaphore']


@contextmanager
def local_slot():
    """Hold one gate-wide compiler slot; the lock dies with its process."""
    directory = os.environ.get(SLOTS)
    if not directory:
        with process_slots():
            yield
        return
    paths = sorted(Path(directory).glob('slot-*'))
    if not paths:
        raise RuntimeError(f'{SLOTS} names no compiler slots: {directory}')
    delay = 0.002
    while True:
        for path in paths:
            descriptor = os.open(path, os.O_RDWR | os.O_CLOEXEC)
            try:
                fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                os.close(descriptor)
                continue
            try:
                yield
            finally:
                os.close(descriptor)
            return
        time.sleep(delay)
        delay = min(delay * 2, 0.05)


@contextmanager
def slot():
    """A local compiler slot plus host capacity, borrowing phase credit first."""
    # A host waiter must not occupy a local slot needed by another phase
    # that can borrow its own reserved credit.
    with global_slot() as waited:
        record_wait(waited)
        with local_slot():
            yield


def run(command, **options):
    """subprocess.run for one compiler, linker or compiled fixture child, inside a slot."""
    with slot():
        return subprocess.run(command, **options)


def run_all(tasks, jobs: int | None = None) -> list:
    """Run callables and return their results in order.

    Serial mode stops at the first failure like a plain loop. Concurrent mode
    runs every submitted task to completion, then raises the first failure in
    task order, so a failure report never depends on scheduling.
    """
    tasks = list(tasks)
    jobs = job_limit() if jobs is None else jobs
    if jobs == 1 or len(tasks) < 2:
        return [task() for task in tasks]
    with ThreadPoolExecutor(max_workers=min(jobs, len(tasks))) as pool:
        futures = [pool.submit(task) for task in tasks]
    for future in futures:
        if future.exception() is not None:
            raise future.exception()
    return [future.result() for future in futures]
