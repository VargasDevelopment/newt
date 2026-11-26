#!/usr/bin/env python3
"""Compile the one X3 firmware build with the approved local toolchain.

There is one build: the full regional Rust app with DEV USB and Stage. Never
installs tools or uploads."""
import argparse
import configparser
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import time

sys.dont_write_bytecode = True
sys.pycache_prefix = None

from freeink_sdk_routes import FIRMWARE_BUILD
import native_toolchain
from native_output import owned_directory, owned_file, owned_output_tree, write_text
from release_tools import git_output, source_identity
from daily_firmware import (FEATURES, IMAGE, generated_configuration,
                            require_configuration, require_generated_configuration)
from rust_app_archive import build_archive, build_environment as rust_environment, verify_toolchain

ROOT = Path(__file__).resolve().parents[1]
TOOLCHAIN = ROOT / ".toolchain"
OUTPUT = ".pio/" + FIRMWARE_BUILD
# Shared toolchain views, checked before each identity capture.
_spec = importlib.util.spec_from_file_location(
    "toolchain_share", Path(__file__).resolve().with_name("toolchain-share.py"))
toolchain_share = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(toolchain_share)

def build_environment() -> dict[str, str]:
    # Do not pass ambient build/package-manager settings into subprocesses.
    # HOME retains its existing identity; all write/cache paths are explicit.
    env = {"PATH": os.defpath, "LANG": "en_US.UTF-8", "LC_ALL": "en_US.UTF-8", "PYTHONDONTWRITEBYTECODE": "1"}
    if "HOME" in os.environ:
        env["HOME"] = os.environ["HOME"]
    paths = {
        "PLATFORMIO_CORE_DIR": TOOLCHAIN / "platformio",
        "PLATFORMIO_CACHE_DIR": TOOLCHAIN / "cache/platformio",
        "PIP_CACHE_DIR": TOOLCHAIN / "cache/pip",
        "UV_CACHE_DIR": TOOLCHAIN / "cache/uv",
        "XDG_CACHE_HOME": TOOLCHAIN / "cache/xdg",
        "TMPDIR": TOOLCHAIN / "tmp",
        "PIP_FIND_LINKS": TOOLCHAIN / "downloads",
        "UV_FIND_LINKS": TOOLCHAIN / "downloads",
        "PIP_CONSTRAINT": ROOT / ("docs/toolchain-python-linux.lock" if sys.platform == "linux" else "docs/toolchain-python.lock"),
        "UV_CONSTRAINT": ROOT / ("docs/toolchain-python-linux.lock" if sys.platform == "linux" else "docs/toolchain-python.lock"),
    }
    env.update({key: str(value) for key, value in paths.items()})
    env.update({
        "PIP_CONFIG_FILE": "/dev/null", "PIP_NO_INDEX": "1", "UV_NO_INDEX": "1",
        "UV_OFFLINE": "1", "UV_NO_BUILD_ISOLATION": "1",
        "PIP_DISABLE_PIP_VERSION_CHECK": "1", "PYTHONDONTWRITEBYTECODE": "1",
        "PLATFORMIO_SETTING_CHECK_PLATFORMIO_INTERVAL": "0",
        "PLATFORMIO_SETTING_CHECK_PRUNE_SYSTEM_THRESHOLD": "0",
        "SCONSFLAGS": "--no-site-dir",
    })
    env["PATH"] = str(TOOLCHAIN / "platformio/penv/bin") + os.pathsep + env["PATH"]
    return env


def require_toolchain() -> Path:
    executable = TOOLCHAIN / "platformio/penv/bin/pio"
    if not executable.is_file():
        raise RuntimeError("Approved project-local toolchain missing; see docs/toolchain-proposal.md")
    versions = {
        "framework-arduinoespressif32": "3.3.7",
        "framework-arduinoespressif32-libs": "5.5.0+sha.87912cd291",
        "toolchain-riscv32-esp": "14.2.0+20251107",
        "tool-esp_install": "5.3.4", "tool-esptoolpy": "5.1.2",
        "tool-scons": "4.40801.0", "contrib-piohome": "3.4.4",
    }
    for package, version in versions.items():
        path = TOOLCHAIN / "platformio/packages" / package
        manifest = json.loads((path / "package.json").read_text())
        if manifest.get("name") != package or manifest.get("version") != version:
            raise RuntimeError(f"Local package differs from approved pin: {package}")
        if not (path / ".piopm").is_file():
            raise RuntimeError(f"Local package registration missing: {package}")
    platform = TOOLCHAIN / "platformio/platforms/espressif32"
    if not (platform / ".piopm").is_file():
        raise RuntimeError("Local platform registration missing")
    manifest = json.loads((platform / "platform.json").read_text())
    if manifest.get("version") != "55.03.37":
        raise RuntimeError("Local platform differs from approved pin")
    return executable


def build_command(executable: Path) -> list[str]:
    # Copied venv entrypoints can retain another checkout's absolute shebang.
    # Select this venv explicitly and ignore ambient Python search paths.
    interpreter = executable.parent / "python"
    if not interpreter.is_file():
        raise RuntimeError("Approved project-local Python interpreter missing")
    return [str(interpreter), "-I", "-B", str(executable), "run", "-e", "newt_x3", "--jobs", "2"]


def generated_config_destination() -> Path:
    return owned_file(ROOT, ROOT / ".build" / FIRMWARE_BUILD / "platformio.ini", create_parents=True)


def require_native_output_directories(environment: dict[str, str]) -> None:
    prefix = ROOT / OUTPUT
    owned_output_tree(ROOT, prefix / "build")
    for directory in (prefix / "build/newt_x3", prefix / "libdeps/newt_x3"):
        owned_directory(ROOT, directory, create=True)
    for key in ("PLATFORMIO_CORE_DIR", "PLATFORMIO_CACHE_DIR", "PIP_CACHE_DIR",
                "UV_CACHE_DIR", "XDG_CACHE_HOME", "TMPDIR"):
        owned_directory(ROOT, Path(environment[key]), create=True)


def prepare_daily_lane(environment: dict[str, str]) -> None:
    """Reject output aliases before any tool or input generator is dispatched."""
    require_configuration(ROOT)
    require_native_output_directories(environment)
    generated_config_destination()
    for directory in ('.build/rust-app/target', '.build/rust-app/ld',
                      '.toolchain/rust/cargo-home'):
        owned_directory(ROOT, ROOT / directory, create=True)
    subprocess.run([sys.executable, '-B', str(ROOT / 'tools/apply-freeink-patch.py'),
                    '--reader-lane', FIRMWARE_BUILD], cwd=ROOT, env=environment, check=True)
    subprocess.run([sys.executable, '-B', str(ROOT / 'tools/wifi-sections.py'), '--rust-app'],
                   cwd=ROOT, env=environment, check=True)
    for tool in ('crt_bundle.py', 'make-builtin-app.py', 'make-stage-backdrop.py'):
        subprocess.run([sys.executable, '-B', str(ROOT / 'tools' / tool),
                        *(['--check'] if tool.startswith('make-') else [])],
                       cwd=ROOT, env=environment, check=True)


def write_resource_report(receipt, report, baseline, variant, *, diagnostic):
    """Diagnostics measure only; product builds keep the existing ratchet."""
    from code_health_resources import check_resources
    write_text(ROOT, receipt, json.dumps(report, indent=2, sort_keys=True) + "\n")
    if not diagnostic:
        failures = check_resources(ROOT, baseline, variant)
        if failures:
            raise RuntimeError("Resource ratchet failed:\n" + "\n".join(failures))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--clean", action="store_true", help="clean build outputs before compiling")
    parser.add_argument("--check-config", action="store_true", help="read-only approved configuration preflight; no toolchain required")
    parser.add_argument("--health-report", action="store_true", help="verify the RAM/flash/stack ceilings and write a source-bound receipt; requires --clean")
    parser.add_argument("--page-turn-trace", action="store_true", help="opt-in page-turn diagnostic trace candidate; never a publication image")
    parser.add_argument("--dev-usb", "--regional-home", dest="retired", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.retired:
        parser.error("there is one firmware build (regional DEV); drop --dev-usb and --regional-home")
    if args.check_config:
        if args.clean or args.health_report or args.page_turn_trace:
            parser.error("--check-config checks only the authored configuration")
        require_configuration(ROOT)
        print("PASS: pinned full Rust daily dependency and build configuration")
        return
    if args.health_report and not args.clean:
        parser.error("--health-report requires --clean so every compiler stack report is fresh")
    variant = FIRMWARE_BUILD
    env = build_environment()
    prepare_daily_lane(env)
    executable = require_toolchain()
    rust_tools = verify_toolchain(ROOT)
    command = build_command(executable)
    release_source = source_identity(ROOT)
    checkpoint = release_source["git_revision"][:12]
    # Generated receipts do not change compiled source identity. All source,
    # configuration, generated headers and authored app inputs still do.
    dirty = release_source["dirty"]
    identifier = checkpoint + ("-dirty" if dirty else "")
    # About shows the captured revision's commit date, never the compile clock.
    date = git_output(ROOT, "show", "-s", "--format=%cs", release_source["git_revision"])
    features = FEATURES + (("page-turn-trace",) if args.page_turn_trace else ())
    config = generated_configuration(ROOT, identifier, date)
    effective_config = generated_config_destination()
    stream = io.StringIO()
    config.write(stream)
    write_text(ROOT, effective_config, stream.getvalue())
    require_generated_configuration(ROOT, effective_config)
    command += ["--project-conf", str(effective_config)]
    if args.health_report:
        receipt = owned_file(ROOT, ROOT / f"artifacts/code-health-{variant}.json", create_parents=True)
        from code_health import base_failures, read_baseline
        from code_health_resources import build_identity, build_report
        baseline = read_baseline(ROOT)
        moved = base_failures(ROOT, baseline)
        if moved:
            raise RuntimeError("Resource ratchet failed:\n" + "\n".join(moved))
    if args.clean:
        with native_toolchain.cleaning(ROOT):
            subprocess.run([*command, "-t", "clean"], cwd=ROOT, env=env, check=True)
    if source_identity(ROOT) != release_source:
        raise RuntimeError("Release source identity changed before compilation")
    if args.health_report:
        # The toolchain lock keeps every identity capture clear of the
        # platform's in-build framework edits (tools/native_toolchain.py).
        with native_toolchain.quiescent(ROOT):
            native_toolchain.require_restored(ROOT)
            toolchain_share.check_views(ROOT)
            expected_identity = build_identity(ROOT, variant, effective_config)
        started_ns = time.time_ns()
    nvs_guard = [sys.executable, "-B", str(ROOT / "tools/verify-nvs-guard.py"), '--rust-app']
    wifi_link = [sys.executable, "-B", str(ROOT / "tools/verify-wifi-link.py"), '--rust-app']
    next_boot = [sys.executable, "-B", str(ROOT / "tools/verify-next-boot.py")]
    # The build and its report hold the lock together: no other build's
    # framework edit falls between them.
    with native_toolchain.quiescent(ROOT):
        rust_env = rust_environment(ROOT, rust_tools, {
            'NEWT_DEV_BUILD_ID': identifier,
            'NEWT_DEV_BUILD_DATE': date})
        build_archive(ROOT, rust_tools, rust_env, clean=args.clean,
                      package='newt-app-image', features=','.join(features))
        subprocess.run(command, cwd=ROOT, env=env, check=True)
        subprocess.run(nvs_guard, cwd=ROOT, env=env, check=True)
        subprocess.run(wifi_link, cwd=ROOT, env=env, check=True)
        subprocess.run(next_boot, cwd=ROOT, env=env, check=True)
        if args.health_report:
            report = build_report(ROOT, variant, effective_config, started_ns, expected_identity)
            write_resource_report(receipt, report, baseline, variant, diagnostic=args.page_turn_trace)
    if args.health_report:
        if source_identity(ROOT) != release_source:
            raise RuntimeError("Release source identity changed during compilation")
        image = OUTPUT + "/build/newt_x3/firmware.bin"
        release_receipt = {**release_source, "variant": variant, "build_id": identifier,
                           'image': IMAGE, 'features': list(features),
                           "binary_sha256": report["artifacts"][image],
                           "resource_identity": report["identity"]}
        write_text(ROOT, ROOT / f"artifacts/firmware-identity-{variant}.json",
                   json.dumps(release_receipt, indent=2, sort_keys=True) + "\n")
        label = (
            "MEASURED: unpublished page-turn-trace candidate"
            if args.page_turn_trace
            else f"PASS: {variant} RAM/flash/selected static-stack ceilings"
        )
        print(f"{label}; {receipt.relative_to(ROOT)}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, TypeError, KeyError, RuntimeError, configparser.Error, subprocess.CalledProcessError) as error:
        print(f"Firmware build stopped: {error}", file=sys.stderr)
        sys.exit(1)
