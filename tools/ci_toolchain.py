#!/usr/bin/env python3
"""Materialize a platform-specific locked compile-only toolchain on a fresh CI runner."""
import argparse
import base64
import csv
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path, PurePosixPath
import platform
import posixpath
import shutil
import stat
import subprocess
import sys
import tarfile
import urllib.parse
import urllib.request
import zipfile

sys.dont_write_bytecode = True
from native_output import owned_directory, owned_file, write_text

ROOT = Path(__file__).resolve().parents[1]
PYTHON_VERSION = (3, 12, 6)
ARCHIVES = {
    'framework-arduinoespressif32': ('esp32-core-3.3.7.tar.xz', None),
    'framework-arduinoespressif32-libs': ('esp32-core-3.3.7-libs.tar.xz', None),
    'toolchain-riscv32-esp': ('riscv32-esp-elf-14.2.0_20251107-aarch64-apple-darwin.tar.xz',
                            'riscv32-esp-elf-14.2.0_20251107.zip'),
    'tool-esptoolpy': ('esptool-5.1.2.zip', 'esptoolpy-v5.1.2.zip'),
    'tool-scons': ('scons-local-4.8.1.tar.gz', 'scons-4.8.1.zip'),
    'tool-esp_install': ('esp_install-v5.3.4.zip', None),
    'contrib-piohome': ('contrib-piohome-3.4.4.tar.gz', None),
}
PLATFORM_ARCHIVE = 'platform-espressif32-55.03.37.zip'
CORE_ARCHIVE = 'pioarduino-core-6.1.19.zip'
TARGETS = {'macos-arm64': ('Darwin', 'arm64', 'toolchain-lock.json'),
           'linux-x86_64': ('Linux', 'x86_64', 'toolchain-linux-lock.json')}
DOWNLOAD_HOSTS = {'github.com', 'files.pythonhosted.org', 'release-assets.githubusercontent.com',
                  'objects.githubusercontent.com', 'github-releases.githubusercontent.com', 'codeload.github.com'}


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise RuntimeError('Duplicate lock key: ' + key)
        result[key] = value
    return result


def safe_name(name):
    if (not isinstance(name, str) or not name or '\\' in name or '\x00' in name
            or name.startswith('/') or any(part == '..' for part in name.split('/'))):
        raise RuntimeError('Unsafe archive or lock path')
    path = PurePosixPath(name)
    if not path.parts or ':' in path.parts[0]:
        raise RuntimeError('Unsafe archive or lock path')
    return path


def allowed_url(url, *, redirect=False):
    parsed = urllib.parse.urlsplit(url)
    hosts = DOWNLOAD_HOSTS if redirect else {'github.com', 'files.pythonhosted.org'}
    if (parsed.scheme != 'https' or parsed.hostname not in hosts or parsed.username
            or parsed.password or parsed.port or (not redirect and (parsed.query or parsed.fragment))):
        raise RuntimeError('Toolchain URL is outside the locked public publishers')


def archives(target):
    if target not in TARGETS:
        raise RuntimeError('Unknown native toolchain target')
    result = dict(ARCHIVES)
    if target == 'linux-x86_64':
        result['toolchain-riscv32-esp'] = (
            'riscv32-esp-elf-14.2.0_20251107-x86_64-linux-gnu.tar.xz', ARCHIVES['toolchain-riscv32-esp'][1])
    return result


def read_lock(root, target='macos-arm64'):
    recipe = archives(target)
    path = owned_file(root, root / 'docs' / TARGETS[target][2])
    lock = json.loads(path.read_text(), object_pairs_hook=unique_object)
    artifacts = lock['artifacts']
    names = set()
    for item in artifacts:
        validate_record(item)
        name = item['filename']
        if name in names:
            raise RuntimeError('Duplicate or nonlocal locked filename')
        names.add(name)
    required = {PLATFORM_ARCHIVE, CORE_ARCHIVE}
    required.update(name for pair in recipe.values() for name in pair if name)
    wheels = [item for item in artifacts if item['filename'].endswith('.whl')]
    if target == 'linux-x86_64':
        requirements = lock['wheel_requirements']
        common_requirements = {line.split(' --hash=sha256:')[0] for line in owned_file(
            root, root / 'docs/toolchain-python.lock').read_text().splitlines() if line}
        hashed = dict(line.split(' --hash=sha256:') for line in owned_file(
            root, root / 'docs/toolchain-python-linux.lock').read_text().splitlines() if line)
        actual = {item['requirement']: item['sha256'] for item in wheels}
        if (lock.get('target') != target or lock.get('python') != '3.12.6'
                or len(set(requirements)) != len(requirements) or set(actual) != set(requirements)
                or not common_requirements <= set(requirements)
                or len(actual) != len(wheels) or actual != hashed):
            raise RuntimeError('Linux wheel requirements or platform hash constraints differ from lock')
        expected_wheels = len(requirements)
        expected_artifacts = len(required) + expected_wheels
    else:
        expected_artifacts, expected_wheels = 63, 51
    if (len(artifacts) != expected_artifacts or len(wheels) != expected_wheels or not required <= names
            or sum(item['size'] for item in artifacts) != lock['download_bytes']):
        raise RuntimeError('Locked toolchain inventory differs from the reviewed platform inputs')
    return lock


def validate_record(item):
    if len(safe_name(item['filename']).parts) != 1:
        raise RuntimeError('Locked archive filename must be a single local name')
    allowed_url(item['url'])
    if (type(item['size']) is not int or item['size'] <= 0
            or not isinstance(item['sha256'], str) or len(item['sha256']) != 64
            or any(c not in '0123456789abcdef' for c in item['sha256'])):
        raise RuntimeError('Incomplete size or SHA-256 pin')


def plan(root, target='macos-arm64'):
    lock = read_lock(root, target)
    return {'python': '.'.join(map(str, PYTHON_VERSION)), 'host': ' '.join(TARGETS[target][:2]),
            'target': target, 'artifact_count': len(lock['artifacts']),
            'wheel_count': sum(item['filename'].endswith('.whl') for item in lock['artifacts']),
            'download_bytes': lock['download_bytes'], 'artifacts': lock['artifacts'],
            'packages': list(ARCHIVES), 'platform': '55.03.37',
            'core': 'pioarduino-core 6.1.19; stock platformio metadata retained',
            'cache': 'Raw archives only; every restored file rechecked by size and SHA-256',
            'fresh_gates_required': True, 'proof_reuse_available': False}


def verify_archive(root, path, item):
    path = owned_file(root, path)
    if not path.is_file() or path.stat().st_size != item['size']:
        raise RuntimeError('Locked archive size mismatch: ' + item['filename'])
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    if digest.hexdigest() != item['sha256']:
        raise RuntimeError('Locked archive SHA-256 mismatch: ' + item['filename'])


def validate_cache(root, cache, records):
    """Offline search paths contain only named, verified raw inputs."""
    cache = owned_directory(root, cache, create=True)
    inventory = {}
    for item in records:
        validate_record(item)
        if item['filename'] in inventory:
            raise RuntimeError('Duplicate cache inventory filename')
        inventory[item['filename']] = item
    for path in cache.iterdir():
        if path.name not in inventory:
            raise RuntimeError('Unexpected restored raw cache input: ' + path.name)
        verify_archive(root, path, inventory[path.name])


def acquire(root, cache, item):
    validate_record(item)
    destination = owned_file(root, cache / item['filename'], create_parents=True)
    if destination.exists():
        verify_archive(root, destination, item)
        return destination
    allowed_url(item['url'])
    temporary = owned_file(root, cache / ('.' + item['filename'] + '.partial'))
    if temporary.exists():
        raise RuntimeError('Incomplete archive exists; use a fresh runner')
    # No ambient proxy credentials or package-manager configuration.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        with opener.open(item['url'], timeout=60) as response, temporary.open('xb') as output:
            allowed_url(response.geturl(), redirect=True)
            total = 0
            while block := response.read(1024 * 1024):
                total += len(block)
                if total > item['size']:
                    raise RuntimeError('Download exceeds its locked size')
                output.write(block)
        verify_archive(root, temporary, item)
        owned_file(root, destination)
        temporary.replace(destination)
    finally:
        owned_file(root, temporary).unlink(missing_ok=True)
    return destination


fetch = acquire


def archive_members(archive):
    if zipfile.is_zipfile(archive):
        handle = zipfile.ZipFile(archive)
        members = []
        for item in handle.infolist():
            mode = item.external_attr >> 16
            if stat.S_ISLNK(mode) or (stat.S_IFMT(mode) not in (0, stat.S_IFREG, stat.S_IFDIR)):
                handle.close()
                raise RuntimeError('ZIP special entries are outside the reviewed package shape')
            members.append((item.filename, 'directory' if item.is_dir() else 'file',
                            mode, item.file_size, None, item))
        return handle, members, handle.open
    handle = tarfile.open(archive, 'r:*')
    members = []
    for item in handle.getmembers():
        kind = ('directory' if item.isdir() else 'file' if item.isfile() else
                'symlink' if item.issym() else 'hardlink' if item.islnk() else None)
        if kind is None:
            handle.close()
            raise RuntimeError('Tar special entries are outside the reviewed package shape')
        members.append((item.name, kind, item.mode, item.size, item.linkname, item))
    return handle, members, handle.extractfile


def extract(root, archive, destination, *, c3_only=False, select=None):
    """Flatten one enclosing archive directory; never write through archive links."""
    archive = owned_file(root, archive)
    destination = owned_directory(root, destination, create=True)
    if any(destination.iterdir()):
        raise RuntimeError('Archive destination must be fresh and empty')
    handle, members, reader = archive_members(archive)
    try:
        paths = [(safe_name(name), *rest) for name, *rest in members if name.rstrip('/.')]
        heads = {path.parts[0] for path, *_ in paths}
        strip = len(heads) == 1 and all(len(path.parts) > 1 or kind == 'directory'
                                      for path, kind, *_ in paths)
        pending, seen = [], set()
        for path, kind, mode, size, link, item in paths:
            parts = path.parts[1:] if strip else path.parts
            if not parts:
                continue
            if c3_only and not (parts[0] == 'esp32c3' or (len(parts) == 1 and kind == 'file')):
                continue
            relative = PurePosixPath(*parts)
            if select is not None and not select(relative, kind):
                continue
            if relative in seen and kind != 'directory':
                raise RuntimeError('Duplicate archive destination')
            seen.add(relative)
            output = destination.joinpath(*parts)
            if kind == 'directory':
                owned_directory(root, output, create=True)
            elif kind == 'file':
                output = owned_file(root, output, create_parents=True)
                with reader(item) as source, output.open('xb') as stream:
                    shutil.copyfileobj(source, stream)
                if output.stat().st_size != size:
                    raise RuntimeError('Extracted archive size mismatch')
                output.chmod(0o755 if mode & 0o111 else 0o644)
            else:
                # Validate link destinations lexically before any links exist.
                if not link or '\\' in link or link.startswith('/') or '\x00' in link:
                    raise RuntimeError('Archive link escapes its package')
                if kind == 'symlink':
                    target = posixpath.normpath(str(relative.parent / link))
                else:
                    target_path = safe_name(link)
                    target_parts = target_path.parts[1:] if strip else target_path.parts
                    target = str(PurePosixPath(*target_parts))
                target_path = safe_name(target)
                owned_file(root, output, create_parents=True)
                pending.append((output, kind, link, destination.joinpath(*target_path.parts)))
        # Copy hardlink data to preserve unaliased writable outputs.
        for output, kind, link, target in pending:
            if kind == 'hardlink':
                owned_file(root, target)
                with target.open('rb') as source, output.open('xb') as stream:
                    shutil.copyfileobj(source, stream)
                output.chmod(target.stat().st_mode & 0o777)
        for output, kind, link, target in pending:
            if kind == 'symlink':
                output.symlink_to(link)
        for output, kind, link, target in pending:
            if kind == 'symlink' and not output.resolve(strict=True).is_relative_to(destination):
                raise RuntimeError('Archive link resolves outside its package')
    finally:
        handle.close()


def offline_environment(root, cache):
    env = {key: value for key, value in os.environ.items()
           if key in ('HOME', 'PATH', 'LANG', 'LC_ALL', 'DEVELOPER_DIR')}
    env.update(PIP_CONFIG_FILE='/dev/null', PIP_NO_INDEX='1', UV_NO_INDEX='1', UV_OFFLINE='1',
               PIP_DISABLE_PIP_VERSION_CHECK='1', PYTHONDONTWRITEBYTECODE='1',
               PIP_FIND_LINKS=str(cache), UV_FIND_LINKS=str(cache),
               PIP_CACHE_DIR=str(root / '.toolchain/cache/pip'),
               UV_CACHE_DIR=str(root / '.toolchain/cache/uv'),
               PLATFORMIO_CORE_DIR=str(root / '.toolchain/platformio'),
               PLATFORMIO_CACHE_DIR=str(root / '.toolchain/cache/platformio'),
               XDG_CACHE_HOME=str(root / '.toolchain/cache/xdg'),
               TMPDIR=str(root / '.toolchain/tmp'))
    return env


def installation_commands(python, wheels, core, package, wheel_dir):
    pip = [str(python), '-I', '-B', '-m', 'pip']
    return [
        [*pip, 'install', '--no-index', '--no-deps', '--no-compile', *map(str, wheels)],
        [*pip, 'wheel', '--no-index', '--no-deps', '--no-build-isolation',
         '--wheel-dir', str(wheel_dir), str(core)],
        [*pip, 'install', '--no-index', '--no-deps', '--no-compile',
         str(wheel_dir / 'pioarduino_core-6.1.19-py3-none-any.whl')],
        [*pip, 'install', '--no-index', '--no-deps', '--no-build-isolation', '--no-compile',
         '--editable', str(package)],
        [*pip, 'check'],
    ]


REGISTER = """import json,sys
from pathlib import Path
from platformio.package.manager.tool import ToolPackageManager
from platformio.package.manager.platform import PlatformPackageManager
from platformio.package.meta import PackageItem,PackageSpec
for path,url,is_platform in json.loads(sys.argv[1]):
    manager=(PlatformPackageManager if is_platform else ToolPackageManager)(str(Path(path).parent))
    PackageItem(path,manager.build_metadata(path,PackageSpec(url))).dump_meta()
"""


def verify_core(penv, wheel):
    site = penv / 'lib/python3.12/site-packages'
    with zipfile.ZipFile(wheel) as archive:
        records = next(name for name in archive.namelist() if name.endswith('.dist-info/RECORD'))
        for name, expected, size in csv.reader(io.StringIO(archive.read(records).decode())):
            if name.startswith('platformio/') and expected:
                algorithm, encoded = expected.split('=', 1)
                if algorithm != 'sha256':
                    raise RuntimeError('Unexpected fork module digest')
                actual = base64.urlsafe_b64encode(hashlib.sha256((site / name).read_bytes()).digest())
                if actual.decode().rstrip('=') != encoded:
                    raise RuntimeError('Loaded Core module differs from the verified fork build')
    for name in ('platformio', 'pioarduino_core'):
        if not (site / (name + '-6.1.19.dist-info')).is_dir():
            raise RuntimeError('Missing stock or fork Core metadata')


def require_runner(target='macos-arm64'):
    archives(target)
    if (os.environ.get('GITHUB_ACTIONS') != 'true'
            or (platform.system(), platform.machine()) != TARGETS[target][:2]
            or sys.version_info[:3] != PYTHON_VERSION):
        raise RuntimeError('Preparation requires GITHUB_ACTIONS=true, the selected host and Python 3.12.6')
    if target == 'linux-x86_64':
        library, version = platform.libc_ver()
        if library != 'glibc' or tuple(map(int, version.split('.')[:2])) < (2, 35):
            raise RuntimeError('Linux wheels require glibc >= 2.35; use Ubuntu 22.04 or newer')


def prepare(root, target='macos-arm64'):
    require_runner(target)  # Before any filesystem mutation, acquisition or package execution.
    lock = read_lock(root, target)
    core_dir = root / '.toolchain/platformio'
    if core_dir.exists() or core_dir.is_symlink():
        raise RuntimeError('Installed trees cannot be restored; prepare requires a fresh runner')
    cache = owned_directory(root, root / '.toolchain/downloads', create=True)
    validate_cache(root, cache, lock['artifacts'])
    for suffix in ('cache/pip', 'cache/uv', 'cache/platformio', 'cache/xdg', 'tmp', 'wheels', 'sources'):
        directory = owned_directory(root, root / '.toolchain' / suffix, create=True)
        if any(directory.iterdir()):
            raise RuntimeError('Only raw downloads may be restored; all prepared trees must be fresh')
    inputs = {item['filename']: acquire(root, cache, item) for item in lock['artifacts']}
    packages = owned_directory(root, core_dir / 'packages', create=True)
    registrations = []
    for name, (source, descriptor) in archives(target).items():
        destination = packages / name
        extract(root, inputs[source], destination, c3_only=name.endswith('espressif32-libs'))
        if descriptor:
            metadata_dir = root / '.toolchain/sources' / (name + '-descriptor')
            extract(root, inputs[descriptor], metadata_dir)
            manifest = owned_file(root, metadata_dir / 'package.json')
            write_text(root, destination / 'package.json', manifest.read_text())
            # Completed upstream descriptor installations omit tools.json.
            owned_file(root, destination / 'tools.json').unlink(missing_ok=True)
        registration_source = source if name == 'tool-scons' else (descriptor or source)
        registrations.append((str(destination), next(item['url'] for item in lock['artifacts']
                                                     if item['filename'] == registration_source), False))
    target_platform = core_dir / 'platforms/espressif32'
    extract(root, inputs[PLATFORM_ARCHIVE], target_platform)
    registrations.append((str(target_platform), next(item['url'] for item in lock['artifacts']
                                                    if item['filename'] == PLATFORM_ARCHIVE), True))
    core_source = root / '.toolchain/sources/pioarduino-core'
    extract(root, inputs[CORE_ARCHIVE], core_source)
    penv = core_dir / 'penv'
    environment = offline_environment(root, cache)
    subprocess.run([sys.executable, '-I', '-B', '-m', 'venv', str(penv)], env=environment, check=True)
    python = penv / 'bin/python'
    commands = installation_commands(python, [inputs[item['filename']] for item in lock['artifacts']
                                              if item['filename'].endswith('.whl')],
                                     core_source, packages / 'tool-esptoolpy', root / '.toolchain/wheels')
    for command in commands:
        subprocess.run(command, cwd=root, env=environment, check=True)
    verify_core(penv, root / '.toolchain/wheels/pioarduino_core-6.1.19-py3-none-any.whl')
    probe = ("import platformio,esptool,sys; from pathlib import Path; "
             "assert platformio.__title__ == 'pioarduino core'; "
             "assert platformio.__version__ == '6.1.19'; "
             "assert Path(esptool.__file__).resolve().is_relative_to(Path(sys.argv[1]).resolve())")
    subprocess.run([str(python), '-I', '-B', '-c', probe, str(packages / 'tool-esptoolpy')],
                   cwd=root, env=environment, check=True)
    for path, _, _ in registrations:
        owned_file(root, Path(path) / '.piopm')
    subprocess.run([str(python), '-I', '-B', '-c', REGISTER, json.dumps(registrations)],
                   cwd=root, env=environment, check=True)
    spec = importlib.util.spec_from_file_location('ci_firmware_build', root / 'tools/firmware-build.py')
    builder = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(builder)
    builder.require_toolchain()
    print('PASS: locked local toolchain prepared; fresh canonical gates remain required')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', choices=TARGETS, default='macos-arm64',
                        help='Explicit platform input lock; macOS remains the compatibility default')
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument('--plan', action='store_true', help='read-only exact input inventory as JSON')
    group.add_argument('--prepare', action='store_true', help='fresh CI runner with exact selected host only')
    args = parser.parse_args()
    if args.plan:
        print(json.dumps(plan(ROOT, args.target), indent=2, sort_keys=True))
    else:
        prepare(ROOT, args.target)


if __name__ == '__main__':
    main()
