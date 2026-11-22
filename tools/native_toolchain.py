"""Coordinate native builds that share this checkout's PlatformIO toolchain.

The pinned espressif32 platform edits its own framework packages during a
build. Configuration rewrites the Arduino idf_component.yml in place (a write
that fails harmlessly in a read-only shared view, tools/toolchain-share.py) and
copies the MCU build script to a backup beside it unless one exists. The end
of a successful build truncates and rewrites the script from that backup and
removes it; a run that stops earlier (a failed build) leaves the backup.
Those packages are build identity inputs (tools/code_health_resources.py),
so every PlatformIO build, and a health build's identity capture and report, hold this checkout's toolchain
lock exclusively: no configuration reads the script while another build
rewrites it, and no capture sees another build's backup. Later identity
rechecks (code_health.py, reading-ready.py, ci_gate.py) take no lock; another
build's edit can only make them fail. Cleaning stops before configuration,
never edits the framework and shares the lock. A health build never captures
its identity beside a leftover backup, which its own build would restore and
remove.
"""
from contextlib import contextmanager
import fcntl
import os
from pathlib import Path

from native_output import owned_file

LOCK = ".build/native-toolchain.lock"
# The backup each MCU build script keeps while a build runs, by package root.
BACKUP = "packages/framework-arduinoespressif32-libs/*/pioarduino-build.py.*"


@contextmanager
def locked(root, operation):
    path = owned_file(root, Path(root) / LOCK, create_parents=True)
    descriptor = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        fcntl.flock(descriptor, operation)
        yield
    finally:
        os.close(descriptor)


def quiescent(root):
    """No other PlatformIO run of this checkout overlaps: build or capture here."""
    return locked(root, fcntl.LOCK_EX)


def cleaning(root):
    return locked(root, fcntl.LOCK_SH)


def held(root):
    """The lock as another process would find it now: free, shared or
    exclusive. For tests and diagnostics; no build decides by it."""
    path = Path(root) / LOCK
    if not path.exists():
        return "free"
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        for operation, state in ((fcntl.LOCK_EX, "free"), (fcntl.LOCK_SH, "shared")):
            try:
                fcntl.flock(descriptor, operation | fcntl.LOCK_NB)
            except BlockingIOError:
                continue
            return state
        return "exclusive"
    finally:
        os.close(descriptor)


def require_restored(root):
    """Refuse a build-script backup that an unfinished PlatformIO run left."""
    leftover = sorted((Path(root) / ".toolchain/platformio").glob(BACKUP))
    if leftover:
        raise RuntimeError(f"An unfinished PlatformIO run (a failed build) left "
                           f"{leftover[0].relative_to(root)}; a plain build restores it: "
                           "python3 tools/firmware-build.py --clean")


def restore(root):
    """What a finished build's last step does: rewrite each MCU build script
    from its backup, then remove the backup. Hold quiescent(root)."""
    for backup in sorted((Path(root) / ".toolchain/platformio").glob(BACKUP)):
        script = owned_file(root, backup.with_suffix(""))
        script.write_bytes(owned_file(root, backup).read_bytes())
        backup.unlink()
