#!/usr/bin/env python3
"""Build the Rust X3 application with the already installed pinned tools."""
import argparse
import configparser
import hashlib
import io
import importlib.util
import sys
from pathlib import Path
import subprocess

from daily_firmware import (CONFIGURATION, FEATURES, generated_configuration,
                            require_configuration, require_generated_configuration)
from rust_app_archive import build_archive, build_environment, verify_toolchain

ROOT = Path(__file__).resolve().parents[1]
CRT_BUNDLE_SHA256 = '2af490c0dbd133a23e122f8eb834bf0f083fbcd9e906f4449f60b7e32f926a56'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / '.build/rust-app')
    parser.add_argument('--package', default='newt-app-image')
    parser.add_argument('--clean', action='store_true', help='clean the current mutable Rust/native build outputs before rebuilding')
    parser.add_argument('--daily', action='store_true', help='the complete shipping image in the canonical firmware output directory')
    parser.add_argument('--features', default='', help='explicit non-bench product features; default bench excludes embedded-sample and wifi-update')
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--native-only', action='store_true', help='compile/link native SDK ports against the existing Rust archive')
    mode.add_argument('--rust-only', action='store_true', help='build only the Rust archive, without linking the SDK image')
    mode.add_argument('--native', action='store_true', help='link the SDK image (the default; retained for existing commands)')
    args = parser.parse_args()
    if args.daily:
        if args.features or args.package != 'newt-app-image' or args.native_only or args.rust_only:
            parser.error('the daily image has fixed features and requires the complete Rust/native build')
        args.features = ','.join(FEATURES)
        require_configuration(ROOT)
    product_features = set(FEATURES)
    requested = {feature for word in args.features.split() for feature in word.split(',') if feature}
    if not requested <= product_features:
        parser.error('firmware builds accept product features only; fixture features are host-only')
    toolchain = verify_toolchain(ROOT)
    args.output.mkdir(parents=True, exist_ok=True)
    head = subprocess.run(['git', 'rev-parse', '--short=12', 'HEAD'], cwd=ROOT,
                          capture_output=True, text=True, check=True).stdout.strip()
    dirty = subprocess.run(['git', 'status', '--porcelain', '--untracked-files=no'], cwd=ROOT,
                           capture_output=True, text=True, check=True).stdout
    date = subprocess.run(['git', 'show', '-s', '--format=%cs', 'HEAD'], cwd=ROOT,
                          capture_output=True, text=True, check=True).stdout.strip()
    environment = build_environment(ROOT, toolchain, {
        'NEWT_DEV_BUILD_ID': head + ('-dirty' if dirty else ''),
        'NEWT_DEV_BUILD_DATE': date})
    if not args.native_only:
        build_archive(ROOT, toolchain, environment, clean=args.clean,
                      package=args.package, features=args.features)
    if not args.rust_only:
        # Derive this image's sections from the same pinned SDK. Align the
        # output section itself so its symbols cover the whole lent range.
        sections_spec = importlib.util.spec_from_file_location('app_wifi_sections', ROOT / 'tools/wifi-sections.py')
        sections = importlib.util.module_from_spec(sections_spec)
        sys.path.insert(0, str(ROOT / 'tools'))
        sections_spec.loader.exec_module(sections)
        linker = ROOT / '.build/rust-app/ld/sections.ld'
        linker.parent.mkdir(parents=True, exist_ok=True)
        linker.write_text(sections.derive(sections.pinned_text(ROOT), force_load=True))
        # Source and patch policy come from this checkout; only installed tools are shared.
        sys.path.insert(0, str(ROOT / 'tools'))
        spec = importlib.util.spec_from_file_location('app_firmware_build', ROOT / 'tools/firmware-build.py')
        native = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(native)
        native_environment = native.build_environment()
        executable = native.require_toolchain()
        subprocess.run([sys.executable, '-B', str(ROOT / 'tools/apply-freeink-patch.py'),
                        '--reader-lane', 'regional-dev-usb'], cwd=ROOT,
                       env=native_environment, check=True)
        subprocess.run([sys.executable, '-B', str(ROOT / 'tools/crt_bundle.py')],
                       cwd=ROOT, env=native_environment, check=True)
        bundle = ROOT / '.build/regional-dev-usb/crt/x509_crt_bundle'
        if hashlib.sha256(bundle.read_bytes()).hexdigest() != CRT_BUNDLE_SHA256:
            raise RuntimeError('pinned existing TLS certificate bundle changed')
        if args.daily:
            config = generated_configuration(ROOT, environment['NEWT_DEV_BUILD_ID'], date)
        else:
            config = configparser.ConfigParser(interpolation=None)
            config.read(ROOT / 'platformio-rust-app.ini')
            config['env:newt_x3']['build_flags'] += (
                '\n' + r'-DNEWT_DEV_BUILD_ID=\"' + environment['NEWT_DEV_BUILD_ID'] + r'\"'
                + '\n' + r'-DNEWT_DEV_BUILD_DATE=\"' + date + r'\"'
                + '\n' + r'-DNEWT_CRT_BUNDLE_PATH=\"' + str(bundle) + r'\"')
        effective = ROOT / (CONFIGURATION if args.daily else '.build/rust-app/platformio.ini')
        effective.parent.mkdir(parents=True, exist_ok=True)
        stream = io.StringIO()
        config.write(stream)
        effective.write_text(stream.getvalue())
        if args.daily:
            require_generated_configuration(ROOT, effective)
        command = [*native.build_command(executable), '--project-dir', str(ROOT),
                   '--project-conf', str(effective)]
        with native.native_toolchain.quiescent(ROOT):
            try:
                if args.clean:
                    subprocess.run([*command, '-t', 'clean'], cwd=ROOT, env=native_environment, check=True)
                subprocess.run(command, cwd=ROOT, env=native_environment, check=True)
            finally:
                native.native_toolchain.restore(ROOT)


if __name__ == '__main__':
    main()
