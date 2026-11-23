"""Required exact-source check of the host-only Swift ImageIO cover helper."""
import argparse
import json
import os
from pathlib import Path
import platform
import subprocess
import sys

sys.dont_write_bytecode = True
sys.pycache_prefix = None
os.environ.pop('PYTHONPYCACHEPREFIX', None)

from ci_gate import committed_source
from native_output import owned_directory, owned_file, write_text

ROOT = Path(__file__).resolve().parents[1]


def run_gate(root, expected):
    directory = owned_directory(root, root / '.build/mac-covers', create=True)
    report = owned_file(root, directory / 'report.json')
    report.unlink(missing_ok=True)
    if platform.system() != 'Darwin':
        raise RuntimeError('The host cover helper gate needs macOS ImageIO')
    before = committed_source(root, expected)
    command = [sys.executable, '-B', str(root / 'tests/library_cover_prepare_test.py')]
    print('START Mac ImageIO cover helper tests', flush=True)
    subprocess.run(command, cwd=root, check=True)
    after = committed_source(root, expected)
    if before != after:
        raise RuntimeError('Committed source changed during required Mac cover gate')
    write_text(root, report, json.dumps({
        'purpose': 'Host-only Swift ImageIO cover helper checks',
        'source': after, 'phases': ['mac-imageio-cover-tests'], 'platform': platform.platform(),
        'reusable_proof': False, 'hardware': 'not-tested', 'owner': 'pending',
    }, indent=2) + '\n')
    print(f'PASS Mac ImageIO cover helper gate for {expected}', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--expected-commit', required=True)
    args = parser.parse_args()
    run_gate(ROOT, args.expected_commit)


if __name__ == '__main__':
    main()
