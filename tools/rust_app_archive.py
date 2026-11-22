"""The pinned offline archive build shared by daily and bench entry points."""
import os
from pathlib import Path
import subprocess
import sys

TARGET = 'riscv32imc-unknown-none-elf'


def verify_toolchain(root: Path) -> Path:
    subprocess.run([sys.executable, '-B', str(root / 'tools/rust-toolchain.py'), '--verify'],
                   cwd=root, check=True)
    return root / '.toolchain/rust/toolchain/bin'


def build_environment(root: Path, toolchain: Path, metadata: dict[str, str]) -> dict[str, str]:
    environment = os.environ.copy()
    for name in tuple(environment):
        if name.startswith(('CARGO_', 'RUST', 'CCC_')) or name in (
                'PYTHONOPTIMIZE', 'LD_PRELOAD', 'LD_LIBRARY_PATH'):
            environment.pop(name)
    environment.update(metadata)
    environment.update(CARGO_HOME=str(root / '.toolchain/rust/cargo-home'),
                       CARGO_NET_OFFLINE='true', CARGO_BUILD_JOBS='2',
                       CARGO_INCREMENTAL='0', RUSTC=str(toolchain / 'rustc'),
                       RUSTDOC=str(toolchain / 'rustdoc'),
                       RUSTFLAGS=('-Dwarnings -Ctarget-feature=+zicsr,+zifencei '
                                  '-Cllvm-args=--force-dwarf-frame-section '
                                  f'--remap-path-prefix={root}=.'))
    return environment


def build_archive(root: Path, toolchain: Path, environment: dict[str, str], *,
                  clean: bool, package: str, features: str) -> None:
    target = root / '.build/rust-app/target'
    manifest = root / 'rust/Cargo.toml'
    if clean:
        subprocess.run([str(toolchain / 'cargo'), 'clean', '--offline', '--release',
                        '--manifest-path', str(manifest), '--target', TARGET,
                        '--target-dir', str(target)], cwd=root, env=environment, check=True)
    subprocess.run([str(toolchain / 'cargo'), 'build', '--offline', '--locked', '--release',
                    '--manifest-path', str(manifest), '--target', TARGET,
                    '--target-dir', str(target), '-p', package, '--lib', '--jobs', '2',
                    *(['--features', features] if features else [])],
                   cwd=root, env=environment, check=True)
