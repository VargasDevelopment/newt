"""Restore signal defaults and mask, then exec in the registered process group.

A tiny exec trampoline avoids unsafe preexec_fn work in the threaded runner.
The close-on-exec error pipe preserves synchronous Popen startup diagnostics.
Resetting SIGINT, SIGTERM and SIGHUP to SIG_DFL keeps inherited ignored stop
signals (nohup, background jobs, IDE launchers) out of phases, while the gate
itself still honours them.
"""
import json
import os
import signal
import sys

held, error_fd, *command = sys.argv[1:]
error_fd = int(error_fd)
os.set_inheritable(error_fd, False)
for number in (signal.SIGPIPE, signal.SIGXFSZ, signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
    signal.signal(number, signal.SIG_DFL)
signal.pthread_sigmask(signal.SIG_SETMASK, {int(number) for number in held.split(',') if number})
try:
    os.execvpe(command[0], command, os.environ)
except OSError as error:
    os.write(error_fd, json.dumps((error.errno, error.strerror, error.filename)).encode())
    sys.exit(127)
