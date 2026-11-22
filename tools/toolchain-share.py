#!/usr/bin/env python3
"""Share one pinned, read-only toolchain among checkouts; never deletes a copy.

`store` copies this checkout's private .toolchain/<component> into
STORE/<component>-<digest>, named by the SHA-256 of its manifest (every path,
kind, executable bit or link target, size and content hash), and makes every
stored file and directory read-only. `link` replaces this checkout's
.toolchain/<component> with a view of a verified store entry: real
project-local directories whose files are hardlinks to the stored files, so
every tool, receipt and identity check reads the same paths and bytes as from
a private copy. The pinned platform rewrites one build script in place and
keeps private state beside its packages; a view holds private writable copies
of exactly those, and every other directory is read-only. Read-only modes stop
the platform's ordinary writes, not an owner who changes them, so a health
build checks every recorded view (check_views) before capturing its identity:
each shared file must still be the stored, unmodified inode with the exact
mode its recorded executable bit gives it. The previous copy (or symlink)
moves aside to .toolchain/<component>.before-share-<time>; nothing is deleted.
`verify` also rehashes every stored byte. `migrate` stores and links both
components; a linked component is only verified, by `store` and `migrate`
alike. Store and checkouts must share one filesystem.
"""
import argparse
from fnmatch import fnmatch
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import stat
import sys
import tempfile
import time

sys.dont_write_bytecode = True

from native_output import owned_directory, write_text
import native_toolchain

ROOT = Path(__file__).resolve().parents[1]
COMPONENTS = ("platformio", "llvm")
RECORD = ".toolchain/shared-toolchain.json"
# Platform state written beside the packages: never stored, private per view.
PRIVATE = {"platformio": (".cache", "appstate.json", "penv/.lock")}
# Stored inputs the platform rewrites in place during a build (with identical
# bytes): each view holds a private writable copy.
REWRITTEN = {"platformio": ("packages/framework-arduinoespressif32-libs/*/pioarduino-build.py",)}
# The transient backup a running build keeps beside each rewritten script.
TRANSIENT = {"platformio": (native_toolchain.BACKUP,)}
# Backups the platform makes on its first build, in directories a view keeps
# read-only: a tree without them was never built and would fail in a view.
REQUIRED = {"platformio": ("packages/framework-arduinoespressif32/idf_component.yml.orig",)}
CHUNK = 1 << 20


def matches(path, patterns):
    parts = PurePosixPath(path).parts
    return any(len(parts) == len(PurePosixPath(pattern).parts) and
               all(fnmatch(part, glob) for part, glob in zip(parts, PurePosixPath(pattern).parts))
               for pattern in patterns)


def private(component, path):
    return any(path == name or path.startswith(name + "/") for name in PRIVATE.get(component, ()))


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        while block := stream.read(CHUNK):
            digest.update(block)
    return digest.hexdigest()


def unreadable(error):
    raise error


def walk(top, skip=lambda path: False):
    """Every entry beneath top, as sorted relative POSIX paths, without following
    links; a skipped entry's contents are never read."""
    entries = []
    for directory, names, files in os.walk(top, onerror=unreadable):
        base = Path(directory).relative_to(top)
        names[:] = [name for name in names if not skip((base / name).as_posix())]
        entries += [path for path in ((base / name).as_posix() for name in names + files) if not skip(path)]
    return sorted(entries)


def manifest(component, source):
    """The store manifest of a component tree; private state is excluded."""
    entries = []
    for path in walk(source, lambda path: private(component, path)):
        if matches(path, TRANSIENT.get(component, ())):
            raise RuntimeError(f"A build is running or left its backup: {path}")
        full = source / path
        mode = full.lstat().st_mode
        if stat.S_ISLNK(mode):
            entries.append([path, "symlink", os.readlink(full)])
        elif stat.S_ISDIR(mode):
            entries.append([path, "directory"])
        elif stat.S_ISREG(mode):
            entries.append([path, "file", bool(mode & stat.S_IXUSR), full.stat().st_size, sha256(full)])
        else:
            raise RuntimeError(f"Unsupported toolchain entry: {full}")
    missing = sorted(set(REQUIRED.get(component, ())) - {entry[0] for entry in entries})
    if missing:
        raise RuntimeError(f"This {component} was never built; build once before storing it: {missing}")
    return {"format": 1, "component": component, "entries": entries}


def digest_of(record):
    return hashlib.sha256(json.dumps(record, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def entry_path(store, component, digest):
    return Path(store) / f"{component}-{digest}"


def stored_mode(executable):
    """The exact mode of a stored directory (executable) or file."""
    return 0o555 if executable else 0o444


def read_only(path, executable=False):
    os.chmod(path, stored_mode(executable), follow_symlinks=False)


def leaving(record):
    """Stored links whose targets lie outside the component: the link is stored, not its target."""
    return [path for path, kind, *details in record["entries"] if kind == "symlink" and
            os.path.normpath(PurePosixPath(path).parent / details[0]).split("/")[0] in ("", "..")]


def pinned_manifest(store, component, digest):
    """A store entry's manifest, which its name pins."""
    entry = entry_path(store, component, digest)
    record = json.loads((entry / "manifest.json").read_text())
    if record.get("component") != component or digest_of(record) != digest:
        raise RuntimeError(f"Store manifest differs from its pinned digest: {entry}")
    return record


def verify_store(store, component, digest):
    """Return the manifest of a store entry whose every byte and mode still match."""
    entry = entry_path(store, component, digest)
    record = pinned_manifest(store, component, digest)
    tree = entry / "tree"
    listed = {item[0]: item for item in record["entries"]}
    if walk(tree) != sorted(listed):
        raise RuntimeError(f"Store entries differ from the manifest: {entry}")
    for path, kind, *details in record["entries"]:
        full = tree / path
        mode = full.lstat().st_mode
        if mode & 0o222 and not stat.S_ISLNK(mode):
            raise RuntimeError(f"Stored toolchain entry is writable: {full}")
        if kind == "symlink":
            if not stat.S_ISLNK(mode) or os.readlink(full) != details[0]:
                raise RuntimeError(f"Stored link differs: {full}")
        elif kind == "directory":
            if not stat.S_ISDIR(mode) or stat.S_IMODE(mode) != stored_mode(True):
                raise RuntimeError(f"Stored directory differs: {full}")
        elif (not stat.S_ISREG(mode) or stat.S_IMODE(mode) != stored_mode(details[0])
              or full.stat().st_size != details[1] or sha256(full) != details[2]):
            raise RuntimeError(f"Stored file differs from the manifest: {full}")
    return record


def store(store_root, component, source):
    """Copy a component into the store, or reuse an identical verified entry."""
    store_root = Path(store_root).resolve(strict=True)
    with native_toolchain.quiescent(ROOT):
        record = manifest(component, source)
        digest = digest_of(record)
        entry = entry_path(store_root, component, digest)
        if entry.exists():
            verify_store(store_root, component, digest)
            return digest
        staging = Path(tempfile.mkdtemp(prefix=f".{component}-", dir=store_root))
        tree = staging / "tree"
        tree.mkdir()
        for path, kind, *details in record["entries"]:
            if kind == "directory":
                (tree / path).mkdir()
            elif kind == "symlink":
                os.symlink(details[0], tree / path)
            else:
                shutil.copyfile(source / path, tree / path, follow_symlinks=False)
        # Recheck the copy against the manifest taken from the source.
        for path, kind, *details in record["entries"]:
            if kind == "file" and sha256(tree / path) != details[2]:
                raise RuntimeError(f"Toolchain changed while it was stored; inspect {staging}")
    (staging / "manifest.json").write_text(json.dumps(record, indent=1) + "\n")
    for path, kind, *details in reversed(record["entries"]):
        if kind != "symlink":
            read_only(tree / path, kind == "directory" or details[0])
    for directory in (tree, staging / "manifest.json", staging):
        read_only(directory, directory.is_dir())
    try:
        os.rename(staging, entry)
    except OSError:
        if not entry.is_dir():
            raise
        verify_store(store_root, component, digest)
        print(f"Another checkout stored {entry.name} first; this copy stays at {staging}", file=sys.stderr)
    return digest


def writable_directories(component, record):
    """The view directories the platform writes into: the component root and
    each parent of private state or a rewritten script."""
    names = {""}
    for name in PRIVATE.get(component, ()):
        parent = PurePosixPath(name).parent.as_posix()
        names.add("" if parent == "." else parent)
    for path, kind, *_ in record["entries"]:
        if kind == "file" and matches(path, REWRITTEN.get(component, ())):
            names.add(PurePosixPath(path).parent.as_posix())
    return names


def build_view(component, record, tree, view, previous):
    """Create a view of a verified store tree; copy private state from previous."""
    rewritten = REWRITTEN.get(component, ())
    for path, kind, *details in record["entries"]:
        if kind == "directory":
            (view / path).mkdir()
        elif kind == "symlink":
            os.symlink(details[0], view / path)
        elif matches(path, rewritten):
            shutil.copyfile(tree / path, view / path)
            os.chmod(view / path, 0o755 if details[0] else 0o644)
        else:
            os.link(tree / path, view / path)
    if previous is not None and previous.is_dir() and not previous.is_symlink():
        for name in PRIVATE.get(component, ()):
            source = previous / name
            if source.is_dir() and not source.is_symlink():
                shutil.copytree(source, view / name, symlinks=True)
            elif source.is_file() and not source.is_symlink():
                shutil.copy2(source, view / name)
    writable = writable_directories(component, record)
    for path, kind, *_ in reversed(record["entries"]):
        if kind == "directory" and path not in writable and not private(component, path):
            read_only(view / path, True)


def read_record(root):
    path = Path(root) / RECORD
    return json.loads(path.read_text()) if path.is_file() else {}


def same_filesystem(first, second):
    return os.stat(first).st_dev == os.stat(second).st_dev


def link(store_root, component, digest):
    """Point this checkout's component at a verified store entry."""
    store_root = Path(store_root).resolve(strict=True)
    toolchain = owned_directory(ROOT, ROOT / ".toolchain", create=True)
    if not same_filesystem(store_root, toolchain):
        raise RuntimeError("The toolchain store must be on this checkout's filesystem (views are hardlinks)")
    record = verify_store(store_root, component, digest)
    tree = entry_path(store_root, component, digest) / "tree"
    current = toolchain / component
    view = Path(tempfile.mkdtemp(prefix=f".{component}-view-", dir=toolchain))
    view.chmod(0o755)
    try:
        build_view(component, record, tree, view, current if current.exists() else None)
    except BaseException:
        print(f"Incomplete view left at {view}; nothing else changed", file=sys.stderr)
        raise
    with native_toolchain.quiescent(ROOT):
        moved = None
        if current.is_symlink() or current.exists():
            stamp = f"{component}.before-share-{time.strftime('%Y%m%d-%H%M%S')}"
            moved, serial = toolchain / stamp, 1
            while os.path.lexists(moved):
                serial += 1
                moved = toolchain / f"{stamp}-{serial}"
            os.rename(current, moved)
        os.rename(view, current)
        shared = dict(read_record(ROOT), **{component: {"store": str(store_root), "digest": digest}})
        write_text(ROOT, ROOT / RECORD, json.dumps(shared, indent=2, sort_keys=True) + "\n")
    return moved, record


def check_view(root, component, store_root, digest, record):
    """Check a checkout's view against a store entry's manifest, entry by
    entry, while the caller holds the toolchain lock. Only the private
    rewritten copies are hashed, and keep their recorded executable bit:
    every other file must be the stored inode with its stored mode,
    unmodified since its manifest sealed the entry, and every other
    directory read-only."""
    tree = entry_path(store_root, component, digest) / "tree"
    view = Path(root) / ".toolchain" / component
    if view.is_symlink() or not view.is_dir():
        raise RuntimeError(f"Toolchain view must be a real directory: {view}")
    # The manifest is written after every stored copy. Neither chmod nor link
    # changes a file's mtime, so an in-place write shows even with its mode restored.
    sealed = (tree.parent / "manifest.json").stat().st_mtime_ns
    listed = {item[0] for item in record["entries"]}
    extra = [path for path in walk(view, lambda path: private(component, path)) if path not in listed]
    if extra:
        raise RuntimeError(f"Unexpected toolchain view entries: {extra[:5]}")
    writable = writable_directories(component, record)
    for path, kind, *details in record["entries"]:
        entry, stored = view / path, tree / path
        if not os.path.lexists(entry):
            raise RuntimeError(f"Missing toolchain view entry: {entry}")
        mode = entry.lstat().st_mode
        if kind == "symlink":
            same = stat.S_ISLNK(mode) and os.readlink(entry) == details[0]
        elif kind == "directory":
            same = stat.S_ISDIR(mode) and (path in writable or not mode & 0o222)
        elif matches(path, REWRITTEN.get(component, ())):
            same = (stat.S_ISREG(mode) and entry.stat().st_nlink == 1
                    and mode & 0o111 == stored_mode(details[0]) & 0o111 and sha256(entry) == details[2])
        else:
            actual, expected = entry.lstat(), stored.lstat()
            same = ((actual.st_dev, actual.st_ino) == (expected.st_dev, expected.st_ino)
                    and stat.S_IMODE(mode) == stored_mode(details[0]) and actual.st_mtime_ns <= sealed)
        if not same:
            raise RuntimeError(f"Toolchain view entry differs from the store: {entry}")


def verify_view(root, component, store_root, digest, record=None):
    """Check a checkout's view against its store entry, verified unless its
    manifest was just verified."""
    record = record or verify_store(store_root, component, digest)
    with native_toolchain.quiescent(root):
        check_view(root, component, store_root, digest, record)


def check_views(root):
    """Every recorded view still matches its pinned manifest: cheap enough for
    each health build, which calls it holding the toolchain lock. `verify`
    also rehashes the stored bytes."""
    for component, pin in sorted(read_record(root).items()):
        record = pinned_manifest(pin["store"], component, pin["digest"])
        check_view(root, component, pin["store"], pin["digest"], record)


def component_source(component):
    """This checkout's current copy; a symlinked component is read through its
    link, without the owning checkout's lock: once that owner shares its
    toolchain, link this checkout to the same digest instead."""
    source = ROOT / ".toolchain" / component
    return Path(os.path.realpath(source)) if source.is_symlink() else source


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("store", "link", "migrate"):
        command = commands.add_parser(name)
        command.add_argument("--store", type=Path, required=True, help="the shared store directory")
        if name != "migrate":
            command.add_argument("--component", choices=COMPONENTS, required=True)
        if name == "link":
            command.add_argument("--digest", required=True, help="the store entry digest that `store` printed")
    commands.add_parser("verify")
    args = parser.parse_args(argv)
    if args.command == "verify":
        shared = read_record(ROOT)
        if not shared:
            raise SystemExit(f"No shared toolchain recorded in {RECORD}")
        for component, pin in sorted(shared.items()):
            verify_view(ROOT, component, pin["store"], pin["digest"])
            print(f"PASS: .toolchain/{component} is a verified view of {pin['store']} ({pin['digest'][:12]})")
        return 0
    for component in (COMPONENTS if args.command == "migrate" else (args.component,)):
        pin = read_record(ROOT).get(component)
        if pin is not None and (args.command != "link" or pin["digest"] == args.digest):
            # A view holds its store entry's bytes: verify it, never store it anew.
            if args.command != "link" and Path(pin["store"]).resolve() != args.store.resolve():
                raise SystemExit(f".toolchain/{component} is already a view of {pin['store']}")
            verify_view(ROOT, component, pin["store"], pin["digest"])
            print(f"Already linked {component}: verified {pin['digest'][:12]} in {pin['store']}")
            continue
        if args.command == "link":
            digest = args.digest
        else:
            digest = store(args.store, component, component_source(component))
            print(f"Stored {component}: {digest}")
            outside = leaving(pinned_manifest(args.store.resolve(), component, digest))
            if outside:
                print(f"  Kept verbatim, leaving {component}: {', '.join(outside[:5])}"
                      + (f" and {len(outside) - 5} more" if len(outside) > 5 else ""))
        if args.command in ("link", "migrate"):
            moved, record = link(args.store, component, digest)
            verify_view(ROOT, component, args.store.resolve(), digest, record)
            print(f"Linked .toolchain/{component} to {digest[:12]}"
                  + (f"; previous copy kept at {moved.relative_to(ROOT)} (remove it yourself)" if moved else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
