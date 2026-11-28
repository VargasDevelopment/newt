#!/usr/bin/env python3
"""Legacy-NVS invariants and actual startup wiring (cold restore, start failures),
using existing sealed host ports."""
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import gate_tier


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def test_binary(run, workspace, target):
    wire = run.cargo(workspace, target, 'test', '--release', '-p', 'newt-native-storage',
                     '--test', 'legacy_reclaim', '--target', 'x86_64-unknown-linux-gnu',
                     '--no-run', '--message-format=json')
    outputs = [row['executable'] for row in map(json.loads, wire.splitlines())
               if row.get('reason') == 'compiler-artifact' and row.get('executable')
               and row['target']['name'] == 'legacy_reclaim']
    if len(outputs) != 1:
        raise RuntimeError('one compiled legacy-reclaim test executable required')
    return outputs[0]


def startup_graph(run, workspace, support, target):
    clock = load('legacy_clock', workspace / 'tests/board/clock/recipe.py')
    board = load('legacy_board', workspace / 'tests/board/recipe.py')
    checkpoint = load('legacy_checkpoint', workspace / 'tests/reader_checkpoint/recipe.py')
    return checkpoint.graph(run, workspace, run.output / target, clock, board, support.require)


def startup_natives(run, workspace, support):
    """(board fake, other link inputs): no control mutates these sources."""
    clock = load('legacy_clock', workspace / 'tests/board/clock/recipe.py')
    home = load('legacy_home', workspace / 'tests/home_startup/recipe.py')
    build = load('legacy_build', workspace / 'tests/phased_reader/physical/build.py')
    build.storage_host(run, workspace, support, workspace / 'tests/phased_reader/physical')
    fake = run.output / 'startup-board.o'
    run.run([run.tools['clang++'], '--no-default-config', '-x', 'c', '-std=c11', '-Wall',
             '-Wextra', '-Werror', '-O1', '-g', '-I', workspace / 'salamander/board/native',
             '-c', workspace / 'tests/reader_statistics/board_fake.c', '-o', fake], workspace)
    hashing, _ = home.sha(run, ROOT)
    return fake, [home.native_io(run, workspace),
                  *(run.output / (name + '.o') for name in clock.NATIVE_LINK),
                  '-fsanitize=address,undefined', '-fno-sanitize-recover=all', *hashing, '-lstdc++']


def link_flags(build, run, graph, inputs, *, control):
    """rustc flags for a program over `graph`. A control only has to reach its
    runtime assertion, so it skips LTO and optimization: most of the compile
    CPU, and no change in behavior."""
    flags = [str(arg).replace('-Clink-arg=-fuse-ld=', '-Clink-arg=--ld-path=')
             for arg in build.host_link_args(run, graph, inputs)]
    return [arg for arg in flags if arg != '-Clto=fat'] + ['-Copt-level=0'] if control else flags


def startup(run, workspace, graph, inputs, label, *, control=False):
    build = load('legacy_build', workspace / 'tests/phased_reader/physical/build.py')
    executable = run.output / label
    run.run([*link_flags(build, run, graph, inputs, control=control), *([] if control else ['-Copt-level=s']),
             '-Dwarnings', '--cfg', 'feature="dev-usb"', '--crate-name=legacy_startup',
             workspace / 'tests/legacy_nvs/startup.rs', '-o', executable], workspace)
    return executable


def start_failure_graph(run, workspace, support):
    """The shipped app_startup's crates; its own source compiles per program."""
    clock = load('failure_clock', workspace / 'tests/board/clock/recipe.py')
    target = run.output / 'start-failure-target'
    run.cargo(workspace, target, 'build', '--release', '--lib', '-p', 'newt-native-input-owner',
              '-p', 'newt-device-ui', '-p', 'newt-tiny-app', '-p', 'newt-stage-home',
              '--features', 'newt-native-input-owner/native-atomics,newt-device-ui/dev-usb',
              '--target', 'x86_64-unknown-linux-gnu')
    return clock.rlibs(target / 'x86_64-unknown-linux-gnu/release/deps', support.require)


def start_failure(run, workspace, graph, label, *, control=False):
    build = load('failure_build', workspace / 'tests/phased_reader/physical/build.py')
    fake = run.output / (label + '-board.o')
    run.run([run.tools['clang++'], '--no-default-config', '-x', 'c', '-std=c11', '-Wall',
             '-Wextra', '-Werror', '-O1', '-g', '-I', workspace / 'salamander/board/native',
             '-c', workspace / 'tests/reader_statistics/board_fake.c', '-o', fake], workspace)
    objects = [fake, '-fsanitize=address,undefined', '-fno-sanitize-recover=all', '-lstdc++']
    executable = run.output / label
    run.run([*link_flags(build, run, graph, objects, control=control), *([] if control else ['-Copt-level=s']),
             '-Dwarnings', '--cfg', 'feature="dev-usb"', '--crate-name=start_failure',
             workspace / 'tests/legacy_nvs/start_failure.rs', '-o', executable], workspace)
    return executable


def prove(run, workspace, support):
    run.env['RUSTFLAGS'] = '-Dwarnings -Ccodegen-units=1'
    binary = test_binary(run, workspace, run.output / 'tests-target')
    observations = run.run([binary, '--nocapture'], workspace)
    support.require('test result: ok.' in observations, 'complete positive test run')
    graph = startup_graph(run, workspace, support, 'startup-target')
    fake, objects = startup_natives(run, workspace, support)
    run.run([startup(run, workspace, graph, [fake, *objects], 'startup')], workspace)
    controls = gate_tier.negative_controls(__file__)
    failures = start_failure_graph(run, workspace, support)
    failure_program = start_failure(run, workspace, failures, 'start-failure')
    for case in ('panel', 'no-panel'):
        run.run([failure_program, case], workspace)
    killed = []
    if controls:
        for control in json.loads((workspace / 'tests/legacy_nvs/controls.json').read_text()):
            name = control['name']
            mutant = run.output / name / 'rust'
            shutil.copytree(workspace, mutant)
            source = mutant / control['file']
            text = source.read_text()
            support.require(text.count(control['old']) == 1, 'one causal mutation anchor')
            source.write_text(text.replace(control['old'], control['new'], 1))
            # Compilation must succeed separately; only the asserted runtime
            # witness may kill a mutation, never a compiler or transport error.
            kind = control.get('kind')
            wiring = kind in ('startup', 'startup-crate', 'start-failure')
            # A startup or start-failure mutation touches only the program's own
            # sources, so it reuses the unmutated crate graph. A startup-crate
            # mutation rebuilds the graph in its own target, so the shared one
            # stays unmutated (rlib names do not depend on the tree's path).
            if kind == 'startup':
                program = startup(run, mutant, graph, [fake, *objects], 'startup-' + name, control=True)
            elif kind == 'startup-crate':
                program = startup(run, mutant, startup_graph(run, mutant, support, name + '-target'),
                                  [fake, *objects], 'startup-' + name, control=True)
            elif kind == 'start-failure':
                program = start_failure(run, mutant, failures, 'start-failure-' + name, control=True)
            else:
                program = test_binary(run, mutant, run.output / ('target-' + name))
            arguments = ([program, control['case']] if kind == 'start-failure' else [program] if wiring
                         else [program, control['test'], '--exact', '--nocapture'])
            number = len(run.commands)
            try:
                run.run(arguments, mutant)
            except RuntimeError:
                stdout = (run.output / f'{number:03d}.stdout').read_text()
                stderr = (run.output / f'{number:03d}.stderr').read_text()
                support.require((wiring or 'test result: FAILED.' in stdout) and 'panicked at ' in stderr
                                and control['witness'] in stderr,
                                'runtime assertion required for mutation kill')
                killed.append(name)
            else:
                raise RuntimeError('mutation survived: ' + name)
    return dict(host_startup=True, hardware=False, controls=killed)


def in_sealed_copy(prove, evidence):
    """Run `prove(run, workspace, support)` over a sealed copy of rust/, keeping
    small command logs and a RESULT.json under .build/`evidence`."""
    support = load('legacy_support', ROOT / 'rust/tests/inflate/run_support.py')
    support.install_signal_handlers()
    evidence = ROOT / '.build' / evidence
    evidence.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='run-', dir=evidence) as directory:
        output = Path(directory)
        sources = output / 'sources'
        workspace = sources / 'rust'
        sources.mkdir()
        original = support.SourceSeal(ROOT / 'rust')
        header = ROOT / 'rust/firmware/native/firmware_version.h'
        header_before = header.read_bytes()
        shutil.copytree(ROOT / 'rust', workspace, ignore=shutil.ignore_patterns('target', '__pycache__'))
        sealed = support.SourceSeal(sources)
        class SealedRun(support.Run):
            def run(self, *args, **kwargs):
                with support.source_commands([original, sealed]):
                    return super().run(*args, **kwargs)
        run = SealedRun(output, support.verify_tools(), time.monotonic() + 600)
        result = dict(complete=False, hardware=False)
        try:
            result.update(prove(run, workspace, support), complete=True)
        finally:
            original.close()
            sealed.close()
            support.require(header.read_bytes() == header_before, 'version header changed')
            support.require(run.tools == support.verify_tools(), 'shared tools changed during the check')
            result['commands'] = run.commands
            # Keep small command logs and receipts; discard generated trees.
            retained = evidence / output.name
            retained = retained.with_name(retained.name + '-receipt')
            retained.mkdir()
            for path in output.iterdir():
                if path.is_file():
                    shutil.copy2(path, retained / path.name)
            (retained / 'RESULT.json').write_text(json.dumps(result, indent=2) + '\n')


def main():
    in_sealed_copy(prove, 'legacy-nvs-evidence')
    print('PASS legacy NVS: startup, start failures, durable preservation, capacity and selected controls')


if __name__ == '__main__':
    main()
