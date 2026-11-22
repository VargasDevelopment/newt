#!/usr/bin/env python3
"""Runs under canonical Run; rejection children inherit its owned process group."""
import importlib.util
from pathlib import Path
import sys
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('reject_support',Path(sys.argv[1]))
support = importlib.util.module_from_spec(spec); spec.loader.exec_module(support)
support.install_signal_handlers()
exe = sys.argv[2]
valid = b'A1 65 86 30 0 792 528 16 480 50 120 52272 0\n'
invalid = [valid[:-1],valid.replace(b'\n',b'\r\n'),valid.replace(b' 65 ',b' 065 '),
           valid.replace(b' 65 ',b' -1 '),valid.replace(b' 30 ',b' 65536 '),
           valid.replace(b' 0 792',b' 256 792'),valid.replace(b' 52272 ',b' 52274 '),
           valid.replace(b' 65 ',b'  65 '),b'\n',b'x'*1048577+b'\n']
for wire in invalid:
    p = support.inherited_child([exe],input=wire)
    if p.returncode != 2 or p.stdout or p.stderr:
        raise RuntimeError('reject driver failure, never a semantic kill')
