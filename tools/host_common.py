"""Local host compilation and exact frame export; Python standard library only."""
from pathlib import Path
import fcntl
import shutil
import os
import struct
import subprocess
import sys
import tempfile
import zlib
from contextlib import contextmanager
from functools import partial
import host_jobs
from host_objects import COMPILE_FLAGS, HostObjects, compiler_environment, compiler_identity, reuse_enabled

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / ".build"
C_COMPILER = str(ROOT / '.toolchain/llvm/bin/clang') if sys.platform == 'linux' else '/usr/bin/clang'
COMPILER = str(ROOT / '.toolchain/llvm/bin/clang++') if sys.platform == 'linux' else '/usr/bin/clang++'
SANITIZERS = ["-O1", "-g", "-fsanitize=address,undefined", "-fno-omit-frame-pointer"]
THREAD_FLAGS = ['-pthread'] if sys.platform == 'linux' else []
FRAME_GEOMETRIES = {(528, 792), (792, 528)}


def crypto_flags() -> list[str]:
    """Linux uses only the freshly built, locked project-local static provider."""
    if sys.platform != 'linux':
        return []
    from native_output import owned_directory, owned_file
    prefix = ROOT / '.toolchain/openssl'
    include = owned_directory(ROOT, prefix / 'include')
    library = owned_file(ROOT, prefix / 'lib/libcrypto.a')
    if not library.is_file():
        raise RuntimeError('Missing locked Linux OpenSSL static provider; no system fallback')
    return ['-I', str(include), str(library), '-pthread', '-ldl']


class ObjectSession:
    """One compilation call over the shared object cache.

    Both compilers are identified once at the start, each header is hashed once,
    lookups run concurrently, and finish() identifies both compilers again. The
    call finishes after its last compiler invocation, the link included."""
    def __init__(self):
        self.environment = compiler_environment(ROOT)
        memo = {}
        self.identities = {compiler: compiler_identity(compiler, self.environment, ROOT, memo)
                           for compiler in (C_COMPILER, COMPILER)}
        self.headers = {}

    def objects(self, compiler):
        return HostObjects(ROOT, compiler, identity=self.identities[compiler], memo=self.headers)

    def finish(self):
        memo = {}
        if {compiler: compiler_identity(compiler, self.environment, ROOT, memo)
                for compiler in self.identities} != self.identities:
            raise RuntimeError('Host compiler changed during object compilation')


def cached_objects(session: ObjectSession, units) -> list[Path]:
    """Look up (compiler, source, code flags, preprocessor flags) units concurrently;
    each lookup holds a compiler slot only while its compiler runs."""
    return host_jobs.run_all([partial(objects.object, source, flags, preprocess)
                              for objects, source, flags, preprocess in units])


SOURCE_SUFFIXES = {".c", ".cc", ".cpp", ".cxx"}
LINK_SUFFIXES = {".o", ".a", ".so"}
SEPARATE_VALUES = {"-o", "-I", "-D", "-U", "-include", "-isystem", "-iquote", "-idirafter", "-L", "-Xlinker"}
NOT_LINKING = {"-c", "-E", "-S", "-M", "-MM", "-fsyntax-only", "-###", "-x"}


def split_build(command: list[str]):
    """Return (compile arguments, sources, link command template) for one plain
    multi-source compile-and-link driver command, else None.

    The clang driver compiles each source with every non-linker argument, then
    links with all arguments. Separate compiles reproduce exactly those jobs."""
    if any(argument in NOT_LINKING for argument in command) or "-o" not in command:
        return None
    compile_arguments, sources, template = [command[0]], [], [command[0]]
    arguments = iter(command[1:])
    for argument in arguments:
        if argument in SEPARATE_VALUES:
            value = next(arguments, None)
            if value is None:
                return None
            template += [argument, value]
            if argument not in ("-o", "-L", "-Xlinker"):
                compile_arguments += [argument, value]
        elif argument.startswith(("-l", "-L", "-Wl,")):
            template.append(argument)
        elif argument.startswith("-"):
            compile_arguments.append(argument)
            template.append(argument)
        elif Path(argument).suffix in SOURCE_SUFFIXES:
            sources.append(argument)
            template.append(len(sources) - 1)
        elif Path(argument).suffix in LINK_SUFFIXES:
            template.append(argument)
        else:
            return None
    return (compile_arguments, sources, template) if len(sources) > 1 else None


def reusable(arguments: list[str], sources: list[str], options: dict):
    """(code flags, preprocessor flags) when the shared object cache can key
    every compile of a split fixture build completely, else None: the project
    C++ compiler from ROOT, absolute C++ sources and include paths, and only
    flags that cache admits."""
    if arguments[0] != COMPILER or Path(options.get("cwd", ROOT)).resolve() != ROOT.resolve():
        return None
    if any(not Path(source).is_absolute() or Path(source).suffix == ".c" for source in sources):
        return None
    code, preprocess = [], []
    values = iter(arguments[1:])
    for argument in values:
        if argument == "-I":
            path = next(values)
            if not Path(path).is_absolute():
                return None
            preprocess += [argument, path]
        elif argument.startswith("-D") and len(argument) > 2:
            preprocess.append(argument)
        elif argument in COMPILE_FLAGS:
            code.append(argument)
        else:
            return None
    return code, preprocess


def run_build(command: list, *, reuse: bool = True, **options) -> subprocess.CompletedProcess:
    """Run a fixture build as subprocess.run(command, check=True, ...) would.

    A multi-source compile-and-link command compiles each source concurrently
    with the same compiler and arguments, then links those objects with the
    original command. Anything else runs unchanged. While reuse is enabled,
    each shared object comes from the shared cache, compiled under its explicit
    environment and complete key, when every compile argument has a role that
    key binds. A source in the fixture's own output directory is written for
    this build, often with a per-run path, so its object could never be reused:
    it compiles fresh, under that same explicit environment, into a private
    directory beside the output. Every object of a build the cache cannot key,
    or of a fresh witness (reuse=False), compiles there with the caller's
    options."""
    if options.get("check") is not True or set(options) - {"check", "cwd", "env"}:
        raise ValueError("Fixture builds run checked, with only cwd/env options")
    command = [str(argument) for argument in command]
    plan = split_build(command)
    if plan is None:
        return host_jobs.run(command, **options)
    compile_arguments, sources, template = plan
    output = Path(options.get("cwd", ".")) / command[command.index("-o") + 1]
    roles = reusable(compile_arguments, sources, options) if reuse and reuse_enabled() else None
    private = output.parent.resolve()
    shared = set() if roles is None else {index for index, source in enumerate(sources)
                                          if not Path(source).resolve().is_relative_to(private)}
    fresh = options if roles is None else dict(options, env=compiler_environment(ROOT))
    with tempfile.TemporaryDirectory(prefix=".objects-", dir=output.parent) as directory:
        objects = [str(Path(directory) / f"{index}-{Path(source).stem}.o") for index, source in enumerate(sources)]
        session = ObjectSession() if shared else None
        cache = session.objects(COMPILER) if shared else None
        built = host_jobs.run_all([
            partial(cache.object, source, *roles) if index in shared else
            partial(host_jobs.run, [*compile_arguments, "-c", source, "-o", objects[index]], **fresh)
            for index, source in enumerate(sources)])
        if shared:
            objects = [str(built[index]) if index in shared else name for index, name in enumerate(objects)]
            print(f"HOST objects {output.name}: reused {cache.hits}, compiled {cache.compiled}, "
                  f"fixture-local fresh {len(sources) - len(shared)}; fresh link", file=sys.stderr, flush=True)
        linked = host_jobs.run([objects[item] if isinstance(item, int) else item for item in template], **options)
        if shared:
            session.finish()
        return linked


def compile_host(main: Path, name: str, *, fresh_directory: Path | None = None,
                 extra_sources: tuple[Path, ...] = (), extra_include_dirs: tuple[Path, ...] = ()) -> Path:
    """Compile and link one standalone sanitized host test, main and any extra C
    or C++ units, to .build/<name>: through the shared object cache, or, for a
    fresh witness, anew in fresh_directory."""
    BUILD.mkdir(exist_ok=True)
    output = (fresh_directory or BUILD) / name
    sources = [main, *extra_sources]
    code_flags = ["-std=c++17", "-Wall", "-Wextra", "-Werror", "-pedantic", *SANITIZERS, *THREAD_FLAGS]
    c_flags = ["-std=c99", *SANITIZERS, *THREAD_FLAGS]
    include_flags = [value for path in extra_include_dirs for value in ("-I", str(path))]
    link = [COMPILER, '--no-default-config', *code_flags]
    if fresh_directory is not None:
        fresh_directory.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='.host-', dir=fresh_directory) as directory:
            authored = [Path(directory) / f'{index}.o' for index in range(len(sources))]
            host_jobs.run_all([
                partial(host_jobs.run,
                        [C_COMPILER if source.suffix == '.c' else COMPILER, '--no-default-config',
                         *(c_flags if source.suffix == '.c' else code_flags), *include_flags,
                         '-c', str(source), '-o', str(target)],
                        check=True, cwd=ROOT, env=compiler_environment(ROOT))
                for source, target in zip(sources, authored)])
            host_jobs.run([*link, *map(str, authored), '-o', str(output)],
                          check=True, cwd=ROOT, env=compiler_environment(ROOT))
        return output
    session = ObjectSession()
    compiler = session.objects(COMPILER)
    authored = cached_objects(session, [(session.objects(C_COMPILER) if source.suffix == '.c' else compiler,
                                         source, c_flags if source.suffix == '.c' else code_flags, include_flags)
                                        for source in sources])
    host_jobs.run([*link, *map(str, authored), "-o", str(output)], check=True, cwd=ROOT,
                  env=compiler_environment(ROOT))
    session.finish()
    print(f"HOST objects {name}: reused {compiler.hits}, compiled {compiler.compiled}; fresh link",
          file=sys.stderr, flush=True)
    return output


# At least the production /newt-sd/Newt (13 B) and /newt-sd/Newt/Cache
# (19 B), preserving its path-buffer headroom in host fixtures too.
NATIVE_READING_ROOT = "data/newt-sd/Newt"
NATIVE_CACHE_ROOT = NATIVE_READING_ROOT + "/Cache"


@contextmanager
def native_storage_root():
    """Short real private storage, independent of the caller's TMPDIR.

    Yield a repo-relative path; callers must prepare and run fixtures from ROOT.
    Validate every ancestor; neither the final component nor its ancestors are
    aliases. Production path and O_NOFOLLOW checks run unchanged.
    """
    from native_output import owned_directory
    parent = Path(os.environ.get(host_jobs.NATIVE_ROOTS, BUILD / 'native'))
    parent = owned_directory(ROOT, parent, create=True)
    with tempfile.TemporaryDirectory(prefix='', dir=parent) as directory:
        physical = owned_directory(ROOT, Path(directory))
        yield physical.relative_to(ROOT).as_posix()


@contextmanager
def fixture_build_root(label: str):
    """Stable compile paths, exclusively owned while a suite generates inputs.

    Empty the workspace on entry and exit so old overlay headers cannot affect
    a later run. Cache identity still binds every newly generated input byte.
    Runtime storage belongs in native_fixture_root/native_storage_root.
    """
    from native_output import owned_directory, owned_file
    if not label or any(character not in 'abcdefghijklmnopqrstuvwxyz0123456789-' for character in label):
        raise ValueError('Fixture build label must be lowercase ASCII words or digits')
    parent = owned_directory(ROOT, BUILD / 'fixture-builds', create=True)
    lock = owned_file(ROOT, parent / (label + '.lock'))
    descriptor = os.open(lock, os.O_CREAT | os.O_RDWR | os.O_CLOEXEC | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(descriptor, fcntl.LOCK_EX)
        directory = parent / label
        if directory.exists() or directory.is_symlink():
            owned_directory(ROOT, directory)
            shutil.rmtree(directory)
        directory.mkdir(mode=0o700)
        try:
            yield directory
        finally:
            shutil.rmtree(directory)
    finally:
        os.close(descriptor)


@contextmanager
def native_fixture_root():
    """Private process cwd for binaries compiled with stable relative roots.

    Build/link still run from ROOT. Only the native executable runs here; its
    input book and binary arguments must be absolute. Keep this cwd explicit so
    independent fixtures never change the Python process's working directory.
    """
    with native_storage_root() as directory:
        physical = ROOT / directory
        (physical / NATIVE_CACHE_ROOT).mkdir(mode=0o700, parents=True)
        yield physical


@contextmanager
def native_cache_root(probe: Path):
    """Stable compile input, private real storage selected by the runtime cwd."""
    with native_fixture_root() as directory:
        probe.write_text(f'#define NEWT_READER_CACHE_ROOT "{NATIVE_CACHE_ROOT}"\n' + probe.read_text())
        yield directory


def png_from_pbm(pbm: bytes) -> bytes:
    """Preserve every PBM pixel; PNG 1 means white, PBM 1 means black."""
    magic, geometry, pixels = pbm.split(b"\n", 2)
    width, height = map(int, geometry.split())
    if magic != b"P4" or (width, height) not in FRAME_GEOMETRIES or len(pixels) != width * height // 8:
        raise ValueError("Expected canonical Newt 528 x 792 or 792 x 528 packed frame")

    def chunk(kind: bytes, data: bytes) -> bytes:
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data)))

    stride = width // 8
    raw = b"".join(b"\0" + bytes(byte ^ 255 for byte in pixels[y:y + stride])
                   for y in range(0, len(pixels), stride))
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 1, 0, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def export_preview(binary: Path, name: str, screen: str, telemetry: str = "unknown",
                   actions: str | None = None, directory: Path | None = None,
                   reader_orientation: str | None = None,
                   reader_progress_bar: str | None = None,
                   clock: str | None = None, stats: str | None = None,
                   other_firmware: str | None = None, update: str | None = None,
                   app_package: str | None = None) -> tuple[Path, Path]:
    destination = directory or ROOT / "artifacts"
    destination.mkdir(parents=True, exist_ok=True)
    command = [str(binary), "--screen", screen, "--telemetry", telemetry]
    if clock is not None:
        command.extend(["--clock", clock])
    if reader_orientation is not None:
        command.extend(["--reader-orientation", reader_orientation])
    if reader_progress_bar is not None:
        command.extend(["--reader-progress-bar", reader_progress_bar])
    if stats is not None:
        command.extend(["--stats", stats])
    # The card fixture precedes the presses that reach update mode.
    if update is not None:
        command.extend(["--update", update])
    if app_package is not None:
        command.extend(["--app-package", app_package])
    if actions is not None:
        command.extend(["--actions", actions])
    if other_firmware is not None:
        command.extend(["--other-firmware", other_firmware])
    result = subprocess.run(command, check=True, capture_output=True)
    png = png_from_pbm(result.stdout)
    pbm_path, png_path = destination / f"{name}.pbm", destination / f"{name}.png"
    pbm_path.write_bytes(result.stdout)
    png_path.write_bytes(png)
    print(f"{png_path.relative_to(ROOT) if png_path.is_relative_to(ROOT) else png_path}: "
          f"{result.stderr.decode().strip()}")
    return pbm_path, png_path
