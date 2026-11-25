"""The one shipping image; bench-only feature omissions never publish."""
import configparser
import hashlib
from pathlib import Path
import re
import shlex

from freeink_sdk_routes import FIRMWARE_BUILD
from native_output import owned_file

IMAGE = 'rust-app'
FEATURES = ('rust-app', 'dev-usb', 'wifi-update', 'embedded-sample')
BUILD_DIRECTORY = f'.pio/{FIRMWARE_BUILD}/build/newt_x3'
IDENTITY_RECEIPT = f'artifacts/firmware-identity-{FIRMWARE_BUILD}.json'
CONFIGURATION = f'.build/{FIRMWARE_BUILD}/platformio.ini'
SOURCE_CONFIGURATION = 'platformio-rust-app.ini'
CONFIGURATION_SHA256 = '096615ecb4a9dfcd2362fedc87f48bd6ec483dcf544d03f45e463676705c4f58'
CRT_BUNDLE_SHA256 = '2af490c0dbd133a23e122f8eb834bf0f083fbcd9e906f4449f60b7e32f926a56'


def require_configuration(root):
    """Changes to dependency routes or build hooks require an explicit review."""
    path = owned_file(Path(root), Path(root) / SOURCE_CONFIGURATION)
    if hashlib.sha256(path.read_bytes()).hexdigest() != CONFIGURATION_SHA256:
        raise ValueError('Daily Rust configuration differs from the reviewed dependency/build scope')


def require_daily_image(receipt):
    """A clean build binds this declaration to the linked image and its inputs."""
    if receipt.get('image') != IMAGE or receipt.get('features') != list(FEATURES):
        raise ValueError('Publication requires the full Rust daily image with DEV USB, Wi-Fi and sample')


def generated_configuration(root, build_id, date, *, wake_timing=False):
    """Only output paths, source identity and the pinned TLS input are generated."""
    root = Path(root)
    require_configuration(root)
    if (not re.fullmatch(r'[0-9a-f]{12}(?:-dirty)?', build_id)
            or not re.fullmatch(r'[0-9]{4}-[0-9]{2}-[0-9]{2}', date)):
        raise ValueError('Development identifier differs from the approved generated format')
    bundle = owned_file(root, root / '.build/regional-dev-usb/crt/x509_crt_bundle')
    if hashlib.sha256(bundle.read_bytes()).hexdigest() != CRT_BUNDLE_SHA256:
        raise ValueError('Pinned existing TLS certificate bundle changed')
    config = configparser.ConfigParser(interpolation=None)
    config.read(root / SOURCE_CONFIGURATION)
    build = Path(BUILD_DIRECTORY)
    config['platformio'].update(build_dir=build.parent.as_posix(),
                               libdeps_dir=(build.parents[1] / 'libdeps').as_posix())
    config['env:newt_x3']['build_flags'] += (
        '\n' + r'-DNEWT_DEV_BUILD_ID=\"' + build_id + r'\"'
        + '\n' + r'-DNEWT_DEV_BUILD_DATE=\"' + date + r'\"'
        + '\n' + r'-DNEWT_CRT_BUNDLE_PATH=\"' + str(bundle) + r'\"')
    if wake_timing:
        config['env:newt_x3']['build_flags'] += '\n-DNEWT_WAKE_TIMING=1'
    return config


def require_generated_configuration(root, path):
    config = configparser.ConfigParser(interpolation=None)
    config.read(owned_file(Path(root), Path(path)))
    flags = shlex.split(config['env:newt_x3']['build_flags'])
    identifiers = []
    for name in ('NEWT_DEV_BUILD_ID', 'NEWT_DEV_BUILD_DATE'):
        found = [flag for flag in flags if flag.startswith('-D' + name + '=')]
        if len(found) != 1:
            raise ValueError('Exactly one generated build ID and date are required')
        identifiers.append(found[0].removeprefix('-D' + name + '=').removeprefix('"').removesuffix('"'))
    diagnostic = [flag for flag in flags if flag.startswith('-DNEWT_WAKE_TIMING')]
    if diagnostic not in ([], ['-DNEWT_WAKE_TIMING=1']):
        raise ValueError('Wake timing requires exactly the opt-in diagnostic define')
    expected = generated_configuration(root, *identifiers, wake_timing=bool(diagnostic))
    if {section: dict(config[section]) for section in config} != {
            section: dict(expected[section]) for section in expected}:
        raise ValueError('Generated daily configuration exceeds the reviewed build scope')
