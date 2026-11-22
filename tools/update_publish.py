#!/usr/bin/env python3
"""Publish a Newt update: `newt.bin` and `newt.manifest`, ready for /Newt/Update/.

One format for every source (docs/083-ota-sd-install.md): the same two files
serve the card, a release and the online check, so nothing here names a
source or a path. Another firmware needs no manifest: its own `.bin` file
goes in /Newt/Update/ as it is. The shipped parser (rust/core/src/update_manifest.rs)
and install checks (rust/core/src/slot_installer.rs) are the authority; this
refuses what they would refuse, and any Newt image that is not a clean build.
Offline: no device, no network.

  update_publish.py newt --out DIR       (this checkout's last clean build)
  update_publish.py newt --image BIN --receipt JSON --out DIR
  update_publish.py newt --candidate DIR --manifest-sha256 HASH --out DIR
"""
from __future__ import annotations
import argparse
import os
from pathlib import Path
import re
import sys

from bench_candidate import VARIANT, Candidate, json_bytes, require, sha, validate_image
from daily_firmware import BUILD_DIRECTORY, IDENTITY_RECEIPT, require_daily_image
import x3_slots
from x3_slots import HEAD, NEWT_BUILD_BYTES, NEWT_IDENTITY_AT, NEWT_VERSION_BYTES

ROOT = Path(__file__).resolve().parents[1]
FORMAT = 'newt-update-1'
# semver.org's grammar in ASCII; tests check it against the device's semver2Valid.
NUMBER = '(?:0|[1-9][0-9]*)'
PRE = f'(?:{NUMBER}|[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
SEMVER2 = re.compile(fr'{NUMBER}\.{NUMBER}\.{NUMBER}(?:-{PRE}(?:\.{PRE})*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?')


def manifest(kind, version, middle, image):
    """The device's exact lines; `middle` is the build= line.

    The longest possible manifest is under 250 bytes, well inside its 512.
    """
    lines = (f'format={FORMAT}', f'kind={kind}', f'version={version}', middle, f'size={len(image)}',
             f'sha256={sha(image)}')
    return ''.join(line + '\n' for line in lines).encode('ascii')


def newt_manifest(image, receipt):
    """A clean build's manifest, from its image and identity receipt."""
    require(isinstance(receipt, dict), 'The identity receipt is not a JSON object')
    require_daily_image(receipt)
    require(receipt.get('dirty') is False, 'The build is dirty: commit the source and rebuild clean')
    require(receipt.get('variant') == VARIANT, f'The receipt is not for the one firmware build ({VARIANT})')
    build, revision = receipt.get('build_id'), receipt.get('git_revision')
    require(isinstance(build, str) and re.fullmatch('[0-9a-f]{12}', build) is not None,
            'The build ID is not exactly 12 lowercase hex')
    require(isinstance(revision, str) and revision[:12] == build, "The build ID is not the receipt's Git revision")
    require(receipt.get('binary_sha256') == sha(image), 'The image differs from its identity receipt')
    version = receipt.get('firmware_version')
    require(isinstance(version, str) and SEMVER2.fullmatch(version) is not None,
            'The firmware version is not SemVer 2.0.0')
    # The identity field keeps a NUL after the version.
    require(len(version) < NEWT_VERSION_BYTES,
            f'The firmware version is longer than {NEWT_VERSION_BYTES - 1} characters')
    # ESP32-C3 structure, checksum, digest, usable slot size and the clean build ID string.
    validate_image(image, build)
    require(x3_slots.classify(image[:HEAD]).kind == 'newt', 'The image lacks the Newt mark')
    # Byte for byte, NUL padding included, as the device's headMatchesManifest compares it.
    identity = image[NEWT_IDENTITY_AT:NEWT_IDENTITY_AT + NEWT_VERSION_BYTES + NEWT_BUILD_BYTES]
    expected = version.encode().ljust(NEWT_VERSION_BYTES, b'\0') + build.encode().ljust(NEWT_BUILD_BYTES, b'\0')
    require(identity == expected, "The image's identity block differs from the receipt's version and build")
    return manifest('newt', version, f'build={build}', image)


def publish(out, kind, image, text):
    """Writes `<kind>.bin`, then `<kind>.manifest`; never replaces either. Returns their SHA-256s."""
    out.mkdir(parents=True, exist_ok=True)
    paths = out / f'{kind}.bin', out / f'{kind}.manifest'
    taken = [path.name for path in paths if os.path.lexists(path)]
    require(not taken, f'{out} already holds {" and ".join(taken)}; publish into a new folder')
    for path, data in zip(paths, (image, text)):
        with path.open('xb') as file:
            file.write(data)
    # Digests of the files as written, not of what was meant to be written.
    return [(path.name, sha(path.read_bytes())) for path in paths]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    kinds = parser.add_subparsers(dest='kind', required=True)
    newt = kinds.add_parser('newt', help="a clean build's image and identity receipt")
    newt.add_argument('--image', type=Path, help=f'default: {BUILD_DIRECTORY}/firmware.bin')
    newt.add_argument('--receipt', type=Path, help=f'default: {IDENTITY_RECEIPT}')
    newt.add_argument('--candidate', type=Path, help='a bench candidate directory (docs/bench-flash.md)')
    newt.add_argument('--manifest-sha256', help="the candidate's reviewed manifest SHA-256")
    newt.add_argument('--out', type=Path, required=True, help='a folder without these two files yet')
    args = parser.parse_args(argv)
    if args.kind == 'newt' and (bool(args.candidate) != bool(args.manifest_sha256) or
                                (args.candidate and (args.image or args.receipt))):
        parser.error('use --candidate with --manifest-sha256, or --image and --receipt')
    try:
        if args.candidate:
            image = Candidate(args.candidate, args.manifest_sha256).image
            receipt = json_bytes((args.candidate / f'firmware-identity-{VARIANT}.json').read_bytes())
            text = newt_manifest(image, receipt)
        else:
            image = (args.image or ROOT / BUILD_DIRECTORY / 'firmware.bin').read_bytes()
            receipt = json_bytes((args.receipt or ROOT / IDENTITY_RECEIPT).read_bytes())
            text = newt_manifest(image, receipt)
        digests = publish(args.out, args.kind, image, text)
    except (OSError, KeyError, TypeError, ValueError) as error:
        print(f'Publish refused: {error}', file=sys.stderr)
        return 1
    print(text.decode(), end='')
    print('Copy newt.bin and newt.manifest to /Newt/Update/ on the card.')
    for name, digest in reversed(digests):
        print(f'{name} sha256 {digest}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
