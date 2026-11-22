"""Read-only identities for procedure and installed-driver Python runtimes."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import sysconfig


def runtime_description(executable=None):
    if executable is not None:
        # Match the installed PlatformIO launcher's isolated interpreter mode.
        probe = ("import json,sys,sysconfig; print(json.dumps(dict("
                 "executable=sys.executable,base_prefix=sys.base_prefix,"
                 "stdlib=sysconfig.get_path('stdlib'),"
                 "platstdlib=sysconfig.get_path('platstdlib',vars={'platbase':sys.base_exec_prefix}),"
                 "version=sys.version,cache_tag=sys.implementation.cache_tag,optimize=sys.flags.optimize)))")
        result = subprocess.run([str(executable), "-I", "-B", "-c", probe],
                                check=True, capture_output=True, text=True)
        return json.loads(result.stdout)
    return {"executable": sys.executable, "base_prefix": sys.base_prefix,
            "stdlib": sysconfig.get_path("stdlib"),
            "platstdlib": sysconfig.get_path("platstdlib", vars={"platbase": sys.base_exec_prefix}),
            "version": sys.version, "cache_tag": sys.implementation.cache_tag, "optimize": sys.flags.optimize}


def file_hash(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.digest()


def runtime_hash(root, inventory=None):
    """Bind names, bytes and aliases, including existing executable bytecode.

    The resolved base installation contains the shared library, stdlib and native
    extensions reused by a venv. Site packages are separate installed-driver
    inputs; global/user packages and ambient user data are not runtime inputs.
    """
    digest = hashlib.sha256()
    for parent, directories, files in os.walk(root, followlinks=False):
        directories[:] = sorted(name for name in directories if name != "site-packages")
        for name in sorted([*directories, *files]):
            path = Path(parent) / name
            relative = path.relative_to(root).as_posix()
            digest.update(relative.encode() + b"\0")
            entry = bytearray()
            if path.is_symlink():
                target = path.resolve(strict=True)
                if not target.is_relative_to(root):
                    raise ValueError("Python runtime alias escapes its base installation")
                if "site-packages" in target.relative_to(root).parts:
                    raise ValueError("Python runtime alias enters excluded site-packages")
                entry.extend(b"link\0" + target.relative_to(root).as_posix().encode() + b"\0")
            if path.is_file():
                entry.extend(b"file\0" + file_hash(path))
            elif path.is_dir():
                entry.extend(b"directory\0")
            else:
                raise ValueError("Python runtime entry must be a regular file or directory")
            digest.update(entry)
            if inventory is not None:
                inventory[relative] = hashlib.sha256(entry).hexdigest()
    return digest.hexdigest()


def home_relative(path):
    """Spell a path under $HOME as ~/..., so a receipt names no personal home.

    Content stays bound by the hashes; one user's receipts spell one location
    one way, so a moved or replaced interpreter still differs."""
    home = Path.home().resolve()
    return "~/" + path.relative_to(home).as_posix() if path.is_relative_to(home) else str(path)


def runtime_identity(executable=None, *, inventory=None):
    description = runtime_description(executable)
    keys = {"executable", "base_prefix", "stdlib", "platstdlib", "version", "cache_tag", "optimize"}
    if (not isinstance(description, dict) or set(description) != keys
            or any(not isinstance(description[key], str) or not description[key] for key in keys - {"optimize"})
            or type(description["optimize"]) is not int or description["optimize"] not in (0, 1, 2)):
        raise ValueError("Invalid Python runtime discovery schema")
    resolved = Path(description["executable"]).resolve(strict=True)
    if executable is not None and resolved != Path(executable).resolve(strict=True):
        raise ValueError("Python runtime discovery changed the requested interpreter")
    root = Path(description["base_prefix"]).resolve(strict=True)
    if not root.is_dir() or not resolved.is_file():
        raise ValueError("Missing Python runtime installation or executable")
    libraries = {}
    for name in ("stdlib", "platstdlib"):
        library = Path(description[name]).resolve(strict=True)
        if not library.is_dir() or not library.is_relative_to(root):
            raise ValueError("Python standard library escapes its base installation")
        if "site-packages" in library.relative_to(root).parts:
            raise ValueError("Python standard library enters excluded site-packages")
        libraries[name] = home_relative(library)
    return {"resolved_path": home_relative(resolved), "sha256": file_hash(resolved).hex(),
            "runtime_root": home_relative(root), "runtime_sha256": runtime_hash(root, inventory),
            **libraries, "version": description["version"], "cache_tag": description["cache_tag"],
            "optimize": description["optimize"]}
