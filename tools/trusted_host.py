"""The pinned FreeInk source and the host compilers' exact identity."""
import hashlib
from pathlib import Path
import subprocess

from host_common import ROOT, C_COMPILER, COMPILER
from freeink_source_pin import SDK_ORIGIN, SDK_PIN, verify_source_pin as verify_freeink_source_pin

SDK = ROOT / 'vendor/freeink-sdk'


def verify_source_pin(source: Path = SDK) -> str:
    return verify_freeink_source_pin(source)


def file_digest(path: Path) -> str:
    if path.is_symlink() or not path.is_file():
        raise RuntimeError('Identity input is not a regular file')
    digestor = hashlib.sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b''):
            digestor.update(chunk)
    return digestor.hexdigest()


def compiler_identity() -> dict:
    result = {}
    for compiler in (C_COMPILER, COMPILER):
        version = subprocess.run([compiler, '--version'], check=True,
                                 capture_output=True, text=True).stdout
        # Apple's compiler launcher is a system symlink; bind its resolved bytes.
        installed = next((line.removeprefix('InstalledDir: ') for line in version.splitlines()
                          if line.startswith('InstalledDir: ')), None)
        executable = Path(installed) / Path(compiler).name if installed else Path(compiler)
        result[compiler] = {'launcher_sha256': file_digest(Path(compiler).resolve()),
                            'executable_sha256': file_digest(executable.resolve()),
                            'version': version.strip()}
    return result
