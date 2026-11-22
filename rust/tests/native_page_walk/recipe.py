# SPDX-License-Identifier: MIT
"""Native adapter command admission helper checked by rust/tests/landing_test.py."""


def admit_adapter_command(run, support, number, source):
    command = run.commands[number]['command']
    support.require(command[0] == support.receipt_path(run.tools['rustc'])
                    and support.receipt_path(source/'salamander/native_io/lib.rs') in command,
                    'actual native compiler must read this mutant, never baseline helper globals')
