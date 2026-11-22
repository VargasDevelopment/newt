"""Reusable host objects from frozen, fully preprocessed translation units.

Every lookup reruns preprocessing (including system headers). Only compilation is
reused; tests and linking are never cached. Fixture builds share these objects
under the same complete key; standalone fresh witnesses never use them.
"""
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import threading

import host_jobs
from native_output import owned_directory, owned_file, write_text

COMPILE_FLAGS = {'-std=c99', '-std=c++17', '-O1', '-O2', '-g', '-Wall', '-Wextra',
                 '-Werror', '-pedantic', '-fsanitize=address,undefined', '-fno-omit-frame-pointer', '-pthread'}
# Scratch placement belongs in the child environment, never its content key.
SCRATCH_ENVIRONMENT = {'TMPDIR', host_jobs.NATIVE_ROOTS}


def require_flags(flags, preprocess_flags):
    # Profile/PCH/plugin/response-file flags can read inputs outside the header
    # closure. New compiler roles require an explicit extension of this boundary.
    if any(flag not in COMPILE_FLAGS for flag in flags):
        raise RuntimeError('Unsupported host object compile flag')
    arguments = iter(preprocess_flags)
    for flag in arguments:
        if flag == '-I':
            path = next(arguments, '')
            if not path or path.startswith(('-', '@')):
                raise RuntimeError('Missing host object include path')
        elif not flag.startswith('-D') or len(flag) == 2:
            raise RuntimeError('Unsupported host object preprocessor flag')


def digest(path, memo=None):
    """SHA-256 of a file. A memo dict, owned by one compilation call, hashes
    each resolved file once within that call; it never outlives the call."""
    if memo is not None:
        key = str(Path(path).resolve(strict=True))
        if key not in memo:
            memo[key] = digest(key)
        return memo[key]
    value = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def procedure():
    """This object procedure's code: this file and every project file it
    imports (tests/procedure_closure_test.py), by name."""
    here = Path(__file__).resolve()
    return {name: digest(here.with_name(name)) for name in ('host_objects.py', 'host_jobs.py', 'native_output.py')}


def reuse_enabled() -> bool:
    """False under NEWT_HOST_OBJECT_CACHE=0, as tools/check.py --fresh-objects sets."""
    return os.environ.get('NEWT_HOST_OBJECT_CACHE', '1') != '0'


def compiler_environment(root):
    temporary = owned_directory(root, root / '.build/host-objects/tmp', create=True)
    # CPATH, SDKROOT, DEVELOPER_DIR and compiler/package overrides are not inputs.
    # Use one explicit environment for discovery, preprocessing and compilation.
    # TMPDIR is the gate scratch contract, also used by compiled native fixtures.
    # UBSan normally recovers with exit zero; host probes must reject every error.
    environment = {'PATH': '/usr/bin:/bin:/usr/sbin:/sbin', 'LC_ALL': 'C',
                   'TMPDIR': os.environ.get('TMPDIR', str(temporary)), 'UBSAN_OPTIONS': 'halt_on_error=1'}
    if mounts := os.environ.get(host_jobs.NATIVE_ROOTS):
        environment[host_jobs.NATIVE_ROOTS] = mounts
    return environment


def compiler_identity(compiler, environment, root, memo=None):
    # Launcher and executable usually resolve to one file; a caller's memo
    # hashes it once per check while recording both roles exactly as before.
    memo = {} if memo is None else memo
    version = subprocess.run([compiler, '--version'], check=True, cwd=root,
                             env=environment, capture_output=True, text=True).stdout
    installed = next((line.removeprefix('InstalledDir: ') for line in version.splitlines()
                      if line.startswith('InstalledDir: ')), None)
    executable = Path(installed) / Path(compiler).name if installed else Path(compiler)
    return {'command': str(compiler), 'launcher': digest(Path(compiler).resolve(strict=True), memo),
            'executable_path': str(executable.resolve(strict=True)),
            'executable': digest(executable.resolve(strict=True), memo), 'version': version}


def dependency_paths(path, root):
    """Clang Make dependencies include system headers and escaped path names."""
    rule = path.read_text().replace('\\\n', '').replace('\\\r\n', '')
    tokens = re.findall(r'(?:\\.|[^\s\\:])+|:', rule)
    if tokens.count(':') != 1 or tokens.index(':') != 1 or len(tokens) < 3:
        raise RuntimeError('Missing complete host compiler dependency rule')
    return sorted({root / re.sub(r'\\(.)', r'\1', token).replace('$$', '$') for token in tokens[2:]})


def dependencies(path, root, memo=None):
    value = hashlib.sha256()
    for dependency in dependency_paths(path, root):
        actual = dependency.resolve(strict=True)
        if not actual.is_file():
            raise RuntimeError('Host compiler dependency must be a regular file')
        value.update(str(dependency).encode() + b'\0' + str(actual).encode() + b'\0')
        value.update(bytes.fromhex(digest(actual, memo)))
    return value.hexdigest()


def scratch_paths(root, temporary, native=None):
    """Lexical, canonical and repo-relative gate paths that must not be inputs.

    A caller may put the checkout inside TMPDIR (for example a checkout below
    the system temporary directory). Its source paths are legitimate inputs;
    that ancestor is not a private per-run scratch identity. Native roots remain
    forbidden independently of that exception.
    """
    root = Path(root).resolve()
    paths = []
    if temporary and not root.is_relative_to(Path(temporary).resolve()):
        paths.append(Path(temporary))
    if native:
        paths.append(Path(native))
    values = set()
    for path in paths:
        for candidate in (path.absolute(), path.resolve()):
            values.add(str(candidate))
            if candidate.is_relative_to(root):
                values.add(candidate.relative_to(root).as_posix())
    return tuple(sorted(values))


def check_scratch_inputs(needles, preprocessed, paths, arguments):
    """Reject scratch before hashing, cache lookup or compilation, not receipts.

    Inspect the actual expanded unit and lexical/resolved dependency paths, so
    stable headers cannot hide a changing root behind the receipt's hashes.
    Check arguments too: even an unused -D or -I must not vary by gate run.
    """
    if not needles:
        return
    for value in needles:
        if value in preprocessed:
            raise RuntimeError('Per-run scratch path in preprocessed host input: ' + os.fsdecode(value))
    for path in paths:
        for candidate in (path, path.resolve()):
            if any(value in os.fsencode(candidate) for value in needles):
                raise RuntimeError('Per-run scratch path in host dependency: ' + str(path))
    for argument in arguments:
        if any(value in os.fsencode(argument) for value in needles):
            raise RuntimeError('Per-run scratch path in host compile argument: ' + str(argument))


class HostObjects:
    """One compiler's reusable objects. Lookups may run concurrently.

    A compilation call that owns several of these may pass the identity it
    just measured and one dependency memo, so the compiler and each header
    are hashed once per call; that call then checks the identity again."""
    def __init__(self, root, compiler, *, identity=None, memo=None):
        self.root = Path(root)
        self.directory = owned_directory(self.root, self.root / '.build/host-objects', create=True)
        self.environment = compiler_environment(self.root)
        self._scratch_context = (None, ())
        self.compiler = str(compiler)
        self.identity = identity or compiler_identity(self.compiler, self.environment, self.root)
        self.memo = memo
        self.hits = self.compiled = 0
        self.counts = threading.Lock()
        self.enabled = reuse_enabled()

    def count(self, field):
        with self.counts:
            setattr(self, field, getattr(self, field) + 1)

    def object(self, source, flags, preprocess_flags):
        require_flags(flags, preprocess_flags)
        source = Path(source).absolute()
        # Select the active gate at lookup time, including owners created before
        # it. Cache only this small path selection, never an input-check result.
        selection = (os.environ.get('TMPDIR'), os.environ.get(host_jobs.NATIVE_ROOTS))
        context = self._scratch_context
        if context[0] != selection:
            needles = tuple(os.fsencode(value) for value in scratch_paths(
                self.root, *selection)) if selection[1] else ()
            context = (selection, needles)
            self._scratch_context = context
        scratch = context[1]
        with tempfile.TemporaryDirectory(prefix='.unit-', dir=self.directory) as name:
            workspace = Path(name)
            suffix = '.i' if source.suffix == '.c' else '.ii'
            frozen = workspace / (source.stem + suffix)
            closure = workspace / 'inputs.d'
            with host_jobs.slot():
                result = subprocess.run([self.compiler, '--no-default-config', *flags, *preprocess_flags,
                                         '-E', '-MD', '-MF', str(closure), '-MT', 'unit.o', str(source)],
                                        check=True, cwd=self.root, env=self.environment,
                                        stdout=subprocess.PIPE)
            if scratch:
                check_scratch_inputs(scratch, result.stdout,
                                     dependency_paths(closure, self.root), [str(source), *preprocess_flags])
            frozen.write_bytes(result.stdout)
            identity = {'schema': 1, 'compiler': self.identity, 'source': str(source),
                        'cwd': str(self.root), 'environment': {name: value for name, value in self.environment.items()
                                                             if name not in SCRATCH_ENVIRONMENT},
                        'flags': flags, 'preprocess_flags': preprocess_flags,
                        'procedure': procedure(),
                        'dependencies': dependencies(closure, self.root, self.memo),
                        'preprocessed': hashlib.sha256(result.stdout).hexdigest()}
            key = hashlib.sha256(json.dumps(identity, sort_keys=True).encode()).hexdigest()
            temporary = workspace / 'unit.o'
            if not self.enabled:
                # Nothing to reuse: lookups of one unit compile side by side and
                # claim it only to publish.
                self.compile(flags, frozen, temporary)
            with self.claim(key):
                output = owned_file(self.root, self.directory / (key + '.o'))
                receipt = owned_file(self.root, self.directory / (key + '.json'))
                if self.enabled and output.is_file() and receipt.is_file():
                    try:
                        record = json.loads(receipt.read_text())
                    except (ValueError, UnicodeError):
                        record = None
                    if record == {'identity': identity, 'object_sha256': digest(output)}:
                        self.count('hits')
                        return output
                if self.enabled:
                    self.compile(flags, frozen, temporary)
                owned_file(self.root, output)
                temporary.replace(output)
                write_text(self.root, receipt, json.dumps({'identity': identity,
                           'object_sha256': digest(output)}, sort_keys=True) + '\n')
                self.count('compiled')
                return output

    def compile(self, flags, frozen, temporary):
        # Compile exactly the frozen preprocessor result used by the key.
        # No live header or source can change between lookup and compilation.
        with host_jobs.slot():
            subprocess.run([self.compiler, '--no-default-config', *flags, '-c', str(frozen),
                            '-o', str(temporary)], check=True, cwd=self.root, env=self.environment)

    @contextmanager
    def claim(self, key):
        """One lookup per key, in any process, checks and publishes at a time,
        with or without reuse: an ownership check that met another lookup's
        replacement could see the replaced file unlinked. While reuse is enabled,
        a lookup also compiles under its claim, so concurrent lookups of one unit
        reuse its object. A waiter holds no compiler slot."""
        lock = owned_file(self.root, self.directory / (key + '.lock'))
        descriptor = os.open(lock, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX)
            yield
        finally:
            os.close(descriptor)

    def finish(self):
        if compiler_identity(self.compiler, self.environment, self.root) != self.identity:
            raise RuntimeError('Host compiler changed during object compilation')
