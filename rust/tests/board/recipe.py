# SPDX-License-Identifier: MIT
"""Callable helpers for building the Salamander board/native_io host adapters."""
from pathlib import Path
HERE = Path(__file__).resolve().parent
BOARD = HERE.parents[1] / 'salamander/board'
NATIVE = HERE.parents[1] / 'salamander/native_io'
HOST = 'x86_64-unknown-linux-gnu'
RV32 = 'riscv32imc-unknown-none-elf'


def common(run, triple, deps, libraries, panic, *, output=None):
    args = [run.tools['rustc'], '--edition=2024', '--target', triple,
            '-Dwarnings', '-Cpanic=' + panic, '-Copt-level=s', '-Ccodegen-units=1',
            '-L', 'dependency=' + str(deps), '-L', 'dependency=' + str(output or run.output)]
    for name in ('newt_core', 'newt_phased_reader', 'newt_phased_sink', 'newt_reader_ui', 'newt_target_expat', 'newt_reader_navigation'):
        args += ['--extern', name + '=' + str(libraries[name])]
    heap = list(deps.glob('libnewt_sdk_heap-*.rlib'))
    if len(heap) != 1:
        raise RuntimeError('one required SDK heap library from the same Cargo graph')
    args += ['--extern', 'newt_sdk_heap=' + str(heap[0])]
    return args


def adapter(run, ws, triple, deps, libraries, *, panic='abort', output=None):
    output = output or run.output
    output.mkdir(exist_ok=True)
    native = output / ('libnewt_native_io-' + triple + '-' + panic + '.rlib')
    run.run([*common(run, triple, deps, libraries, panic, output=output), '--crate-type=rlib',
             '--crate-name=newt_native_io', '--extern',
             'newt_native_storage=' + str(libraries['newt_native_storage']),
             '--extern', 'newt_library_current=' + str(libraries['newt_library_current']),
             NATIVE / 'lib.rs', '-o', native], ws)
    policy = output / ('libnewt_input_policy-board-' + triple + '-' + panic + '.rlib')
    run.run([*common(run, triple, deps, libraries, panic, output=output), '--crate-type=rlib',
             '--crate-name=newt_input_policy', BOARD.parent / 'input/lib.rs',
             '-o', policy], ws)
    library = output / ('libnewt_c3_board-' + triple + '-' + panic + '.rlib')
    run.run([*common(run, triple, deps, libraries, panic, output=output), '--crate-type=rlib',
             '--crate-name=newt_c3_board', '--extern', 'newt_native_io=' + str(native),
             '--extern', 'newt_input_policy=' + str(policy),
             BOARD / 'lib.rs', '-o', library], ws)
    return library
