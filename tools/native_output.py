"""Canonical destinations for directly authored native-build files."""
import os
from pathlib import Path
import sys
import tempfile


def owned_directory(root: Path, directory: Path, *, create: bool = False) -> Path:
    root, directory = Path(root), Path(directory)
    if root.is_symlink() or not root.is_dir() or root.resolve(strict=True) != root:
        raise RuntimeError("Native output requires a real canonical project root")
    if not directory.is_relative_to(root) or ".." in directory.relative_to(root).parts:
        raise RuntimeError("Native output directory must remain inside its project root")
    current = root
    for part in directory.relative_to(root).parts:
        current /= part
        if current.is_symlink():
            raise RuntimeError("Native output directory must not be a symlink or alias")
        if not current.exists() and create:
            try:
                current.mkdir()
            except FileExistsError:
                pass  # created concurrently: checked below like any existing entry
        if not current.is_dir() or current.resolve(strict=True) != current:
            raise RuntimeError("Native output requires real project-local directories")
    return directory


def owned_file(root: Path, output: Path, *, create_parents: bool = False) -> Path:
    output = Path(output)
    owned_directory(root, output.parent, create=create_parents)
    if output.is_symlink() or (output.exists() and
                              (not output.is_file() or output.stat().st_nlink != 1)):
        raise RuntimeError("Native output must be a regular unaliased project-local file")
    return output


def owned_output_tree(root: Path, directory: Path) -> None:
    """Compiler output descendants must be real directories or unaliased files."""
    pending = [owned_directory(root, directory, create=True)]
    while pending:
        for entry in pending.pop().iterdir():
            if entry.is_dir() and not entry.is_symlink():
                pending.append(owned_directory(root, entry))
            else:
                owned_file(root, entry)


def write_text(root: Path, output: Path, text: str) -> None:
    write_bytes(root, output, text.encode("utf-8"))


def write_bytes(root: Path, output: Path, data: bytes) -> None:
    """Stage with exclusive creation; replace only a revalidated destination."""
    # Reports are checked before they are written, often with assert: never
    # publish one from an interpreter that removed those checks.
    if sys.flags.optimize:
        raise RuntimeError("Native output is evidence: write it without -O or PYTHONOPTIMIZE")
    output = owned_file(root, output, create_parents=True)
    descriptor, name = tempfile.mkstemp(prefix="." + output.name + ".", suffix=".tmp", dir=output.parent)
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(data)
        owned_file(root, output)
        os.replace(temporary, output)
    finally:
        temporary.unlink(missing_ok=True)
