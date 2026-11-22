"""Toolchains for a GitHub-hosted CI job (.github/workflows/public-ci.yml).

Before this runs, the workflow restores with actions/cache, keyed on the locks'
hashes, the toolchain trees an earlier fresh materialization made, with their
RECORD; on a miss it restores the raw locked archives instead. Neither cache is
trusted over a check:
- a non-full tier reuses the trees only when RECORD was made from exactly these
  locks at this workspace path, and only when every entry matches the manifest
  recorded when they were made (tools/ci_persist.py's check for the
  self-hosted store); anything else is set aside and made fresh;
- a fresh materialization rechecks every raw archive against its lock's size
  and SHA-256 and downloads a missing one from its pinned URL
  (tools/ci_toolchain.py).
The full tier always materializes fresh, as on the self-hosted runner.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

import ci_persist
import gate_tier

ROOT = Path(__file__).resolve().parents[1]
RECORD = '.build/ci-toolchain.json'  # cached with the trees it describes


def reuse(root):
    """True when the restored trees fit this run and match their manifest."""
    record = root / RECORD
    if not record.is_file():
        print('NO cached toolchain', flush=True)
        return False
    saved = json.loads(record.read_text())
    if saved.get('stamp') != ci_persist.stamp(root):
        print('STALE cached toolchain: made from other locks or another workspace', flush=True)
        return False
    if ci_persist.manifest(root) != saved.get('manifest'):
        print('REJECTED cached toolchain: it differs from the manifest recorded when it was made', flush=True)
        return False
    print(f"REUSED cached toolchain: {len(saved['manifest'])} entries match their recorded manifest", flush=True)
    return True


def prepare(root, tier):
    ci_persist.require_rust_approval(root)
    if tier != 'full' and reuse(root):
        ci_persist.verify_rust(root)
        return
    # A rejected or unused restore never reaches a fresh materialization; raw archives stay.
    (root / RECORD).unlink(missing_ok=True)
    shutil.rmtree(root / '.release-venv', ignore_errors=True)
    if (root / '.toolchain').is_dir():
        for entry in (root / '.toolchain').iterdir():
            if entry.name != 'downloads':
                shutil.rmtree(entry) if entry.is_dir() and not entry.is_symlink() else entry.unlink()
    for command in ci_persist.PREPARE:
        subprocess.run([sys.executable, '-B', *command], cwd=root, check=True)
    ci_persist.verify_rust(root)
    (root / RECORD).write_text(json.dumps({'stamp': ci_persist.stamp(root),
                                           'manifest': ci_persist.manifest(root)}) + '\n')
    print('PASS fresh locked toolchains; recorded for the next run to check', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--expected-commit', required=True)
    args = parser.parse_args()
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        sys.exit('Hosted toolchain preparation is for GitHub Actions only')
    prepare(ROOT, gate_tier.ci_tier(ROOT, args.expected_commit))


if __name__ == '__main__':
    main()
