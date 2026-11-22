"""Actual runner boundary: interrupt one compiler-like child and its descendant."""
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

HERE = Path(__file__).resolve().parents[1]
DRIVER = '''import sys,time
from pathlib import Path
sys.path.insert(0,sys.argv[1])
from check import Run,install_signal_handlers
install_signal_handlers()
out=Path(sys.argv[2]);out.mkdir()
runner=Run(out,{'rustc':sys.executable,'rustdoc':sys.executable},time.monotonic()+20)
runner.run([sys.executable,'-c',sys.argv[3],sys.argv[4]],Path(sys.argv[2]))
'''
CHILD = '''import os,subprocess,sys,time
from pathlib import Path
child=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)'])
Path(sys.argv[1]).write_text(str(os.getpid())+' '+str(child.pid))
time.sleep(30)
'''


def active(pid):
    try:
        # Killed grandchildren may briefly await their init reaper; zombies
        # cannot continue work. Run.communicate reaps the direct child itself.
        return Path(f'/proc/{pid}/stat').read_text().split()[2] != 'Z'
    except FileNotFoundError:
        return False


class ProcessCleanup(unittest.TestCase):
    def test_interrupts_stop_the_entire_active_command_group(self):
        for signum in (signal.SIGTERM, signal.SIGINT):
            with self.subTest(signum=signum), tempfile.TemporaryDirectory() as directory:
                root=Path(directory);ready=root/'ready';pids=[]
                process=subprocess.Popen([sys.executable,'-c',DRIVER,str(HERE),str(root/'out'),CHILD,str(ready)],
                                         stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
                try:
                    until=time.monotonic()+5
                    while not ready.exists() and process.poll() is None and time.monotonic()<until:
                        time.sleep(0.01)
                    self.assertTrue(ready.exists(),'runner did not start its child')
                    pids=list(map(int,ready.read_text().split()))
                    os.kill(process.pid,signum)
                    process.communicate(timeout=5)
                    self.assertNotEqual(process.returncode,0)
                    until=time.monotonic()+2
                    while any(active(pid) for pid in pids) and time.monotonic()<until:
                        time.sleep(0.01)
                    self.assertFalse(any(active(pid) for pid in pids),'interrupted command left working descendants')
                finally:
                    if pids:
                        try:os.killpg(pids[0],signal.SIGKILL)
                        except ProcessLookupError:pass
                    if process.poll() is None:
                        os.killpg(process.pid,signal.SIGKILL)
                    process.communicate()


if __name__=='__main__':unittest.main()
