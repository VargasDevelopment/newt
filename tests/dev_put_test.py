#!/usr/bin/env python3
"""The development bridge's card write (#26).

rust/tests/dev_put/lease.rs drives newt_core::dev_put over a fake card that
stores every write at once and a scripted host. It pins the name and folder
rules, admission, and that the final name holds only the old file or the whole
verified new one, whatever ends the lease, while a marker a power loss left
refuses the folder and recovery deletes nothing that may share data. This runs it in a scratch copy of rust/. Each
negative control then breaks one rule there and expects the named test to
fail, never to fail to compile.
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

BINARY = 'dev_put'
TESTS = 32
# Recovery's one rule: a marker the next FILE finds refuses.
MARKED = '        if self.hidden(folder.marker(), "marker")? { return Err(Refused::Recovery("commit", 0)); }\n'
# (what breaks, its file under rust/core/src, its text there, the broken text, the test that must fail)
CONTROLS = (
    ('the SHA-256 is not compared', 'dev_put/lease.rs', "        if digest != sha256 { return Err(Refused::DigestMismatch); }\n", "",
     'a_digest_mismatch_leaves_no_final_file'),
    ('a `..` name is allowed', 'dev_put.rs', "pub fn name_allowed(name: &[u8]) -> bool {\n",
     "pub fn name_allowed(name: &[u8]) -> bool {\n    if name == b\"..\" { return true; }\n",
     'names_outside_the_safe_set_are_refused'),
    ('an open book does not refuse', 'dev_put.rs', "    if facts.reader_open { return Err(Busy::Reading); }\n", "",
     'admission_refuses_reading_opening_and_away_from_home'),
    ('bytes go straight to the final name', 'dev_put/lease.rs', "let fd = self.card.create(folder.temp())",
     "let fd = self.card.create(path(&target))", 'physical_input_abandons_and_keeps_the_old_file'),
    ('recovery deletes a file or clears the marker while a marker stands', 'dev_put/commit.rs', MARKED,
     "        if self.hidden(folder.marker(), \"marker\")? { let _ = self.card.remove(folder.old()); "
     "let _ = self.card.remove(folder.temp()); let _ = self.card.remove(folder.marker()); return Ok(()); }\n",
     'commit::a_rename_a_power_loss_cut_short_is_never_unlinked'),
    ('a marker clears once no backup or temp shows (the target alone, or nothing)', 'dev_put/commit.rs', MARKED,
     "        if self.hidden(folder.marker(), \"marker\")? {\n"
     "            if !old && self.card.entry(folder.temp()) == Ok(Entry::Missing) { return self.card.remove(folder.marker())"
     ".map_err(|code| Refused::Recovery(\"marker\", code)); }\n"
     "            return Err(Refused::Recovery(\"commit\", 0));\n        }\n",
     'commit::a_marker_left_by_any_commit_refuses_whatever_the_names_show'),
    ('a rename that fails after moving reads as a plain refusal', 'dev_put/commit.rs',
     "        if !unchanged { return Refused::Recovery(step, code); }\n", "",
     'commit::a_rename_that_fails_after_moving_keeps_the_marker'),
    ('the read-back ignores a takeover', 'dev_put/lease.rs',
     "            if let Some(reason) = self.interruption() { return Err(Refused::Abandoned(reason)); }\n", "",
     'commit::a_press_while_done_reads_back_stops_it_there'),
    ('the first rename ignores a takeover', 'dev_put/lease.rs',
     "        if let Some(reason) = self.interruption() {\n"
     "            // No rename ran, so the temp the exit removes is no other name's.\n"
     "            let _ = self.card.remove(folder.marker());\n            return Err(Refused::Abandoned(reason));\n        }\n", "",
     'commit::a_press_just_before_the_first_rename_keeps_the_old_file'),
    ('the deadline is not checked', 'dev_put/lease.rs',
     "        if self.wire.now().wrapping_sub(self.started) >= TOTAL_MS { return Some(\"total-time\"); }\n", "",
     'commit::the_deadline_stops_done_before_its_first_rename'),
    ('a failed restore reads as a plain refusal', 'dev_put/commit.rs',
     'is_err() { return Err(Refused::Recovery("rename", code)); }', 'is_err() { return Err(Refused::Card("rename", code)); }',
     'commit::a_failed_restore_keeps_the_backup_and_says_so'),
)


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_binary(run, workspace, target):
    """Compiles the test; a compiler error is a failure, never a control's kill."""
    wire = run.cargo(workspace, target, 'test', '-p', 'newt-core', '--test', BINARY, '--no-run',
                     '--message-format=json')
    outputs = [row['executable'] for row in map(json.loads, wire.splitlines())
               if row.get('reason') == 'compiler-artifact' and row.get('executable')
               and row['target']['name'] == BINARY]
    if len(outputs) != 1:
        raise RuntimeError(f'one compiled {BINARY} test executable required')
    return outputs[0]


def control(run, workspace, target, support, name, source, good, bad, test):
    path = workspace / 'core/src' / source
    text = path.read_text()
    support.require(text.count(good) == 1, f'control "{name}": its anchor is not in rust/core/src/{source} exactly once')
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
    support = load('dev_put_support', ROOT / 'rust/tests/inflate/run_support.py')
    support.install_signal_handlers()
    scratch = ROOT / '.build/dev-put'
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
                            f'all {TESTS} {BINARY} tests ran and passed')
            print('PASS the bridge card write keeps the final name old or whole and verified', flush=True)
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
