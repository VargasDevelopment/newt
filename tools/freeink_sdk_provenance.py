"""Canonical, read-only verification for composed FreeInk SDK lane copies."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

from freeink_sdk_routes import reader_patch_order, reader_sdk, reader_sdk_provenance
from freeink_source_pin import SDK_ORIGIN, SDK_PIN, git_executable, source_git, verify_source_pin


def checked(condition, message):
    if not condition:
        raise RuntimeError(message)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def digest_bytes(contents):
    return hashlib.sha256(contents).hexdigest()


def tree_digest_hashes(hashes):
    digestor = hashlib.sha256()
    for name in sorted(hashes):
        digestor.update(name.encode() + b'\0')
        digestor.update(bytes.fromhex(hashes[name]))
    return digestor.hexdigest()


def tree_hashes(directory):
    directory, hashes = Path(directory), {}
    checked(not directory.is_symlink() and directory.is_dir(), 'SDK tree must be a real directory')
    for path in sorted(directory.rglob('*')):
        checked(not path.is_symlink() and (path.is_dir() or path.is_file()),
                f'SDK copy holds a link or special file: {path.relative_to(directory)}')
        if path.is_file():
            hashes[path.relative_to(directory).as_posix()] = digest(path)
    return hashes


def tree_structure(directory):
    """Git-representable file kinds, executable bits and exact directories."""
    directory, structure = Path(directory), {}
    checked(not directory.is_symlink() and directory.is_dir(), 'SDK tree must be a real directory')
    for path in sorted(directory.rglob('*')):
        name = path.relative_to(directory).as_posix()
        checked(not path.is_symlink(), f'SDK tree holds a link: {name}')
        if path.is_dir():
            structure[name] = 'directory'
        elif path.is_file():
            structure[name] = 'file-executable' if git_executable(path.stat().st_mode) else 'file'
        else:
            checked(False, f'SDK tree holds a special file: {name}')
    return structure


def structure_digest(structure):
    digestor = hashlib.sha256()
    for name in sorted(structure):
        digestor.update(name.encode() + b'\0' + structure[name].encode() + b'\0')
    return digestor.hexdigest()


def named_hashes(directory, names):
    directory, hashes = Path(directory), {}
    checked(not directory.is_symlink() and directory.is_dir(), 'SDK source must be a real directory')
    for name in names:
        path = directory / name
        checked(not path.is_symlink() and path.is_file(), f'Missing or linked SDK source file: {name}')
        hashes[name] = digest(path)
    return hashes


def committed_structure(source):
    """Model blobs and the directories that `git archive` emits for HEAD."""
    listing = source_git(source, 'ls-tree', '-r', '-t', '--full-tree', '-z', 'HEAD')
    names, structure = [], {}
    for entry in filter(None, listing.split(b'\0')):
        metadata, relative = entry.split(b'\t', 1)
        mode, kind, _ = metadata.split()
        name = relative.decode('utf-8')
        path = Path(name)
        checked(not path.is_absolute() and '..' not in path.parts, f'Unsafe pinned SDK path: {name}')
        for parent in path.parents:
            if parent != Path('.'):
                structure[parent.as_posix()] = 'directory'
        if kind == b'blob':
            checked(mode in (b'100644', b'100755'), f'Unsupported pinned SDK blob mode: {name}')
            names.append(name)
            structure[name] = 'file-executable' if mode == b'100755' else 'file'
        elif kind == b'commit':
            checked(mode == b'160000', f'Unsupported pinned SDK gitlink mode: {name}')
            structure[name] = 'directory'
        elif kind == b'tree':
            checked(mode == b'040000', f'Unsupported pinned SDK tree mode: {name}')
            structure[name] = 'directory'
        else:
            checked(False, f'Unsupported pinned SDK entry type: {name}')
    return tuple(names), structure


def patch_inputs(root, name):
    checked(name in ('regional', 'producer'), f'Unknown FreeInk patch: {name}')
    stem = 'freeink-regional-gray' if name == 'regional' else 'freeink-producer-policy'
    return root / 'patches' / (stem + '.patch'), root / 'patches' / (stem + '.json')


def validate_reader_copy(root, variant, source=None):
    """Derive every expected source, intermediate and final state from the pin."""
    root = Path(root)
    source = Path(source) if source is not None else root / 'vendor/freeink-sdk'
    checked(verify_source_pin(source) == SDK_PIN, 'FreeInk source differs from the reviewed pin')
    target = root / reader_sdk(variant)
    receipt_path = root / reader_sdk_provenance(variant)
    receipt = json.loads(receipt_path.read_text())
    checked(isinstance(receipt, dict), 'Reader SDK provenance must be an object')
    order, specs = reader_patch_order(variant), []
    for name in order:
        patch, manifest_path = patch_inputs(root, name)
        patch_bytes, manifest_bytes = patch.read_bytes(), manifest_path.read_bytes()
        manifest = json.loads(manifest_bytes)
        checked(isinstance(manifest, dict), f'Patch manifest must be an object: {name}')
        checked(manifest.get('source_commit') == SDK_PIN,
                f'Patch manifest source differs from reviewed pin: {name}')
        patch_sha = digest_bytes(patch_bytes)
        checked(patch_sha == manifest.get('patch_sha256'),
                f'Patch content differs from reviewed manifest: {name}')
        specs.append((name, patch, manifest_path, manifest, patch_bytes, patch_sha,
                      digest_bytes(manifest_bytes)))
    names, canonical_structure = committed_structure(source)
    canonical = named_hashes(source, names)
    source_tree, source_structure = tree_digest_hashes(canonical), structure_digest(canonical_structure)
    actual, actual_structure = tree_hashes(target), tree_structure(target)
    checked(set(actual) == set(canonical), 'Reader SDK inventory differs from the pinned source')
    checked(actual_structure == canonical_structure,
            'Reader SDK structure differs from the pinned archive')
    checked(receipt.get('schema') == 2 and receipt.get('lane') == variant,
            'Reader SDK provenance identifies the wrong lane')
    checked(receipt.get('destination') == target.relative_to(root).as_posix(),
            'Reader SDK provenance identifies the wrong destination')
    checked(receipt.get('source_commit') == SDK_PIN,
            'Reader SDK source commit differs from the reviewed pin')
    checked(receipt.get('source_origin') == SDK_ORIGIN,
            'Reader SDK source origin differs from official FreeInk')
    checked(receipt.get('source_tree_sha256') == source_tree,
            'Reader SDK source tree differs from the canonical pin')
    checked(receipt.get('source_structure_sha256') == source_structure,
            'Reader SDK source structure differs from the canonical pin')
    patch_receipts = receipt.get('patches')
    checked(isinstance(patch_receipts, list) and all(isinstance(item, dict) for item in patch_receipts),
            'Reader SDK patch provenance must be a list of objects')
    checked(tuple(item.get('name') for item in patch_receipts) == order,
            'Reader SDK patch order differs from the required composition')
    with tempfile.TemporaryDirectory(prefix='newt-freeink-validate-') as temporary:
        temporary, staging = Path(temporary), Path(temporary) / 'sdk'
        staging.mkdir()
        for name, kind in canonical_structure.items():
            if kind == 'directory':
                (staging / name).mkdir(parents=True, exist_ok=True)
        for name in names:
            relative = Path(name)
            destination = staging / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source / relative, destination)
            destination.chmod(0o755 if canonical_structure[name] == 'file-executable' else 0o644)
        checked(tree_hashes(staging) == canonical, 'Staged SDK source differs from the canonical pin')
        checked(tree_structure(staging) == canonical_structure,
                'Staged SDK structure differs from the canonical archive')
        expected = dict(canonical)
        for index, (item, spec) in enumerate(zip(patch_receipts, specs)):
            name, patch, manifest_path, declared, patch_bytes, patch_sha, manifest_sha = spec
            input_tree = tree_digest_hashes(expected)
            checked(item.get('manifest') == manifest_path.relative_to(root).as_posix()
                    and item.get('manifest_sha256') == manifest_sha
                    and item.get('patch') == patch.relative_to(root).as_posix()
                    and item.get('patch_sha256') == patch_sha,
                    f'Reader SDK provenance changed for {name}')
            checked(isinstance(declared.get('license'), str) and item.get('license') == declared['license'],
                    f'Reader SDK license declaration differs for {name}')
            declared_files = declared.get('files')
            checked(isinstance(declared_files, dict) and isinstance(item.get('files'), dict),
                    f'Reader SDK file receipt differs for {name}')
            patch_input = temporary / f'{index}-{name}.patch'
            patch_input.write_bytes(patch_bytes)
            subprocess.run(['/usr/bin/patch', '--batch', '--forward', '-p1', '-i', str(patch_input)],
                           cwd=staging, check=True, capture_output=True, text=True,
                           env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C', 'TMPDIR': str(temporary)})
            result = tree_hashes(staging)
            result_structure = tree_structure(staging)
            checked(set(result) == set(canonical), f'Reader SDK inventory changed after {name}')
            checked(result_structure == canonical_structure,
                    f'Reader SDK structure changed after {name}')
            changed = {path for path in expected if expected[path] != result[path]}
            checked(set(declared_files) == changed and set(item['files']) == changed,
                    f'Reader SDK manifest file set differs from applied patch: {name}')
            for path in changed:
                hashes = declared_files[path]
                checked(Path(path).parts[0] == 'libs' and '..' not in Path(path).parts,
                        f'Unsafe reader SDK manifest path: {path}')
                checked(hashes == {'source_sha256': expected[path], 'patched_sha256': result[path]},
                        f'Reader SDK manifest hashes differ from applied patch: {name}: {path}')
                checked(item['files'][path] == {'input_sha256': expected[path],
                                                'result_sha256': result[path]},
                        f'Reader SDK file receipt differs from applied patch: {name}: {path}')
            checked(result.get('LICENSE') == canonical.get('LICENSE'),
                    f'Reader SDK license changed after {name}')
            result_tree = tree_digest_hashes(result)
            checked(item.get('input_tree_sha256') == input_tree,
                    f'Reader SDK derived input tree differs for {name}')
            checked(item.get('result_tree_sha256') == result_tree,
                    f'Reader SDK derived result tree differs for {name}')
            checked(item.get('input_structure_sha256') == source_structure
                    and item.get('result_structure_sha256') == source_structure,
                    f'Reader SDK structural provenance differs for {name}')
            expected = result
    final_tree = tree_digest_hashes(expected)
    checked(receipt.get('patched_tree_sha256') == final_tree,
            'Reader SDK final tree differs from ordered patches')
    checked(receipt.get('patched_structure_sha256') == source_structure,
            'Reader SDK final structure differs from the canonical archive')
    checked(actual == expected, 'Reader SDK file content differs from the canonical composed tree')
    checked(receipt.get('license_sha256') == canonical.get('LICENSE')
            and actual.get('LICENSE') == canonical.get('LICENSE'),
            'Reader SDK license differs from the canonical pinned license')
    checked(verify_source_pin(source) == SDK_PIN, 'FreeInk source changed during SDK validation')
    return receipt
