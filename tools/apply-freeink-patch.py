#!/usr/bin/env python3
"""Build a hash-verified experimental SDK copy; never edit the pinned vendor tree."""
import argparse
from pathlib import Path
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
sys.pycache_prefix = None

from native_output import owned_directory, owned_file, write_text
from freeink_sdk_routes import READER_VARIANTS, reader_patch_order, reader_sdk
from freeink_sdk_provenance import (structure_digest, tree_structure,
                                    validate_reader_copy as validate_composed_reader_copy)
from freeink_source_pin import SDK_ORIGIN, SDK_PIN, source_git, verify_source_pin

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'vendor/freeink-sdk'
PATCH = ROOT / 'patches/freeink-regional-gray.patch'
MANIFEST = ROOT / 'patches/freeink-regional-gray.json'
BUILD = ROOT / '.build'
DESTINATION = BUILD / 'freeink-sdk'
# Legacy regional-only destinations retained for the existing host tests.
SUFFIXES = {'regional': '', 'regional-dev-usb': '-dev-usb'}
PRODUCER_DESTINATION = BUILD / 'freeink-producer-sdk'
PATCH_NAMES = ('regional', 'producer')


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def tree_digest_hashes(hashes):
    digestor = hashlib.sha256()
    for name in sorted(hashes):
        digestor.update(name.encode() + b'\0')
        digestor.update(bytes.fromhex(hashes[name]))
    return digestor.hexdigest()


def tree_hashes(directory):
    hashes = {}
    checked(not directory.is_symlink() and directory.is_dir(), 'SDK tree must be a real directory')
    for path in sorted(directory.rglob('*')):
        checked(not path.is_symlink() and (path.is_dir() or path.is_file()),
                f'SDK copy holds a link or special file: {path.relative_to(directory)}')
        if path.is_file():
            hashes[path.relative_to(directory).as_posix()] = digest(path)
    return hashes


def named_hashes(directory, names):
    hashes = {}
    checked(not directory.is_symlink() and directory.is_dir(), 'SDK source must be a real directory')
    for name in names:
        path = directory / name
        checked(not path.is_symlink() and path.is_file(), f'Missing or linked SDK source file: {name}')
        hashes[name] = digest(path)
    return hashes


def tree_digest(directory):
    return tree_digest_hashes(tree_hashes(directory))


def checked(condition, message):
    if not condition:
        raise RuntimeError(message)


def git(*args):
    return source_git(SOURCE, *args, text=True).strip()


def destination(variant='regional'):
    """Each regional variant compiles its own copy, so both can build at once."""
    return DESTINATION.with_name(DESTINATION.name + SUFFIXES[variant])


def reader_destination(variant):
    return ROOT / reader_sdk(variant)


def patch_inputs(name, legacy_regional=False):
    checked(name in PATCH_NAMES, f'Unknown FreeInk patch: {name}')
    if name == 'regional' and legacy_regional:
        return PATCH, MANIFEST
    stem = 'freeink-regional-gray' if name == 'regional' else 'freeink-producer-policy'
    return ROOT / 'patches' / (stem + '.patch'), ROOT / 'patches' / (stem + '.json')


def require_build_destination(target=None):
    target = target or DESTINATION
    owned_directory(ROOT, BUILD)
    checked(not BUILD.is_symlink(), 'Build root must not be a symlink')
    checked(BUILD.is_dir() and BUILD.resolve() == ROOT / '.build',
            'Build root must be a real project-local directory')
    checked(not target.is_symlink() and target.resolve().parent == BUILD.resolve(),
            'Build destination must remain directly beneath the build root')
    checked(not target.exists() or target.is_dir(), 'Build destination must be a directory')


def tracked_files():
    tracked = source_git(SOURCE, 'ls-files', '-z').decode().split('\0')
    return tuple(path for path in tracked if path and (SOURCE / path).is_file())


def validate_source(manifests):
    commits = {manifest['source_commit'] for manifest in manifests}
    checked(commits == {SDK_PIN} and verify_source_pin(SOURCE) == SDK_PIN,
            'SDK differs from declared official pin')


def apply_copy(target, named_patches, *, lane=None, legacy=False):
    specs = []
    for name, patch, manifest_path in named_patches:
        manifest = json.loads(manifest_path.read_text())
        checked(isinstance(manifest.get('license'), str) or legacy,
                f'Patch manifest omits license provenance: {manifest_path.name}')
        checked(digest(patch) == manifest['patch_sha256'], 'Patch content differs from reviewed manifest')
        specs.append((name, patch, manifest_path, manifest))
    validate_source([spec[3] for spec in specs])
    owned_directory(ROOT, BUILD, create=True)
    require_build_destination(target)
    provenance = owned_file(ROOT, BUILD / (target.name + '.provenance.json'))
    with tempfile.TemporaryDirectory(prefix='.freeink-patch-', dir=BUILD) as temp:
        staging = Path(temp) / 'sdk'
        staging.mkdir()
        archive = Path(temp) / 'sdk.tar'
        archive.write_bytes(source_git(SOURCE, 'archive', '--format=tar', 'HEAD'))
        subprocess.run(['tar', '-xf', str(archive), '-C', str(staging)], check=True)
        files = tracked_files()
        expected = set(files)
        checked({str(path.relative_to(staging)) for path in staging.rglob('*') if path.is_file()} == expected,
                'SDK copy has unexpected or missing files')
        source_hash = tree_digest(staging)
        source_structure = structure_digest(tree_structure(staging))
        patch_receipts = []
        for name, patch, manifest_path, manifest in specs:
            before = {path: digest(staging / path) for path in files}
            for path, hashes in manifest['files'].items():
                checked(Path(path).parts[0] == 'libs' and '..' not in Path(path).parts,
                        'Unsafe manifest path')
                checked(before.get(path) == hashes['source_sha256'],
                        f'Ordered patch input hash differs: {name}: {path}')
            input_tree = tree_digest(staging)
            input_structure = structure_digest(tree_structure(staging))
            subprocess.run(['patch', '--batch', '--forward', '-p1', '-i', str(patch)], cwd=staging,
                           check=True, capture_output=True, text=True)
            after = {path: digest(staging / path) for path in files}
            for path, hashes in manifest['files'].items():
                checked(after.get(path) == hashes['patched_sha256'],
                        f'Patched hash differs: {name}: {path}')
            for path in set(files) - set(manifest['files']):
                checked(after[path] == before[path], f'Unexpected copy mutation after {name}: {path}')
            checked({str(path.relative_to(staging)) for path in staging.rglob('*') if path.is_file()} == expected,
                    f'SDK inventory changed after {name}')
            result_structure = structure_digest(tree_structure(staging))
            checked(result_structure == source_structure, f'SDK structure changed after {name}')
            patch_receipts.append({
                'name': name,
                'manifest': manifest_path.relative_to(ROOT).as_posix(),
                'manifest_sha256': digest(manifest_path),
                'patch': patch.relative_to(ROOT).as_posix(),
                'patch_sha256': manifest['patch_sha256'],
                'input_tree_sha256': input_tree,
                'result_tree_sha256': tree_digest(staging),
                'input_structure_sha256': input_structure,
                'result_structure_sha256': result_structure,
                'files': {path: {'input_sha256': before[path], 'result_sha256': after[path]}
                          for path in sorted(manifest['files'])},
                'license': manifest.get('license'),
            })
        final_hash = tree_digest(staging)
        receipt = {
            'schema': 2,
            'source_commit': specs[0][3]['source_commit'],
            'source_origin': SDK_ORIGIN,
            'source_tree_sha256': source_hash,
            'source_structure_sha256': source_structure,
            'license_sha256': digest(staging / 'LICENSE'),
            'destination': (str(target) if legacy else target.relative_to(ROOT).as_posix()),
            'patches': patch_receipts,
            'patched_tree_sha256': final_hash,
            'patched_structure_sha256': structure_digest(tree_structure(staging)),
        }
        if lane is not None:
            receipt['lane'] = lane
        if legacy and len(specs) == 1:
            # Preserve the original single-patch receipt interface for the host
            # probes while recording the stronger ordered receipt alongside it.
            receipt.update(specs[0][3])
        require_build_destination(target)
        owned_file(ROOT, provenance)
        text = json.dumps(receipt, indent=2) + '\n'
        # An exact copy stays in place, so gate phases can read one shared
        # copy at once while each of them still verifies it (tools/gate_plan.py).
        if (target.is_dir() and provenance.is_file() and provenance.read_text() == text
                and tree_digest(target) == final_hash
                and structure_digest(tree_structure(target)) == receipt['patched_structure_sha256']):
            return receipt
        if target.exists():
            shutil.rmtree(target)
        os.replace(staging, target)
        write_text(ROOT, provenance, text)
    return receipt


def validate_reader_copy(variant):
    return validate_composed_reader_copy(ROOT, variant, SOURCE)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--variant', choices=tuple(SUFFIXES), default='regional')
    mode.add_argument('--producer-policy', action='store_true')
    mode.add_argument('--reader-lane', choices=READER_VARIANTS)
    args = parser.parse_args(argv)
    if args.reader_lane:
        order = reader_patch_order(args.reader_lane)
        target = reader_destination(args.reader_lane)
        named = [(name, *patch_inputs(name)) for name in order]
        receipt = apply_copy(target, named, lane=args.reader_lane)
        validate_reader_copy(args.reader_lane)
        print(f'SDK lane {args.reader_lane}: {target.relative_to(ROOT)}; patches: {" -> ".join(order)}')
    else:
        name = 'producer' if args.producer_policy else 'regional'
        target = PRODUCER_DESTINATION if args.producer_policy else destination(args.variant)
        patch, manifest = patch_inputs(name, legacy_regional=not args.producer_policy)
        receipt = apply_copy(target, [(name, patch, manifest)], legacy=True)
    print(f'PASS: official SDK unchanged; verified experimental copy {target}')
    print(f'Patched tree SHA256: {receipt["patched_tree_sha256"]}')


if __name__ == '__main__':
    main()
