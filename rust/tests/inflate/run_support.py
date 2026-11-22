#!/usr/bin/env python3
"""Future offline full packet check. No acquisition, leases, firmware or devices."""
from contextlib import contextmanager, nullcontext
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import signal
import stat
import subprocess
import sys
import shutil
import time

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
TARGET = 'riscv32imc-unknown-none-elf'
_cargo_spec = importlib.util.spec_from_file_location(__name__ + '_cargo_context', HERE / 'cargo_context.py')
_cargo = importlib.util.module_from_spec(_cargo_spec)
_cargo_spec.loader.exec_module(_cargo)
_python_identity = None


def cargo_execution(command, cwd, env):
    tools = _cargo.pinned_host_link_tools(ROOT, HERE / 'toolchains.json')
    return _cargo.cargo_context(command, cwd, env, tools)


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def digest(path, entry=None):
    if entry is None:
        # Tool aliases may be symlinks; inventory entries must never be followed.
        path = path.resolve(strict=True)
        entry = path.lstat()
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW), 'rb') as stream:
        opened = os.fstat(stream.fileno())
        require(stat.S_ISREG(opened.st_mode) and
                (opened.st_dev, opened.st_ino) == (entry.st_dev, entry.st_ino),
                'sealed source file changed before hashing: ' + str(path))
        prefix = stream.read(65537)
        if len(prefix) <= 65536:
            return hashlib.sha256(prefix).hexdigest()
        stream.seek(0)
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def source_inventory(root):
    """Seal a source-only tree, including its full file set, without following links."""
    root = Path(root)
    require(root.is_dir() and not root.is_symlink(), 'source root absent or symlinked')
    files = {}
    for path in sorted(root.rglob('*')):
        entry = path.lstat()
        require(not stat.S_ISLNK(entry.st_mode), 'symlink in sealed source tree: ' + str(path))
        if stat.S_ISREG(entry.st_mode):
            files[str(path.relative_to(root))] = digest(path, entry)
        else:
            require(stat.S_ISDIR(entry.st_mode), 'special file in sealed source tree: ' + str(path))
    return files


def verify_source_inventory(root, expected):
    actual = source_inventory(root)
    if actual != expected:
        changes = {'added': sorted(actual.keys() - expected.keys()),
                   'missing': sorted(expected.keys() - actual.keys()),
                   'modified': sorted(name for name in actual.keys() & expected.keys()
                                      if actual[name] != expected[name])}
        detail = {kind: {'count': len(names), 'paths': names[:12]}
                  for kind, names in changes.items() if names}
        raise RuntimeError('sealed source inventory changed: ' + json.dumps(detail, sort_keys=True))


def source_metadata(root):
    """A boundary tripwire; this never substitutes for opening/closing hashes."""
    root = Path(root)
    entries = {}
    pending = [(str(root), '.')]
    while pending:
        path, relative = pending.pop()
        entry = os.lstat(path)
        kind = stat.S_IFMT(entry.st_mode)
        require(kind in (stat.S_IFREG, stat.S_IFDIR) and
                (relative != '.' or kind == stat.S_IFDIR),
                'invalid entry in sealed source tree: ' + str(path))
        entries[relative] = (kind, entry.st_dev, entry.st_ino,
            entry.st_size, entry.st_mtime_ns, entry.st_ctime_ns)
        if kind == stat.S_IFDIR:
            # Full census without rebuilding Path objects for every command.
            with os.scandir(path) as children:
                pending.extend((child.path, child.name if relative == '.' else
                                relative + '/' + child.name) for child in children)
    return entries


def copy_workspace(source, destination):
    """Copy each Cargo subtree once, including nested firmware members."""
    import tomllib
    source, destination = Path(source), Path(destination)
    members = tomllib.loads((source / 'Cargo.toml').read_text())['workspace']['members']
    for name in members:
        require(isinstance(name, str) and not Path(name).is_absolute()
                and all(part not in ('.', '..') for part in name.split('/'))
                and (source / name / 'Cargo.toml').is_file(), 'local crate member required')
    destination.mkdir()
    for name in members:
        if any(Path(parent) in Path(name).parents for parent in members):
            continue
        shutil.copytree(source / name, destination / name,
                        ignore=shutil.ignore_patterns('target', '__pycache__'))
    for name in ('Cargo.toml', 'Cargo.lock'):
        shutil.copyfile(source / name, destination / name)


class SourceSeal:
    """Full bytes at entry/exit, with fresh metadata at each command boundary."""
    # Non-root processes cannot restore ctime: writes, chmod and renames trip
    # this even if bytes/mtime return. Root, clock changes and a deliberate
    # adversary are outside this run-integrity threat model.
    def __init__(self, root, expected=None):
        self.root = Path(root)
        self.metadata = source_metadata(self.root)
        self.hashes = source_inventory(self.root)
        require(expected is None or self.hashes == expected,
                'opening source bytes differ from sealed inputs: ' + str(root))
        self.check()

    def check(self):
        actual = source_metadata(self.root)
        changed = sorted(name for name in actual.keys() | self.metadata.keys()
                         if actual.get(name) != self.metadata.get(name))
        require(not changed, 'sealed source metadata changed: ' +
                json.dumps({'root': str(self.root), 'paths': changed[:12]}))

    def close(self):
        verify_source_inventory(self.root, self.hashes)
        self.check()


def close_sources(seals):
    errors = []
    for seal in seals:
        try:
            seal.close()
        except BaseException as error:
            errors.append(error)
    if errors:
        raise errors[0]


@contextmanager
def source_commands(seals):
    for seal in seals:
        seal.check()
    try:
        yield
    finally:
        for seal in seals:
            seal.check()



def receipt_path(value):
    """Keep checkout locations out of receipts without changing executed arguments."""
    value = str(value)
    root = str(ROOT.resolve())
    return '.' if value == root else value.replace(root + '/', '')


class Run:
    def __init__(self, output, tools, deadline):
        _cargo.reject_optimized_python()
        self.output = output
        self.tools = tools
        self.deadline = deadline
        self.commands = []
        self.env = {key: value for key, value in os.environ.items()
                    if key.startswith('NEWT_HOST_') or key == 'NEWT_GATE_NATIVE_ROOTS'}
        for name in ('tmp', 'cargo-home', 'cache'):
            (output / name).mkdir()
        self.env.update(PATH='/usr/bin:/bin:/usr/sbin:/sbin', LC_ALL='C',
                        CARGO_HOME=str(output / 'cargo-home'), CARGO_NET_OFFLINE='true',
                        CARGO_BUILD_JOBS='2', CARGO_INCREMENTAL='0', RUST_TEST_THREADS='2',
                        ASAN_OPTIONS='halt_on_error=1:abort_on_error=1',
                        UBSAN_OPTIONS='halt_on_error=1:abort_on_error=1',
                        RUSTC=str(tools['rustc']), RUSTDOC=str(tools['rustdoc']),
                        RUSTFLAGS='-Ccodegen-units=1', PYTHONDONTWRITEBYTECODE='1',
                        TMPDIR=str(output / 'tmp'), TMP=str(output / 'tmp'),
                        TEMP=str(output / 'tmp'), XDG_CACHE_HOME=str(output / 'cache'))

    def run(self, command, cwd, stdin=None):
        command = list(map(str, command))
        number = len(self.commands)
        log = self.output / f'{number:03d}'
        receipt = {'command': [receipt_path(arg) for arg in command],
                   'cwd': receipt_path(cwd), 'stdin_sha256':
                   hashlib.sha256((stdin or '').encode()).hexdigest(), 'log': log.name}
        self.commands.append(receipt)
        (log.with_suffix('.command.json')).write_text(json.dumps(receipt, indent=2) + '\n')
        remaining = self.deadline - time.monotonic()
        require(remaining > 0, 'phase allowance exhausted')
        with log.with_suffix('.stdout').open('w') as stdout, log.with_suffix('.stderr').open('w') as stderr:
            try:
                context = (cargo_execution(command, cwd, self.env)
                           if command[0] == str(self.tools.get('cargo', '')) else nullcontext(self.env))
                with context as env:
                    with owned_process(command, cwd=cwd, env=env, stdin=subprocess.PIPE,
                                       stdout=stdout, stderr=stderr, text=True,
                                       start_new_session=True) as process:
                        process.communicate(stdin, timeout=remaining)
            except subprocess.TimeoutExpired as error:
                raise RuntimeError(f'timeout, never a mutation kill: {log}') from error
        receipt['returncode'] = process.returncode
        log.with_suffix('.command.json').write_text(json.dumps(receipt, indent=2) + '\n')
        require(process.returncode == 0, f'command failure, never a mutation kill: {log}')
        return read_output(log.with_suffix('.stdout'))

    def execute(self, command, cwd, stdin=None):
        """Admit a clean runtime result before any semantic comparison."""
        number = len(self.commands)
        result = self.run(command, cwd, stdin)
        errors = self.output / f'{number:03d}.stderr'
        require(errors.read_bytes() == b'', f'runtime diagnostics, never a mutation kill: {errors}')
        return result

    def cargo(self, workspace, target, *arguments):
        return self.run([self.tools['cargo'], *arguments, '--manifest-path', workspace / 'Cargo.toml',
                         '--offline', '--frozen', '--locked', '--jobs', '2', '--target-dir', target], workspace)



def rust_payload_identity(receipt):
    """Installer manifests/logs vary by prefix; compiler payloads must not."""
    metadata = {'lib/rustlib/install.log'}
    for component in receipt['lock']['components']:
        name = component['name']
        if name == 'rust-std':
            name += '-' + component['target']
        metadata.add('lib/rustlib/manifest-' + name)
    files = receipt['files']
    require(metadata <= files.keys() and all('sha256' in files[name] for name in metadata),
            'Rust installer metadata missing or not regular')
    # These five generated bookkeeping files remain bound by the full receipt
    # below; this independent pin covers every compiler/library/support byte.
    payload = {name: entry for name, entry in files.items() if name not in metadata}
    return hashlib.sha256(json.dumps(payload, sort_keys=True).encode()).hexdigest()


def python_tool():
    """The runtime owner seals the interpreter and its library across the check."""
    global _python_identity
    spec = importlib.util.spec_from_file_location('packet_python_runtime', ROOT / 'tools/python_runtime.py')
    runtime = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runtime)
    files = {}
    identity = runtime.runtime_identity(inventory=files)
    if _python_identity is None:
        _python_identity = identity, files
    before, before_files = _python_identity
    if identity != before:
        fields = sorted(key for key in before.keys() | identity.keys() if before.get(key) != identity.get(key))
        changed = sorted(name for name in before_files.keys() | files.keys()
                         if before_files.get(name) != files.get(name))
        raise RuntimeError('shared Python runtime changed during the check: fields '
                           + ', '.join(fields) + '; paths ' + ', '.join(changed))
    return Path(identity['resolved_path']).expanduser()


def verify_tools():
    lock = json.loads((HERE / 'toolchains.json').read_text())
    tools = {name: ROOT / entry['path'] for name, entry in lock['executables'].items()}
    for name, entry in lock['executables'].items():
        require(digest(tools[name]) == entry['sha256'], f'approved tool changed: {name}')
    tools['python'] = python_tool()
    receipt_path = ROOT / '.toolchain/rust/installed.json'
    receipt = json.loads(receipt_path.read_text())
    require(receipt['lock'] == lock['rust_lock'], 'Rust approval lock changed')
    require(rust_payload_identity(receipt) == lock['rust_payload_sha256'], 'Rust compiler payload changed')
    prefix = ROOT / '.toolchain/rust/toolchain'
    actual = {str(path.relative_to(prefix)) for path in prefix.rglob('*')
              if path.is_file() or path.is_symlink()}
    require(actual == receipt['files'].keys(), 'Rust installation file set changed')
    for relative, entry in receipt['files'].items():
        path = prefix / relative
        require(path.resolve().is_relative_to(prefix.resolve()), 'toolchain entry escapes prefix')
        if 'link' in entry:
            require(path.is_symlink() and os.readlink(path) == entry['link'], 'Rust symlink changed')
        else:
            require(not path.is_symlink() and path.is_file() and path.stat().st_size == entry['bytes']
                    and bool(path.stat().st_mode & 0o111) == entry['executable']
                    and digest(path) == entry['sha256'], f'Rust component changed: {relative}')
    return tools


def read_output(path):
    # Universal-newline text I/O would hide malformed transport bytes.
    return path.read_bytes().decode('utf-8')


def stop_for_signal(signum, _frame):
    # Once stopping, a repeated request must not interrupt child cleanup.
    signal.signal(signal.SIGINT, signal.SIG_IGN)
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    raise KeyboardInterrupt(f'received signal {signum}')


def install_signal_handlers():
    for signum in (signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, stop_for_signal)



@contextmanager
def owned_process(command, *, cleanup=None, **kwargs):
    """Register cleanup before delivering cancellation, including during spawn.

    Python handlers are deferred, not POSIX masks: exec children retain normal
    signal delivery. Separate process groups belong to this scope through exit.
    """
    previous = {value: signal.getsignal(value) for value in (signal.SIGINT, signal.SIGTERM)}
    pending = []
    process = None
    def defer(value, _frame):
        pending.append(value)
    try:
        for value in previous: signal.signal(value, defer)
        process = subprocess.Popen(command, **kwargs)
        try:
            for value in previous: signal.signal(value, stop_for_signal)
            if pending: stop_for_signal(pending[0], None)
            yield process
        finally:
            for value in previous: signal.signal(value, defer)
    finally:
        # Cleanup is registered before spawning; even cancellation while arming
        # handlers or entering the body must pass through this owner.
        try:
            if process is not None:
                if kwargs.get('start_new_session'):
                    try: os.killpg(process.pid, signal.SIGTERM)
                    except ProcessLookupError: pass
                    try: process.communicate(timeout=3)
                    except subprocess.TimeoutExpired: pass
                    finally:
                        try: os.killpg(process.pid, signal.SIGKILL)
                        except ProcessLookupError: pass
                        process.communicate()
                else:
                    if process.poll() is None: process.kill()
                    process.wait()
        finally:
            try:
                if cleanup is not None: cleanup()
            finally:
                for value, handler in previous.items(): signal.signal(value, handler)
        if pending: raise KeyboardInterrupt(f"received signal {pending[0]}")


def inherited_child(command, *, input, timeout=5):
    """Run a rejection transport inside the enclosing Run's process group."""
    _cargo.reject_optimized_python()
    env = os.environ.copy()
    env.pop('PYTHONOPTIMIZE', None)
    with owned_process(list(map(str, command)), env=env, stdin=subprocess.PIPE,
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE) as process:
        stdout, stderr = process.communicate(input, timeout=timeout)
    return subprocess.CompletedProcess(command, process.returncode, stdout, stderr)


def process_running(pid):
    """A reaped process is stopped, including disappearance during this read."""
    try:
        state = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[0]
    except (FileNotFoundError, ProcessLookupError):
        return False
    return state != 'Z'
