"""Pinned ZIP catalog/reader family using the actual shared Rust core."""
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


def check(gate, snapshot, positive_only):
    root = snapshot / 'rust'
    packet = root / 'tests/zip_container'
    support = load(root / 'tests/inflate/run_support.py', 'zip_container_support')
    support.ROOT = gate.ROOT
    canonical = support.source_inventory(root)
    with ExitStack() as seals:
        seals.callback(support.verify_source_inventory, root, canonical)
        support.install_signal_handlers()
        tools = support.verify_tools()
        output = Path(tempfile.mkdtemp(prefix='zip-container-', dir=gate.SCRATCH))
        support.require(not output.resolve().is_relative_to(root.resolve()), 'build output inside sealed source')
        runner = support.Run(output, tools, time.monotonic() + 240)
        runner.env['RUSTFLAGS'] = '-Dwarnings -Ccodegen-units=1'
        provenance = json.loads((packet / 'PROVENANCE.json').read_text())
        support.require(provenance['commit'] == 'cd6f5b831c8fad899723b6d406586f00c575f021', 'ZIP SDK pin changed')
        runner.run([sys.executable, '-B', packet / 'protocol_test.py'], packet)
        protocol = load(packet / 'protocol.py', 'zip_container_protocol')
        expectations = load(packet / 'expectations.py', 'zip_container_expectations')
        cases = expectations.fixtures(load(packet / 'corpus.py', 'zip_container_corpus'))
        lookup = {case['label']: case for case in cases}
        cpp = packet / 'expected.json.zlib'
        support.require(support.digest(cpp) == 'f6bfd2c78b49d486c07fd41575d7956cf09aaeb21fcf8246759036f5d05f4e7e',
                        'frozen expected data changed')
        expected_data = load(root / 'expected.py', 'zip_container_expected')
        reference_requests = [dict(name=c['label'], input=c['input'] + '\n', args=['--reopen'] if c.get('reopen') else []) for c in cases]
        document = expected_data.load_document(cpp, reference_requests)
        support.require(document.get('source_provenance') == provenance, 'frozen expected provenance changed')
        support.require(document.get('output_format') == 'semantic_sha256_v1', 'frozen expected format changed')
        reference = {}
        for request, response in zip(reference_requests, document['outputs']):
            support.require(
                isinstance(response, dict)
                and isinstance(response.get('semantic_sha256'), str) and len(response['semantic_sha256']) == 64
                and isinstance(response.get('wire_sha256'), str) and len(response['wire_sha256']) == 64
                and isinstance(response.get('source_read_calls'), int),
                'invalid frozen semantic digest entry',
            )
            reference[(request['input'], tuple(request['args']))] = response

        def build(workspace, target, expected=None):
            support.require(not target.resolve().is_relative_to(workspace.resolve()), 'target inside sealed source')
            support.verify_source_inventory(workspace, canonical if expected is None else expected)
            messages = runner.cargo(workspace, target, 'build', '-p', 'newt-parity',
                                    '--bin', 'zip-container', '--message-format=json')
            fresh = set()
            for line in messages.splitlines():
                message = json.loads(line)
                if message.get('reason') == 'compiler-artifact' and message['target']['name'] in ('newt_core', 'zip-container'):
                    support.require(message.get('fresh') is False, 'ZIP core/transport was not compiled freshly')
                    fresh.add(message['target']['name'])
            support.require(fresh == {'newt_core', 'zip-container'}, 'ZIP actual-core compiler artifacts missing')
            return target / 'debug/zip-container'

        def reply(binary, case, implementation):
            args = ['--reopen'] if case.get('reopen') else []
            wire = runner.execute([binary, *args], root, case['input'] + '\n').encode('utf-8')
            return protocol.validate(case['input'], wire, implementation, bool(args)), wire

        target = output / 'baseline-target'
        rust = build(root, target)
        support.verify_source_inventory(root, canonical)
        runner.cargo(root, target, 'test', '-p', 'newt-core', '--test', 'zip_container')
        baselines = {}
        positives = []
        for case in cases:
            args = ['--reopen'] if case.get('reopen') else []
            ref = reference[(case['input'] + '\n', tuple(args))]
            rust_result, rust_wire = reply(rust, case, 'rust')
            expectations.check(case, rust_result['semantic'])
            rust_semantic_sha256 = protocol.semantic_digest(rust_result['semantic'])
            support.require(rust_semantic_sha256 == ref['semantic_sha256'],
                            f'ZIP parity: {case["label"]} (expected {ref["semantic_sha256"]}, got {rust_semantic_sha256})')
            baselines[case['label']] = ref['semantic_sha256']
            positives.append({'name': case['label'], 'cpp_sha256': ref['wire_sha256'],
                              'rust_sha256': hashlib.sha256(rust_wire).hexdigest(),
                              'semantic_sha256': rust_semantic_sha256,
                              'cpp_resources': ref['resources'], 'rust_resources': rust_result['resources'],
                              'source_reads': ref['source_read_calls']})
        controls = json.loads((packet / 'mutations.json').read_text())
        support.require(len(controls) == 10 and len({c['site'] for c in controls}) == 10, 'ZIP controls incomplete')
        detected = []
        for index, control in enumerate([] if positive_only else controls):
            support.require(control['witness'] in lookup, 'ZIP mutation lacks a transport witness')
            workspace = output / f'mutant-{index:02d}'
            support.verify_source_inventory(root, canonical)
            shutil.copytree(root, workspace, symlinks=True)
            support.verify_source_inventory(workspace, canonical)
            source = workspace / 'core/src/zip_container.rs'
            pristine = source.read_text()
            support.require(pristine.count(control['from']) == 1 and control['from'] != control['to'], 'ZIP mutation site changed')
            mutated = pristine.replace(control['from'], control['to'], 1)
            mutant_inventory = dict(canonical)
            support.require(str(source.relative_to(workspace)) in canonical, 'mutation path outside canonical inventory')
            mutant_inventory[str(source.relative_to(workspace))] = hashlib.sha256(mutated.encode()).hexdigest()
            with ExitStack() as mutant_seals:
                mutant_seals.callback(support.verify_source_inventory, workspace, mutant_inventory)
                source.write_bytes(mutated.encode())
                support.verify_source_inventory(workspace, mutant_inventory)
                mutant = build(workspace, output / f'mutant-target-{index:02d}', expected=mutant_inventory)
                case = lookup[control['witness']]
                actual, wire = reply(mutant, case, 'rust')
                support.require(protocol.semantic_digest(actual['semantic']) != baselines[control['witness']],
                                f'ZIP mutation survived: {control["site"]}')
                detected.append({'site': control['site'], 'witness': control['witness'],
                                 'fresh_core_and_transport': True, 'clean_exit_and_complete_protocol': True,
                                 'source_sha256': support.digest(source), 'binary_sha256': support.digest(mutant),
                                 'response_sha256': hashlib.sha256(wire).hexdigest()})
        support.require(support.verify_tools() == tools, 'ZIP approved tools changed')
        return {'complete': not positive_only, 'comparisons': len(positives), 'positive_cases': positives,
                'semantic_controls': detected, 'commands': runner.commands, 'provenance': provenance,
                'expected_sha256': support.digest(cpp), 'rust_sha256': support.digest(rust),
                'resource_domain': 'C++ byte arena and Rust typed slots/name arena retained separately; no byte parity claim',
                'logs_sha256': {p.name: support.digest(p) for p in sorted(output.iterdir()) if p.is_file()}}


def verify_target(gate, artifact):
    lock = json.loads((gate.ROOT / 'rust/tests/inflate/toolchains.json').read_text())
    entry = lock['executables']['nm']
    nm = gate.ROOT / entry['path']
    if gate.digest(nm) != entry['sha256']:
        raise RuntimeError('ZIP target nm changed')
    output = gate.run([nm, '-S', '--defined-only', artifact], env=gate.environment()).stdout
    names = ['newt_rust_zip_container_catalog_probe', 'newt_rust_zip_container_reader_probe']
    for name in names:
        found = [line.split() for line in output.splitlines() if line.split() and line.split()[-1] == name]
        if not any(len(fields) == 4 and fields[-2] == 'T' and int(fields[1], 16) > 0 for fields in found):
            raise RuntimeError(f'ZIP nonempty runtime-borrowed target body missing: {name}')
    return names
