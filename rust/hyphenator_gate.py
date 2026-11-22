"""Borrowed FIBH matcher: independent native parity and separate Rust-safe domain."""
from contextlib import ExitStack
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
import time
import zlib


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def check(gate, snapshot, positive_only):
    root = snapshot / 'rust'
    packet = root / 'tests/hyphenator'
    support = load(root / 'tests/inflate/run_support.py', 'hyphenator_support')
    support.ROOT = gate.ROOT
    canonical = support.source_inventory(root)
    with ExitStack() as seals:
        seals.callback(support.verify_source_inventory, root, canonical)
        support.install_signal_handlers()
        tools = support.verify_tools()
        output = Path(tempfile.mkdtemp(prefix='hyphenator-', dir=gate.SCRATCH))
        support.require(not output.resolve().is_relative_to(root.resolve()), 'build output inside sealed source')
        runner = support.Run(output, tools, time.monotonic() + 300)
        runner.env['RUSTFLAGS'] = '-Dwarnings -Ccodegen-units=1'
        provenance = json.loads((packet / 'provenance.json').read_text())
        support.require(provenance['sdk_commit'] == 'cd6f5b831c8fad899723b6d406586f00c575f021', 'Hyphenator source pin changed')
        seen = set()
        corpus = load(packet / 'corpus.py', 'hyphenator_corpus')
        protocol = load(packet / 'protocol.py', 'hyphenator_protocol')
        cases = corpus.cases()
        protocol.require_cases(cases, json.loads((packet / 'case_labels.json').read_text()))
        lookup = {case.name: case for case in cases}
        runner.run([sys.executable, '-B', packet / 'protocol_test.py'], packet)
        cpp = packet / 'expected.json.zlib'
        support.require(support.digest(cpp) == '4bdbfc1a0220d93f3078b4a0f3b2a8323eb9b4446cbd233954c337a1ed8b1aba',
                        'frozen expected data changed')
        expected_data = load(root / 'expected.py', 'hyphenator_expected')
        reference_requests = [dict(name=c.name, input='\n'.join(c.commands) + '\n', args=[]) for c in cases if c.native]
        support.require(json.loads(zlib.decompress(cpp.read_bytes()))['source_provenance'] == provenance,
                        'frozen expected provenance changed')
        reference_outputs = expected_data.load(cpp, reference_requests)
        reference = {}
        for request, response in zip(reference_requests, reference_outputs):
            support.require(response['returncode'] == 0 and not response['stderr'], 'invalid frozen native response')
            reference[(request['input'], tuple(request['args']))] = bytes.fromhex(response['stdout']).decode('utf-8')

        def build(workspace, target, expected=None):
            support.require(not target.resolve().is_relative_to(workspace.resolve()), 'target inside sealed source')
            support.verify_source_inventory(workspace, canonical if expected is None else expected)
            messages = runner.cargo(workspace, target, 'build', '-p', 'newt-parity',
                                    '--bin', 'hyphenator', '--message-format=json')
            fresh = set()
            for line in messages.splitlines():
                message = json.loads(line)
                if message.get('reason') == 'compiler-artifact' and message['target']['name'] in ('newt_core', 'hyphenator'):
                    support.require(message.get('fresh') is False, 'Hyphenator core/transport compilation was reused')
                    fresh.add(message['target']['name'])
            support.require(fresh == {'newt_core', 'hyphenator'}, 'Hyphenator fresh actual-core artifacts missing')
            return target / 'debug/hyphenator'

        def reply(binary, case):
            wire = '\n'.join(case.commands) + '\n'
            raw = (reference[(wire, ())] if binary == cpp else runner.execute([binary], root, wire)).encode('utf-8')
            return protocol.validate(case.commands, raw), raw

        target = output / 'baseline-target'
        rust = build(root, target)
        support.verify_source_inventory(root, canonical)
        runner.cargo(root, target, 'test', '-p', 'newt-core', '--test', 'hyphenator')
        baseline = {}
        positives = []
        rust_only = []
        for case in cases:
            actual, raw = reply(rust, case)
            support.require(actual == case.expected, f'Hyphenator independent Rust expectation: {case.name}')
            entry = {'name': case.name, 'rust_sha256': hashlib.sha256(raw).hexdigest()}
            if case.native:
                expected, original = reply(cpp, case)
                support.require(expected == case.expected, f'Hyphenator independent C++ expectation: {case.name}')
                support.require(original == raw, f'Hyphenator original C++ mismatch: {case.name}')
                baseline[case.name] = original
                entry['cpp_sha256'] = hashlib.sha256(original).hexdigest()
                positives.append(entry)
            else:
                rust_only.append(entry)
        rejection = runner.run([sys.executable, '-B', packet / 'rejection_test.py', cpp, rust], packet)
        controls = json.loads((packet / 'mutations.json').read_text())
        support.require(len(controls) == 13 and len({c['id'] for c in controls}) == 13, 'Hyphenator control set incomplete')
        detected = []
        for index, control in enumerate([] if positive_only else controls):
            case = lookup[control['witness']]
            support.require(case.native and control['file'] == 'src/hyphenator.rs', 'Hyphenator mutant must have a native witness')
            workspace = output / f'mutant-{index:02d}'
            support.verify_source_inventory(root, canonical)
            shutil.copytree(root, workspace, symlinks=True)
            support.verify_source_inventory(workspace, canonical)
            source = workspace / 'core/src/hyphenator.rs'
            pristine = source.read_text()
            support.require(pristine.count(control['find']) == control['expected_occurrences'] == 1
                            and control['find'] != control['replace'], 'Hyphenator mutation site changed')
            mutated = pristine.replace(control['find'], control['replace'], 1)
            mutant_inventory = dict(canonical)
            support.require(str(source.relative_to(workspace)) in canonical, 'mutation path outside canonical inventory')
            mutant_inventory[str(source.relative_to(workspace))] = hashlib.sha256(mutated.encode()).hexdigest()
            with ExitStack() as mutant_seals:
                mutant_seals.callback(support.verify_source_inventory, workspace, mutant_inventory)
                source.write_bytes(mutated.encode())
                support.verify_source_inventory(workspace, mutant_inventory)
                mutant = build(workspace, output / f'mutant-target-{index:02d}', expected=mutant_inventory)
                _, raw = reply(mutant, case)
                support.require(raw != baseline[case.name], f'Hyphenator semantic mutation survived: {control["id"]}')
                detected.append({'name': control['id'], 'witness': case.name, 'fresh_core_and_transport': True,
                                 'clean_exit_and_complete_protocol': True, 'source_sha256': support.digest(source),
                                 'binary_sha256': support.digest(mutant), 'response_sha256': hashlib.sha256(raw).hexdigest()})
        support.require(support.verify_tools() == tools, 'Hyphenator approved tools changed')
        return {'complete': not positive_only, 'comparisons': len(positives), 'positive_cases': positives,
                'rust_only_safety_cases': rust_only, 'semantic_controls': detected,
                'malformed_request_checks': rejection.strip(), 'commands': runner.commands, 'provenance': provenance,
                'expected_sha256': support.digest(cpp), 'rust_sha256': support.digest(rust),
                'logs_sha256': {p.name: support.digest(p) for p in sorted(output.iterdir()) if p.is_file()}}


def verify_target(gate, artifact):
    lock = json.loads((gate.ROOT / 'rust/tests/inflate/toolchains.json').read_text())
    entry = lock['executables']['nm']
    nm = gate.ROOT / entry['path']
    if gate.digest(nm) != entry['sha256']:
        raise RuntimeError('Hyphenator target nm changed')
    output = gate.run([nm, '-S', '--defined-only', artifact], env=gate.environment()).stdout
    name = 'newt_hyphenator_borrowed'
    rows = [line.split() for line in output.splitlines() if line.split() and line.split()[-1] == name]
    if not any(len(f) == 4 and f[-2] == 'T' and int(f[1], 16) > 0 for f in rows):
        raise RuntimeError('Hyphenator nonempty borrowed target code missing')
    return name
