#!/usr/bin/env python3
"""The installed-app record loader, characterized (NEWT-102).

rust/tests/app_store/record.rs drives the firmware's own load_boot over a
backend that answers like NvsBlobBackend and a host SHA-256 that can fail. It
pins today's validation order and the Failed, then Unsupported, then
selection precedence. This runs it in a scratch copy of rust/. Each negative
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

LOADER = 'firmware/app_runtime/package.rs'
TESTS = 11
# (what breaks, file under rust/, its text, the broken text, the test that must fail)
CONTROLS = (
    ("Tiny's decoder is not consulted", LOADER, "digest != expected || !decode(Some(bytes), out)",
     "digest != expected", 'a_malformed_package_loses_to_its_older_valid_peer'),
    ('the record hash is not compared', LOADER, "if digest != bytes[HASH_OFFSET..] { return Slot::Corrupt; }", "",
     'damage_without_a_valid_slot_is_unavailable'),
    ('a hash provider failure reads as damage', LOADER,
     "if !crate::reader_hash::package_hash(&bytes[..HASH_OFFSET], &mut digest) { return Slot::Failed; }",
     "if !crate::reader_hash::package_hash(&bytes[..HASH_OFFSET], &mut digest) { return Slot::Corrupt; }",
     'a_hash_provider_failure_is_failed_even_beside_a_valid_slot'),
    ('Unsupported outranks Failed', LOADER,
     "    if first == Slot::Failed || second == Slot::Failed { return Status::Failed; }\n"
     "    if first == Slot::Unsupported || second == Slot::Unsupported { return Status::Unsupported; }",
     "    if first == Slot::Unsupported || second == Slot::Unsupported { return Status::Unsupported; }\n"
     "    if first == Slot::Failed || second == Slot::Failed { return Status::Failed; }",
     'failed_outranks_unsupported_which_outranks_selection'),
    ('equal generations with different bytes are not a conflict', LOADER,
     "0 if self.bytes[0] != self.bytes[1] => return Status::Ambiguous,", "",
     'conflicting_generations_are_ambiguous'),
    ('generations compare as plain numbers', LOADER, "delta => u32::from(delta >= 0x8000_0000),",
     "_ => u32::from(self.generations[1] > self.generations[0]),", 'generations_use_serial_order'),
    ('damage beside absence loads the bundled app', LOADER, "(Slot::Missing, Slot::Missing) => return Status::Empty,",
     "(Slot::Missing | Slot::Corrupt, Slot::Missing | Slot::Corrupt) => return Status::Empty,",
     'damage_without_a_valid_slot_is_unavailable'),
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
