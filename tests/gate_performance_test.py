"""Parity scheduling coverage and CPU budgets, independent of host load."""
from concurrent.futures import ThreadPoolExecutor
from contextlib import redirect_stdout
import io
import json
import os
import resource
import signal
import subprocess
import threading
import time
import tempfile
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'tools'), str(ROOT / 'tests')]
import gate_plan
import host_jobs
import gate_stop_test as stops
import gate_runner


class PerformanceTests(unittest.TestCase):
    def setUp(self):
        # These fixtures intentionally require simultaneous fake phases even
        # when the enclosing real gate has --host-jobs 1.
        temporary=tempfile.TemporaryDirectory(prefix='newt-performance-pool-')
        self.addCleanup(temporary.cleanup)
        environment=patch.dict(os.environ,{host_jobs.GLOBAL_JOBS:'8',host_jobs.GLOBAL_ROOT:temporary.name,'NEWT_GATE_CONTROLS':'all'})
        environment.start();self.addCleanup(environment.stop)

    def test_stop_reports_before_scratch_cleanup(self):
        events = []
        finished = threading.Semaphore(0)
        with patch.object(gate_runner, 'STOP_REMOVES', ['scratch']), \
             patch.object(gate_runner.os, 'write', side_effect=lambda fd, text: events.append('report') or len(text)), \
             patch.object(gate_runner.shutil, 'rmtree', side_effect=lambda *a, **kw: events.append('cleanup')):
            gate_runner.conclude(signal.SIGTERM, 1, finished)
        self.assertEqual(events, ['report', 'cleanup'])
        self.assertTrue(finished.acquire(blocking=False))

    def test_exec_trampoline_restores_native_signal_defaults(self):
        for number in (signal.SIGPIPE, signal.SIGXFSZ):
            with self.subTest(signal=number):
                children = gate_runner.Children()
                try:
                    result = children.run(['/bin/sh', '-c', f'ulimit -c 0; kill -{number.value} $$; exit 99'],
                                          stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                finally:
                    children.stop()
                self.assertEqual(result.returncode, -number)

    def run_cpu(self, name, program, budget):
        children = gate_runner.Children(cpu_budgets={'default_seconds': budget,
                                                     'critical_path_seconds': budget, 'phases': {}})
        return gate_runner.execute(gate_runner.Phase(name, (sys.executable, '-B', '-c', program)),
                                   ROOT, True, children), children.timings

    def test_child_and_grandchild_cpu_exceeds_budget(self):
        burn = 'import time; start=time.process_time()\nwhile time.process_time()-start<0.15: pass'
        program = f'import subprocess,sys; subprocess.run([sys.executable,"-c",{burn!r}],check=True)'
        (passed, output, status), timings = self.run_cpu('descendants', program, 0.05)
        self.assertFalse(passed, (output, status))
        self.assertGreaterEqual(timings['descendants']['cpu_seconds'], 0.15)
        self.assertIn(b'CPU budget exceeded', output)

    def test_sleep_does_not_charge_another_phases_cpu(self):
        burn = 'import time; start=time.process_time()\nwhile time.process_time()-start<1.0: pass'
        # Three cold interpreter starts count as phase CPU. Keep them well below
        # the burn, and reap the busy phase before releasing the blocked sleeper.
        with tempfile.TemporaryDirectory() as directory, ThreadPoolExecutor(2) as pool:
            fifo, ready = Path(directory)/'release', Path(directory)/'ready'
            os.mkfifo(fifo, 0o600)
            with fifo.open('r+b', buffering=0) as release:
                program = (f'import os; fd=os.open({str(fifo)!r},os.O_RDONLY); '
                           f'open({str(ready)!r},"w").close(); assert os.read(fd,1)==b"1"')
                asleep = pool.submit(self.run_cpu, 'sleep', program, 5)
                try:
                    deadline = time.monotonic()+30
                    while not ready.exists() and time.monotonic()<deadline:
                        time.sleep(0.01)
                    self.assertTrue(ready.exists(), 'sleep phase never started')
                    busy_result, busy_times = pool.submit(self.run_cpu, 'busy', burn, 5).result()
                finally:
                    release.write(b'1')
                sleep_result, sleep_times = asleep.result()
        self.assertTrue(busy_result[0], busy_result)
        self.assertTrue(sleep_result[0], sleep_result)
        self.assertGreaterEqual(busy_times['busy']['cpu_seconds'], 1.0)
        self.assertLess(sleep_times['sleep']['cpu_seconds'], 0.5)

    def test_sleep_rejects_shared_parent_cpu_accounting(self):
        original = gate_runner.Children.run

        def shared_usage(children, *args, **kwargs):
            before = resource.getrusage(resource.RUSAGE_CHILDREN)
            result = original(children, *args, **kwargs)
            after = resource.getrusage(resource.RUSAGE_CHILDREN)
            children.timings[kwargs['phase_name']]['cpu_seconds'] = (
                after.ru_utime+after.ru_stime-before.ru_utime-before.ru_stime)
            return result

        case = PerformanceTests('test_sleep_does_not_charge_another_phases_cpu')
        result = unittest.TestResult()
        with patch.object(gate_runner.Children, 'run', shared_usage):
            case.run(result)
        self.assertEqual(result.errors, [])
        self.assertEqual(len(result.failures), 1)
        self.assertIn('not less than 0.5', result.failures[0][1])

    def test_budget_failure_blocks_dependents_and_reports_totals(self):
        budgets = {'default_seconds': 5, 'critical_path_seconds': 0.01, 'phases': {}}
        output = io.StringIO()
        with redirect_stdout(output), self.assertRaises(gate_runner.GateFailure):
            gate_runner.run([gate_runner.Phase('over', (sys.executable, '-c',
                                 'import time; start=time.process_time()\nwhile time.process_time()-start<0.05: pass')),
                             gate_runner.Phase('dependent', (sys.executable, '-c', 'pass'), after=('over',))],
                            ROOT, 2, cpu_budgets=budgets)
        self.assertNotIn('START dependent', output.getvalue())
        self.assertIn('GATE CPU total', output.getvalue())

    def test_measured_stop_still_ends_wrappers_children_and_blocked_output(self):
        fixture = stops.StopTests()
        fixture.setUp()
        try:
            policy = gate_runner.cpu_budgets()
            supervisor = stops.SUPERVISOR.replace(
                'gate_runner.run(plan, Path.cwd(), int(jobs))',
                f'gate_runner.run(plan, Path.cwd(), int(jobs), cpu_budgets={policy!r})')
            cases = [('SIGTERM', 1), ('stubborn', 2), ('full-SIGTERM', 2), ('broken', 2)]
            with patch.object(stops, 'SUPERVISOR', supervisor):
                problems = fixture.stopping_problems([('measured', stops.TOOLS, case, jobs) for case, jobs in cases])
            self.assertEqual(problems, {('measured', case, jobs): [] for case, jobs in cases})
        finally:
            fixture.tearDown()

    def test_measured_prerequisite_interrupt_ends_detached_descendants(self):
        for number in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                program = ("import sys; sys.path.insert(0,sys.argv[1]); "
                           "import gate_runner; gate_runner.GRACE=1; "
                           "gate_runner.phase_run([sys.executable,'-B','-c',sys.argv[2],'a.pids','stubborn'], "
                           "check=True,cpu_budgets=gate_runner.cpu_budgets())")
                parent = subprocess.Popen([sys.executable, '-B', '-c', program, str(ROOT/'tools'), stops.LONG],
                                          cwd=root, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                try:
                    deadline=time.monotonic()+30
                    while not (root/'a.pids').exists() and parent.poll() is None and time.monotonic()<deadline:
                        time.sleep(0.01)
                    self.assertTrue((root/'a.pids').exists(), 'prerequisite never started')
                    parent.send_signal(number)
                    parent.wait(timeout=10)
                    pids=stops.recorded(root)
                    deadline=time.monotonic()+3
                    while any(stops.alive(pid) for pid in pids) and time.monotonic()<deadline:
                        time.sleep(0.01)
                    self.assertFalse([pid for pid in pids if stops.alive(pid)], 'detached prerequisite outlived signal')
                finally:
                    for pid in stops.recorded(root):
                        try: os.kill(pid,signal.SIGKILL)
                        except ProcessLookupError: pass
                    parent.kill();parent.wait()

    def test_invalid_policy_fails_closed_and_new_phases_have_a_default(self):
        policy = gate_runner.cpu_budgets(fresh_objects=False)
        fresh = gate_runner.cpu_budgets(fresh_objects=True)
        self.assertEqual(fresh['default_seconds'], policy['default_seconds'] * policy['fresh_objects_multiplier'])
        self.assertEqual(fresh['critical_path_seconds'], policy['critical_path_seconds'] * policy['fresh_objects_multiplier'])
        self.assertNotIn('a-new-phase', policy['phases'])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'budgets.json'
            for replacement in ({'default_seconds': 0}, {'critical_path_seconds': float('inf')},
                                {'default_reason': ''}, {'phases': {'bad': {'seconds': -1, 'reason': 'bad'}}}):
                path.write_text(json.dumps({**policy, **replacement}))
                with self.assertRaises(ValueError):
                    gate_runner.cpu_budgets(path)

    def test_producer_root_has_no_linked_ancestors(self):
        import host_common
        with host_common.native_storage_root() as directory:
            root=ROOT/directory
            self.assertFalse(Path(directory).is_absolute())
            self.assertEqual(root,root.resolve())
            self.assertTrue(all(not parent.is_symlink() for parent in (root,*root.parents)))
            self.assertTrue(root.is_relative_to(ROOT / ".build"))
            self.assertEqual(root.stat().st_mode & 0o777,0o700)
        self.assertFalse(root.exists())

    def test_native_storage_rejects_linked_parent(self):
        import host_common
        with tempfile.TemporaryDirectory(dir=ROOT/'.build') as directory:
            parent=Path(directory)
            real=parent/'real';real.mkdir()
            linked=parent/'linked';linked.symlink_to(real,target_is_directory=True)
            with patch.dict(os.environ,{host_jobs.NATIVE_ROOTS:str(linked)}):
                with self.assertRaisesRegex(RuntimeError,'symlink or alias'):
                    with host_common.native_storage_root():
                        self.fail('linked ancestor accepted')

    def test_phase_budget_names_must_exist_in_the_gate_plan(self):
        policy=gate_runner.cpu_budgets(fresh_objects=False)
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'budgets.json'
            entry={'seconds':10,'reason':'test name validation'}
            path.write_text(json.dumps({**policy,'phases':{'typo-not-in-plan':entry}}))
            with self.assertRaisesRegex(ValueError,'Unknown gate CPU phase'):
                gate_runner.cpu_budgets(path,fresh_objects=False)
            path.write_text(json.dumps({**policy,'phases':{gate_plan.plan('linux',False)[0].name:entry}}))
            self.assertIn(gate_plan.plan('linux',False)[0].name,gate_runner.cpu_budgets(path)['phases'])

    def test_interrupt_between_spawn_and_registration_reaps_the_prerequisite(self):
        program = """import os,signal,subprocess,sys,time
from pathlib import Path
sys.path.insert(0,sys.argv[1])
import gate_runner
real=gate_runner.subprocess.Popen
def interrupted(*args,**kwargs):
 child=real(*args,**kwargs)
 Path('spawned').write_text(str(child.pid))
 deadline=time.monotonic()+10
 while not Path('ready').exists() and time.monotonic()<deadline: time.sleep(0.01)
 os.kill(os.getpid(),signal.SIGTERM)
 return child
gate_runner.subprocess.Popen=interrupted
gate_runner.GRACE=0.5
gate_runner.phase_run([sys.executable,'-B','-c',"from pathlib import Path;import time,signal;assert not {signal.SIGINT,signal.SIGTERM,signal.SIGHUP}&signal.pthread_sigmask(signal.SIG_BLOCK,[]);Path('ready').touch();time.sleep(600)"],check=True,cpu_budgets=gate_runner.cpu_budgets())
"""
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            parent=subprocess.Popen([sys.executable,'-B','-c',program,str(ROOT/'tools')],cwd=root,
                                    stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            pid=None
            try:
                parent.wait(timeout=20)
                self.assertTrue((root/'ready').exists(),'prerequisite failed to reach launch window')
                pid=int((root/'spawned').read_text())
                deadline=time.monotonic()+3
                while stops.alive(pid) and time.monotonic()<deadline: time.sleep(0.01)
                self.assertFalse(stops.alive(pid),'signal escaped between Popen and live registration')
            finally:
                parent.kill();parent.wait()
                if (root/'spawned').exists():
                    try: os.killpg(int((root/'spawned').read_text()),signal.SIGKILL)
                    except ProcessLookupError: pass

    def test_measured_signal_and_nonzero_exits_stay_failures(self):
        for code in ('raise SystemExit(7)', 'import os,signal; os.kill(os.getpid(),signal.SIGKILL)'):
            (passed, output, status), timings = self.run_cpu('failure', code, 5)
            self.assertFalse(passed, (output, status))
            self.assertIn('failure', timings)


if __name__ == '__main__':
    unittest.main()
