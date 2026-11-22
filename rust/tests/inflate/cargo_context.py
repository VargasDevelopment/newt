"""Closed Cargo configuration and authenticated host linking for offline checks."""
from contextlib import contextmanager
import hashlib
import json
from pathlib import Path
import shlex
import sys

HOST = 'x86_64-unknown-linux-gnu'
TARGET = 'riscv32imc-unknown-none-elf'


def reject_optimized_python():
    if sys.flags.optimize:
        raise RuntimeError('Validation requires Python assertions; optimization is unsupported')


def verify_no_cargo_configuration(cwd, env):
    """Cargo reads ancestor configs even with an explicit manifest and Cargo home."""
    if not env.get('CARGO_HOME'):
        raise RuntimeError('Cargo requires an explicit project-local CARGO_HOME')
    cwd = Path(cwd).absolute()
    directories = [parent / '.cargo' for location in (cwd, cwd.resolve())
                   for parent in (location, *location.parents)]
    directories.append(Path(env['CARGO_HOME']))
    for directory in dict.fromkeys(directories):
        if directory.is_symlink():
            raise RuntimeError('Cargo configuration directory is symlinked: ' + str(directory))
        for name in ('config', 'config.toml'):
            path = directory / name
            if path.exists() or path.is_symlink():
                raise RuntimeError('Unsealed Cargo configuration is unsupported: ' + str(path))


def pinned_host_link_tools(root, lock_path=None):
    lock_path = lock_path or Path(root) / 'rust/tests/inflate/toolchains.json'
    lock = json.loads(Path(lock_path).read_text())
    return {name: {'path': Path(root) / lock['executables'][name]['path'],
                   'sha256': lock['executables'][name]['sha256']}
            for name in ('clang++', 'native-ld')}


def verify_host_link_tools(tools):
    for name, entry in tools.items():
        with entry['path'].open('rb') as stream:
            actual = hashlib.file_digest(stream, 'sha256').hexdigest()
        if actual != entry['sha256']:
            raise RuntimeError('Authenticated Cargo host linker changed: ' + name)


@contextmanager
def cargo_context(arguments, cwd, env, tools):
    reject_optimized_python()
    verify_no_cargo_configuration(cwd, env)
    verify_host_link_tools(tools)
    clean = env.copy()
    clean.pop('PYTHONOPTIMIZE', None)
    for name in list(clean):
        if name.startswith('CCC_'):
            clean.pop(name)
    clean['RUSTC_WRAPPER'] = ''
    clean['RUSTC_WORKSPACE_WRAPPER'] = ''
    clang = str(tools['clang++']['path'])
    clean['CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER'] = clang
    target = None
    for index, value in enumerate(arguments):
        value = str(value)
        if value == '--target':
            if index + 1 == len(arguments):
                raise RuntimeError('Cargo target value absent')
            target = str(arguments[index + 1])
        elif value.startswith('--target='):
            target = value.split('=', 1)[1]
    if target not in (None, HOST, TARGET):
        raise RuntimeError('Unapproved Cargo check target: ' + target)
    if target in (None, HOST):
        # Append after a family's sanitizer flags; both driver and native ld are
        # explicit. These host-only options must not reach the bare RV32 linker.
        flags = ['-Clinker=' + clang, '-Clink-arg=--no-default-config',
                 '-Clink-arg=-fuse-ld=' + str(tools['native-ld']['path'])]
        clean['RUSTFLAGS'] = clean.get('RUSTFLAGS', '') + ' ' + shlex.join(flags)
    try:
        yield clean
    finally:
        verify_no_cargo_configuration(cwd, env)
        verify_host_link_tools(tools)
