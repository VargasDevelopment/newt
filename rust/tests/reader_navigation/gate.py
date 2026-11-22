#!/usr/bin/env python3
"""Canonical N1 comparison, direct semantic controls and borrowed RV32 retention."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import time
sys.dont_write_bytecode = True
HOST = 'x86_64-unknown-linux-gnu'
RV32 = 'riscv32imc-unknown-none-elf'


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def sha(text):
    return hashlib.sha256(text.encode()).hexdigest()


def run_gate(root, output, seconds=540, only=None, source_root=None, positive_only=False):
    root, output = Path(root).resolve(), Path(output).resolve()
    if not 0 < seconds <= 540:
        raise RuntimeError('internal allowance must be <=540 seconds')
    started = time.monotonic()
    rust = Path(source_root).resolve() if source_root else root / 'rust'
    boot = load('navigation_boot', rust / 'tests/inflate/run_support.py')
    boot.require(output.is_relative_to(root / '.build/rust') and not output.is_relative_to(rust),
                 'output must be fresh under .build/rust outside sources')
    boot.install_signal_handlers()
    output.mkdir()
    sources = output / 'sources'
    sources.mkdir()
    baseline = sources / 'rust'
    snapshots = []
    inventory = boot.source_inventory(rust)
    shutil.copytree(rust, baseline)
    boot.verify_source_inventory(baseline, inventory)
    snapshots.extend(((rust, inventory), (baseline, inventory)))
    products = [sources]
    support = load('navigation_run', baseline / 'tests/inflate/run_support.py')
    support.ROOT = root
    tools = support.verify_tools()
    canonical_tools = dict(tools)
    declared = json.loads((baseline / 'tests/jpeg_decoder/tool_bindings.json').read_text())
    extras = {name: declared[name] for name in ('rust-lld',)}
    tools.update({name: root / value['path'] for name, value in extras.items()})

    def seal():
        for path, inventory in snapshots:
            support.verify_source_inventory(path, inventory)
        support.require(support.verify_tools() == canonical_tools, 'canonical tools changed')
        for name, value in extras.items():
            support.require(support.digest(tools[name]) == value['sha256'], 'approved tool changed: ' + name)

    seal()
    run = support.Run(output, tools, started + seconds)
    run.env['RUSTFLAGS'] = '-Dwarnings -Ccodegen-units=1'
    recipe = baseline / 'tests/reader_navigation'
    controls = load('navigation_controls', recipe / 'controls.py').CONTROLS
    support.require(len(controls) == 16, 'full 16-control set absent')
    selected = [c for c in controls if not only or c[0] in only]
    support.require(not only or set(only) <= {c[0] for c in controls}, 'unknown control')
    if positive_only:
        selected = []
    report = dict(complete=False, full_controls=not bool(only) and not positive_only,
                  scope='canonical pure navigation policy; descriptor transport, not parser integration',
                  controls=[], firmware=False, hardware=False, native_XML_guard_wiring=False,
                  full_PhasedReader_integration=False, target_footprint_accepted=False)
    try:
        provenance = json.loads((recipe / 'provenance.json').read_text())
        support.require(provenance['reference'] == 'bc9ffda52b85f277e46c63718ac88d62fba5d368'
                        and provenance['native_sdk_reference'] == 'cd6f5b831c8fad899723b6d406586f00c575f021',
                        'original pins changed')
        target = output / 'baseline-target'
        products.append(target)
        tests = run.cargo(baseline, target, 'test', '-p', 'newt-reader-navigation', '--lib', '--target', HOST)
        support.require('test result: ok. 15 passed; 0 failed;' in tests, 'focused tests absent')
        run.cargo(baseline, target, 'build', '-p', 'newt-reader-navigation',
                  '--bin', 'reader-navigation-host', '--target', HOST)
        binary = target / HOST / 'debug/reader-navigation-host'
        corpus = load('navigation_corpus', recipe / 'corpus.py')
        protocol = load('navigation_protocol', recipe / 'protocol.py')
        run.run([tools['python'], '-B', recipe / 'protocol_test.py'], baseline)
        base_cases = list(corpus.cases())
        control_cases = dict(base_cases)
        control_cases['declared-conflict-unobserved'] = {**control_cases['declared-conflict'], 'anchors': []}
        all_cases = base_cases + [(name, control_cases[name]) for name in control_cases if name not in dict(base_cases)]
        frozen_transport = load('navigation_expected', baseline / 'tests/inflate/expected_transport.py')
        reference = frozen_transport.Frozen(recipe / 'expected-v2.json.zlib',
            [dict(mode=0, input=protocol.wire(case)) for _, case in all_cases], provenance)
        cpp = recipe / 'expected-v2.json.zlib'
        rejected = run.execute([tools['python'], '-B', recipe / 'rejection.py', sources, cpp, binary], baseline)
        support.require(rejected == 'N1_REJECTIONS 9\n', 'transport controls absent')
        receipts = []
        for name, case in corpus.cases():
            wire = protocol.wire(case)
            expected = reference.text(wire)
            protocol.response(expected, case)
            actual = run.execute([binary], baseline, wire)
            protocol.response(actual, case)
            if name == 'independent-resolved':
                support.require(expected == corpus.BASE_EXPECTED, 'independent complete expectation differs')
            if name == 'independent-cached-generic':
                support.require(expected.splitlines()[-6:-1] == [
                    'M 1 3 0 0 3 1 1 1',
                    'R 0 0 65535 1 0 1 1 53656374696f6e2031',
                    'R 1 0 0 0 1 0 1 4669727374',
                    'R 2 2 1 0 1 0 1 5365636f6e64',
                    'U 1 0'], 'independent generic-current menu count/order differs')
            support.require(actual == expected, 'N1 policy mismatch: ' + name)
            receipts.append(dict(case=name, wire_sha256=sha(wire), response_sha256=sha(actual)))
        support.require(len(receipts) == 110, 'full 110 trace corpus absent')
        report.update(comparisons=len(receipts), cases=receipts, unit_tests=15,
                      independent_complete_cases=1, transport_controls=9,
                      cpp_sha256=support.digest(cpp), rust_sha256=support.digest(binary))
        run.cargo(baseline, target, 'build', '-p', 'newt-reader-navigation', '--lib', '--release', '--target', RV32)
        cases = dict(corpus.cases())
        cases['declared-conflict-unobserved'] = {**cases['declared-conflict'], 'anchors': []}
        for name, file, old, new, witnesses in selected:
            workspace = sources / ('mutant-' + name)
            # The canonical copier handles nested Cargo members once.
            support.copy_workspace(baseline, workspace)
            source = workspace / 'reader-navigation/src' / file
            clean_source = source.read_text()
            support.require(clean_source.count(old) == 1, 'mutation anchor not unique: ' + name)
            source.write_text(clean_source.replace(old, new, 1))
            snapshots.append((workspace, support.source_inventory(workspace)))
            mutant_target = output / ('target-' + name)
            products.append(mutant_target)
            run.cargo(workspace, mutant_target, 'build', '-p', 'newt-reader-navigation',
                      '--bin', 'reader-navigation-host', '--target', HOST)
            mutant = mutant_target / HOST / 'debug/reader-navigation-host'
            receipt = dict(name=name, source=file, original_source_sha256=sha(clean_source),
                           mutated_source_sha256=support.digest(source), cpp_sha256=report['cpp_sha256'],
                           baseline_rust_sha256=report['rust_sha256'], mutant_sha256=support.digest(mutant),
                           witnesses=[], killed=False)
            for witness in witnesses:
                case = cases[witness]
                wire = protocol.wire(case)
                expected = reference.text(wire)
                protocol.response(expected, case)
                clean = run.execute([binary], baseline, wire)
                protocol.response(clean, case)
                support.require(clean == expected, 'fresh witness positive differs: ' + witness)
                actual = run.execute([mutant], workspace, wire)
                protocol.response(actual, case)  # Full strict parsing precedes any semantic kill.
                different = actual != expected
                receipt['witnesses'].append(dict(case=witness, wire_sha256=sha(wire),
                    cpp_response_sha256=sha(expected), mutant_response_sha256=sha(actual), semantic_difference=different))
                receipt['killed'] |= different
            report['controls'].append(receipt)
            support.require(receipt['killed'], 'surviving control: ' + name)
            seal()
        report['retention'] = retain(run, support, recipe, baseline, target, products)
        report['complete'] = True
    finally:
        report['elapsed_seconds'] = time.monotonic() - started
        report['commands'] = run.commands
        try:
            seal()
            support.require(time.monotonic() <= run.deadline, 'internal allowance exceeded')
        except BaseException:
            report['complete'] = False
            report['closing_seal_failed'] = True
            raise
        finally:
            (output / 'evidence.json').write_text(json.dumps(report, indent=2) + '\n')
            (output / 'bindings.json').write_text(json.dumps({'sources': {str(p): v for p, v in snapshots}}, indent=2) + '\n')
            (output / 'commands.json').write_text(json.dumps(run.commands, indent=2) + '\n')
    # Preserve all failed products. Remove passing scratch only after receipts/logs.
    if report['complete']:
        for path in products:
            if path.is_dir():
                shutil.rmtree(path)
            else:
                path.unlink()
    return report


def retain(run, support, recipe, baseline, target, products):
    # Build the ACTUAL canonical staticlib with canonical LTO/profile/lock.
    run.cargo(baseline, target, 'build', '-p', 'newt-target-probe', '--release', '--target', RV32, '--lib')
    archive = target / RV32 / 'release/libnewt_target_probe.a'
    obj, elf = run.output / 'retain.o', run.output / 'navigation-retained.elf'
    products.extend([obj, elf])
    run.run([run.tools['clang++'], '--no-default-config', '--target=riscv32', '-march=rv32imc',
             '-mabi=ilp32', '-x', 'c', '-std=c99', '-Os', '-ffreestanding', '-fno-builtin',
             '-c', recipe / 'retain.c', '-o', obj], baseline)
    run.run([run.tools['rust-lld'], '-flavor', 'gnu', '-m', 'elf32lriscv', '--gc-sections',
             '-e', 'navigation_retained_probe', obj, archive, '-o', elf], baseline)
    symbols = run.execute([run.tools['nm'], '-S', '--defined-only', elf], baseline)
    (run.output / 'retained-symbols.txt').write_text(symbols)
    rows = [line.split() for line in symbols.splitlines()]
    bodies = {}
    for name in ('navigation_retained_probe', 'navigation_bind_runtime',
                 'navigation_anchors_runtime', 'navigation_menu_runtime'):
        matches = [r for r in rows if len(r) == 4 and r[3] == name and r[2] in ('t', 'T')]
        support.require(len(matches) == 1 and int(matches[0][1], 16) > 0, 'nonempty body absent: ' + name)
        bodies[name] = int(matches[0][1], 16)
    sizes = [r for r in rows if len(r) == 4 and r[3] == 'navigation_type_size']
    support.require(len(sizes) == 1 and int(sizes[0][1], 16) > 0, 'target type-size symbol absent')
    support.require(not run.execute([run.tools['nm'], '--undefined-only', elf], baseline).strip(), 'unresolved ELF imports')
    forbidden = ('__rust_alloc', '__rust_realloc', '__rust_dealloc', '__rdl_alloc', '__rdl_dealloc',
                 'malloc', 'calloc', 'realloc', '_ZN5alloc')
    support.require(not any(n in symbols for n in forbidden), 'allocator retained')
    return dict(scope='actual RV32 target staticlib/link retention; not execution, firmware, HW or resource proof',
                navigation_type_bytes=int(sizes[0][1], 16), original_cpp_object_bytes=976, retained_bodies=bodies,
                elf_sha256=support.digest(elf), archive_sha256=support.digest(archive),
                runtime_nonempty_exercised=False, stack_accepted=False, heap_accepted=False, ABI_accepted=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--seconds', type=int, default=540)
    parser.add_argument('--only', action='append', help='diagnostic subset; never full acceptance')
    parser.add_argument('--positive-only', action='store_true', help='diagnostic; never full acceptance')
    args = parser.parse_args()
    run_gate(args.root, args.output, args.seconds, args.only, positive_only=args.positive_only)
