"""The host preview is the real firmware: everything it adds is on an allowlist.

The preview's own code is the native-symbol fakes (rust/host/preview/fakes),
the C main() that calls the firmware's own native entry (fakes/entry.c), the
update fixture writer, the driver/CLI (tools/preview_driver.py) and the
builder call (tools/rust_preview.py). It has no Rust of its own. Pinned here:
- no other file joins rust/host/preview;
- entry.c calls nothing but newt_rust_app_run;
- a fake defines only natives Salamander (rust/salamander) or a counted
  native exception (rust-health/baseline.json) declares, or preview_/fixture_
  helpers the firmware cannot call;
- the compiled firmware copy is rust/firmware as the shared builder copies it,
  so no edited ("wired") copy of a firmware screen or owner reaches the binary.
Negative controls break each rule once. Then two repros run the firmware's
own owners: Quick Start's orientation and Settings' sleep extras.
"""
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from gate_tier import negative_controls
import firmware_host_build as host_build
from daily_firmware import FEATURES
import rust_preview

PREVIEW = ROOT / "rust/host/preview"
FILES = frozenset({"update_fixtures.py"})
FAKES = ("fakes/", (".c", ".h"))
HELPERS = ("preview_", "fixture_")
NATIVE = re.compile(r"\bfn ((?:nb|np|nsc|nh|newt)_\w+)\s*[(<]")
# A top-level, non-static C function definition.
DEFINED = re.compile(r"^(?!static\b)(?:[A-Za-z_][\w \*]*?[\s\*])(\w+)\s*\([^;{]*\)\s*\{", re.M)


def file_problems(names: set[str]) -> list[str]:
    return [f"{name} is not a preview file" for name in sorted(names)
            if name not in FILES and not (name.startswith(FAKES[0]) and name.endswith(FAKES[1]))]


def entry_problems(text: str) -> list[str]:
    code = re.sub(r"//[^\n]*", "", text)
    found = [f"entry.c calls {name}" for name in sorted(set(re.findall(r"\b(\w+)\s*\(", code)))
             if name not in ("main", "newt_rust_app_run")]
    return found + ["entry.c uses #include"] * ("#include" in code)


def declared_natives() -> set[str]:
    """The ratchet's native sources: Salamander plus the exceptions still outside it."""
    exceptions = json.loads((ROOT / "rust-health/baseline.json").read_text())["native_exceptions"]
    paths = [*(ROOT / "rust/salamander").rglob("*.rs"), *(ROOT / path for path in exceptions)]
    texts = [path.read_text() for path in paths]
    return {name for text in texts if 'extern "C"' in text for name in NATIVE.findall(text)}


def fake_problems(fakes: dict[str, str], natives: set[str]) -> list[str]:
    return [f"{name} defines {symbol}, which no firmware crate declares"
            for name, text in sorted(fakes.items()) for symbol in DEFINED.findall(text)
            if symbol not in natives and not symbol.startswith(HELPERS)]


def expected_copy() -> dict[str, str]:
    """The shared builder's copy of rust/firmware, as rust_preview.WORK names it."""
    with tempfile.TemporaryDirectory(prefix="newt-preview-owners-") as scratch:
        build = host_build.Build(Path(scratch), ROOT, None)
        build.generate_sources()
        build.wire_sources()
        build.root(None, features=FEATURES)
        prefix = str(Path(scratch).resolve()) + "/"
        work = str(rust_preview.WORK.resolve()) + "/"
        files = {"host.rs": Path(scratch, "host.rs").read_text()}
        files |= {f"source/{path.name}": path.read_text() for path in Path(scratch, "source").glob("*.rs")}
        return {name: text.replace(prefix, work) for name, text in files.items()}


def copy_problems(compiled: dict[str, str], expected: dict[str, str]) -> list[str]:
    return [f"{name} differs from rust/firmware" for name in sorted(set(compiled) | set(expected))
            if compiled.get(name) != expected.get(name)]


def compiled_copy() -> dict[str, str]:
    files = {"host.rs": (rust_preview.WORK / "host.rs").read_text()}
    return files | {f"source/{path.name}": path.read_text() for path in (rust_preview.WORK / "source").glob("*.rs")}


def frame(binary: Path, *arguments: str) -> bytes:
    return subprocess.run([str(binary), *arguments], check=True, capture_output=True).stdout


def repros(binary: Path) -> None:
    # Quick Start's orientation choice is the book's: the driver plays the
    # firmware's own Quick Start pages, so Sideways turns the book page.
    for orientation, size in (("portrait", b"528 792"), ("landscape-ccw", b"792 528")):
        page = frame(binary, "--screen", "epub", "--reader-orientation", orientation)
        assert page.split(b"\n", 2)[1] == size, orientation
    # Each sleep extra reaches the shared sleep composition; When you read
    # needs reading history, so the card holds counted sessions.
    toggles = {"off": "", "week": "next,activate,", "habit": "next,next,activate,"}
    frames = {frame(binary, "--screen", "device", "--stats", "ready", "--clock", "21:30",
                    "--actions", f"next,next,activate,{toggle}back,back,sleep") for toggle in toggles.values()}
    assert len(frames) == 3, "Off, Week and Habit draw the same sleep frame"


def control_flags() -> None:
    root = ['rustc', '-Copt-level=1', '-Cpanic=abort']
    assert rust_preview._control_command(root, True) == root
    cheap = rust_preview._control_command(root, False)
    assert cheap == ['rustc', '-Copt-level=0', '-Cpanic=abort',
                     '-Cdebug-assertions=no', '-Coverflow-checks=no']
    assert root == ['rustc', '-Copt-level=1', '-Cpanic=abort']
    overrides = [[], ['-O'], ['-Copt-level=1'], ['-C', 'opt-level=2'],
                 ['--codegen=opt-level=2'], ['--codegen', 'opt-level=2'], ['-C'],
                 ['-gO'], ['-Og'], ['-gCopt-level=2'], ['-gCdebug-assertions=yes'],
                 ['-gCoverflow-checks=yes'], ['@flags.rsp']]
    for key in ('debug-assertions', 'overflow-checks', 'debug_assertions', 'overflow_checks'):
        overrides.extend([[f'-C{key}=yes'], ['-C', f'{key}=yes'],
                          [f'--codegen={key}=yes'], ['--codegen', f'{key}=yes']])
    for extra in overrides:
        command = root + extra if extra else ['rustc', '-Cpanic=abort']
        try:
            rust_preview._control_command(command, False)
        except ValueError:
            continue
        raise AssertionError(f'cheap control accepted a different root policy: {command}')


def main():
    control_flags()
    files = {str(path.relative_to(PREVIEW)) for path in PREVIEW.rglob("*") if path.is_file()
             and "__pycache__" not in path.parts}
    entry = (PREVIEW / "fakes/entry.c").read_text()
    # entry.c defines main(), the one non-native it may; entry_problems pins it.
    fakes = {path.name: path.read_text() for path in sorted((PREVIEW / "fakes").glob("*.c")) if path.name != "entry.c"}
    natives = declared_natives()
    binary = Path(sys.argv[1]) if len(sys.argv) == 2 else rust_preview.cli(rust_preview.build())
    # Also pin the real builder's command, not only synthetic flag matrices.
    root_command = json.loads(rust_preview.LINK.read_text())['command']
    assert rust_preview._control_command(root_command, False) == [
        '-Copt-level=0' if arg == '-Copt-level=1' else arg for arg in root_command
    ] + ['-Cdebug-assertions=no', '-Coverflow-checks=no']
    compiled, expected = compiled_copy(), expected_copy()
    found = file_problems(files) + entry_problems(entry) + fake_problems(fakes, natives) \
        + copy_problems(compiled, expected)
    assert not found, found
    repros(binary)
    if negative_controls(__file__):
        assert file_problems(files | {"src/home.rs"}), "a preview screen source passed"
        assert entry_problems(entry.replace("    newt_rust_app_run(&", "    paint_home(&")), \
            "an entry calling a screen passed"
        assert fake_problems({**fakes, "ports.c": fakes["ports.c"] + "\nint compose_home(void) { return 0; }\n"},
                             natives), "a fake that is not a native passed"
        # A wired copy: one firmware screen source edited inside the build.
        name = "source/app_image.rs"
        wired = {**compiled, name: compiled[name].replace("pub struct Application", "pub struct Application ", 1)}
        assert wired != compiled and copy_problems(wired, expected), "a wired firmware copy passed"
    print("PASS: the preview adds only allowlisted fakes, entry and driver; the firmware copy is unchanged")


if __name__ == "__main__":
    main()
