"""Checked C3 build receipts; static frames are not runtime stack bounds."""
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
from python_runtime import runtime_identity
from code_health_flash_policy import (APP_PARTITION_CAPACITY, VARIANTS, adoption_failures, usable_flash,
                                     validate_records)
from freeink_sdk_routes import (SDK_LIBRARIES, reader_patch_order, reader_sdk,
                                reader_sdk_provenance, require_sdk_dependency_routes)
from freeink_sdk_provenance import validate_reader_copy
from code_health_sram import RAM_CAPACITY, ram_measurements
from daily_firmware import SOURCE_CONFIGURATION
from rust_resource_inputs import ARCHIVE, archive_dependencies, generated_inputs, native_sources
import rust_image_measurements as rust_image

SHARED_DATA_INPUT = "rust/device-ui/whats_new_notes.h"
PROCEDURE_NAMES = (
    "firmware-build.py", "freeink_sdk_routes.py", "freeink_sdk_provenance.py", "freeink_source_pin.py",
    "release_tools.py", "code_health.py", "code_health_resources.py", "code_health_flash_policy.py",
    "python_runtime.py", "native_output.py", "native_toolchain.py", "toolchain-share.py",
    "verify-nvs-guard.py", "rust_open_stack.py", "crash_output_guard.py", "crash_console_guard.py", "linked_code.py",
    "code_health_sram.py", "wifi-sections.py", "crt_bundle.py", "verify-wifi-link.py", "wifi_link_image.py",
    "object_relocations.py", "wifi_lent_heap.py", "verify-next-boot.py", "make-builtin-app.py", "newt_package.py",
    "make-reader-fixture.py", "make-stage-backdrop.py", "brand_source.py", "pack-reader-atlas.py",
    "make-native-reader-sizes.py", "apply-freeink-patch.py", "daily_firmware.py", "rust_app_archive.py",
    "rust-toolchain.py", "rust_app_link.py", "rust_resource_inputs.py", "rust_image_measurements.py")

# Absolute budget bound for the explicitly declared existing X3 build target.
# Actual capacity is measured from the generated table, never overridden here.
X3_PARTITIONS = ((b"nvs",1,2,0x9000,0x5000,0), (b"otadata",1,0,0xe000,0x2000,0),
                 (b"app0",0,16,0x10000,0x770000,0), (b"app1",0,17,0x780000,0x770000,0),
                 (b"spiffs",1,130,0xef0000,0x100000,0), (b"coredump",1,3,0xff0000,0x10000,0))
LIMITATION = "Shared SRAM occupancy includes linker padding; the lent Wi-Fi range is wifi_lent_bytes, not ram_bytes; RTC is separate. Individual compiler frames exclude heap, call chains and runtime peaks. Flash capacity is enforced from the generated table for the declared existing X3 layout. The table is not installed; fresh device preflight remains required."


def build_directory(root, variant):
    if variant not in VARIANTS:
        raise ValueError(f"Unknown resource variant: {variant}")
    return Path(root) / ".pio" / variant / "build/newt_x3"


def configuration(root, variant):
    """The generated configuration that adds the build ID to the authored one."""
    build_directory(root, variant)
    return Path(root) / ".build" / variant / "platformio.ini"


def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def tree_hash(root, paths):
    paths, base = sorted(set(paths)), os.path.join(root, "")
    names = [str(path)[len(base):] if str(path).startswith(base) else path.relative_to(root).as_posix() for path in paths]
    lanes = max(1, min(8, len(paths)))  # Lanes only overlap reads and hashing.
    with ThreadPoolExecutor(lanes) as pool:
        digests = list(pool.map(lambda lane: [sha256(path) for path in paths[lane::lanes]], range(lanes)))
    digest = hashlib.sha256()
    for index, name in enumerate(names):
        digest.update(name.encode() + b"\0")
        digest.update(bytes.fromhex(digests[index % lanes][index // lanes]))
    return digest.hexdigest()


def input_files(directory, include_python_cache=False, owned_roots=None):
    """Whole approved input roots: include names and compiler roles are unrestricted.

    Every listed file resolves inside the canonical root, recorded in owned_roots."""
    if directory.is_symlink():
        raise ValueError(f"Compiler input directory symlink is unsupported: {directory}")
    if not directory.is_dir():
        raise ValueError(f"Missing build input directory: {directory}")
    excluded = {".git"} if include_python_cache else {".git", "__pycache__"}
    owned_root = directory.resolve()
    owned, files, pending = os.path.join(owned_root, ""), [], [directory]
    while pending:
        with os.scandir(pending.pop()) as entries:
            for entry in (entry for entry in entries if entry.name not in excluded):
                path = Path(entry.path)
                # Directory aliases can retarget among owned trees without changing the
                # file list or bytes hashed. Require canonical directory ownership.
                if entry.is_symlink():
                    if os.path.isdir(path):
                        raise ValueError(f"Compiler input directory symlink is unsupported: {path}")
                    target = os.path.realpath(path)
                    if (not os.path.exists(path) or not target.startswith(owned)
                            or excluded.intersection(Path(target[len(owned):]).parts)):
                        raise ValueError(f"Compiler input symlink escapes its owned root: {directory}")
                elif entry.is_dir():
                    pending.append(path)
                if entry.is_file():
                    files.append(path)
    if owned_roots is not None and files:
        owned_roots.append(owned_root)
    return files


def identity_inputs(root, variant):
    """One complete ownership set for hashing and actual compiler dependency checks."""
    root, owned_roots = Path(root), []
    build_directory(root, variant)
    try:
        validate_reader_copy(root, variant)
    except RuntimeError as error:
        raise ValueError(f"Invalid composed reader SDK: {error}") from error
    directories = ["rust", "assets", "apps", "patches", "vendor/freeink-sdk", reader_sdk(variant)]
    inputs = []
    for directory in directories:
        inputs.extend(input_files(root / directory, owned_roots=owned_roots))
    # The release notes build.rs reads must be an owned ordinary file, never a link.
    notes = root / SHARED_DATA_INPUT
    if notes.exists() or notes.is_symlink():
        parents = (root / "rust", root / "rust/device-ui")
        if (notes.is_symlink() or not notes.is_file() or any(path.is_symlink() for path in parents)
                or not notes.resolve().is_relative_to(root.resolve())):
            raise ValueError("Shared update notes must be an owned ordinary input")
        inputs.append(notes)
    provenance = root / reader_sdk_provenance(variant)
    if provenance.is_symlink() or not provenance.is_file() or not provenance.resolve().is_relative_to(root.resolve()):
        raise ValueError("Missing or symlinked reader SDK provenance")
    inputs.append(provenance)
    compiler_paths = input_files(root / ".toolchain/platformio/packages/toolchain-riscv32-esp", owned_roots=owned_roots)
    compiler_paths += input_files(root / '.toolchain/rust/toolchain', owned_roots=owned_roots)
    named = list((root / ".toolchain/platformio/packages").glob("*/package.json"))
    named.append(root / '.toolchain/rust/installed.json')
    platform = root / ".toolchain/platformio/platforms/espressif32"
    named.extend(platform / name for name in ("platform.json", "platform.py", "boards/esp32-c3-devkitm-1.json"))
    toolchain_inputs = named + input_files(platform, include_python_cache=True, owned_roots=owned_roots)
    # Package versions do not make mutable local source/archive content immutable.
    # This acquired libs package contains C3 only; caches/downloads stay outside.
    for directory in (".toolchain/platformio/packages/framework-arduinoespressif32",
                      ".toolchain/platformio/packages/framework-arduinoespressif32-libs"):
        toolchain_inputs.extend(input_files(root / directory, owned_roots=owned_roots))
    # Build drivers and installed Python dependencies can change compilation even
    # when their version manifests stay unchanged. Bytecode is executable input.
    for directory in (".toolchain/platformio/penv/lib",
                      ".toolchain/platformio/packages/tool-scons",
                      ".toolchain/platformio/packages/tool-esptoolpy",
                      ".toolchain/platformio/packages/tool-esp_install"):
        toolchain_inputs.extend(input_files(root / directory, include_python_cache=True, owned_roots=owned_roots))
    penv = root / ".toolchain/platformio/penv"
    entrypoints = [penv / name for name in ("bin/pio", "bin/platformio", "pyvenv.cfg")]
    if any(not path.resolve().is_relative_to(penv.resolve()) for path in entrypoints):
        raise ValueError("Build driver symlink escapes its owned root")
    named.extend(entrypoints)
    toolchain_inputs.extend(entrypoints)
    if any(not path.is_file() for path in toolchain_inputs):
        raise ValueError("Missing pinned build driver input")
    if not inputs or len(toolchain_inputs) < 2:
        raise ValueError("Missing compiled input or pinned toolchain manifests")
    groups = {"inputs_sha256": inputs, "compiler_sha256": compiler_paths, "toolchain_sha256": toolchain_inputs}
    # Listed files resolve inside their canonical roots: check each root once and
    # resolve only individually named inputs.
    project_root = root.resolve()
    if any(not path.is_relative_to(project_root) for path in owned_roots + [path.resolve() for path in named]):
        raise ValueError("Compiler input root escapes the isolated project")
    return groups


def interpreter_identity(root):
    """Bind the installed driver runtime without granting compiler ownership."""
    executable = Path(root) / ".toolchain/platformio/penv/bin/python"
    return runtime_identity(executable)


def procedure_files(root, variant):
    # Native build/collection and code-health verification, including their
    # authored subprocess/import helpers. These do not grant compiler ownership.
    directory = root / "tools"
    paths = [directory / name for name in PROCEDURE_NAMES]
    # Both Frosted and sleep-brand checks import this already source-bound helper.
    paths.append(root / "assets/frosted-home/source/png_tools.py")
    return authored_procedure_files(root, paths)


def authored_procedure_files(root, paths):
    """One source/cache ownership boundary for named native or acceptance tools."""
    if sys.pycache_prefix is not None:
        raise ValueError("Authored procedure cache prefix must use the project-local default")
    if (Path(root, "tools").is_symlink() or any(path.is_symlink() or not path.is_file()
            or not path.resolve().is_relative_to(Path(root).resolve()) for path in paths)):
        raise ValueError("Missing or symlinked authored build procedure")
    caches = []
    for source in paths:
        cache_directory = source.parent / "__pycache__"
        if cache_directory.is_symlink() or (cache_directory.exists() and not cache_directory.is_dir()):
            raise ValueError("Authored procedure cache directory must be canonical")
        # -B suppresses writes, but Python can still execute existing bytecode.
        # Bind tagged/optimized variants and adjacent legacy bytecode only for
        # the actual procedure stems, leaving unrelated caches outside identity.
        candidates = [source.with_suffix(".pyc"), *cache_directory.glob(source.stem + ".*.pyc")]
        for path in candidates:
            if path.is_symlink() or (path.exists() and not path.is_file()):
                raise ValueError("Authored procedure cache must be a regular owned file")
            if path.is_file():
                caches.append(path)
    return paths + caches


def build_identity(root, variant, config_path=None):
    """Capture before a clean build and compare again before issuing its receipt."""
    root = Path(root)
    config = configuration(root, variant)
    if config_path is not None and Path(config_path).resolve() != config.resolve():
        raise ValueError("Resource evidence requires the canonical variant configuration")
    groups = identity_inputs(root, variant)
    identity = {"variant": variant,
            "authored_config_sha256": sha256(root / SOURCE_CONFIGURATION),
            "config_sha256": sha256(config),
            "procedures_sha256": tree_hash(root, procedure_files(root, variant)),
            "python_interpreter": interpreter_identity(root),
            "procedure_python": runtime_identity(),
            **{key: tree_hash(root, files) for key, files in groups.items()}}
    return identity


def partition_capacity(data):
    if len(data) != 0xc00:
        raise ValueError("Invalid built partition table")
    entries = []
    for offset in range(0, len(X3_PARTITIONS)*32, 32):
        magic, kind, subtype, start, size, label, flags = struct.unpack_from("<HBBII16sI", data, offset)
        if magic != 0x50AA or size == 0:
            raise ValueError("Invalid built partition entry")
        entries.append((label.rstrip(b"\0"), kind, subtype, start, size, flags))
    if tuple(entries) != X3_PARTITIONS:
        raise ValueError("Declared X3 partition layout changed")
    offset = len(entries)*32
    expected = b"\xeb\xeb" + b"\xff"*14 + hashlib.md5(data[:offset]).digest()
    if data[offset:offset+32] != expected or data[offset+32:] != b"\xff"*(len(data)-offset-32):
        raise ValueError("Invalid built partition MD5 or padding")
    # A single image must fit every application slot in the declared table.
    return min(entry[4] for entry in entries if entry[1] == 0)


def stack_frames(root, paths):
    frames = {}
    for path in paths:
        for line in path.read_text().splitlines():
            fields = line.split("\t")
            if len(fields) != 3 or not fields[1].isdigit() or fields[2] not in ("static", "dynamic", "dynamic,bounded"):
                raise ValueError(f"Invalid compiler stack measurement in {path.name}")
            match = re.fullmatch(r"(.*):\d+:\d+:(.+)", fields[0])
            if not match:
                raise ValueError(f"Invalid compiler stack identifier in {path.name}")
            source = Path(match[1])
            if source.is_absolute():
                try:
                    source = source.resolve().relative_to(Path(root).resolve())
                except ValueError:
                    continue
            if not source.parts or source.parts[0] != "rust" or ".." in source.parts:
                continue
            key = source.as_posix() + "::" + match[2]
            value = {"bytes": int(fields[1]), "kind": fields[2]}
            old = frames.get(key)
            if old is None or value["bytes"] > old["bytes"]:
                frames[key] = value
            if old and old["kind"] == "dynamic":
                frames[key]["kind"] = "dynamic"
            if value["kind"] == "dynamic":
                frames[key]["kind"] = "dynamic"
    if not frames:
        raise ValueError("Missing authored compiler stack frames")
    return frames


def artifact_paths(root, variant):
    """The outputs required for every canonical variant receipt and verification."""
    root = Path(root)
    directory = build_directory(root, variant)
    stack_paths = [directory / 'src' / source.with_suffix(source.suffix + '.su')
                   for source in native_sources(root)]
    objects = sorted(directory.rglob("*.o"))
    # Source filters are pinned by firmware-build; authored objects cannot vanish
    # along with their dependency proof and leave a seemingly complete receipt.
    required = [path.with_suffix(".o") for path in stack_paths]
    if not objects or any(path not in objects for path in required):
        raise ValueError("Missing compiled authored object/dependency evidence")
    stack_paths = sorted(set(stack_paths) | set(directory.rglob('*.su')))
    archive = directory / "libFrameworkArduino.a"
    return ([directory / name for name in ("firmware.elf", "firmware.bin", "firmware.map", "partitions.bin")]
            + stack_paths + objects + [path.with_suffix(".d") for path in objects]
            + [root / ARCHIVE, (root / ARCHIVE).with_suffix('.d')] + generated_inputs(root)
            + ([archive] if archive.is_file() else []))


def dependency_files(root, path):
    """Parse GCC Make rules, including continuations and escaped path characters."""
    lines = path.read_text().replace("\\\r\n", "").replace("\\\n", "").splitlines()
    dependencies = set()
    for index, line in enumerate(lines):
        if not line.strip():
            continue
        tokens = re.findall(r"(?:\\.|[^\s\\:])+|:", line)
        if tokens.count(":") != 1 or tokens.index(":") != 1:
            raise ValueError(f"Invalid compiler dependency rule: {path}")
        tokens = [re.sub(r"\\(.)", r"\1", token).replace("$$", "$") for token in tokens]
        target = (root / tokens[0]).resolve()
        if index == 0:
            if target != path.with_suffix(".o").resolve() or len(tokens) < 3:
                raise ValueError(f"Missing compiler dependency closure: {path}")
            dependencies.update((root / name).resolve() for name in tokens[2:])
        elif len(tokens) != 2:
            raise ValueError(f"Unexpected secondary compiler dependency rule: {path}")
    if not dependencies:
        raise ValueError(f"Missing compiler dependencies: {path}")
    return dependencies


def require_dependency_ownership(root, variant, paths):
    groups = identity_inputs(root, variant)
    owned = {path.resolve() for files in groups.values() for path in files}
    objects = {path.resolve() for path in paths if path.suffix == ".o"}
    proofs = [path.with_suffix(".d") for path in objects]
    dependencies = set().union(*(dependency_files(root, path) for path in proofs))
    dependencies.update(archive_dependencies(root))
    owned.update(path.resolve() for path in generated_inputs(root) if path in paths)
    # Diagnose a wrong SDK lane/source before the general ownership rejection.
    require_sdk_dependency_routes(root, variant, dependencies)
    for path in dependencies:
        if path not in owned or not path.is_file():
            raise ValueError(f"Unbound or missing compiler dependency: {path}")
    directory = build_directory(root, variant)
    archive = (directory / "libFrameworkArduino.a").resolve()
    loads = re.findall(r"^LOAD (.+)$", (directory / "firmware.map").read_text(), re.MULTILINE)
    if not loads:
        raise ValueError("Missing actual linker input evidence")
    rust_archive = (root / ARCHIVE).resolve()
    if sum((root / name).resolve() == rust_archive for name in loads) != 1:
        raise ValueError('The daily linker must consume exactly the freshly built Rust archive')
    for name in loads:
        path = (root / name).resolve()
        generated_archive = path == archive and any(obj.parent == directory.resolve() / "FrameworkArduino" for obj in objects)
        if not path.is_file() or not (path in owned or path in objects or generated_archive or path == rust_archive):
            raise ValueError(f"Unbound or missing linker input: {path}")


def measure_outputs(root, variant, paths):
    """Read measurements from actual outputs, never from editable receipt fields."""
    root = Path(root)
    require_dependency_ownership(root, variant, paths)
    directory = build_directory(root, variant)
    size = root / ".toolchain/platformio/packages/toolchain-riscv32-esp/bin/riscv32-esp-elf-size"
    memory = ram_measurements(subprocess.run([str(size), "-A", str(directory / "firmware.elf")],
                                            check=True, capture_output=True, text=True).stdout,
                              (directory / "firmware.map").read_text())
    capacity=partition_capacity((directory / "partitions.bin").read_bytes())
    return {**memory, "flash_bytes": (directory / "firmware.bin").stat().st_size,
            "flash_capacity_bytes": capacity,
            "stack_bytes": {**stack_frames(root, [path for path in paths if path.suffix == ".su"]),
                            **rust_image.open_frame(root, directory / 'firmware.elf')}}


def build_report(root, variant, config_path, started_ns, expected_identity):
    """Measure fresh outputs of the successful canonical clean build; writes nothing."""
    root = Path(root)
    identity = build_identity(root, variant, config_path)
    if identity != expected_identity:
        raise ValueError("Build inputs changed during compilation")
    outputs = artifact_paths(root, variant)
    for path in outputs:
        if not path.is_file() or path.stat().st_mtime_ns < started_ns:
            raise ValueError(f"Missing or stale build output: {path.relative_to(root)}; use --clean")
    measured = measure_outputs(root, variant, outputs)
    return {"schema": 1, "identity": identity,
            "artifacts": {path.relative_to(root).as_posix(): sha256(path) for path in outputs},
            **measured, "limitations": LIMITATION}


def nonnegative(value):
    return type(value) is int and value >= 0


def validate_budgets(baseline):
    """Validate every reviewed variant even when only structural checks are requested."""
    variants = baseline.get("resources")
    if not isinstance(variants, dict) or set(variants) != set(VARIANTS):
        raise ValueError("Resource budgets must declare exactly the one firmware build")
    for variant, budgets in variants.items():
        if not isinstance(budgets, dict) or set(budgets) - {"wifi_lent_bytes"} != {"ram_bytes", "flash_bytes", "stack_bytes"}:
            raise ValueError(f"Incomplete resource budget: {variant}")
        for field, capacity in (("ram_bytes", RAM_CAPACITY), ("flash_bytes", usable_flash(APP_PARTITION_CAPACITY)),
                                ("wifi_lent_bytes", RAM_CAPACITY)):
            if field in budgets and (not nonnegative(budgets[field]) or budgets[field] > capacity):
                raise ValueError(f"Invalid absolute budget: {variant} {field}")
        stacks = budgets["stack_bytes"]
        if not isinstance(stacks, dict) or not stacks:
            raise ValueError(f"Missing selected stack budgets: {variant}")
        for key, ceiling in stacks.items():
            if not isinstance(key, str) or "::" not in key:
                raise ValueError("Invalid selected stack identifier")
            source, signature = key.split("::", 1)
            path = Path(source)
            if (not path.parts or path.is_absolute() or path.parts[0] != "rust" or ".." in path.parts
                    or path.as_posix() != source
                    or not signature or any(char in key for char in "\n\r\t") or not nonnegative(ceiling)):
                raise ValueError(f"Invalid selected stack budget: {key}")
    validate_records(baseline)


def check_resources(root, baseline, variant, adoption_base=None):
    """Read-only comparison. Invalid or missing evidence raises ValueError/OSError.

    adoption_base, the destination tip's baseline, binds raised ceilings to this build."""
    root = Path(root)
    validate_budgets(baseline)
    report = json.loads((root / f"artifacts/code-health-{variant}.json").read_text())
    expected_keys = {"schema", "identity", "artifacts", "ram_sections", "ram_bytes", "ram_capacity_bytes", "ram_alignment_bytes", "wifi_lent_bytes", "rtc_sections", "rtc_bytes",
                     "flash_bytes", "flash_capacity_bytes", "stack_bytes", "limitations"}
    if set(report) != expected_keys or report["schema"] != 1 or report["limitations"] != LIMITATION:
        raise ValueError("Invalid resource report schema")
    if report["identity"] != build_identity(root, variant):
        raise ValueError("Stale or wrong-variant resource report; rebuild with --clean")
    expected_paths = artifact_paths(root, variant)
    if set(report["artifacts"]) != {path.relative_to(root).as_posix() for path in expected_paths}:
        raise ValueError("Incomplete or wrong-variant artifact identity")
    for name, digest in report["artifacts"].items():
        if sha256(root / name) != digest:
            raise ValueError(f"Changed build artifact: {name}")
    for name in ("ram_bytes", "ram_capacity_bytes", "ram_alignment_bytes", "wifi_lent_bytes", "rtc_bytes", "flash_bytes", "flash_capacity_bytes"):
        if not nonnegative(report[name]):
            raise ValueError(f"Invalid resource measurement: {name}")
    if (not report["ram_sections"] or not all(nonnegative(value) for value in report["ram_sections"].values())
            or sum(report["ram_sections"].values()) + report["ram_alignment_bytes"] != report["ram_bytes"] or report["ram_capacity_bytes"] != RAM_CAPACITY):
        raise ValueError("Invalid SRAM accounting")
    # Re-measure from the hash-bound outputs instead of trusting editable JSON numbers.
    measured = measure_outputs(root, variant, expected_paths)
    if any(report[key] != value for key, value in measured.items()):
        raise ValueError("Resource report differs from actual build outputs")
    budgets = baseline["resources"][variant]
    diagnostics = [] if adoption_base is None else adoption_failures(adoption_base, baseline, variant, report)
    usable = usable_flash(measured["flash_capacity_bytes"])
    if budgets["flash_bytes"] > usable:
        raise ValueError("Invalid absolute budget: flash exceeds generated partition reserve")
    if measured["flash_bytes"] > usable:
        diagnostics.append(f"{variant} flash_bytes: {measured['flash_bytes']} exceeds generated partition usable limit {usable} (10% sector-rounded reserve)")
    for name in ("ram_bytes", "flash_bytes"):
        ceiling = budgets[name]
        if ceiling > report[name.replace("_bytes", "_capacity_bytes")]:
            raise ValueError(f"Invalid absolute budget: {name}")
        if report[name] > ceiling:
            diagnostics.append(f"{variant} {name}: {report[name]} exceeds reviewed ceiling {ceiling}")
    # A build without a lent range needs no ceiling; one with it needs its own.
    lent_ceiling = budgets.get("wifi_lent_bytes", 0)
    if report["wifi_lent_bytes"] > lent_ceiling:
        diagnostics.append(f"{variant} wifi_lent_bytes: {report['wifi_lent_bytes']} exceeds reviewed ceiling {lent_ceiling}")
    for key, ceiling in budgets["stack_bytes"].items():
        frame = measured["stack_bytes"].get(key)
        if frame is None:
            raise ValueError(f"Missing selected stack measurement: {key}")
        if frame["kind"] == "dynamic":
            diagnostics.append(f"{variant} unbounded compiler stack frame: {key}")
        elif frame["bytes"] > ceiling:
            diagnostics.append(f"{variant} static frame {key}: {frame['bytes']} exceeds reviewed ceiling {ceiling}")
    return diagnostics
