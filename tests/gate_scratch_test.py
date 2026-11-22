"""The actual gate entry owns scratch before prerequisites and every phase."""
from contextlib import nullcontext, redirect_stdout
import importlib
import importlib.util
import io
import json
import os
from pathlib import Path
import signal
import stat
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import gate_runner
import host_jobs

spec = importlib.util.spec_from_file_location('scratch_host_check', ROOT / 'tools/check.py')
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)
BAD_SCRATCH = ('/tmp/new-fixed-root', '/var/tmp/new-fixed-root', '/var/tmp')
BAD_LOOKUP = "os.environ['TMPDIR']"
PROBE = '''import json,os,stat,sys,tempfile
from pathlib import Path
root=os.environ.get('TMPDIR')
assert root, 'gate did not supply TMPDIR'
root=Path(root)
assert root.is_absolute() and root.is_dir()
with tempfile.TemporaryFile() as stream:
 stream.write(b'private')
assert Path(tempfile.gettempdir())==root
mounts=Path(os.environ.get('NEWT_GATE_NATIVE_ROOTS'))
assert mounts.is_dir() and stat.S_IMODE(mounts.stat().st_mode)==0o700
row={'phase':sys.argv[2],'root':str(root),'mounts':str(mounts),'uid':root.stat().st_uid,'mode':stat.S_IMODE(root.stat().st_mode)}
with open(sys.argv[1],'a') as stream: stream.write(json.dumps(row)+'\\n')
raise SystemExit(int(sys.argv[3]))
'''


class ScratchTests(unittest.TestCase):
    def setUp(self):
        exceptions = patch.object(importlib.import_module('check_scratch'), 'EXCEPTIONS', ())
        exceptions.start()
        self.addCleanup(exceptions.stop)
        temporary = tempfile.TemporaryDirectory(prefix='newt-scratch-proof-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        for name in ('tests', 'tools'):
            (self.root / name).mkdir()
        self.journal = self.root / 'journal'

    def invoke(self, provided=None, failure=False):
        self.journal.unlink(missing_ok=True)
        environment = {**os.environ, host_jobs.GLOBAL_ROOT: str(self.root / 'pool'), host_jobs.GLOBAL_JOBS: '4'}
        environment.pop('TMPDIR', None)
        if provided is not None:
            environment['TMPDIR'] = provided
        def command(name, code=0):
            return (sys.executable, '-B', '-c', PROBE, str(self.journal), name, str(code))
        plan = [gate_runner.Phase(name, command(name, 7 if failure and name == 'last' else 0))
                for name in ('one', 'two', 'last')]
        real_pre_step = check.phase_run
        counter = []
        def pre_step(arguments, **options):
            name = 'pre-' + str(len(counter))
            counter.append(name)
            return real_pre_step(command(name), stderr=subprocess.PIPE, **options)
        with patch.dict(os.environ, environment, clear=True), \
             patch.object(sys, 'argv', ['check.py', '--profile', 'linux', '-j', '2']), \
             patch.object(check, 'ROOT', self.root), patch.object(check, 'verify_source_pin'), \
             patch.object(check, 'phase_run', side_effect=pre_step), \
             patch.object(check.gate_plan, 'plan', return_value=plan), \
             patch.object(gate_runner, 'cpu_budgets', return_value={
                 'default_seconds': 30, 'critical_path_seconds': 30, 'phases': {}}), \
             redirect_stdout(io.StringIO()):
            check.main()
        return [json.loads(row) for row in self.journal.read_text().splitlines()]

    def test_unset_parent_gives_every_pre_step_and_phase_one_private_root_then_removes_it(self):
        previous = None
        for _ in range(2):
            rows = self.invoke()
            self.assertEqual({r['phase'] for r in rows}, {'pre-0', 'pre-1', 'one', 'two', 'last'})
            roots = {r['root'] for r in rows}
            self.assertEqual(len(roots), 1)
            root = Path(roots.pop())
            self.assertNotEqual(root, previous)
            self.assertFalse(root.exists())
            self.assertEqual(len({r['mounts'] for r in rows}),1)
            self.assertFalse(Path(rows[0]['mounts']).exists())
            self.assertTrue(all(r['uid'] == os.getuid() and r['mode'] == 0o700 for r in rows))
            previous = root

    def test_caller_root_is_resolved_preserved_and_inherited(self):
        provided = self.root / 'provided'
        provided.mkdir(mode=0o700)
        rows = self.invoke(os.path.relpath(provided))
        self.assertEqual({r['root'] for r in rows}, {str(provided.resolve())})
        self.assertTrue(provided.is_dir())

    def test_empty_parent_gets_an_owned_private_root(self):
        rows = self.invoke('')
        root = Path(rows[0]['root'])
        self.assertNotEqual(root, ROOT)
        self.assertFalse(root.exists())
        self.assertTrue(all(r['uid'] == os.getuid() and r['mode'] == 0o700 for r in rows))

    def test_unused_scratch_exception_fails(self):
        guard = importlib.import_module('check_scratch')
        path = self.root / 'tests/text.py'
        with patch.object(guard, 'EXCEPTIONS', (('tests/text.py', '# inert:', 'Rejection text.'),)):
            path.write_text('# inert: ' + BAD_SCRATCH[0])
            self.assertEqual(guard.violations(self.root), [])
            path.write_text('# obsolete exception\n')
            self.assertTrue(any('unused' in error for error in guard.violations(self.root)))

    def test_removing_entry_default_fails_before_any_temporary_file_is_opened(self):
        scratch = importlib.import_module('gate_scratch')
        with patch.object(scratch, 'environment', return_value=nullcontext()), \
             self.assertRaises(subprocess.CalledProcessError):
            self.invoke()
        self.assertFalse(self.journal.exists())

    def test_failing_phase_still_removes_the_owned_root(self):
        with self.assertRaises(gate_runner.GateFailure):
            self.invoke(failure=True)
        rows = [json.loads(row) for row in self.journal.read_text().splitlines()]
        self.assertTrue(rows)
        self.assertFalse(Path(rows[0]['root']).exists())

    def test_static_guard_rejects_python_and_native_roots_and_unset_lookups(self):
        guard = importlib.import_module('check_scratch')
        for folder, filename, source in (
            ('tests', 'bad.py', 'import tempfile\ntempfile.TemporaryDirectory(dir={!r})'),
            ('tools', 'bad.cpp', 'auto root = "{}";'),
        ):
            path = self.root / folder / filename
            for value in BAD_SCRATCH:
                path.write_text(source.format(value))
                self.assertTrue(guard.violations(self.root), (folder, filename, value))
            path.unlink()
        for lookup in (BAD_LOOKUP, BAD_LOOKUP.replace('[', '[\n').replace(']', '\n]')):
            (self.root / 'tests/bad.py').write_text('import os\nvalue=' + lookup)
            self.assertTrue(guard.violations(self.root))
        (self.root / 'tests/bad.py').write_text('import tempfile\nroot=tempfile.TemporaryDirectory()')
        self.assertEqual(guard.violations(self.root), [])

    def test_entry_static_guard_stops_before_pre_steps(self):
        (self.root / 'tests/new_fixture.py').write_text('root=' + repr(BAD_SCRATCH[0]))
        with self.assertRaisesRegex(RuntimeError, 'Scratch policy failed'):
            self.invoke()
        self.assertFalse(self.journal.exists())

    def test_compiler_and_native_environment_preserves_the_gate_root(self):
        import host_objects
        with patch.dict(os.environ, {'TMPDIR': str(self.root)}):
            self.assertEqual(host_objects.compiler_environment(self.root)['TMPDIR'], str(self.root))

    def test_sigterm_cleanup_includes_the_entry_owned_root(self):
        program = '''import os,sys
from pathlib import Path
sys.path.insert(0,sys.argv[1])
import gate_scratch,gate_runner,host_jobs,json
with gate_scratch.environment():
 Path(sys.argv[2]).write_text(json.dumps([os.environ.get('TMPDIR'),os.environ.get(host_jobs.NATIVE_ROOTS)]))
 command=[sys.executable,'-B','-c',"from pathlib import Path;import sys,time;Path(sys.argv[1]).touch();time.sleep(600)",sys.argv[3]]
 gate_runner.run([gate_runner.Phase('hold',command)],Path.cwd())
'''
        record, ready = self.root / 'root-record', self.root / 'ready'
        environment = {**os.environ, host_jobs.GLOBAL_ROOT: str(self.root / 'pool')}
        environment.pop('TMPDIR', None)
        child = subprocess.Popen([sys.executable, '-B', '-c', program, str(ROOT / 'tools'), str(record), str(ready)],
                                 cwd=self.root, env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 30
            while not ready.exists() and child.poll() is None and time.monotonic() < deadline:
                time.sleep(0.01)
            self.assertTrue(ready.exists())
            child.send_signal(signal.SIGTERM)
            self.assertEqual(child.wait(timeout=15), 128 + signal.SIGTERM)
            self.assertTrue(all(not Path(path).exists() for path in json.loads(record.read_text())))
        finally:
            if child.poll() is None:
                child.kill()
                child.wait()


if __name__ == '__main__':
    unittest.main()
