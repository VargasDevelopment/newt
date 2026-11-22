#!/usr/bin/env python3
"""Machine-derived reading admission; journey explicitly connects the selected port."""
import os
import sys

sys.dont_write_bytecode = True
sys.pycache_prefix = None
os.environ.pop("PYTHONPYCACHEPREFIX", None)

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import time

from code_health import read_baseline
from code_health_resources import build_directory, check_resources
from freeink_sdk_routes import FIRMWARE_BUILD
from host_common import ROOT
import rust_preview
from reading_acceptance import load, procedure_identity, validate_install, validate_plan, validate_journey, write_journey
from system_ui_contract import render_frames, verify_reviewed


def latency_bundle(receipt, install, plan, expected, target, variant, conditions):
    """Current authority comes from verified native outputs, never a journey claim."""
    from release_tools import source_identity
    resource = load(ROOT / f'artifacts/code-health-{variant}.json')
    native = load(ROOT / f'artifacts/firmware-identity-{variant}.json')
    source = source_identity(ROOT)
    if (set(native) != {'firmware_version', 'git_revision', 'dirty', 'variant',
                       'binary_sha256', 'resource_identity'} or source['dirty'] or
            {key: native[key] for key in source} != source or
            native['variant'] != variant or native['binary_sha256'] != expected['binary_sha256'] or
            native['resource_identity'] != resource['identity'] or
            resource['identity']['inputs_sha256'] != expected['inputs_sha256']):
        raise ValueError('Latency requires current clean native source/image identity')
    return {'journey': receipt, 'install': install, 'plan': plan, 'expected': expected,
            'target': target, 'procedure_sha256': procedure_identity(ROOT),
            'source_revision': source['git_revision'], 'image_bytes': resource['flash_bytes'],
            'mode': 'physical-usb-serial', 'conditions': conditions}


def private_json(destination, value):
    """An explicit private output is exclusive; no approval or baseline overwrite."""
    destination = destination.resolve()
    if not destination.is_relative_to((ROOT / '.build').resolve()):
        raise ValueError('Private latency output must stay below ignored .build')
    destination.parent.mkdir(parents=True, mode=0o700, exist_ok=True)
    descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as output:
        json.dump(value, output, indent=2, sort_keys=True)
        output.write('\n')


def latency_admission(receipt, install, plan, expected, args):
    """Separate mandatory serial timing evaluation; physical goals stay separate."""
    from reading_latency import evaluate_latency
    if not args.latency_conditions:
        raise ValueError('Device latency requires explicit owner-observed --latency-conditions')
    current = latency_bundle(receipt, install, plan, expected, args.target,
                             args.variant, load(args.latency_conditions))
    if args.latency_bundle:
        private_json(args.latency_bundle, current)
    baseline = None
    if args.latency_baseline:
        with args.latency_baseline.open('rb') as stream:
            baseline = stream.read(1024 * 1024 + 1)
        if len(baseline) > 1024 * 1024:
            raise ValueError('Latency baseline exceeds evidence limit')
    report = evaluate_latency(current, load(ROOT / 'docs/reading-latency-policy.json'),
                              (ROOT / 'docs/reading-latency-contract.json').read_bytes(),
                              time.time(), baseline)
    if args.latency_report:
        private_json(args.latency_report, report)
    return report


def latency_summary(report):
    # Raw actions, request identities, private conditions and bundle bindings stay
    # in the explicitly requested private files. These statuses confer no optics.
    return {'state': report['state'], 'reasons': report['reasons'],
            'software_serial_measurement': True, 'physical_optical_measurement': False,
            'required_groups': sum(group['required'] for group in report['groups']),
            'provisional_groups': sum(not group['required'] for group in report['groups'])}


def current_build(variant):
    failures = check_resources(ROOT, read_baseline(ROOT), variant)
    if failures:
        raise ValueError('\n'.join(failures))
    report = load(ROOT / f'artifacts/code-health-{variant}.json')
    binary = build_directory(ROOT, variant) / 'firmware.bin'
    config = (ROOT / '.build' / variant / 'platformio.ini').read_text()
    matches = re.findall(r'NEWT_DEV_BUILD_ID=\\?"([^"\s\\]+)\\?"', config)
    if len(matches) != 1:
        raise ValueError('Missing exact installed HELLO identity in verified native configuration')
    return {'build': matches[0], 'binary_sha256': report['artifacts'][binary.relative_to(ROOT).as_posix()],
            'inputs_sha256': report['identity']['inputs_sha256']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('state', choices=['software-verified', 'installable', 'journey', 'hardware-validated', 'owner-accepted'])
    parser.add_argument('--variant', choices=[FIRMWARE_BUILD])
    parser.add_argument('--install', type=Path)
    parser.add_argument('--journey', type=Path)
    parser.add_argument('--plan', type=Path, help='Private admission plan; never commit private book identities')
    parser.add_argument('--target', help='Explicit installation/device alias')
    parser.add_argument('--ui-review', type=Path)
    parser.add_argument('--owner-review', type=Path)
    parser.add_argument('--port', help='explicit local serial port; journey only, never discovered')
    parser.add_argument('--output', type=Path, help='private journey receipt below project .build')
    parser.add_argument('--latency-cycles', type=int, default=0,
                        help='journey only: explicitly sample the observed adjacent same-spine pair 20..100 times; 0 records provisional timing')
    parser.add_argument('--latency-conditions', type=Path, help='private explicit owner-observed comparison conditions')
    parser.add_argument('--latency-baseline', type=Path, help='exact independently validated baseline bundle approved by checked-in policy')
    parser.add_argument('--latency-bundle', type=Path, help='exclusive private candidate bundle under .build; never approves it')
    parser.add_argument('--latency-report', type=Path, help='exclusive private timing analysis under .build')
    parser.add_argument('--software-proof-shadow', action='store_true',
                        help='Record incomplete input observations; still run the full gate every time')
    args = parser.parse_args()
    if args.latency_cycles != 0 and (args.state != 'journey' or not 20 <= args.latency_cycles <= 100):
        parser.error('--latency-cycles requires journey and 20..100 explicit cycles')
    if (any((args.latency_conditions, args.latency_baseline, args.latency_bundle, args.latency_report)) and
            args.state not in ('hardware-validated', 'owner-accepted')):
        parser.error('Latency comparison/output arguments require hardware-validated or owner-accepted')
    # This single software gate already includes focused acceptance negative tests.
    # No additional native compile or duplicate full-build verification is added.
    command = [sys.executable, '-B', str(ROOT / 'tools/check.py')]
    if args.software_proof_shadow:
        from software_gate_shadow import run_shadow
        run_shadow(ROOT, command)
    else:
        subprocess.run(command, check=True, cwd=ROOT)
    result = {'state': 'software-verified', 'hardware': 'not-tested', 'owner': 'pending'}
    if args.state != 'software-verified':
        if not args.variant:
            parser.error('installable and later states require --variant')
        expected = current_build(args.variant)
        result['state'] = 'installable'
        if args.state == 'journey':
            if not all((args.install, args.plan, args.target, args.port, args.output)):
                parser.error('journey requires install, private plan, target, explicit port and private output')
            if '://' in args.port or not (args.port.startswith('/dev/') or re.fullmatch(r'COM[1-9][0-9]*', args.port)):
                parser.error('Use an explicit local serial port')
            destination = args.output.resolve()
            if not destination.is_relative_to((ROOT / '.build').resolve()):
                parser.error('Raw private journey receipt must stay below ignored .build')
            install, plan = load(args.install), load(args.plan)
            validate_plan(plan)
            validate_install(install, expected, args.target, time.time())
            spec = importlib.util.spec_from_file_location('dev_usb_client', ROOT / 'tools/dev-usb.py')
            usb = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(usb)
            with usb.lease_port(args.port):
                serial = usb.approved_serial()
                receipt = write_journey(destination, lambda: usb.SerialClient(serial, args.port, 30),
                                        install, plan, procedure_identity(ROOT), sampling_cycles=args.latency_cycles)
            print(json.dumps({'state': 'journey-recorded', 'terminal': receipt['terminal'], 'readiness': 'not-evaluated', 'ready_to_use': False}))
            return 0 if receipt['terminal'] == 'complete' else 1

        if args.state != 'installable':
            if not all((args.install, args.journey, args.plan, args.target, args.ui_review)):
                parser.error('hardware/owner admission requires install, journey, private plan, target and UI review')
            receipt = load(args.journey)
            install, plan = load(args.install), load(args.plan)
            result = validate_journey(receipt, install, plan, expected,
                                      args.target, procedure_identity(ROOT), time.time())
            latency = latency_admission(receipt, install, plan, expected, args)
            result['latency'] = latency_summary(latency)
            if latency['state'] != 'pass':
                result.update(state='correctness-validated', ready_to_use=False)
                print(json.dumps(result, sort_keys=True))
                return 1
            verify_reviewed(render_frames(rust_preview.cli(rust_preview.build())), load(args.ui_review))
            if args.state == 'owner-accepted':
                if not args.owner_review:
                    parser.error('owner acceptance requires explicit manual/physical --owner-review')
                review = load(args.owner_review)
                if (type(review) is not dict or set(review) != {'journey_sha256', 'battery_wake', 'optical_buttons', 'owner'}
                        or review['journey_sha256'] != hashlib.sha256(args.journey.read_bytes()).hexdigest()
                        or any(review[key] != 'accepted' for key in ('battery_wake', 'optical_buttons', 'owner'))):
                    raise ValueError('Manual physical/owner acceptance incomplete or belongs to another journey')
                result.update(state='owner-accepted', battery_wake='accepted', optical_buttons='accepted', owner='accepted')
    result['ready_to_use'] = result['state'] == 'owner-accepted'
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (ValueError, OSError, TypeError, KeyError, subprocess.CalledProcessError) as error:
        print('Reading readiness blocked: ' + str(error), file=sys.stderr)
        raise SystemExit(1)
