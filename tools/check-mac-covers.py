"""Required exact-source Mac ImageIO/20-frame cover software gate; no device proof."""
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
from reading_acceptance import load
import rust_preview
from system_ui_contract import render_frames, verify_reviewed

ROOT = Path(__file__).resolve().parents[1]


def reviewed_frames(root):
    """Every reviewed frame, rendered by the real firmware built for the host."""
    binary = rust_preview.cli(rust_preview.build())
    verify_reviewed(render_frames(binary), load(root / 'artifacts/library-ui-review.json'))


def run_gate(root, expected):
    directory = owned_directory(root, root / '.build/mac-covers', create=True)
    report = owned_file(root, directory / 'report.json')
    report.unlink(missing_ok=True)
    if platform.system() != 'Darwin':
        raise RuntimeError('Required cover gate needs macOS ImageIO; Linux fixture coverage cannot replace it')
    before = committed_source(root, expected)
    environment = dict(os.environ, NEWT_HOST_OBJECT_CACHE='0', PYTHONDONTWRITEBYTECODE='1')
    command = [sys.executable, '-B', str(root / 'tests/library_cover_prepare_test.py')]
    print('START required Mac ImageIO cover tests', flush=True)
    subprocess.run(command, cwd=root, env=environment, check=True)
    print('START all 22 reviewed Mac UI frames with prepared ImageIO covers', flush=True)
    reviewed_frames(root)
    after = committed_source(root, expected)
    if before != after:
        raise RuntimeError('Committed source changed during required Mac cover gate')
    write_text(root, report, json.dumps({
        'purpose': 'Mac ImageIO cover software checks and all 22 frames against committed reviewed UI authority',
        'source': after, 'phases': ['mac-imageio-cover-tests', 'reviewed-ui-20-frames'], 'platform': platform.platform(),
        'reusable_proof': False, 'hardware': 'not-tested', 'owner': 'pending',
    }, indent=2) + '\n')
    print(f'PASS required Mac cover software gate for {expected}; physical acceptance remains separate', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--expected-commit', required=True)
    args = parser.parse_args()
    run_gate(ROOT, args.expected_commit)


if __name__ == '__main__':
    main()
