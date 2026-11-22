"""CI-only fresh platform-specific tools; raw downloads are never execution proof."""
import argparse
import json
import os
from pathlib import Path, PurePosixPath
import platform
import shutil
import subprocess
import sys
import venv

from native_output import owned_directory, owned_file
from ci_toolchain import acquire, extract, validate_cache, require_runner
from host_objects import compiler_environment, compiler_identity

ROOT = Path(__file__).resolve().parents[1]
LLVM_VERSION = '22.1.8'
LLVM_HEADERS = PurePosixPath('lib/clang') / LLVM_VERSION.split('.')[0] / 'include'


def selected_target():
    return 'linux-x86_64' if platform.system() == 'Linux' else 'macos-arm64'


def settings(root, target=None):
    target = target or selected_target()
    if target not in ('macos-arm64', 'linux-x86_64'):
        raise RuntimeError('Unknown CI platform target')
    suffix = '-linux' if target == 'linux-x86_64' else ''
    tools = json.loads((root / f'.github/ci-tool{suffix}-lock.json').read_text())
    release = json.loads((root / f'.github/ci-release{suffix}-lock.json').read_text())
    pins = [line for line in (root / 'requirements-release.txt').read_text().splitlines()
            if line and not line.startswith('#')]
    if [record['requirement'] for record in release['artifacts']] != pins:
        raise RuntimeError('CI wheel lock differs from reviewed release requirements')
    if tools['python'] != '3.12.6' or release['python'] != tools['python']:
        raise RuntimeError('CI runtime differs from approved exact Python')
    return tools, release


def require_host(tools):
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise RuntimeError('Acquisition/materialization is hosted CI only; local --plan acquires nothing')
    if tools.get('target') == 'linux-x86_64':
        require_runner('linux-x86_64')
        if platform.python_version() != tools['python']:
            raise RuntimeError('CI requires exact Python 3.12.6')
        os_release = Path('/etc/os-release').read_text()
        if 'ID=ubuntu\n' not in os_release:
            raise RuntimeError('Linux CI requires the reviewed Ubuntu runner family')
        for executable in ('/usr/bin/make', '/usr/bin/perl'):
            if not Path(executable).is_file():
                raise RuntimeError('Missing explicit OpenSSL build prerequisite: ' + executable)
        return
    if (platform.system(), platform.machine(), platform.python_version()) != ('Darwin', 'arm64', tools['python']):
        raise RuntimeError('CI requires macOS ARM64 and exact Python 3.12.6')
    if os.environ.get('DEVELOPER_DIR') != tools['xcode'] or not Path(tools['xcode']).is_dir():
        raise RuntimeError('Selected pinned Xcode is unavailable; no SDK fallback')
    result = subprocess.run(['/usr/bin/xcodebuild', '-version'], check=True, capture_output=True, text=True)
    if result.stdout.strip() != f"Xcode {tools['xcode_version']}\nBuild version {tools['xcode_build']}":
        raise RuntimeError('Selected Xcode version/build differs from CI pin')
    if os.environ.get('SDKROOT'):
        raise RuntimeError('Ambient SDKROOT overrides the selected CI SDK')
    # The canonical host environment strips DEVELOPER_DIR. Inspect that actual
    # selection rather than changing its boundary or the runner's global state.
    environment = compiler_environment(ROOT)
    pinned = Path(tools['xcode']).resolve(strict=True)
    for compiler in ('/usr/bin/clang', '/usr/bin/clang++'):
        identity = compiler_identity(compiler, environment, ROOT)
        if not Path(identity['executable_path']).is_relative_to(pinned):
            raise RuntimeError('Canonical host compiler differs from pinned Xcode; no global-selection fallback')


def llvm_member(path, kind):
    # LibTooling needs its builtin headers relative to the installed executable.
    # C++ development headers/static archives and other tools are unnecessary.
    return (path.as_posix() == 'bin/clang-tidy'
            or path.is_relative_to(LLVM_HEADERS)
            or (path.parts[0] == 'lib' and ('.dylib' in path.name or '.so' in path.name)))


def linux_llvm_member(path, kind):
    # Host ASan/UBSan links require the pinned compiler-rt archives and shared
    # runtimes as well as resource headers. Never use a system sanitizer fallback.
    return (llvm_member(path, kind) or path.is_relative_to(LLVM_HEADERS.parent)
            or path.as_posix() in ('bin/clang', 'bin/clang++', 'bin/clang-22'))


def require_llvm_resources(root, destination):
    include = owned_directory(root, destination.joinpath(*LLVM_HEADERS.parts))
    for name in ('stddef.h', 'stdint.h', 'stdarg.h'):
        header = owned_file(root, include / name)
        if not header.is_file() or not header.stat().st_size:
            raise RuntimeError('Pinned LLVM builtin resource header is missing: ' + name)


def installation_command(python, wheels):
    return [str(python), '-I', '-B', '-m', 'pip', '--isolated', 'install',
            '--no-index', '--no-deps', '--no-compile', *map(str, wheels)]


def prepare_openssl(root, record, cache, llvm):
    source = root / '.toolchain/sources/openssl'
    destination = root / '.toolchain/openssl'
    if source.exists() or source.is_symlink() or destination.exists() or destination.is_symlink():
        raise RuntimeError('OpenSSL source/provider must be freshly materialized')
    extract(root, acquire(root, cache, record), source)
    owned_directory(root, destination, create=True)
    # Reviewed OpenSSL 3.5.9 C-only exception: safestack.h.in and sparse_array.h
    # erase typed object-pointer callbacks; digestcommon.h dispatches SHA256_Update
    # through void*, and nullprov.c uses OSSL_PROVIDER* for the void* query API.
    # These are upstream function-type UB on otherwise matching x86_64 calling
    # conventions, not false positives. Keep ASan and every other UBSan check;
    # Newt's separately compiled C++ retains the complete undefined group.
    # Source: https://github.com/openssl/openssl/tree/openssl-3.5.9
    environment = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C', 'HOME': os.environ.get('HOME', ''),
                   'CC': str(llvm / 'bin/clang') + ' --no-default-config',
                   'CFLAGS': '-O1 -g -fsanitize=address,undefined -fno-sanitize=function '
                             '-fno-sanitize-recover=all -fno-omit-frame-pointer',
                   'LDFLAGS': '-fsanitize=address,undefined', 'OPENSSL_CONF': '/dev/null',
                   'UBSAN_OPTIONS': 'halt_on_error=1',
                   'TMPDIR': str(owned_directory(root, root / '.toolchain/tmp', create=True))}
    configure = ['/usr/bin/perl', str(source / 'Configure'), 'linux-x86_64',
                 'no-asm', 'no-shared', 'no-module', 'no-dso', 'no-engine', 'no-legacy',
                 'no-tests', 'no-apps', 'no-docs', '--prefix=' + str(destination), '--libdir=lib']
    subprocess.run(configure, cwd=source, env=environment, check=True)
    subprocess.run(['/usr/bin/make', '-j2', 'build_libs'], cwd=source, env=environment, check=True)
    include = owned_directory(root, source / 'include')
    shutil.copytree(include, destination / 'include', symlinks=False)
    library = owned_file(root, source / 'libcrypto.a')
    output = owned_file(root, destination / 'lib/libcrypto.a', create_parents=True)
    shutil.copyfile(library, output)
    if not output.stat().st_size:
        raise RuntimeError('Locked OpenSSL static provider is empty')
    # A source archive/version label cannot grant runtime correctness.
    subprocess.run([sys.executable, '-B', str(root / 'tests/host_sha256_test.py')],
                   cwd=root, env={**environment, 'PYTHONDONTWRITEBYTECODE': '1'}, check=True)


def prepare(root):
    tools, release = settings(root)
    require_host(tools)
    cache = owned_directory(root, root / '.build/ci-downloads', create=True)
    records = [*release['artifacts'], tools['llvm']]
    linux = tools.get('target') == 'linux-x86_64'
    if linux:
        records.append(tools['openssl'])
    validate_cache(root, cache, records)
    wheel_paths = [acquire(root, cache, record) for record in release['artifacts']]
    release_directory = root / '.release-venv'
    if release_directory.exists() or release_directory.is_symlink():
        raise RuntimeError('CI release environment must be freshly created')
    owned_directory(root, release_directory, create=True)
    # Managed Linux Python resolves its shared library beside its real executable.
    venv.EnvBuilder(with_pip=True, symlinks=linux).create(release_directory)
    python = release_directory / 'bin/python'
    subprocess.run(installation_command(python, wheel_paths), cwd=root, check=True)
    subprocess.run([str(python), '-I', '-B', '-m', 'pip', '--isolated', 'check'], cwd=root, check=True)
    archive = acquire(root, cache, tools['llvm'])
    destination = root / '.toolchain/llvm'
    if destination.exists() or destination.is_symlink():
        raise RuntimeError('CI LLVM environment must be freshly materialized')
    extract(root, archive, destination, select=linux_llvm_member if linux else llvm_member)
    require_llvm_resources(root, destination)
    # The extracted package must be the locked LLVM release.
    reported = subprocess.run([str(destination / 'bin/clang-tidy'), '--version'], check=True,
                              capture_output=True, text=True).stdout
    if f'LLVM version {LLVM_VERSION}' not in reported:
        raise RuntimeError(f'Unreviewed LLVM version: {reported.strip()}')
    if linux:
        environment = compiler_environment(root)
        for name in ('clang', 'clang++'):
            compiler = destination / 'bin' / name
            owned_file(root, compiler.resolve(strict=True))
            identity = compiler_identity(compiler, environment, root)
            if not Path(identity['executable_path']).is_relative_to(destination):
                raise RuntimeError('Linux host compiler escaped the locked LLVM package')
            if f'clang version {LLVM_VERSION}' not in identity['version']:
                raise RuntimeError('Linux host compiler differs from exact LLVM pin')
        prepare_openssl(root, tools['openssl'], cache, destination)
    print('PASS fresh pinned release tools and LLVM; no stored passing proof', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument('--plan', action='store_true', help='Read-only exact input inventory; no downloads')
    action.add_argument('--prepare', action='store_true', help='Hosted-only scoped materialization')
    action.add_argument('--check-host', action='store_true', help='Hosted runtime/source preflight before acquisition')
    args = parser.parse_args()
    if args.plan:
        tools, release = settings(ROOT)
        print(json.dumps({'tools': tools, 'release': release, 'fresh': 'every full-tier run; per-push runs reuse the checked snapshot of tools/ci_persist.py'}, indent=2))
    elif args.check_host:
        tools, _ = settings(ROOT)
        require_host(tools)
        from ci_gate import committed_source
        committed_source(ROOT, os.environ['EXPECTED_COMMIT'])
        print('PASS exact hosted runtime and committed source before acquisition', flush=True)
    else:
        prepare(ROOT)


if __name__ == '__main__':
    main()
