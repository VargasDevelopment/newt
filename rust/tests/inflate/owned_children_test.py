"""Rejection drivers inherit their Run-owned group; accidental detachment is rejected."""
import ast
import json
from run_support import install_signal_handlers, owned_process, process_running as alive
from unittest.mock import patch
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

HERE = Path(__file__).resolve().parent


def validate_driver(source):
    tree = ast.parse(source)
    for node in ast.walk(tree):
        if isinstance(node, ast.Import) and any(name.name == 'subprocess' for name in node.names):
            raise ValueError('rejection drivers must use the canonical inherited-child launcher')
        if isinstance(node, ast.ImportFrom) and node.module == 'subprocess':
            raise ValueError('rejection drivers must use the canonical inherited-child launcher')
    if not any(isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
               and node.func.id == 'inherited_child' for node in ast.walk(tree)):
        raise ValueError('rejection driver has no canonical launcher')



class OwnedChildrenTest(unittest.TestCase):
    def test_all_rejection_drivers_use_the_same_launcher(self):
        drivers = list(HERE.parent.rglob('rejection*.py'))
        self.assertGreaterEqual(len(drivers), 5)
        for path in drivers:
            with self.subTest(path=path): validate_driver(path.read_text())
        for source in ('import subprocess\nsubprocess.Popen([])',
                       'from subprocess import Popen as spawn\nspawn([])',
                       'import subprocess as sp\nsp.run([])', ''):
            with self.assertRaises(ValueError): validate_driver(source)

    def test_reaping_during_process_status_read_is_stopped(self):
        import run_support
        for failure in (FileNotFoundError(), ProcessLookupError()):
            with self.subTest(failure=failure), patch.object(run_support.Path,'read_text',side_effect=failure):
                self.assertFalse(alive(123))
        for record, expected in [('123 (name with spaces) S 1 2',True),
                                 ('123 (name with spaces) Z 1 2',False)]:
            with self.subTest(record=record), patch.object(run_support.Path,'read_text',return_value=record):
                self.assertEqual(alive(123),expected)

    def test_cancellation_during_spawn_is_delivered_after_ownership(self):
        import run_support
        real = subprocess.Popen
        for stop in (signal.SIGINT,signal.SIGTERM):
            child = None
            def spawn(*args,**kwargs):
                nonlocal child
                child = real(*args,**kwargs)
                os.kill(os.getpid(),stop)
                return child
            with self.subTest(stop=stop), patch.object(run_support.subprocess,'Popen',spawn):
                with self.assertRaises(KeyboardInterrupt):
                    with owned_process([sys.executable,'-c','import time;time.sleep(20)'],
                                       start_new_session=True) as _child:
                        self.fail('deferred cancellation did not fire')
            self.assertIsNotNone(child)
            self.assertIsNotNone(child.poll())

    def test_cancellation_during_teardown_finishes_the_registered_cleanup(self):
        for stop in (signal.SIGINT,signal.SIGTERM):
            completed=[]
            def cleanup():
                os.kill(os.getpid(),stop)
                completed.append(True)
            with self.subTest(stop=stop), self.assertRaises(KeyboardInterrupt):
                with owned_process([sys.executable,'-c','import time;time.sleep(20)'],
                                   start_new_session=True,cleanup=cleanup):
                    pass
            self.assertEqual(completed,[True])

    def test_enclosing_cancel_stops_inherited_child_and_detects_detachment(self):
        for stop in (signal.SIGINT, signal.SIGTERM):
            for detached in (False, True):
                with self.subTest(stop=stop, detached=detached), tempfile.TemporaryDirectory() as temporary:
                    directory = Path(temporary)
                    ready = directory/'ready'
                    child = directory/'child.py'
                    child.write_text('import os,time\nfrom pathlib import Path\n'
                                     f'p=Path({str(ready)!r});q=p.with_suffix(".partial");q.write_text(str(os.getpid())+" "+str(os.getppid()));q.replace(p)\n'
                                     'time.sleep(20)\n')
                    driver = directory/'driver.py'
                    prefix = f'import sys\nsys.path.insert(0,{str(HERE)!r})\n'
                    disabled=directory/'disabled'
                    driver_ready=directory/'driver-ready'
                    if detached:
                        body = ('import subprocess,signal\nfrom pathlib import Path\n'
                                'from run_support import owned_process\n'
                                f'with owned_process([sys.executable,{str(child)!r}],start_new_session=True) as p:\n'
                                ' def disable(_signal,_frame):\n'
                                '  for value in (signal.SIGINT,signal.SIGTERM):signal.signal(value,signal.SIG_DFL)\n'
                                f'  Path({str(disabled)!r}).touch()\n'
                                ' signal.signal(signal.SIGUSR1,disable)\n'
                                f' Path({str(driver_ready)!r}).touch()\n'
                                ' p.wait()\n')
                    else:
                        body = f'from run_support import inherited_child\ninherited_child([sys.executable,{str(child)!r}],input=b"",timeout=15)\n'
                    driver.write_text(prefix+body)
                    outer = directory/'outer.py'
                    outer.write_text(prefix+'import time\nfrom pathlib import Path\nimport run_support as s\ns.install_signal_handlers()\n'
                                     f'out=Path({str(directory/"logs")!r});out.mkdir()\n'
                                     'run=s.Run(out,{"rustc":Path(sys.executable),"rustdoc":Path(sys.executable)},time.monotonic()+15)\n'
                                     f'run.run([sys.executable,{str(driver)!r}],Path({str(directory)!r}))\n')
                    pid = None
                    driver_pid = None
                    def cleanup_fixture():
                        nonlocal pid, driver_pid
                        if pid is None and ready.exists():
                            pid, driver_pid = map(int, ready.read_text().split())
                        if driver_pid is not None:
                            try:
                                os.killpg(driver_pid, signal.SIGKILL)
                            except ProcessLookupError:
                                pass
                        if pid is not None:
                            try:
                                os.kill(pid, signal.SIGKILL)
                            except ProcessLookupError:
                                pass
                    with owned_process([sys.executable, '-B', outer], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True, cleanup=cleanup_fixture) as process:
                        deadline = time.monotonic() + 5
                        while not ready.exists() and time.monotonic() < deadline:
                            self.assertIsNone(process.poll(), 'cancellation fixture setup failed')
                            time.sleep(0.01)
                        self.assertTrue(ready.exists(), 'cancellation fixture readiness timed out')
                        pid, driver_pid = map(int, ready.read_text().split())
                        if detached:
                            deadline = time.monotonic() + 2
                            while not driver_ready.exists() and time.monotonic() < deadline:
                                time.sleep(.01)
                            self.assertTrue(driver_ready.exists(), 'driver handler readiness timed out')
                            os.kill(driver_pid, signal.SIGUSR1)
                            deadline = time.monotonic() + 2
                            while not disabled.exists() and time.monotonic() < deadline:
                                time.sleep(0.01)
                            self.assertTrue(disabled.exists(), 'negative cleanup handler removal failed')
                        hold = os.environ.get('NEWT_OWNED_FIXTURE_HOLD')
                        selected = os.environ.get('NEWT_OWNED_FIXTURE_DETACHED') == '1'
                        if hold and detached == selected:
                            marker = Path(hold)
                            partial = marker.with_suffix('.partial')
                            partial.write_text(json.dumps({'harness': os.getpid(), 'outer': process.pid, 'driver': driver_pid, 'child': pid}))
                            partial.replace(marker)
                            while True:
                                signal.pause()
                        process.send_signal(stop)
                        self.assertNotEqual(process.wait(timeout=5), 0)
                        self.assertEqual(alive(pid), detached, 'detached negative must survive, inherited positive must stop')


    def test_enclosing_cancel_reaches_the_test_owned_subgroups(self):
        for stop, detached in ((value, mode) for value in (signal.SIGINT,signal.SIGTERM) for mode in (False,True)):
            with self.subTest(stop=stop, detached=detached), tempfile.TemporaryDirectory() as temporary:
                directory=Path(temporary);marker=directory/'pids.json'
                enclosing=directory/'enclosing.py'
                enclosing.write_text(f'import sys,time\nfrom pathlib import Path\nsys.path.insert(0,{str(HERE)!r})\n'
                                     'import run_support as s\ns.install_signal_handlers()\n'
                                     f'out=Path({str(directory/"logs")!r});out.mkdir()\n'
                                     'run=s.Run(out,{"rustc":Path(sys.executable),"rustdoc":Path(sys.executable)},time.monotonic()+20)\n'
                                     f'run.env["NEWT_OWNED_FIXTURE_HOLD"]={str(marker)!r}\n'
                                     f'run.env["NEWT_OWNED_FIXTURE_DETACHED"]={str(int(detached))!r}\n'
                                     f'run.run([sys.executable,"-B",{str(HERE/"owned_children_test.py")!r},'
                                     '"OwnedChildrenTest.test_enclosing_cancel_stops_inherited_child_and_detects_detachment"],'
                                     f'Path({str(directory)!r}))\n')
                pids = None
                def cleanup_fixture():
                    nonlocal pids
                    if pids is None and marker.exists():
                        pids = json.loads(marker.read_text())
                    if pids:
                        for key in ('outer', 'driver'):
                            try:
                                os.killpg(pids[key], signal.SIGKILL)
                            except ProcessLookupError:
                                pass
                        for pid in pids.values():
                            try:
                                os.kill(pid, signal.SIGKILL)
                            except ProcessLookupError:
                                pass
                with owned_process([sys.executable, '-B', enclosing], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True, cleanup=cleanup_fixture) as process:
                    deadline = time.monotonic() + 5
                    while not marker.exists() and time.monotonic() < deadline:
                        self.assertIsNone(process.poll(), 'enclosing fixture setup failed')
                        time.sleep(0.01)
                    self.assertTrue(marker.exists(), 'enclosing readiness timed out')
                    pids = json.loads(marker.read_text())
                    process.send_signal(stop)
                    self.assertNotEqual(process.wait(timeout=8), 0)
                    self.assertEqual([key for key, pid in pids.items() if alive(pid)], [], 'a test-owned process escaped enclosing cancellation')



if __name__ == '__main__':
    install_signal_handlers()
    unittest.main()
