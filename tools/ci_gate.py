"""Fresh canonical software gates for one committed source; never device proof."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import time

import ci_persist
from code_health import read_baseline
from code_health_resources import check_resources
from daily_firmware import require_daily_image
from freeink_sdk_routes import FIRMWARE_BUILD
import gate_tier
import host_jobs
from native_output import owned_directory, owned_file, write_text
from release_tools import source_identity
from trusted_host import SDK_PIN, verify_source_pin

ROOT = Path(__file__).resolve().parents[1]
UI_REVIEW_AUTHORITY = 'artifacts/library-ui-review.json'


def committed_ui_authority(root):
    """Generated receipts are excluded, but reviewed golden authority is source."""
    listing = subprocess.run(['git', 'ls-tree', '-z', 'HEAD', '--', UI_REVIEW_AUTHORITY],
                             cwd=root, check=True, capture_output=True).stdout
    entries = list(filter(None, listing.split(b'\0')))
    if len(entries) != 1:
        raise RuntimeError('Missing committed reviewed UI authority')
    metadata, relative = entries[0].split(b'\t', 1)
    mode, kind, blob = metadata.split()
    if relative.decode() != UI_REVIEW_AUTHORITY or mode != b'100644' or kind != b'blob':
        raise RuntimeError('Unsupported committed reviewed UI authority file type')
    path = owned_file(root, root / UI_REVIEW_AUTHORITY)
    data = path.read_bytes()
    actual = hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()
    if actual != blob.decode() or stat.S_IMODE(path.stat().st_mode) & 0o111:
        raise RuntimeError('Reviewed UI authority bytes/mode differ from requested commit')
    return hashlib.sha256(data).hexdigest()


def committed_source(root, expected):
    if not re.fullmatch('[0-9a-f]{40}', expected):
        raise RuntimeError('CI requires an explicit complete commit SHA')
    identity = source_identity(root)
    if identity['git_revision'] != expected or identity['dirty']:
        raise RuntimeError('CI source differs from the requested clean commit')
    listing = subprocess.run(['git', 'ls-tree', '-r', '--full-tree', '-z', 'HEAD'], cwd=root,
                             check=True, capture_output=True).stdout
    digest = hashlib.sha256()
    for entry in filter(None, listing.split(b'\0')):
        metadata, relative = entry.split(b'\t', 1)
        mode, kind, blob = metadata.split()
        if kind == b'commit':
            if relative != b'vendor/freeink-sdk' or blob.decode() != SDK_PIN:
                raise RuntimeError('Unexpected CI source gitlink')
            continue
        if relative.startswith(b'artifacts/'):
            continue  # Generated receipts cannot confer admission; all gates rerun.
        if mode not in (b'100644', b'100755') or kind != b'blob':
            raise RuntimeError('Unsupported committed source file type')
        path = owned_file(root, root / relative.decode())
        data = path.read_bytes()
        actual = hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()
        executable = bool(stat.S_IMODE(path.stat().st_mode) & 0o111)
        if actual != blob.decode() or executable != (mode == b'100755'):
            raise RuntimeError('CI source bytes/mode differ from requested commit')
        digest.update(relative + b'\0' + bytes.fromhex(actual))
    verify_source_pin(root / 'vendor/freeink-sdk')
    return {**identity, 'tracked_source_sha256': digest.hexdigest(), 'sdk_revision': SDK_PIN,
            'reviewed_ui_manifest_sha256': committed_ui_authority(root)}


def commands(root, python, tier, part='all'):
    """part: 'all', the self-hosted job's whole sequence; 'native', the firmware
    builds alone; or 'I/N', shard I of N of the host gate (tools/gate_tier.py),
    one of the public workflow's parallel jobs."""
    prefix = [str(python), '-B']
    # The job's own budget covers the host gate; check.py's is for an idle 16-thread host.
    result = [('host', [*prefix, str(root / 'tools/check.py'), '--tier', tier, '--no-time-budget',
                        *(['--fresh-objects'] if tier == 'full' else []),
                        '-j', str(host_jobs.available_cpus()[0])]),
              # A plain compile makes the platform's first-use package edits
              # before any resource identity is captured. No resource proof.
              ('native-preparation', [*prefix, str(root / 'tools/firmware-build.py'), '--clean'])]
    if sys.platform == 'linux':
        result[0][1].extend(['--profile', 'linux'])
    result.append(('native-' + FIRMWARE_BUILD,
                   [*prefix, str(root / 'tools/firmware-build.py'), '--clean', '--health-report']))
    if part == 'native':
        return result[1:]
    if part != 'all':
        result[0][1].extend(['--shard', part])
        return result[:1]
    return result


def validate_native(root, expected):
    baseline = read_baseline(root)
    version = source_identity(root)['firmware_version']
    variant = FIRMWARE_BUILD
    failures = check_resources(root, baseline, variant)
    if failures:
        raise RuntimeError('CI final native resource validation failed: ' + '; '.join(failures))
    receipt = json.loads(owned_file(root, root / f'artifacts/firmware-identity-{variant}.json').read_text())
    require_daily_image(receipt)
    resource = json.loads(owned_file(root, root / f'artifacts/code-health-{variant}.json').read_text())
    image = f'.pio/{variant}/build/newt_x3/firmware.bin'
    if (receipt['git_revision'] != expected or receipt['dirty'] is not False
            or receipt['variant'] != variant or receipt['firmware_version'] != version
            or receipt['resource_identity'] != resource['identity']
            or receipt['binary_sha256'] != resource['artifacts'][image]):
        raise RuntimeError('CI native release identity differs from exact committed source')


def run_gate(root, expected, store=None, started=None, part='all'):
    started = time.time() if started is None else started
    tier = gate_tier.ci_tier(root, expected)
    print(f'TIER {tier}', flush=True)
    directory = owned_directory(root, root / '.build/ci-gate', create=True)
    report = owned_file(root, directory / 'report.json')
    report.unlink(missing_ok=True)
    before = committed_source(root, expected)
    if store is not None:
        ci_persist.restore_objects(root, store, tier)
    try:
        phases = run_phases(root, expected, tier, part)
    finally:
        if store is not None:
            ci_persist.save_objects(root, store)
    # A part is one job, so its tier's whole-job budget applies to it.
    gate_tier.check_time_budget(f'CI {tier} tier job' + ('' if part == 'all' else f' part {part}'),
                                time.time() - started, gate_tier.time_budget(tier, 'ci'),
                                {phase['phase']: {'wall_seconds': phase['seconds']} for phase in phases})
    if part in ('all', 'native'):
        validate_native(root, expected)
    after = committed_source(root, expected)
    if before != after:
        raise RuntimeError('Committed source changed during CI gates')
    write_text(root, report, json.dumps({'purpose': 'Fresh software diagnostics; no reusable/device proof',
               'source': after, 'tier': tier, 'part': part, 'phases': phases, 'reusable_proof': False,
               'coverage': {'profile': 'linux' if sys.platform == 'linux' else 'canonical',
                            'reviewed_ui_frames': 20,  # tests/system_ui_contract_test.py on the host-built firmware
                            'complete_software_coverage': sys.platform != 'linux',
                            'required_mac_imageio_gate': 'pending separate exact-source run' if sys.platform == 'linux' else 'included',
                            'fixture_decoder_scope': 'four SHA-locked authored PNGs; no arbitrary image decode equivalence'},
               'hardware': 'not-tested', 'ui_approval': 'separate', 'owner': 'pending'}, indent=2) + '\n')
    if part != 'all':
        print(f'PASS {tier} tier part {part} for {expected}; the run passes only when every part does', flush=True)
    elif sys.platform == 'linux':
        print(f'PASS {tier} tier Linux software checks for {expected}; required exact-source Mac ImageIO coverage remains separate', flush=True)
    else:
        print(f'PASS {tier} tier software gates for {expected}; physical acceptance remains separate', flush=True)


def run_phases(root, expected, tier, part='all'):
    phases = []
    for name, command in commands(root, sys.executable, tier, part):
        started = time.perf_counter()
        print(f'START CI {name}', flush=True)
        # UBSan recovers by default; a printed diagnostic must fail admission.
        environment = {**os.environ, 'UBSAN_OPTIONS': 'halt_on_error=1',
                       'ASAN_OPTIONS': 'halt_on_error=1:abort_on_error=1'}
        result = subprocess.run(command, cwd=root, env=environment)
        seconds = round(time.perf_counter() - started, 3)
        phases.append({'phase': name, 'command': command, 'returncode': result.returncode, 'seconds': seconds})
        print(f'{"PASS" if result.returncode == 0 else "FAIL"} CI {name} ({seconds:.3f}s)', flush=True)
        if result.returncode:
            raise subprocess.CalledProcessError(result.returncode, command)
    return phases


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--expected-commit', required=True)
    parser.add_argument('--part', default='all', help="all (default), native, or a host gate shard I/N")
    args = parser.parse_args()
    if args.part not in ('all', 'native') and not re.fullmatch(r'(\d+)/(\d+)', args.part):
        parser.error('--part needs all, native or I/N')
    # The workflow's first step records when the job started, so the budget covers its setup too.
    started = float(os.environ['NEWT_JOB_STARTED']) if os.environ.get('NEWT_JOB_STARTED') else None
    # Only CI keeps objects between runs; a gate run by hand starts from none.
    store = ci_persist.STORE if os.environ.get('GITHUB_ACTIONS') == 'true' else None
    run_gate(ROOT, args.expected_commit, store, started, args.part)


if __name__ == '__main__':
    main()
