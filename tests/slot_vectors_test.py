#!/usr/bin/env python3
"""The Rust card installer and firmware switch held to the shared X3 slot
vectors (#362).

rust/tests/slot_vectors/vectors.rs checks the Rust rules that write flash on
the device (slot_installer/identity.rs, slot_installer.rs, update_card.rs)
against installer/test/x3-slot-vectors.json, the vectors tools/x3_slots.py and
the install page already pass. This flattens the vectors for it and runs it in
a scratch copy of rust/. Each negative control then breaks one rule there and
expects the named test to fail on the named vector, never to fail to compile.
"""
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
sys.path.insert(0, str(ROOT / 'tests'))
from gate_tier import negative_controls
from installer_test import OUTPUT as VECTORS

IDENTITY = 'core/src/slot_installer/identity.rs'
INSTALLER = 'core/src/slot_installer.rs'
CARD = 'core/src/update_card.rs'
IMAGE, INSTALL, HEADS, SELECTIONS = ('image_check_matches_every_vector', 'card_install_accepts_only_intact_images',
                                     'slot_names_match_every_head', 'boot_and_switch_records_match_every_selection')
WRITE = 'card_install_writes_only_the_image'
RETURN = 'official_return::only_a_confirmed_matching_return_can_erase'
TESTS = 6
# (what breaks, file under rust/, its text, the broken text, the test that must fail, the vector it fails on)
CONTROLS = (
    ('an older hand-copied pair bypasses the return authorization', INSTALLER,
     'if order < 0 {', 'if order < 0 && false {', RETURN, 'official return'),
    ('the image magic is not checked', IDENTITY, "header[0] != 0xe9 || ", "", IMAGE, 'magic'),
    ('the chip id is not checked', IDENTITY, " || chip != 5", "", IMAGE, 'chip'),
    ('a segment may end past the slot', IDENTITY, "at + u64::from(length) > u64::from(slot_bytes) || ", "",
     IMAGE, 'truncated'),
    ('segment placement is not checked', IDENTITY, "length % 4 == 0 && (!mapped || u64::from(load % 65536) == data_at % 65536)",
     "length % 4 == 0", IMAGE, 'mapped address off its data'),
    ('the checksum is not checked', IDENTITY, " || chunk[tail-1] != checksum", "", IMAGE, 'checksum'),
    ('the appended SHA-256 is not compared', IDENTITY, " && digest == stored", "", IMAGE, 'digest'),
    ('the digest may lie past the slot', IDENTITY, "digest_at + (if appended { 32 } else { 0 }) > u64::from(slot_bytes) || ",
     "", IMAGE, 'digest beyond slot'),
    ('a card file larger than the slot is accepted', INSTALLER, " || manifest.size > slot_bytes", "", INSTALL,
     'digest beyond slot'),
    ('the card install accepts a file without an intact image', INSTALLER,
     "hash) else { return InstallIssue::Image; };", "hash) else { return InstallIssue::None; };", INSTALL, 'chip'),
    ('a card file may end in bytes other than erased padding', INSTALLER,
     "if chunk[image..count].iter().any(|&byte| byte != 0xff) { return InstallIssue::Image; }", "", INSTALL,
     'extra data'),
    ('erased padding after a card image is refused', INSTALLER, "byte != 0xff", "byte != 0xff || true", INSTALL,
     'erased padding'),
    ('the card install writes the whole padded file', INSTALLER, "if !self.write(bytes, target)",
     "if !self.write(manifest.size, target)", WRITE, 'erased padding'),
    ('a name matches a longer project', CARD, 'b"CrossInk\\0"', 'b"CrossInk"', HEADS, 'crossink-prefix'),
    ('a record with a bad CRC is followed', IDENTITY, "record.crc == ota_seq_crc(record.seq) && ", "", SELECTIONS,
     'bad-crc'),
    ('a pending record is followed', IDENTITY, "if records[followed].state != STATE_PENDING { return followed as i32; }",
     "return followed as i32;", SELECTIONS, 'pending'),
    ('a switch may take sequence 0xffffffff', IDENTITY, "if seq == u32::MAX { return OtaSelection::Exhausted; }", "",
     SELECTIONS, 'exhausted'),
)


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def flatten(vectors):
    """One tab-separated row per vector, in the order vectors.rs reads them."""
    rows = [('image', v['name'], v['hex'], v['slotBytes'], int(v['intact']), int(v['extraData']), v['length'])
            for v in vectors['images']]
    rows += [('head', v['name'], v['hex'], v['kind'], v['shown']) for v in vectors['heads']]
    for v in vectors['selections']:
        boot = '-' if v['boot'] is None else f"{v['boot']['record']}:{v['boot']['slot']}"
        select = [s if isinstance(s, str) else f"{s['into']}:{s['record']}" for s in v['select']]
        rows.append(('selection', v['name'], *v['records'], boot, *select))
    # Synthetic intact images exercise the real installer, not just SemVer.
    sys.path.insert(0, str(ROOT / 'rust/host/preview'))
    from update_fixtures import newt_image, manifest
    for version in ('0.5.1-dev.20261125.1', '0.5.0', '0.5.0+changed'):
        image = newt_image(version)
        rows.append(('official', version, image.hex(), manifest(version, image).hex()))
    return ''.join('\t'.join(map(str, row)) + '\n' for row in rows)


def test_binary(run, workspace, target):
    """Compiles the test; a compiler error is a failure, never a control's kill."""
    wire = run.cargo(workspace, target, 'test', '-p', 'newt-core', '--test', 'slot_vectors', '--no-run',
                     '--message-format=json')
    outputs = [row['executable'] for row in map(json.loads, wire.splitlines())
               if row.get('reason') == 'compiler-artifact' and row.get('executable')
               and row['target']['name'] == 'slot_vectors']
    if len(outputs) != 1:
        raise RuntimeError('one compiled slot_vectors test executable required')
    return outputs[0]


def control(run, workspace, target, support, name, source, good, bad, test, vector):
    path = workspace / source
    text = path.read_text()
    support.require(text.count(good) == 1, f'control "{name}": its anchor is not in rust/{source} exactly once')
    path.write_text(text.replace(good, bad))
    try:
        binary = test_binary(run, workspace, target)
        number = len(run.commands)
        try:
            run.run([binary, test, '--exact'], workspace)
        except RuntimeError:
            stdout = (run.output / f'{number:03d}.stdout').read_text()
            # Each mismatch line starts with its vector's name.
            failed = any(line.startswith(f'{vector}: ') for line in stdout.splitlines())
            support.require('test result: FAILED.' in stdout and failed,
                            f'control "{name}": {test} must fail on the {vector} vector')
        else:
            raise RuntimeError(f'control "{name}": {test} passed with the rule broken')
    finally:
        path.write_text(text)
    print(f'PASS control: {test} fails on {vector} when {name}', flush=True)


def main():
    support = load('slot_vectors_support', ROOT / 'rust/tests/inflate/run_support.py')
    support.install_signal_handlers()
    scratch = ROOT / '.build/slot-vectors'
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='run-', dir=scratch) as directory:
        output = Path(directory)
        workspace = output / 'rust'
        shutil.copytree(ROOT / 'rust', workspace, ignore=shutil.ignore_patterns('target', '__pycache__'))
        vectors = output / 'vectors.tsv'
        vectors.write_text(flatten(json.loads(VECTORS.read_text())))
        (output / 'logs').mkdir()
        run = support.Run(output / 'logs', support.verify_tools(), time.monotonic() + 300)
        run.env.update(RUSTFLAGS='-Dwarnings -Ccodegen-units=1', NEWT_SLOT_VECTORS=str(vectors))
        target = output / 'target'
        try:
            result = run.run([test_binary(run, workspace, target)], workspace)
            support.require(f'test result: ok. {TESTS} passed; 0 failed; 0 ignored' in result,
                            f'all {TESTS} slot vector tests ran and passed')
            print(f'PASS Rust slot rules match {VECTORS.relative_to(ROOT)}', flush=True)
            if negative_controls(__file__):
                # A broken rule can leave a value unused; only the test may kill it.
                run.env['RUSTFLAGS'] = '-Ccodegen-units=1'
                for row in CONTROLS:
                    control(run, workspace, target, support, *row)
        except RuntimeError:
            for log in sorted((output / 'logs').glob('*.std*'))[-2:]:
                print(log.read_text(), file=sys.stderr, end='')
            raise


if __name__ == '__main__':
    main()
