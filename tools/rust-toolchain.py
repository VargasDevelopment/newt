#!/usr/bin/env python3
"""Install only the owner-approved Rust components, under .toolchain/rust.

Default: use verified local archives. --acquire explicitly permits fetching
the four pinned official archives; no Cargo package or system install occurs.
"""
import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT / '.toolchain/rust'
CI_CACHE = '.build/ci-rust-downloads'


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


@contextmanager
def lease(*, shared=False, base=BASE):
    base.mkdir(parents=True, exist_ok=True)
    with (base / 'toolchain.lock').open('a') as lock:
        try:
            fcntl.flock(lock, (fcntl.LOCK_SH if shared else fcntl.LOCK_EX) | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise RuntimeError('Rust toolchain is in use by another check or installer') from error
        try:
            yield
        finally:
            fcntl.flock(lock, fcntl.LOCK_UN)


def installed_identity(prefix):
    files = {}
    for path in sorted(prefix.rglob('*')):
        relative = str(path.relative_to(prefix))
        if path.is_symlink():
            if not path.resolve().is_relative_to(prefix.resolve()):
                raise RuntimeError('Installed Rust symlink escapes its prefix: ' + relative)
            files[relative] = {'link': os.readlink(path)}
        elif path.is_file():
            files[relative] = {'sha256': digest(path), 'bytes': path.stat().st_size,
                               'executable': bool(path.stat().st_mode & 0o111)}
        elif not path.is_dir():
            raise RuntimeError('Unsupported installed toolchain entry: ' + relative)
    return files


def verify_installed(expected, base=BASE):
    receipt = json.loads((base / 'installed.json').read_text())
    if receipt.get('lock') != expected or not receipt.get('files'):
        raise RuntimeError('Installed Rust receipt differs from the approved lock; reproduce installation')
    files = installed_identity(base / 'toolchain')
    if files != receipt['files']:
        raise RuntimeError('Installed Rust files differ from their verified installation receipt')
    rustc = subprocess.run([str(base / 'toolchain/bin/rustc'), '--version', '--verbose'],
                           capture_output=True, text=True, check=True).stdout
    cargo = subprocess.run([str(base / 'toolchain/bin/cargo'), '--version'],
                           capture_output=True, text=True, check=True).stdout
    if f'release: {expected["version"]}\n' not in rustc or not cargo.startswith(f'cargo {expected["version"]} '):
        raise RuntimeError('Installed Rust/Cargo version differs from the approved lock')
    return {'rustc': rustc, 'cargo': cargo,
            'installed_identity_sha256': hashlib.sha256(json.dumps(files, sort_keys=True).encode()).hexdigest()}


def install(acquire=False, *, ci_cache=False):
    lock = json.loads((ROOT / 'rust/toolchain.json').read_text())
    downloads = ROOT / CI_CACHE if ci_cache else BASE / 'downloads'
    downloads.mkdir(parents=True, exist_ok=True)
    for component in lock['components']:
        url = component['url']
        if not url.startswith('https://static.rust-lang.org/dist/2026-02-12/'):
            raise RuntimeError('Rust archive is outside the approved release')
        archive = downloads / url.rsplit('/', 1)[1]
        if not archive.is_file() or digest(archive) != component['sha256']:
            if not acquire:
                raise RuntimeError(f'Missing verified archive {archive.name}; acquisition requires --acquire')
            partial = archive.with_suffix('.partial')
            with urllib.request.urlopen(url, timeout=60) as source, partial.open('wb') as output:
                while chunk := source.read(1024 * 1024):
                    output.write(chunk)
            if digest(partial) != component['sha256']:
                raise RuntimeError(f'Rust archive checksum mismatch: {archive.name}')
            partial.replace(archive)
        if digest(archive) != component['sha256']:
            raise RuntimeError(f'Rust archive checksum mismatch: {archive.name}')
        directory = BASE / 'unpack' / archive.name.removesuffix('.tar.xz')
        # The hash is checked before extraction or execution of install.sh.
        with tarfile.open(archive) as source:
            source.extractall(BASE / 'unpack', filter='data')
        prefix = BASE / 'toolchain'
        subprocess.run(['sh', str(directory / 'install.sh'), f'--prefix={prefix}',
                        '--sysconfdir=' + str(prefix / 'etc'), '--disable-ldconfig'], check=True)
    version = subprocess.run([str(BASE / 'toolchain/bin/rustc'), '--version', '--verbose'],
                             capture_output=True, text=True, check=True).stdout
    if f'release: {lock["version"]}\n' not in version:
        raise RuntimeError('Installed Rust version differs from lock')
    print(version, end='')
    if not (BASE / 'toolchain/lib/rustlib/riscv32imc-unknown-none-elf/lib').is_dir():
        raise RuntimeError('Missing approved RISC-V target')
    receipt = {'lock': lock, 'files': installed_identity(BASE / 'toolchain')}
    (BASE / 'installed.json').write_text(json.dumps(receipt, indent=2) + '\n')
    verify_installed(lock)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--acquire', action='store_true')
    mode.add_argument('--verify', action='store_true', help='Verify the installed receipt without installation')
    parser.add_argument('--ci-cache', action='store_true', help='Use the existing CI raw archive cache')
    args = parser.parse_args()
    with lease(shared=args.verify):
        if args.verify:
            verify_installed(json.loads((ROOT / 'rust/toolchain.json').read_text()))
        else:
            install(args.acquire, ci_cache=args.ci_cache)
