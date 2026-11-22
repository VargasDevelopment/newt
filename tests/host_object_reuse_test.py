"""One real compilation must survive two fresh gate scratch environments."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
PROBE = '''import json,os,sys
from pathlib import Path
sys.path.insert(0,sys.argv[1])
import gate_scratch,host_common,host_jobs
from host_objects import HostObjects
root=Path(sys.argv[2])
with gate_scratch.environment(root):
 cache=HostObjects(root,host_common.COMPILER)
 result=cache.object(root/'unit.cpp',['-std=c++17','-O1'],[])
 cache.finish()
 assert cache.environment['TMPDIR']==os.environ.get('TMPDIR')
 assert cache.environment[host_jobs.NATIVE_ROOTS]==os.environ.get(host_jobs.NATIVE_ROOTS)
 print('REUSE '+json.dumps({'compiled':cache.compiled,'hits':cache.hits,'object':str(result),
                          'scratch':cache.environment['TMPDIR'],'mounts':cache.environment[host_jobs.NATIVE_ROOTS]}))
'''


class ReuseTests(unittest.TestCase):
    def test_two_gate_processes_compile_once_despite_different_run_paths(self):
        with tempfile.TemporaryDirectory(prefix='newt-reuse-proof-') as directory:
            root = Path(directory).resolve()
            (root / 'unit.cpp').write_text('int value() { return 7; }\n')
            rows = []
            for name in ('first', 'second'):
                scratch = root / name
                scratch.mkdir(mode=0o700)
                child = subprocess.run([sys.executable, '-B', '-c', PROBE, str(ROOT / 'tools'), str(root)],
                                       cwd=ROOT, check=True, capture_output=True, text=True,
                                       env={**os.environ, 'TMPDIR': str(scratch), 'NEWT_HOST_OBJECT_CACHE': '1'})
                rows.append(json.loads(next(line.removeprefix('REUSE ') for line in child.stdout.splitlines()
                                            if line.startswith('REUSE '))))
            self.assertNotEqual(rows[0]['scratch'], rows[1]['scratch'])
            self.assertNotEqual(rows[0]['mounts'], rows[1]['mounts'])
            self.assertEqual([(r['compiled'], r['hits']) for r in rows], [(1, 0), (0, 1)], rows)
            self.assertEqual(rows[0]['object'], rows[1]['object'])
            print('PASS two fresh gate processes: exactly one compiled object and one cache hit')


if __name__ == '__main__':
    unittest.main()
