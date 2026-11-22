# SPDX-License-Identifier: MIT
"""Compiler command receipt helper checked by rust/tests/landing_test.py."""


def compiler_commands(run, support):
    compilers = {support.receipt_path(run.tools[name]) for name in ('rustc', 'cargo', 'clang++')}
    output = support.receipt_path(run.output)
    commands = [['@OUTPUT@' if arg == output else arg.replace(output + '/', '@OUTPUT@/')
                 for arg in row['command']]
                for row in run.commands if row['command'][0] in compilers]
    support.require(bool(commands), 'compiler admission requires recorded compiler commands')
    return commands
