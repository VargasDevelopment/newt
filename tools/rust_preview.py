#!/usr/bin/env python3
"""Build newt-preview: the real firmware, compiled for the host over C fakes.

tools/firmware_host_build.py compiles every rust/firmware source unchanged,
with the owner's daily features (tools/daily_firmware.py). The preview's own
code is only rust/host/preview: the C fakes of the board, input, NVS, serial,
card, radio and partition natives, the C main() that calls the firmware's own
native entry newt_rust_app_run (fakes/entry.c), the update fixture writer,
and the driver (tools/preview_driver.py) that turns the CLI into boots,
presses and fixture files. .build/newt-preview runs that driver.

`build(edits=...)` relinks a control: the generated firmware copy with exact
text edits, never rust/firmware. A nested module the copy includes by its
path (app_runtime/home_cover.rs) is copied in for its edit.
`run(binary, directory, keys)` is one preview world's boots: presses through
the input fake, a software reset on each restart (exit 75), until the presses
are used up and the loop is idle.
"""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import firmware_host_build as host_build
from daily_firmware import FEATURES

BINARY = ROOT / '.build/newt-preview'
WORK = ROOT / '.build/rust-preview'
PREVIEW = ROOT / 'rust/host/preview'
FIRMWARE = WORK / 'firmware'
LINK = WORK / 'link.json'
# A boot that neither idles, sleeps nor restarts in this long is a fake bug.
BOOT_SECONDS = 60
WAKE = 'wake'
# The daily image's embedded sample reaches these two crates.
DEPENDENCY_FEATURES = ('newt-c3-board/embedded-sample', 'newt-reader-ui/embedded-sample')


def relocate(text: str) -> str:
    """A native POSIX source with its paths on the preview card."""
    for call in ('open', 'opendir', 'mkdir', 'unlink', 'rmdir', 'stat', 'lstat'):
        text = re.sub(r'\b' + call + r'\(path(?=[,)])', call + '(fixture_path(path)', text)
    return text.replace('rename(from, to)', 'rename(fixture_path(from), fixture_path(to))')


def native_io(text: str) -> str:
    """native_io.c on the preview card, with the store-fault hooks."""
    text = text.replace('#include "native_io.h"', '#include "native_io.h"\n'
                        'extern char *fixture_path(const char *);\n'
                        'extern void preview_io_open(int32_t, const char *);\n'
                        'extern void preview_io_list(const char *);\n'
                        'extern int preview_io_fault(const char *, int32_t);\n'
                        'extern int preview_stat_fault(const char *);', 1)
    text = text.replace('    *out = fd;', '    preview_io_open(fd, path);\n    *out = fd;', 1)
    text = text.replace('    DIR *dir = opendir(path);', '    preview_io_list(path);\n    DIR *dir = opendir(path);', 1)
    text = text.replace('int32_t newt_io_stat(const char *path, uint32_t no_follow, struct newt_io_stat *out) {',
                        'int32_t newt_io_stat(const char *path, uint32_t no_follow, struct newt_io_stat *out) {\n'
                        '    if (preview_stat_fault(path)) return failure(EIO);', 1)
    # A refused read or write moves nothing; a dropped sync reports the medium failed.
    text = text.replace('    const ssize_t result = read(fd, data, bytes);',
                        '    if (preview_io_fault("card-read", fd) == 1) return failure(EIO);\n'
                        '    const ssize_t result = read(fd, data, bytes);', 1)
    text = text.replace('    const ssize_t result = write(fd, data, bytes);',
                        '    if (preview_io_fault("card-write", fd) == 1) return failure(EIO);\n'
                        '    const ssize_t result = write(fd, data, bytes);', 1)
    text = text.replace('return status(fsync(fd));',
                        'if (preview_io_fault("card-sync", fd) == 2) return failure(EIO); return status(fsync(fd));', 1)
    return relocate(text)


def download(text: str) -> str:
    """download.c on the preview card; its clock is the fake board clock."""
    text = text.replace('#include <unistd.h>', '#include <unistd.h>\n'
                        'extern char *fixture_path(const char *);\nextern uint32_t nb_millis(void);', 1)
    start = text.index('uint32_t newt_download_millis(void) {')
    text = text[:start] + 'uint32_t newt_download_millis(void) { return nb_millis(); }\n'
    return relocate(text)


def build(edits=None) -> Path:
    """The firmware host binary. With `edits` ([(firmware file under
    rust/firmware, exact old text, new text)]), a control relinked from the
    last build's generated copy; each old text must occur exactly once."""
    if edits:
        return control(edits)
    WORK.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    run = host_build.logged_runner(WORK, env, timeout=900)
    build = host_build.Build(WORK, ROOT, run)
    env.update(build.environment(env, 'host-preview'))
    build.dependencies(DEPENDENCY_FEATURES)
    build.parser_objects(PREVIEW / 'fakes/board.c')
    build.generate_sources()
    build.wire_sources()
    (WORK / 'download.c').write_text(download((host_build.SALAMANDER / 'native_io/native/download.c').read_text()))
    # The entry, fixture, radio, HTTPS and partition fakes sit beside board.c and ports.c.
    others = [(path.stem, path) for path in sorted((PREVIEW / 'fakes').glob('*.c'))
              if path.name not in ('board.c', 'ports.c')]
    build.posix_objects(build.native_io(native_io), PREVIEW / 'fakes/ports.c',
                        extra=[('download', WORK / 'download.c'), *others])
    build.root(None, features=FEATURES)
    build.link(FIRMWARE)
    # Controls in later processes relink with this exact command and root.
    LINK.write_text(json.dumps({'command': list(map(str, build.command)), 'environment': {
        key: env[key] for key in ('RUSTC', 'CARGO_HOME', 'NEWT_DEV_BUILD_ID', 'NEWT_DEV_BUILD_DATE')}}, indent=1) + '\n')
    cli(FIRMWARE, BINARY)
    return FIRMWARE


def cli(firmware: Path, path: Path | None = None) -> Path:
    """The newt-preview command (tools/preview_driver.py) over `firmware`;
    beside it unless `path` is given."""
    path = path or firmware.parent / 'newt-preview'
    path.unlink(missing_ok=True)
    path.write_text(f'#!/bin/sh\nexec "{sys.executable}" -B "{ROOT / "tools/preview_driver.py"}" '
                    f'--firmware "{firmware}" "$@"\n')
    path.chmod(0o755)
    return path


def _control_command(command, optimize):
    if optimize:
        return list(command)
    # Response files and bundled short options can hide codegen overrides.
    # Refuse those spellings instead of implementing rustc's option parser.
    for arg in command:
        if arg.startswith('@') or (arg.startswith('-') and not arg.startswith(('--', '-C', '-L'))
                                   and len(arg) > 2 and any(flag in arg for flag in 'OC')):
            raise ValueError('control cannot validate response files or bundled O/C flags')
    options, pending = [], False
    for arg in command:
        if pending:
            options.append(arg)
            pending = False
        elif arg in ('-C', '--codegen'):
            pending = True
        elif arg.startswith('-C'):
            options.append(arg[2:])
        elif arg.startswith('--codegen='):
            options.append(arg.removeprefix('--codegen='))
        elif arg == '-O':
            options.append('opt-level=2')
    keys = [option.split('=', 1)[0].replace('_', '-') for option in options]
    if pending or command.count('-Copt-level=1') != 1 or keys.count('opt-level') != 1:
        raise ValueError('control requires the root opt-level=1 policy exactly once')
    if any(key in ('debug-assertions', 'overflow-checks') for key in keys):
        raise ValueError('control requires the root default debug/overflow policies')
    # opt-level=1 defaults both policies off. Reject an explicit root override
    # rather than silently changing its semantics in the cheaper control.
    return ['-Copt-level=0' if arg == '-Copt-level=1' else arg for arg in command] + [
        '-Cdebug-assertions=no', '-Coverflow-checks=no']


def control(edits, *, optimize: bool = True) -> Path:
    """Relink the last build over a private copy of its generated sources, so
    parallel controls never see each other's edits. Timing-independent suites
    may skip root optimization; the positive preview stays optimized."""
    link = json.loads(LINK.read_text())
    command = _control_command(link['command'], optimize)
    source = WORK / 'source'
    key = '%08x' % zlib.crc32(json.dumps((edits, optimize)).encode())
    out = WORK / 'controls' / key
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    copies = out / 'source'
    shutil.copytree(source, copies)
    rewire = lambda text: text.replace(json.dumps(str(source))[1:-1] + '/', json.dumps(str(copies))[1:-1] + '/')
    for path in copies.rglob('*.rs'):
        path.write_text(rewire(path.read_text()))
    for name, old, new in edits:
        path = copies / name
        if not path.exists():
            original = host_build.FIRMWARE / name
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(original, path)
            quoted = json.dumps(str(original))
            for module in copies.glob('*.rs'):
                text = module.read_text()
                if quoted in text:
                    module.write_text(text.replace(quoted, json.dumps(str(path))))
        text = path.read_text()
        if text.count(old) != 1:
            raise ValueError(f'{name}: the control text must occur exactly once')
        path.write_text(text.replace(old, new))
    host = out / 'host.rs'
    host.write_text(rewire((WORK / 'host.rs').read_text()))
    env = {**os.environ, **link['environment']}
    program = out / 'firmware'
    subprocess.run([*command, host, '-o', program], cwd=ROOT, env=env, check=True,
                   capture_output=True, text=True, timeout=900)
    return program


def other_slot_selected(directory: Path) -> bool:
    """The restart boots app1: otadata's newest valid record (not invalid or
    aborted, as the bootloader skips) selects it, so the next boot is
    another firmware and the preview ends."""
    try:
        data = (directory / 'partitions/otadata.bin').read_bytes()
    except FileNotFoundError:
        return False
    newest = 0
    for at in (0, 4096):
        seq = int.from_bytes(data[at:at + 4], 'little')
        state = int.from_bytes(data[at + 24:at + 28], 'little')
        crc = int.from_bytes(data[at + 28:at + 32], 'little')
        if (seq not in (0, 0xffffffff) and state not in (3, 4)
                and crc == zlib.crc32(data[at:at + 4], 0xffffffff)):
            newest = max(newest, seq)
    return bool(newest) and (newest - 1) % 2 == 1


class Unplayable(ValueError):
    """Presses the device could not take: a wake while awake, or presses
    left after a sleep or after the restart into another firmware."""


def boot(binary, directory: Path, keys, kind: str) -> tuple[str, list]:
    """One launch: (why it ended, presses it left)."""
    (directory / 'script.txt').write_text(''.join(f'{key}\n' for key in keys))
    (directory / 'boot.txt').write_text(kind + '\n')
    (directory / 'exit.txt').unlink(missing_ok=True)
    result = subprocess.run([str(binary)], env={**os.environ, 'NEWT_PREVIEW_DIR': str(directory)},
                            capture_output=True, text=True, timeout=BOOT_SECONDS)
    reason = (directory / 'exit.txt').read_text() if (directory / 'exit.txt').exists() else ''
    expected = {'idle': 0, 'sleep': 0, 'freeze': 0, 'restart': 75}
    if expected.get(reason) != result.returncode:
        raise RuntimeError(f'preview boot ended {result.returncode} ({reason or "no reason"}): '
                           + result.stderr[-4000:])
    return reason, [int(key) for key in (directory / 'script.txt').read_text().split()]


def run(binary, directory, keys, boot_kind: str = 'power-on') -> tuple[list[bytes], str]:
    """(final panel bytes of each launch, serial output of every launch).
    `keys` are button codes (0 Back, 1 Confirm, 2 Left, 3 Right, 4 Up,
    5 Down, 6 Power) and WAKE, which powers a sleeping device back on."""
    directory = Path(directory)
    segments, current = [], []
    for key in keys:
        if key == WAKE:
            segments.append(current)
            current = []
        else:
            current.append(int(key))
    segments.append(current)
    log = directory / 'serial.log'
    log.unlink(missing_ok=True)
    frames, kind, reason = [], boot_kind, None
    for index, script in enumerate(segments):
        if index:
            if reason != 'sleep':
                raise Unplayable('wake while awake')
            kind = 'deep-sleep'
        while True:
            reason, script = boot(binary, directory, script, kind)
            frames.append((directory / 'panel.bin').read_bytes() if (directory / 'panel.bin').exists() else b'')
            if reason != 'restart':
                break
            if other_slot_selected(directory):
                if script or index + 1 < len(segments):
                    raise Unplayable('presses after the restart into another firmware')
                return frames, log.read_text(errors='replace')
            kind = 'software'
        if script:
            raise Unplayable(f'{len(script)} presses left after {reason}')
    return frames, log.read_text(errors='replace') if log.exists() else ''


if __name__ == '__main__':
    print(build().relative_to(ROOT))
