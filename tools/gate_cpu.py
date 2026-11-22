"""Per-phase CPU accounting in a private process, never a shared rusage delta.

All waited descendants contribute user+system CPU to RUSAGE_CHILDREN. Include
this small wrapper's own CPU, so interpreter/setup overhead is not hidden.
The wrapper stays in the phase's process group for the runner's stop protocol.
"""
import json
import os
from pathlib import Path
import resource
import signal
import subprocess
import sys

import host_jobs


def main():
    receipt, limit, *command = sys.argv[1:]
    try:
        wait_log = Path(receipt).with_name('slot-waits')
        with host_jobs.global_slot() as waited:
            environment = {**os.environ, host_jobs.CREDIT: str(Path(receipt).with_name('phase-credit')),
                           host_jobs.CREDIT_POOL: str(host_jobs.global_root()), host_jobs.WAIT_LOG: str(wait_log)}
            try:
                result = subprocess.run(command, env=environment)
            finally:
                worker_wait = sum(map(float, wait_log.read_text().splitlines())) if wait_log.exists() else 0.0
                if waited or worker_wait:
                    print(f'HOST slots waited {waited:.3f}s before phase; '
                          f'{worker_wait:.3f}s aggregate nested job wait', flush=True)
    finally:
        children = resource.getrusage(resource.RUSAGE_CHILDREN)
        own = resource.getrusage(resource.RUSAGE_SELF)
        cpu = children.ru_utime + children.ru_stime + own.ru_utime + own.ru_stime
        Path(receipt).write_text(json.dumps({'cpu_seconds': cpu}) + '\n')
    if result.returncode < 0:
        number = -result.returncode
        if number not in (signal.SIGKILL, signal.SIGSTOP):
            signal.signal(number, signal.SIG_DFL)
        os.kill(os.getpid(), number)
    if cpu > float(limit):
        print(f'CPU budget exceeded: {cpu:.3f}s > {float(limit):.3f}s; '
              'investigate before editing tools/gate-cpu-budgets.json', flush=True)
        return result.returncode or 1
    return result.returncode


if __name__ == '__main__':
    sys.exit(main())
