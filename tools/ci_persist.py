"""CI state kept between runs on the persistent self-hosted runner.

The checkout removes every untracked file each run, so this state lives in a
store beside the workspace. It holds three things:

- raw locked archives: each is rechecked against its lock (size and SHA-256)
  whenever a fresh materialization reads it;
- a snapshot of the toolchains a fresh materialization made: a per-push run
  copies it in only when it was made from exactly the current locks at this
  workspace path, and only after every copied file matches the manifest
  recorded when it was made (path, kind, mode bit, size and SHA-256);
- host objects, which carry their own complete keys (compiler, flags, every
  preprocessed input; tools/host_objects.py) and are rechecked on every hit.

The full tier reuses no toolchain or object: it materializes every toolchain
from its locked archives, as every run did before, snapshots the result and
starts the object cache empty.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys

import gate_tier
from native_output import owned_directory

ROOT = Path(__file__).resolve().parents[1]
# Beside the workspace: the checkout cleans the workspace, not its parent.
STORE = ROOT.parent / 'newt-ci-store'
LOCKS = ('docs/toolchain-linux-lock.json', '.github/ci-release-linux-lock.json', '.github/ci-tool-linux-lock.json',
         'rust/toolchain.json', 'tools/rust-toolchain.py')
RAW = ('.toolchain/downloads', '.build/ci-downloads', '.build/ci-rust-downloads')
# What the prepare commands make, minus the raw archives (RAW) they read.
TREES = ('.toolchain', '.release-venv')
OBJECTS = '.build/host-objects'
PREPARE = (('tools/ci_toolchain.py', '--prepare', '--target', 'linux-x86_64'),
           ('tools/ci_environment.py', '--prepare'),
           ('tools/rust-toolchain.py', '--acquire', '--ci-cache'))


def require_rust_approval(root):
    """Prepared source alone does not authorize new CI downloads or installs."""
    approval = os.environ.get('NEWT_RUST_ACQUISITION_APPROVED', '')
    if not approval.startswith('sha256:') or approval != 'sha256:' + sha256(root / 'rust/toolchain.json'):
        raise RuntimeError('Rust CI preparation requires owner approval: '
                           'set repository variable NEWT_RUST_ACQUISITION_APPROVED=sha256:<rust/toolchain.json digest>')


def verify_rust(root):
    """Cache authentication precedes the installed Rust receipt and version check."""
    subprocess.run([sys.executable, '-B', 'tools/rust-toolchain.py', '--verify'], cwd=root, check=True)


def sha256(path):
    digest = hashlib.sha256()
    with open(path, 'rb') as stream:
        while block := stream.read(1 << 20):
            digest.update(block)
    return digest.hexdigest()


def manifest(root):
    """Every entry of the toolchain trees, without following links."""
    entries, files = [], []
    for tree in TREES:
        for directory, names, filenames in os.walk(root / tree):
            base = Path(directory)
            if base == root / RAW[0]:
                names[:], filenames = [], []
            for name in sorted(names + filenames):
                path = base / name
                relative = path.relative_to(root).as_posix()
                if relative == RAW[0]:
                    continue
                mode = path.lstat().st_mode
                if stat.S_ISLNK(mode):
                    entries.append([relative, 'symlink', os.readlink(path)])
                elif stat.S_ISDIR(mode):
                    entries.append([relative, 'directory'])
                elif stat.S_ISREG(mode):
                    entries.append([relative, 'file', bool(mode & stat.S_IXUSR), path.stat().st_size])
                    files.append((entries[-1], path))
                else:
                    raise RuntimeError(f'Unsupported toolchain entry: {relative}')
    with ThreadPoolExecutor(8) as pool:  # hashlib releases the GIL on large blocks
        for (entry, _), digest in zip(files, pool.map(lambda item: sha256(item[1]), files)):
            entry.append(digest)
    return sorted(entries)


def stamp(root):
    """What a snapshot must have been made from to be reused here."""
    return {'locks': {name: sha256(root / name) for name in LOCKS}, 'workspace': str(root)}


def move(source, destination):
    """Replace destination with source; a missing source leaves nothing."""
    if destination.is_dir():
        shutil.rmtree(destination)
    elif destination.exists():
        destination.unlink()
    if source.exists():
        destination.parent.mkdir(parents=True, exist_ok=True)
        source.rename(destination)


def reuse(root, store):
    """Copy the toolchain snapshot in if it fits this run; True when it did."""
    record = store / 'toolchain.json'
    if not record.is_file():
        print('NO toolchain snapshot yet', flush=True)
        return False
    saved = json.loads(record.read_text())
    if saved['stamp'] != stamp(root):
        print('STALE toolchain snapshot: made from other locks or another workspace', flush=True)
        return False
    for tree in TREES:
        if (root / tree).exists():
            raise RuntimeError(f'{tree} exists before the toolchain snapshot was copied in')
        subprocess.run(['/usr/bin/cp', '-a', '--', str(store / 'toolchain' / tree), str(root / tree)], check=True)
    if manifest(root) != saved['manifest']:
        raise RuntimeError('The copied toolchain differs from the manifest recorded when it was made')
    print(f"REUSED toolchain snapshot: {len(saved['manifest'])} entries match their recorded manifest", flush=True)
    return True


def snapshot(root, store):
    """Record the trees a fresh materialization made, replacing any older snapshot."""
    staging = store / 'toolchain.new'
    if staging.exists():
        shutil.rmtree(staging)
    staging.mkdir(parents=True)
    for tree in TREES:
        ignore = shutil.ignore_patterns('downloads') if tree == '.toolchain' else None
        shutil.copytree(root / tree, staging / tree, symlinks=True, ignore=ignore)
    (store / 'toolchain.json').unlink(missing_ok=True)
    move(staging, store / 'toolchain')
    record = {'stamp': stamp(root), 'manifest': manifest(store / 'toolchain')}
    (store / 'toolchain.json').write_text(json.dumps(record) + '\n')


def migrate_legacy_rust_cache(store):
    """Separate old Rust archives; their lock digests are rechecked on use."""
    legacy = store / 'raw/downloads/rust'
    if legacy.exists() or legacy.is_symlink():
        owned_directory(store, legacy)
        current = store / 'raw/ci-rust-downloads'
        if current.exists() or current.is_symlink():
            shutil.rmtree(legacy)
        else:
            legacy.rename(current)


def prepare(root, store, tier):
    """Make the toolchains this run uses; a push tier reuses a fitting snapshot."""
    require_rust_approval(root)
    store.mkdir(parents=True, exist_ok=True)
    if tier == 'push' and reuse(root, store):
        verify_rust(root)
        return
    for tree in TREES:  # A rejected copy never reaches a fresh materialization.
        if (root / tree).exists():
            shutil.rmtree(root / tree)
    migrate_legacy_rust_cache(store)
    for name in RAW:
        move(store / 'raw' / Path(name).name, root / name)
    try:
        for command in PREPARE:
            subprocess.run([sys.executable, '-B', *command], cwd=root, check=True)
    finally:
        for name in RAW:
            move(root / name, store / 'raw' / Path(name).name)
    verify_rust(root)
    snapshot(root, store)
    print('PASS fresh locked toolchains; snapshot recorded for per-push runs', flush=True)


def restore_objects(root, store, tier):
    """The push and local tiers start from the last run's objects; the full tier
    from none. All keep the last phase seconds, which only order the phases' starts."""
    move(store / 'gate-phase-seconds.json', root / gate_tier.SECONDS)
    if tier != 'full':
        move(store / 'host-objects', root / OBJECTS)
    elif (store / 'host-objects').exists():
        shutil.rmtree(store / 'host-objects')


def save_objects(root, store):
    move(root / gate_tier.SECONDS, store / 'gate-phase-seconds.json')
    move(root / OBJECTS, store / 'host-objects')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--expected-commit', required=True)
    args = parser.parse_args()
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        sys.exit('Toolchain reuse is for the CI runner only')
    prepare(ROOT, STORE, gate_tier.ci_tier(ROOT, args.expected_commit))


if __name__ == '__main__':
    main()
