"""Descriptive software-gate observations; no stored record grants admission.

Existing collectors do not close the complete gate's external execution inputs.
Matching observations never permit skipping the canonical gate.
"""
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

from code_health import discover
from code_health_resources import input_files, tree_hash
from native_output import owned_directory, owned_file, write_text
from python_runtime import runtime_identity
from reading_acceptance import load
from trusted_host import compiler_identity, file_digest, verify_source_pin

SOURCE_ROOTS = ('src', 'include', 'firmware', 'tools', 'tests', 'assets', 'apps', 'patches', 'docs')
UNCLOSED = (
    'Fresh/direct compiler invocations inherit ambient SDK/include/default-config inputs.',
    'Nested PATH-selected python3/c++ and CXX-selected upstream compilers lack a complete tool closure.',
    'Nonisolated Python startup imports, site-packages, PYTHONPATH and startup hooks are not closed.',
    'Linker/SDK/sanitizer dependencies and dynamically loaded operating-system libraries are not closed.',
    'Swift compiler, ImageIO/CoreGraphics and their native runtime closure are not collected.',
    'Git discovery observations do not close all external Git config/index/ignore execution inputs.',
    'Tracked artifact observations do not inventory every generated executable or temporary output.',
    'Editable observations/logs cannot establish that a prior trusted gate execution occurred.',
)


def canonical_hash(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def observe(root, command):
    """Available canonical collectors only; explicitly NOT a complete proof key."""
    for name in (*SOURCE_ROOTS, 'vendor/freeink-sdk', 'artifacts'):
        owned_directory(root, root / name)
    revision = verify_source_pin(root / 'vendor/freeink-sdk')
    tracked = subprocess.run(['git', 'ls-files', '--cached', '-z'], cwd=root,
                             check=True, capture_output=True).stdout.decode().split('\0')
    tracked = sorted(set(tracked) - {''})
    groups = {name: input_files(root / name, include_python_cache=True) for name in SOURCE_ROOTS}
    groups['vendor'] = input_files(root / 'vendor/freeink-sdk', include_python_cache=True)
    groups['root_configuration'] = [owned_file(root, root / name) for name in tracked if '/' not in name]
    # Only tracked public artifacts; never traverse private .build/installation/media data.
    artifacts = {name: file_digest(owned_file(root, root / name))
                 for name in tracked if name.startswith('artifacts/')}
    return {'source_groups': {name: {'files': len(paths), 'sha256': tree_hash(root, paths)}
                              for name, paths in groups.items()},
            'vendor_revision': revision, 'compilers': compiler_identity(),
            'python': runtime_identity(),
            'alternate_python': runtime_identity(root / '.toolchain/platformio/penv/bin/python'),
            'git_tracked_names_sha256': canonical_hash(tracked),
            'git_discovered_sources_sha256': canonical_hash(sorted(discover(root))),
            'command': list(command), 'environment_sha256': canonical_hash(dict(os.environ)),
            'platform': platform.platform(), 'machine': platform.machine(),
            'tracked_artifacts': artifacts}


def observation(root, command):
    try:
        return {'available': True, 'observed': observe(root, command)}
    except (OSError, RuntimeError, ValueError, TypeError, KeyError, subprocess.CalledProcessError) as error:
        # An incomplete observation still requires the fresh canonical gate.
        return {'available': False, 'error_type': type(error).__name__}


def execute(root, command, log):
    """Stream the fresh canonical gate while retaining diagnostic bytes."""
    with owned_file(root, log).open('wb') as destination:
        with subprocess.Popen(command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT) as process:
            for line in process.stdout:
                destination.write(line)
                destination.flush()
                sys.stdout.buffer.write(line)
                sys.stdout.buffer.flush()
            return process.wait()


def run_shadow(root, command):
    directory = owned_directory(root, root / '.build/software-gate-shadow', create=True)
    report = owned_file(root, directory / 'observation.json')
    log = owned_file(root, directory / 'gate.log')
    try:
        previous = load(report) if report.exists() else None
    except (OSError, ValueError, TypeError, KeyError, RecursionError):
        previous = None
    report.unlink(missing_ok=True)
    before = observation(root, command)
    # A historical record contributes only an equality observation, never trust.
    matching = (before['available'] and type(previous) is dict
                and previous.get('after') == before)
    started = time.perf_counter()
    returncode = execute(root, command, log)
    seconds = round(time.perf_counter() - started, 3)
    after = observation(root, command)
    write_text(root, report, json.dumps({
        'schema': 1, 'purpose': 'Descriptive shadow; not a reusable software proof',
        'reuse_enabled': False, 'reuse_eligible': False, 'decision': 'fresh-required',
        'complete_input_closure': False, 'unclosed_inputs': UNCLOSED,
        'historical_observed_inputs_match': matching,
        'observed_inputs_stable': before['available'] and before == after,
        'before': before, 'after': after,
        'fresh_gate': {'command': command, 'returncode': returncode, 'seconds': seconds,
                       'log_sha256': file_digest(log)},
        'avoided_gate_invocations': 0, 'eliminated_gate_seconds': 0,
    }, indent=2) + '\n')
    print(f'Software shadow: fresh gate exit {returncode}; reuse disabled; '
          f'{report.relative_to(root)}', file=sys.stderr, flush=True)
    if returncode:
        raise subprocess.CalledProcessError(returncode, command)
