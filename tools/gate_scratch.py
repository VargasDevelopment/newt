"""Own the gate's scratch environment before any prerequisite or phase starts."""
from contextlib import contextmanager
import os
from pathlib import Path
import shutil
import tempfile

from gate_runner import STOP_REMOVES
import host_jobs
from native_output import owned_directory
from host_objects import scratch_paths

BASE = Path('/var/tmp')


@contextmanager
def environment(root=Path(__file__).resolve().parents[1]):
    previous = os.environ.get('TMPDIR')
    previous_mounts = os.environ.get(host_jobs.NATIVE_ROOTS)
    cached = tempfile.tempdir
    owned = not previous
    directory = (Path(tempfile.mkdtemp(prefix='', dir=BASE)) if owned
                 else Path(previous).resolve(strict=True))
    if owned:
        STOP_REMOVES.append(str(directory))  # run()'s bounded signal-stop path skips finally.
    try:
        if not directory.is_dir():
            raise ValueError(f'TMPDIR must name a writable directory: {directory}')
        # Probe the explicit path: tempfile's normal fallback must not hide an
        # invalid caller directory by silently selecting a different location.
        with tempfile.TemporaryFile(dir=directory):
            pass
        build = owned_directory(root, root / '.build', create=True)
        # Real private native parents keep device paths short without aliases.
        with tempfile.TemporaryDirectory(prefix='', dir=build) as mounts:
            # File times come from the kernel's coarse clock, which can trail
            # time.time_ns() by a tick. This new directory's time, on the
            # receipts' file system, marks the start: no later receipt is older.
            started = Path(mounts).stat().st_mtime_ns
            STOP_REMOVES.append(mounts)
            try:
                os.environ.update(TMPDIR=str(directory), **{host_jobs.NATIVE_ROOTS: mounts})
                tempfile.tempdir = str(directory)
                print(f'GATE scratch {directory} ({"private per-run" if owned else "caller supplied"})', flush=True)
                print(f'GATE native roots {mounts}', flush=True)
                yield directory
            finally:
                try:
                    check_receipts(root, started, (directory, Path(mounts)))
                finally:
                    STOP_REMOVES.remove(mounts)
    finally:
        try:
            if owned:
                STOP_REMOVES.remove(str(directory))
                shutil.rmtree(directory)
        finally:
            if previous is None:
                os.environ.pop('TMPDIR', None)
            else:
                os.environ.update(TMPDIR=previous)
            if previous_mounts is None:
                os.environ.pop(host_jobs.NATIVE_ROOTS, None)
            else:
                os.environ[host_jobs.NATIVE_ROOTS] = previous_mounts
            tempfile.tempdir = cached


def check_receipts(root, started, scratch):
    """Audit every primary host-object receipt published during this invocation.

    Check lexical and canonical forms, including ROOT-relative native paths.
    This is a secondary metadata check for late/alternate receipt writers;
    HostObjects checks expanded contents and dependencies before any compile.
    Existing receipts are not this run's evidence and are left untouched.
    """
    needles = scratch_paths(root, scratch[0], scratch[1] if len(scratch) > 1 else None)
    receipts = [path for path in (root / '.build/host-objects').glob('*.json')
                if path.stat().st_mtime_ns >= started]
    errors = [str(path.relative_to(root)) for path in receipts
              if any(value in path.read_text() for value in needles)]
    if errors:
        raise RuntimeError('Host-object receipts contain per-run scratch paths:\n' + '\n'.join(errors))
    print(f'PASS scratch-free host-object receipts: {len(receipts)} written this run', flush=True)
