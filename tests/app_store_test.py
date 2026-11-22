#!/usr/bin/env python3
"""The installed-app record loader, characterized (NEWT-102).

rust/tests/app_store/record.rs drives the firmware's own load_boot (over
newt_core::app_store) with a backend that answers like NvsBlobBackend and a
host SHA-256 that can fail. It pins the validation order and the Failed, then
Unsupported, then selection precedence, and holds Tiny's check to decode.
This runs it in a scratch copy of rust/. Each negative
control then breaks one rule there and expects the named test to fail, never
to fail to compile.
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
from gate_tier import negative_controls

STORE = 'core/src/app_store.rs'
LOADER = 'firmware/app_runtime/package.rs'
TINY = 'tiny-app/src/lib.rs'
TESTS = 12
MALFORMED = 'a_malformed_package_loses_to_its_older_valid_peer'
DAMAGE = 'damage_without_a_valid_slot_is_unavailable'
FAILED = "    if first == Slot::Failed || second == Slot::Failed {\n        return Load::Failed;\n    }\n"
UNSUPPORTED = "    if first == Slot::Unsupported || second == Slot::Unsupported {\n        return Load::Unsupported;\n    }\n"
FAILED_FIRST, UNSUPPORTED_FIRST = FAILED + UNSUPPORTED, UNSUPPORTED + FAILED
# (what breaks, file under rust/, its text, the broken text, the test that must fail)
CONTROLS = (
    ("core ignores the decoder's verdict", STORE, "digest != expected || !payload.accepts(wire)",
     "digest != expected", MALFORMED),
    ("the firmware port accepts every wire", LOADER, "-> bool { check(wire) }", "-> bool { let _ = wire; true }",
     MALFORMED),
    ('the record hash is not compared', STORE,
     "    if digest != bytes[HASH_AT..] {\n        return Slot::Corrupt;\n    }\n", "", DAMAGE),
    ('a hash provider failure reads as damage', STORE,
     "if !payload.hash(&bytes[..HASH_AT], &mut digest) {\n        return Slot::Failed;",
     "if !payload.hash(&bytes[..HASH_AT], &mut digest) {\n        return Slot::Corrupt;",
     'a_hash_provider_failure_is_failed_even_beside_a_valid_slot'),
    ('Unsupported outranks Failed', STORE, FAILED_FIRST, UNSUPPORTED_FIRST,
     'failed_outranks_unsupported_which_outranks_selection'),
    ('equal generations with different bytes are not a conflict', STORE,
     "0 if slots[0] != slots[1] => Load::Ambiguous,", "", 'conflicting_generations_are_ambiguous'),
    ('generations compare as plain numbers', STORE, "delta => Load::Selected(usize::from(delta >= 0x8000_0000)),",
     "_ => Load::Selected(usize::from(b > a)),", 'generations_use_serial_order'),
    ('damage beside absence loads the bundled app', STORE, "(Slot::Missing, Slot::Missing) => Load::Empty,",
     "(Slot::Missing | Slot::Corrupt, Slot::Missing | Slot::Corrupt) => Load::Empty,", DAMAGE),
    ("Tiny's check skips a card beyond the count", TINY, "if b[at..at + 99].iter().any(|&v| v != 0) {",
     "if false {", 'check_agrees_with_decode_over_a_wire_corpus'),
)


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_binary(run, workspace, target):
    """Compiles the test; a compiler error is a failure, never a control's kill."""
    wire = run.cargo(workspace, target, 'test', '-p', 'newt-tiny-app', '--test', 'app_store', '--no-run',
                     '--message-format=json')
    outputs = [row['executable'] for row in map(json.loads, wire.splitlines())
               if row.get('reason') == 'compiler-artifact' and row.get('executable')
               and row['target']['name'] == 'app_store']
    if len(outputs) != 1:
        raise RuntimeError('one compiled app_store test executable required')
    return outputs[0]


def control(run, workspace, target, support, name, source, good, bad, test):
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
            support.require('test result: FAILED. 0 passed; 1 failed' in stdout and 'panicked at' in stdout,
                            f'control "{name}": {test} must fail on an assertion')
        else:
            raise RuntimeError(f'control "{name}": {test} passed with the rule broken')
    finally:
        path.write_text(text)
    print(f'PASS control: {test} fails when {name}', flush=True)


def main():
    support = load('app_store_support', ROOT / 'rust/tests/inflate/run_support.py')
    support.install_signal_handlers()
    scratch = ROOT / '.build/app-store'
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='run-', dir=scratch) as directory:
        output = Path(directory)
        workspace = output / 'rust'
        shutil.copytree(ROOT / 'rust', workspace, ignore=shutil.ignore_patterns('target', '__pycache__'))
        (output / 'logs').mkdir()
        run = support.Run(output / 'logs', support.verify_tools(), time.monotonic() + 300)
        run.env['RUSTFLAGS'] = '-Dwarnings -Ccodegen-units=1'
        target = output / 'target'
        try:
            result = run.run([test_binary(run, workspace, target)], workspace)
            support.require(f'test result: ok. {TESTS} passed; 0 failed; 0 ignored' in result,
                            f'all {TESTS} installed-app record tests ran and passed')
            print('PASS installed-app record loader matches its characterization', flush=True)
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
