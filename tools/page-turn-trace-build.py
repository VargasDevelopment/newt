#!/usr/bin/env python3
"""Developer-only diagnostic build with opt-in `page-turn-trace` telemetry.

Kept separate from `tools/firmware-build.py` so the release build procedure and
its resource ratchet stay untouched. Diagnostic images include `page-turn-trace`
in their feature list and are rejected by publication checks.
"""
import argparse
import configparser
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import time

sys.dont_write_bytecode = True
sys.pycache_prefix = None

from daily_firmware import FEATURES, IMAGE, generated_configuration, require_generated_configuration
from freeink_sdk_routes import FIRMWARE_BUILD
import native_toolchain
from native_output import owned_file, write_text
from release_tools import git_output, source_identity
from rust_app_archive import build_archive, build_environment as rust_environment, verify_toolchain

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ".pio/" + FIRMWARE_BUILD
DIAGNOSTIC_FEATURES = FEATURES + ("page-turn-trace",)


def load_firmware_build():
    spec = importlib.util.spec_from_file_location(
        "diagnostic_firmware_build", Path(__file__).resolve().with_name("firmware-build.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def write_diagnostic_report(receipt: Path, report: dict) -> None:
    """Write a measurement-only resource receipt without touching the product ratchet."""
    write_text(ROOT, receipt, json.dumps(report, indent=2, sort_keys=True) + "\n")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--clean", action="store_true", help="clean build outputs before compiling")
    parser.add_argument("--health-report", action="store_true",
                        help="write a source-bound diagnostic resource & identity receipt; requires --clean")
    args = parser.parse_args()
    if args.health_report and not args.clean:
        parser.error("--health-report requires --clean so every compiler stack report is fresh")

    fb = load_firmware_build()
    variant = FIRMWARE_BUILD
    env = fb.build_environment()
    fb.prepare_daily_lane(env)
    executable = fb.require_toolchain()
    rust_tools = verify_toolchain(ROOT)
    command = fb.build_command(executable)
    release_source = source_identity(ROOT)
    checkpoint = release_source["git_revision"][:12]
    identifier = checkpoint + ("-dirty" if release_source["dirty"] else "")
    date = git_output(ROOT, "show", "-s", "--format=%cs", release_source["git_revision"])

    config = generated_configuration(ROOT, identifier, date)
    effective_config = fb.generated_config_destination()
    stream = io.StringIO()
    config.write(stream)
    write_text(ROOT, effective_config, stream.getvalue())
    require_generated_configuration(ROOT, effective_config)
    command += ["--project-conf", str(effective_config)]

    if args.health_report:
        receipt = owned_file(ROOT, ROOT / f"artifacts/code-health-{variant}.json", create_parents=True)
        from code_health_resources import build_identity, build_report

    if args.clean:
        with native_toolchain.cleaning(ROOT):
            subprocess.run([*command, "-t", "clean"], cwd=ROOT, env=env, check=True)
    if source_identity(ROOT) != release_source:
        raise RuntimeError("Source identity changed before compilation")

    if args.health_report:
        with native_toolchain.quiescent(ROOT):
            native_toolchain.require_restored(ROOT)
            fb.toolchain_share.check_views(ROOT)
            expected_identity = build_identity(ROOT, variant, effective_config)
        started_ns = time.time_ns()

    nvs_guard = [sys.executable, "-B", str(ROOT / "tools/verify-nvs-guard.py"), "--rust-app"]
    wifi_link = [sys.executable, "-B", str(ROOT / "tools/verify-wifi-link.py"), "--rust-app"]
    next_boot = [sys.executable, "-B", str(ROOT / "tools/verify-next-boot.py")]

    with native_toolchain.quiescent(ROOT):
        rust_env = rust_environment(ROOT, rust_tools, {
            "NEWT_DEV_BUILD_ID": identifier,
            "NEWT_DEV_BUILD_DATE": date,
        })
        build_archive(ROOT, rust_tools, rust_env, clean=args.clean,
                      package="newt-app-image", features=",".join(DIAGNOSTIC_FEATURES))
        subprocess.run(command, cwd=ROOT, env=env, check=True)
        subprocess.run(nvs_guard, cwd=ROOT, env=env, check=True)
        subprocess.run(wifi_link, cwd=ROOT, env=env, check=True)
        subprocess.run(next_boot, cwd=ROOT, env=env, check=True)
        if args.health_report:
            report = build_report(ROOT, variant, effective_config, started_ns, expected_identity)
            write_diagnostic_report(receipt, report)

    if args.health_report:
        if source_identity(ROOT) != release_source:
            raise RuntimeError("Source identity changed during compilation")
        image = OUTPUT + "/build/newt_x3/firmware.bin"
        identity_receipt = {
            **release_source,
            "variant": variant,
            "build_id": identifier,
            "image": IMAGE,
            "features": list(DIAGNOSTIC_FEATURES),
            "binary_sha256": report["artifacts"][image],
            "resource_identity": report["identity"],
        }
        write_text(ROOT, ROOT / f"artifacts/firmware-identity-{variant}.json",
                   json.dumps(identity_receipt, indent=2, sort_keys=True) + "\n")
        print(f"MEASURED: unpublished page-turn-trace candidate; {receipt.relative_to(ROOT)}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, TypeError, KeyError, RuntimeError, configparser.Error,
            subprocess.CalledProcessError) as error:
        print(f"Diagnostic firmware build stopped: {error}", file=sys.stderr)
        sys.exit(1)
