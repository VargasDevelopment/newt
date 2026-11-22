#!/usr/bin/env python3
"""Future offline full packet check. No acquisition, leases, firmware or devices."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
TARGET = 'riscv32imc-unknown-none-elf'


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def sources():
    return {str(p.relative_to(HERE)): digest(p) for p in sorted(HERE.rglob('*'))
            if p.is_file() and 'out' not in p.relative_to(HERE).parts
            and '__pycache__' not in p.parts}


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class Run:
    def __init__(self, output, tools, deadline):
        self.output = output
        self.tools = tools
        self.deadline = deadline
        self.commands = []
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith(('CARGO', 'RUST', 'SCCACHE', 'CCACHE'))
                    and key not in ('LD_PRELOAD', 'LD_LIBRARY_PATH')}
        for name in ('tmp', 'cargo-home', 'cache'):
            (output / name).mkdir()
        self.env.update(CARGO_HOME=str(output / 'cargo-home'), CARGO_NET_OFFLINE='true',
                        CARGO_BUILD_JOBS='2', CARGO_INCREMENTAL='0', RUST_TEST_THREADS='2',
                        RUSTC=str(tools['rustc']), RUSTDOC=str(tools['rustdoc']),
                        RUSTFLAGS='-Ccodegen-units=1', PYTHONDONTWRITEBYTECODE='1',
                        TMPDIR=str(output / 'tmp'), TMP=str(output / 'tmp'),
                        TEMP=str(output / 'tmp'), XDG_CACHE_HOME=str(output / 'cache'))

    def run(self, command, cwd, stdin=None):
        command = list(map(str, command))
        number = len(self.commands)
        log = self.output / f'{number:03d}'
        receipt = {'command': command, 'cwd': str(cwd), 'stdin_sha256':
                   hashlib.sha256((stdin or '').encode()).hexdigest(), 'log': log.name}
        self.commands.append(receipt)
        (log.with_suffix('.command.json')).write_text(json.dumps(receipt, indent=2) + '\n')
        remaining = self.deadline - time.monotonic()
        require(remaining > 0, 'ZIP phase allowance exhausted')
        with log.with_suffix('.stdout').open('w') as stdout, log.with_suffix('.stderr').open('w') as stderr:
            process = subprocess.Popen(command, cwd=cwd, env=self.env, stdin=subprocess.PIPE,
                                       stdout=stdout, stderr=stderr, text=True, start_new_session=True)
            try:
                process.communicate(stdin, timeout=remaining)
            except BaseException as error:
                # A signal must never release the machine lease while this
                # command's separately grouped compiler descendants keep working.
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                finally:
                    process.communicate()
                if isinstance(error, subprocess.TimeoutExpired):
                    raise RuntimeError(f'timeout, never a mutation kill: {log}') from error
                raise
        receipt['returncode'] = process.returncode
        log.with_suffix('.command.json').write_text(json.dumps(receipt, indent=2) + '\n')
        require(process.returncode == 0, f'command failure, never a mutation kill: {log}')
        return read_output(log.with_suffix('.stdout'))

    def cargo(self, workspace, target, *arguments):
        return self.run([self.tools['cargo'], *arguments, '--manifest-path', workspace / 'Cargo.toml',
                         '--offline', '--frozen', '--locked', '--jobs', '2', '--target-dir', target], workspace)


def compiled(output):
    names = set()
    for line in output.splitlines():
        message = json.loads(line)
        if message.get('reason') == 'compiler-artifact':
            name = message['target']['name']
            if name in ('newt_staged_zip_window', 'newt-zip-window-transport'):
                require(message.get('fresh') is False, 'transport/library reused a complete artifact')
                names.add(name)
    require(names == {'newt_staged_zip_window', 'newt-zip-window-transport'},
            'fresh library and real transport compiler artifacts required')


def verify_tools():
    lock = json.loads((HERE / 'toolchains.json').read_text())
    tools = {name: ROOT / entry['path'] for name, entry in lock['executables'].items()}
    for name, entry in lock['executables'].items():
        require(digest(tools[name]) == entry['sha256'], f'approved tool changed: {name}')
    receipt_path = ROOT / '.toolchain/rust/installed.json'
    require(digest(receipt_path) == lock['rust_receipt_sha256'], 'Rust installation receipt changed')
    receipt = json.loads(receipt_path.read_text())
    require(receipt['lock'] == lock['rust_lock'], 'Rust approval lock changed')
    prefix = ROOT / '.toolchain/rust/toolchain'
    for relative, entry in receipt['files'].items():
        path = prefix / relative
        require(path.resolve().is_relative_to(prefix.resolve()), 'toolchain entry escapes prefix')
        if 'link' in entry:
            require(path.is_symlink() and os.readlink(path) == entry['link'], 'Rust symlink changed')
        else:
            require(path.is_file() and path.stat().st_size == entry['bytes']
                    and digest(path) == entry['sha256'], f'Rust component changed: {relative}')
    return tools


def read_output(path):
    # Universal-newline text I/O would hide malformed transport bytes.
    return path.read_bytes().decode('utf-8')


def stop_for_signal(signum, _frame):
    raise KeyboardInterrupt(f'received signal {signum}')


def install_signal_handlers():
    for signum in (signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, stop_for_signal)
