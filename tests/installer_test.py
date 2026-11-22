#!/usr/bin/env python3
"""The install page's checks (installer/): its slot rule against the Python
rule, its wordmark against the approved brand source, its type check and its
vitest suite.

installer/src/slots.ts copies the X3 slot rule. This exports
tests/x3_slots_test.py's byte vectors, with tools/x3_slots.py's answers, to
installer/test/x3-slot-vectors.json and checks that file is current, so the
Python rule and its vectors cannot change without the TypeScript copy seeing
it. The negative controls break one safety rule each in a scratch copy and
expect vitest to fail.

  installer_test.py           the gate
  installer_test.py --write   regenerate the vectors after a reviewed rule change

Public CI installs the Node packages first. The self-hosted CI runner has
none (installer/README.md, "CI"), so there, and only there, the vector and
wordmark checks run alone and the rest is skipped loudly.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
sys.path.insert(0, str(ROOT / 'tests'))
from gate_tier import negative_controls
import x3_slots
from x3_slots import SECTOR, Refused
from x3_slots_test import KINDS, SELECTIONS, VECTORS, images

INSTALLER = ROOT / 'installer'
OUTPUT = INSTALLER / 'test/x3-slot-vectors.json'
BIN = INSTALLER / 'node_modules/.bin'
# What the page's typed sources and tests are, for a scratch copy.
SOURCES = ('src', 'test', 'package.json', 'tsconfig.json', 'vite.config.ts')
# Each control breaks one safety rule in the page's source; vitest must then fail.
CONTROLS = {
    'blank otadata selects app1 with sequence 1':
        ('src/slots.ts', 'selection = { offset: layout.otadata, sector: recordSector(2) };',
         'selection = { offset: layout.otadata, sector: recordSector(1) };'),
    'the write guard allows any write':
        ('src/slots.ts', 'if (!app && !selection) throw', 'if (false && !app && !selection) throw'),
    'a selection may take the reserved sequence 0xffffffff':
        ('src/slots.ts', 'if (seq >= 0xffffffff) throw', 'if (false && seq >= 0xffffffff) throw'),
    'the firmware that stays is not checked whole':
        ('src/session.ts', 'if (!(await slotImageIntact(', 'if (false && !(await slotImageIntact('),
    'an image the bootloader refuses for segment placement passes':
        ('src/image.ts', 'if (!segmentPlaced(', 'if (false && !segmentPlaced('),
    'erased padding after an image is refused':
        ('src/image.ts', '.every((byte) => byte === 0xff)', '.every((byte) => byte === 0xff && false)'),
    'bytes after an image are accepted':
        ('src/image.ts', '.every((byte) => byte === 0xff)', '.every((byte) => byte === 0xff || true)'),
}
RECORD = 32


def record_hex(selection, index):
    """One record's 32 bytes; the rest of its sector must read erased."""
    sector = selection[index * SECTOR:(index + 1) * SECTOR]
    assert sector[RECORD:] == b'\xff' * (SECTOR - RECORD), 'a selection vector holds bytes past its record'
    return sector[:RECORD].hex()


def selection_answer(selection, slot):
    try:
        layout = x3_slots.Layout(0, 1, x3_slots.boot_slot(selection), ())
        write = x3_slots.selection_write(layout, selection, slot)
    except x3_slots.SequenceExhausted:
        return 'exhausted'
    except Refused:
        return 'unknown'
    if write is None:
        return 'selected'
    return {'into': (write[0] - x3_slots.OTADATA) // SECTOR, 'record': write[1][:RECORD].hex()}


def boot(selection):
    try:
        return {'record': x3_slots.boot_record(selection), 'slot': x3_slots.boot_slot(selection)}
    except Refused:
        return None


def vectors():
    heads = [{'name': name, 'hex': data.hex(), 'kind': KINDS[name], 'project': x3_slots.classify(data).project,
              'shown': x3_slots.classify(data).name}
             for name, data in VECTORS.items()]
    selections = [{'name': name, 'records': [record_hex(data, 0), record_hex(data, 1)], 'boot': boot(data),
                   'select': [selection_answer(data, 0), selection_answer(data, 1)]}
                  for name, data in SELECTIONS.items()]
    image_vectors = [{'name': name, 'hex': data.hex(), 'slotBytes': slot, 'intact': intact,
                      'length': x3_slots.image_length(data), 'extraData': x3_slots.extra_data(data)}
                     for name, data, slot, intact in images()]
    return {'source': 'tests/installer_vectors.py from tools/x3_slots.py and tests/x3_slots_test.py',
            'heads': heads, 'selections': selections, 'images': image_vectors}


def check_vectors():
    text = json.dumps(vectors(), indent=1) + '\n'
    if not OUTPUT.exists() or OUTPUT.read_text() != text:
        sys.exit(f'{OUTPUT.relative_to(ROOT)} differs from tools/x3_slots.py: run tests/installer_test.py --write '
                 'and update installer/src/slots.ts to match')
    print(f'PASS {OUTPUT.relative_to(ROOT)} matches tools/x3_slots.py', flush=True)


def node(tool, *arguments, cwd=INSTALLER, quiet=False):
    output = subprocess.DEVNULL if quiet else None
    return subprocess.run([str(BIN / tool), *arguments], cwd=cwd, stdout=output, stderr=output).returncode


def run_controls():
    for name, (source, good, bad) in CONTROLS.items():
        with tempfile.TemporaryDirectory() as scratch:
            copy = Path(scratch)
            for part in SOURCES:
                (shutil.copytree if (INSTALLER / part).is_dir() else shutil.copy)(INSTALLER / part, copy / part)
            (copy / 'node_modules').symlink_to(INSTALLER / 'node_modules')
            broken = copy / source
            text = broken.read_text()
            if text.count(good) != 1:
                sys.exit(f'FAIL control "{name}": its anchor is not in installer/{source} exactly once')
            broken.write_text(text.replace(good, bad))
            if node('vitest', 'run', cwd=copy, quiet=True) == 0:
                sys.exit(f'FAIL control "{name}": vitest passed with the rule broken')
        print(f'PASS control: vitest fails when {name}', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--write', action='store_true', help='regenerate the vectors file')
    parser.add_argument('--negative-controls', action='store_true', help='also run the negative controls')
    args = parser.parse_args()
    if args.write:
        OUTPUT.write_text(json.dumps(vectors(), indent=1) + '\n')
        print(f'Wrote {OUTPUT.relative_to(ROOT)}')
        return
    check_vectors()
    if subprocess.run([sys.executable, '-B', str(ROOT / 'tools/make-install-wordmark.py'), '--check']).returncode:
        sys.exit('FAIL installer wordmark')
    if not BIN.is_dir():
        if os.environ.get('GITHUB_ACTIONS') == 'true':
            print('SKIP installer type check, vitest and controls: this CI runner has no installer/node_modules '
                  '(installer/README.md, "CI")', flush=True)
            return
        sys.exit('FAIL installer/node_modules is missing: run pnpm install --frozen-lockfile in installer/')
    if node('tsc', '--noEmit') != 0:
        sys.exit('FAIL installer type check')
    if node('vitest', 'run') != 0:
        sys.exit('FAIL installer vitest')
    if args.negative_controls and negative_controls(__file__):
        run_controls()


if __name__ == '__main__':
    main()
