# SPDX-License-Identifier: MIT
"""Shared native_io and host SHA-256 object builders for startup tests."""
from pathlib import Path
import sys


def native_io(run, ws):
    obj = run.output / 'startup-native-io.o'
    run.run([run.tools['clang++'], '--no-default-config', '-x', 'c', '-std=c11',
             '-Wall', '-Wextra', '-Werror', '-O1', '-I', Path(ws) / 'salamander/board/native',
             '-c', Path(ws) / 'salamander/native_io/native/native_io.c', '-o', obj], ws)
    return obj


def sha(run, root):
    """SHA-256 fixture over Root's locked provider; returns the object and its link flags."""
    sys.path.insert(0, str(Path(root) / 'tools'))
    import host_common
    crypto = host_common.crypto_flags()
    obj = run.output / 'startup-sha.o'
    run.run([run.tools['clang++'], '--no-default-config', '-std=c++17', '-O1', '-g',
             '-I', Path(root) / 'rust/tests/include', *crypto, '-c',
             Path(root) / 'rust/tests/phased_reader/physical/sha_fixture.cpp', '-o', obj], root)
    return [obj, *crypto[2:]], Path(crypto[2])
