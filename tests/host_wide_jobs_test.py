"""Real concurrent fake gates share capacity, lend compiler credit and survive kills."""
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
TOOLS = ROOT / 'tools'
sys.path.insert(0, str(TOOLS))
import host_jobs
import gate_runner

WORK = '''import fcntl,json,sys,time
from pathlib import Path
root,name=sys.argv[1:3]; root=Path(root)
def change(delta):
 with (root/'state.lock').open('a+') as lock:
  fcntl.flock(lock,fcntl.LOCK_EX)
  state=json.loads((root/'state.json').read_text())
  state['active']+=delta;state['peak']=max(state['peak'],state['active'])
  if delta<0:state['completed'].append(name)
  (root/'state.json').write_text(json.dumps(state))
change(1);time.sleep(0.15);change(-1)
'''
NESTED = '''import sys
sys.path.insert(0,sys.argv[1])
import host_jobs
work,root,name=sys.argv[2:5]
host_jobs.run_all([lambda i=i:host_jobs.run([sys.executable,'-B','-c',work,root,name+str(i)],check=True) for i in range(3)])
'''
GATE = '''import sys
from pathlib import Path
sys.path.insert(0,sys.argv[1])
import gate_runner
work,nested,root,name,mode=sys.argv[2:7]
command=lambda i: (sys.executable,'-B','-c',nested,sys.argv[1],work,root,name+str(i)) if mode=='nested' else (sys.executable,'-B','-c',work,root,name+str(i))
phases=[gate_runner.Phase(name+str(i),command(i)) for i in range(4)]
with gate_runner.job_environment(Path.cwd(),4):
 gate_runner.run(phases,Path.cwd(),4,cpu_budgets={'default_seconds':60,'critical_path_seconds':60,'phases':{}})
'''
HOLDER = '''import sys,time
from pathlib import Path
sys.path.insert(0,sys.argv[1])
import host_jobs
with host_jobs.global_slot():
 Path(sys.argv[2]).write_text('held')
 time.sleep(600)
'''


class HostSlotsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='newt-global-slots-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root/'state.json').write_text(json.dumps({'active':0,'peak':0,'completed':[]}))

    def environment(self, count):
        environment = {**os.environ, host_jobs.GLOBAL_JOBS:str(count),
                       host_jobs.GLOBAL_ROOT:str(self.root/'pool')}
        for name in (host_jobs.CREDIT,host_jobs.CREDIT_POOL,host_jobs.WAIT_LOG):
            environment.pop(name,None)
        return environment

    def gates(self, count, mode='plain'):
        children=[]; outputs=[]
        try:
            for name in ('a','b'):
                directory=self.root/name;directory.mkdir()
                output=(directory/'output').open('w+');outputs.append(output)
                children.append(subprocess.Popen([sys.executable,'-B','-c',GATE,str(TOOLS),WORK,NESTED,
                                                  str(self.root),name,mode],cwd=directory,
                                                 env=self.environment(count),stdout=output,stderr=subprocess.STDOUT))
            for child,output in zip(children,outputs):
                self.assertEqual(child.wait(timeout=60),0,Path(output.name).read_text())
            result=json.loads((self.root/'state.json').read_text())
            self.assertEqual(result['active'],0)
            self.assertEqual(len(result['completed']),24 if mode=='nested' else 8)
            self.assertEqual(len(set(result['completed'])),len(result['completed']))
            self.assertLessEqual(result['peak'],count)
            for output in outputs:output.seek(0)
            return ''.join(output.read() for output in outputs)
        finally:
            for child in children:
                if child.poll() is None:child.kill();child.wait()
            for output in outputs:output.close()

    def test_two_concurrent_gates_never_exceed_host_capacity(self):
        output=self.gates(2)
        self.assertIn('HOST slots waited',output)
        self.assertTrue({p.name for p in (self.root/'pool').iterdir()} <= {'slot-0','slot-1'})

    def test_one_slot_lends_credit_to_nested_compilers_without_deadlock(self):
        self.assertIn('aggregate nested job wait',self.gates(1,'nested'))

    def test_sigkill_releases_the_holders_slot_without_unlinking_it(self):
        ready=self.root/'ready'
        holder=subprocess.Popen([sys.executable,'-B','-c',HOLDER,str(TOOLS),str(ready)],env=self.environment(1))
        try:
            deadline=time.monotonic()+30
            while not ready.exists() and holder.poll() is None and time.monotonic()<deadline:time.sleep(0.01)
            self.assertTrue(ready.exists())
            inode=(self.root/'pool/slot-0').stat().st_ino
            holder.kill();holder.wait()
            probe=subprocess.run([sys.executable,'-B','-c',
                                  'import sys;sys.path.insert(0,sys.argv[1]);import host_jobs\nwith host_jobs.global_slot(): print("acquired")',
                                  str(TOOLS)],env=self.environment(1),capture_output=True,text=True,timeout=10)
            self.assertEqual((probe.returncode,probe.stdout.strip()),(0,'acquired'),probe.stderr)
            self.assertEqual((self.root/'pool/slot-0').stat().st_ino,inode)
        finally:
            if holder.poll() is None:holder.kill();holder.wait()

    def test_default_pool_is_private_to_current_uid(self):
        with patch.dict(os.environ, {}, clear=True):
            self.assertEqual(host_jobs.global_root(), Path(f'/var/tmp/newt-host-job-slots-v2-{os.getuid()}'))

    def test_pool_rejects_symlinks_and_foreign_owners(self):
        target=self.root/'target';target.mkdir()
        pool=self.root/'pool';pool.symlink_to(target, target_is_directory=True)
        with patch.dict(os.environ,self.environment(1)):
            with self.assertRaisesRegex(RuntimeError,'owned.*directory'):
                with host_jobs.global_slot(): pass
            pool.unlink();pool.mkdir(mode=0o700)
            with patch.object(host_jobs.os,'getuid',return_value=os.getuid()+1):
                with self.assertRaisesRegex(RuntimeError,'owned.*directory'):
                    with host_jobs.global_slot(): pass

    def test_acquiring_an_existing_slot_refreshes_its_cleanup_age(self):
        pool=self.root/'pool';pool.mkdir(mode=0o700)
        slot=pool/'slot-0';slot.touch();os.utime(slot,(1,1))
        with patch.dict(os.environ,self.environment(1)):
            before=time.time()
            with host_jobs.global_slot():
                # Filesystem timestamps can use a coarse kernel clock. The old
                # value (epoch second 1) must become a current acquisition time.
                self.assertGreaterEqual(slot.stat().st_mtime,before-2)
                self.assertLessEqual(slot.stat().st_mtime,time.time()+2)

    def test_invalid_capacity_fails_closed(self):
        for value in ('0','-1','nope'):
            result=subprocess.run([sys.executable,'-B','-c',
                                   'import sys;sys.path.insert(0,sys.argv[1]);import host_jobs;host_jobs.global_limit()',str(TOOLS)],
                                  env={**self.environment(1),host_jobs.GLOBAL_JOBS:value},capture_output=True,text=True)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('must be a positive integer',result.stderr)


class CpuCapacityTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='newt-cgroup-cpus-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.cgroup = self.root / 'cgroup'
        self.membership = self.root / 'membership'
        # A v1 controller entry must not be mistaken for the unified membership.
        self.membership.write_text('7:cpu:/wrong\n0::/service/worker\n')
        for target, name, value in ((host_jobs, 'CGROUP_ROOT', self.cgroup),
                                    (host_jobs, 'CGROUP_MEMBERSHIP', self.membership)):
            mocked = patch.object(target, name, value, create=True)
            mocked.start(); self.addCleanup(mocked.stop)
        for mocked in (patch.dict(os.environ, {}, clear=True),
                       patch.object(os, 'cpu_count', return_value=16),
                       patch.object(os, 'sched_getaffinity', return_value=set(range(16)), create=True)):
            mocked.start(); self.addCleanup(mocked.stop)

    def quota(self, group, value):
        path = self.cgroup / group / 'cpu.max'
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value + '\n')

    def defaults(self, capacity):
        self.assertEqual(host_jobs.global_limit(), capacity)
        self.assertEqual(host_jobs.job_limit(), capacity)
        self.assertEqual(gate_runner.default_jobs(), capacity)

    def test_quota_rounds_up_and_never_below_one(self):
        for value, expected in (('200000 100000', 2), ('250000 100000', 3), ('1 100000', 1)):
            with self.subTest(value=value):
                self.quota('service/worker', value)
                self.defaults(expected)
                self.assertIn(f'cgroup v2 cpu.max={expected}', host_jobs.available_cpus()[1])

    def test_max_leaves_affinity_authoritative(self):
        self.quota('service/worker', 'max 100000')
        with patch.object(os, 'sched_getaffinity', return_value={1, 3, 5, 7, 9, 11}):
            self.defaults(6)
            self.assertIn('cpu.max=unlimited', host_jobs.available_cpus()[1])

    def test_tightest_ancestor_including_root_bounds_nested_max(self):
        self.quota('service/worker', 'max 100000')
        self.quota('service', '800000 100000')
        self.quota('', '200000 100000')
        self.defaults(2)
        self.assertIn(str(self.cgroup / 'cpu.max'), host_jobs.available_cpus()[1])
        self.quota('service/worker', '100000 100000')
        self.defaults(1)

    def test_affinity_smaller_than_quota_wins(self):
        self.quota('service/worker', '800000 100000')
        with patch.object(os, 'sched_getaffinity', return_value={2, 4}):
            self.defaults(2)
            self.assertIn('affinity=2', host_jobs.available_cpus()[1])

    def test_missing_or_malformed_child_still_checks_parent(self):
        self.quota('service', '200000 100000')
        for value in (None, 'broken', '0 100000', '100000 0'):
            with self.subTest(value=value):
                if value is not None:
                    self.quota('service/worker', value)
                self.defaults(2)

    def test_unavailable_cgroup_and_affinity_fallbacks(self):
        with patch.object(os, 'sched_getaffinity', return_value={0, 2, 4}):
            self.defaults(3)
            self.assertIn('cpu.max=unavailable', host_jobs.available_cpus()[1])
        self.quota('service/worker', '200000 100000')
        with patch.object(os, 'sched_getaffinity', side_effect=OSError('unsupported')):
            self.defaults(2)
            self.assertIn('affinity unavailable', host_jobs.available_cpus()[1])

    def test_explicit_overrides_and_reported_source(self):
        import check
        self.quota('service/worker', '400000 100000')
        for arguments, capacity, local, source in (([], 7, 4, host_jobs.GLOBAL_JOBS),
                                                   (['--host-jobs', '9', '-j', '3'], 9, 3, '--host-jobs')):
            with self.subTest(arguments=arguments), patch.dict(os.environ, {host_jobs.GLOBAL_JOBS: '7'}):
                def gate(args):
                    self.assertEqual(host_jobs.global_limit(), capacity)
                    self.assertEqual(host_jobs.job_limit(), local)
                output = io.StringIO()
                with patch.object(sys, 'argv', ['check.py', '--profile', 'linux', *arguments]), \
                     patch.object(check.sys, 'platform', 'linux'), patch.object(check, 'gate', side_effect=gate), \
                     patch('sys.stdout', output):
                    check.configured_main()
                self.assertIn(f'HOST slots capacity {capacity} ({source})', output.getvalue())
                self.assertIn(f'local jobs {local}', output.getvalue())
        with patch.dict(os.environ, {host_jobs.JOBS: '5'}):
            self.assertEqual(host_jobs.job_limit(), 5)
            self.assertEqual(host_jobs.global_limit(), 4)


if __name__=='__main__':unittest.main()
