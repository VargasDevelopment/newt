#!/usr/bin/env python3
"""Reader recovery on the real firmware driver with host ports: the short-turn
route, boot restore policy, Pages cache upgrades, boot resume and picture
restore (rust/tests/reader_short_turn), with their compiled controls.

It links against the same startup crate graph and native objects as
legacy_nvs_test.py, and has its own gate phase so neither phase nears its CPU
budget.
"""
from pathlib import Path
import signal
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import gate_tier  # noqa: E402

legacy = None


def reader_recovery(run, workspace, graph, objects, support, *, controls):
    here = workspace / 'tests/reader_short_turn'
    checks = legacy.load('reader_short_turn', here / 'check.py')
    build = legacy.load('recovery_build', workspace / 'tests/phased_reader/physical/build.py')
    out = run.output / 'reader-recovery'
    out.mkdir()
    fake = out / 'checkpoint-board.o'
    run.run([run.tools['clang++'], '--no-default-config', '-x', 'c', '-std=c11', '-O1', '-g',
             '-fsanitize=address,undefined', '-fno-sanitize-recover=all',
             '-I', workspace / 'salamander/board/native',
             '-c', workspace / 'tests/reader_checkpoint/board_fake.c', '-o', fake], workspace)
    # The startup graph and parser objects already cover this firmware fixture.
    flags = [arg for arg in legacy.link_flags(build, run, graph, [fake, *objects], control=True)
             if arg != '-Dwarnings']
    executable = out / 'positive'
    # Its runs open many pages, so the positive keeps opt 1 (rustc takes the last opt-level).
    run.run([*flags, '-Copt-level=1', here / 'host.rs', '-o', executable], workspace)

    def recorded(command, *, witness=None):
        if witness is None:
            return run.run(command, workspace)
        number = len(run.commands)
        try:
            run.run(command, workspace)
        except RuntimeError:
            stderr = (run.output / f'{number:03d}.stderr').read_text()
            support.require(run.commands[number].get('returncode') == -signal.SIGABRT
                            and 'panicked at ' in stderr and witness in stderr,
                            'short-turn or recovery control must reach its runtime assertion')
        else:
            raise RuntimeError('mutation survived: ' + witness)

    checks.check_short_turn(recorded, flags, workspace, out, executable, controls=controls)
    checks.check_recovery(recorded, flags, workspace, out, executable, controls=controls)


def prove(run, workspace, support):
    run.env['RUSTFLAGS'] = '-Dwarnings -Ccodegen-units=1'
    graph = legacy.startup_graph(run, workspace, support, 'startup-target')
    _, objects = legacy.startup_natives(run, workspace, support)
    controls = gate_tier.negative_controls(__file__)
    reader_recovery(run, workspace, graph, objects, support, controls=controls)
    return dict(host_startup=True, hardware=False, controls=controls)


def main():
    global legacy
    import importlib.util
    spec = importlib.util.spec_from_file_location('legacy_nvs_test', ROOT / 'tests/legacy_nvs_test.py')
    legacy = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(legacy)
    legacy.in_sealed_copy(prove, 'reader-recovery-evidence')
    print('PASS reader recovery: short turns, restore policy, Pages upgrades, boot resume and picture restore')


if __name__ == '__main__':
    main()
