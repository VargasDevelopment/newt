"""Offline actual-core XML-input gate hook, following css_gate.check."""
from contextlib import ExitStack
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
import time


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def run(gate, snapshot, positive_only=False):
    root = Path(snapshot) / 'rust'
    packet = root / 'tests/xml_input'
    support = load(root / 'tests/inflate/run_support.py', 'xml_input_support')
    support.ROOT = gate.ROOT
    canonical = support.source_inventory(root)
    with ExitStack() as seals:
        seals.callback(support.verify_source_inventory, root, canonical)
        support.install_signal_handlers()
        tools = support.verify_tools()
        build_root = Path(gate.SCRATCH).resolve()
        support.require(build_root.is_relative_to((Path(gate.ROOT) / '.build/rust').resolve()), 'XML output must stay inside .build/rust')
        output = Path(tempfile.mkdtemp(prefix='xml-input-', dir=build_root))
        support.require(not output.resolve().is_relative_to(root.resolve()), 'build output inside sealed source')
        runner = support.Run(output, tools, time.monotonic() + 300)
        runner.env.update(RUSTFLAGS='-Dwarnings -Ccodegen-units=1', CARGO_BUILD_JOBS='2', RUST_TEST_THREADS='2')
        provenance = json.loads((packet / 'PROVENANCE.json').read_text())
        support.require(provenance['commit'] == 'cd6f5b831c8fad899723b6d406586f00c575f021', 'XML source pin changed')
        runner.run([sys.executable, '-B', packet / 'protocol_test.py'], packet)
        corpus = load(packet / 'corpus.py', 'xml_input_corpus')
        protocol = load(packet / 'protocol.py', 'xml_input_protocol')
        records = load(packet / 'xml_input_records.py', 'xml_input_records')
        controls = json.loads((packet / 'mutations.json').read_text())
        support.require(len(controls) == 24 and len({c['name'] for c in controls}) == 24, 'XML controls incomplete')
        cases = records.fixtures(corpus, controls)
        protocol.require_cases(cases, json.loads((packet / 'case_labels.json').read_text()))
        lookup = {case['input']: case for case in cases}
        frozen_transport = load(root / 'tests/inflate/expected_transport.py', 'frozen_transport')
        reference = frozen_transport.Frozen(packet / 'expected.json', [dict(name=c['name'], input=c['input'] + '\n', args=[]) for c in cases], provenance)
        cpp = packet / 'expected.json'
        flags = ['-O1', '-g', '-fsanitize=address,undefined', '-fno-sanitize-recover=all', '-fno-omit-frame-pointer']
        sanitizer = output / 'sanitizer-control'
        support.verify_source_inventory(root, canonical)
        runner.run([tools['clang++'], '--no-default-config', '-x', 'c', '-std=c99', *flags,
                    root / 'tests/inflate/sanitizer_control.c', '-o', sanitizer], root)
        sanitizer_result = runner.run([sys.executable, '-B', root / 'tests/inflate/sanitizer_test.py', sanitizer], root)

        def build(workspace, target, expected=None):
            support.require(not target.resolve().is_relative_to(workspace.resolve()), 'target inside sealed source')
            support.verify_source_inventory(workspace, canonical if expected is None else expected)
            messages = runner.cargo(workspace, target, 'build', '-p', 'newt-parity',
                                    '--bin', 'xml-input', '--message-format=json')
            fresh = set()
            for line in messages.splitlines():
                message = json.loads(line)
                if message.get('reason') == 'compiler-artifact' and message['target']['name'] in ('newt_core', 'xml-input'):
                    support.require(message.get('fresh') is False, 'XML actual core/transport was not freshly compiled')
                    fresh.add(message['target']['name'])
            support.require(fresh == {'newt_core', 'xml-input'}, 'XML actual-core artifacts missing')
            return target / 'debug/xml-input'

        def reply(binary, case):
            request = case['input'] + '\n'
            wire = (reference.text(request) if binary == cpp else runner.execute([binary], root, request)).encode('utf-8')
            return protocol.validate(case['input'] + '\n', wire), wire

        rust = build(root, output / 'baseline-target')
        baseline = {}
        positives = []
        for case in cases:
            left, cpp_wire = reply(cpp, case)
            right, rust_wire = reply(rust, case)
            protocol.independent(case, left); protocol.independent(case, right)
            support.require(cpp_wire == rust_wire, f'XML original-C++ mismatch: {case["name"]}')
            baseline[case['input']] = cpp_wire
            positives.append({'name': case['name'], 'cpp_sha256': hashlib.sha256(cpp_wire).hexdigest(),
                              'rust_sha256': hashlib.sha256(rust_wire).hexdigest(), 'response_rows': len(left['lines'])})
        rejection_result = runner.run([sys.executable, '-B', packet / 'rejection_test.py', cpp, rust], packet)
        detected = []
        for index, control in enumerate([] if positive_only else controls):
            support.require(control['file'] == 'src/lib.rs', 'XML mutation source mapping changed')
            workspace = output / f'mutant-{index:02d}'
            support.verify_source_inventory(root, canonical)
            shutil.copytree(root, workspace, symlinks=True)
            support.verify_source_inventory(workspace, canonical)
            source = workspace / control.get('rust_file', 'core/src/xml_input.rs')
            pristine = source.read_text()
            support.require(pristine.count(control['old']) == 1 and control['old'] != control['new'], 'XML mutation site changed')
            mutated = pristine.replace(control['old'], control['new'], 1)
            mutant_inventory = dict(canonical)
            support.require(str(source.relative_to(workspace)) in canonical, 'mutation path outside canonical inventory')
            mutant_inventory[str(source.relative_to(workspace))] = hashlib.sha256(mutated.encode()).hexdigest()
            with ExitStack() as mutant_seals:
                mutant_seals.callback(support.verify_source_inventory, workspace, mutant_inventory)
                source.write_bytes(mutated.encode())
                support.verify_source_inventory(workspace, mutant_inventory)
                mutant = build(workspace, output / f'mutant-target-{index:02d}', expected=mutant_inventory)
                # A complete valid response and exit zero are prerequisites, not kills.
                case = lookup[control['witness']]
                _, wire = reply(mutant, case)
                support.require(wire != baseline[case['input']], f'XML semantic mutation survived: {control["name"]}')
                detected.append({'name': control['name'], 'witness': case['name'],
                                 'fresh_core_and_transport': True, 'clean_exit_and_complete_protocol': True,
                                 'source_sha256': support.digest(source), 'binary_sha256': support.digest(mutant),
                                 'response_sha256': hashlib.sha256(wire).hexdigest()})
        support.require(support.verify_tools() == tools, 'XML approved tools changed')
        return {'complete': not positive_only, 'complete_positive_corpus': True,
                'comparisons': len(positives), 'positive_cases': positives,
                'semantic_controls': detected, 'commands': runner.commands, 'provenance': provenance,
                'malformed_request_checks': rejection_result.strip(), 'fatal_sanitizer_controls': sanitizer_result.strip(),
                'expected_sha256': support.digest(cpp), 'rust_sha256': support.digest(rust),
                'scope': 'XML-input helpers and entity filtering only; no full XML parser, hardware or resource proof',
                'logs_sha256': {p.name: support.digest(p) for p in sorted(output.iterdir()) if p.is_file()}}


# Existing modular gates use check(gate, snapshot, positive_only).
check = run
