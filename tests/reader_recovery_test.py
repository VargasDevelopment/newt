#!/usr/bin/env python3
"""Reader recovery on the real firmware driver with host ports: the short-turn
route, boot restore policy, Pages cache upgrades, boot resume and picture
restore (rust/tests/reader_short_turn), with their compiled controls.

It links against the same startup crate graph and native objects as
legacy_nvs_test.py, and has its own gate phase so neither phase nears its CPU
budget.
"""
from pathlib import Path
import shutil
import signal
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import gate_tier  # noqa: E402

legacy = None

# #199: these live in the reader-ui crate, so each needs its own crate graph.
CRATE_CONTROLS = (
    # A reload that reports Ready without reloading leaves the source with no read path.
    ('skip-reload', 'reader-ui/src/physical_source.rs',
     '|| self.reload_closed_receipt(self.digest) {', '|| true {',
     'Contents after a cancelled list load reloads the receipt and rebuilds the list'),
    # A cancelled Contents load latched as an I/O failure, as before #199.
    ('menu-cancel-latches', 'reader-ui/src/physical_owner.rs',
     'return MenuPreparation { status: Status::Stale, valid: false };',
     'owner.menu_result(Status::IoError);\n'
     '            return MenuPreparation { status: Status::IoError, valid: false };',
     'a cancelled Contents list load keeps the reader and its page'),
    # A failed reload read as a fresh cancel keeps a changed book open.
    ('changed-book-open', 'reader-ui/src/physical_source.rs',
     'if self.revision.cancelled() {', 'if true {',
     'a changed receipt after a cancel gives Stale and closes the reader'),
)


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
    if controls:
        crate_controls(run, workspace, support, build, [fake, *objects], recorded)


def crate_controls(run, workspace, support, build, inputs, recorded):
    """One mutant tree and target serve every crate control, so after the first
    graph each control rebuilds only reader-ui and the crates above it. The
    sealed workspace and its graph stay unmutated."""
    mutant = run.output / 'crate-controls' / 'rust'
    shutil.copytree(workspace, mutant)
    for name, file, original, mutation, witness in CRATE_CONTROLS:
        source = mutant / file
        text = source.read_text()
        support.require(text.count(original) == 1, 'one causal mutation anchor: ' + name)
        source.write_text(text.replace(original, mutation, 1))
        graph = legacy.startup_graph(run, mutant, support, 'crate-controls-target')
        flags = [arg for arg in legacy.link_flags(build, run, graph, inputs, control=True) if arg != '-Dwarnings']
        program = run.output / 'reader-recovery' / name
        run.run([*flags, mutant / 'tests/reader_short_turn/host.rs', '-o', program], mutant)
        recorded([program, '--cache-only-cancel'], witness=witness)
        source.write_text(text)
    print(f'PASS {len(CRATE_CONTROLS)} reader-ui cancel-reload controls rejected')


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
