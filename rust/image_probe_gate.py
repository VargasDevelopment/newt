"""ImageProbe dimension-probe family of the actual shared-core gate."""
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
    spec.loader.exec_module(module)
    return module


def check(gate, snapshot, positive_only):
    root = snapshot / 'rust'
    packet = root / 'tests/image_probe'
    support = load(root / 'tests/inflate/run_support.py', 'image_probe_support')
    support.ROOT = gate.ROOT
    canonical = support.source_inventory(root)
    with ExitStack() as seals:
        seals.callback(support.verify_source_inventory, root, canonical)
        support.install_signal_handlers()
        tools = support.verify_tools()
        output = Path(tempfile.mkdtemp(prefix='image_probe-', dir=gate.SCRATCH))
        support.require(not output.resolve().is_relative_to(root.resolve()), 'build output inside sealed source')
        runner = support.Run(output, tools, time.monotonic() + 360)
        runner.env['RUSTFLAGS'] = '-Dwarnings -Ccodegen-units=1'
        provenance = json.loads((packet / 'provenance.json').read_text())
        support.require(provenance['commit'] == 'cd6f5b831c8fad899723b6d406586f00c575f021', 'ImageProbe source pin changed')
        runner.run([sys.executable, '-B', packet / 'protocol_test.py'], packet)
        runner.run([sys.executable, '-B', root / 'tests/inflate/owned_children_test.py'], root)
        corpus = load(packet / 'corpus.py', 'image_probe_corpus')
        protocol = load(packet / 'protocol.py', 'image_probe_protocol')
        cases = corpus.corpus()
        protocol.require_cases(cases, json.loads((packet / 'case_labels.json').read_text()))
        lookup = {case['name']: case for case in cases}
        cpp = packet / 'expected.json.zlib'
        support.require(support.digest(cpp) == '29c9d77930379ed0bc275f5c2922f5bbc7330b647434243f488d40049fbff481',
                        'frozen expected data changed')
        expected_data = load(root / 'expected.py', 'image_probe_expected')
        reference_requests = [dict(name=c['name'], input=c['input'], args=[]) for c in cases] + [dict(name='native-resources', input='', args=['--resources'])]
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
            messages = runner.cargo(workspace, target, 'build', '-p', 'newt-parity', '--bin', 'image-probe', '--message-format=json')
            fresh = set()
            for line in messages.splitlines():
                message = json.loads(line)
                if message.get('reason') == 'compiler-artifact' and message['target']['name'] in ('newt_core', 'image-probe'):
                    support.require(message.get('fresh') is False, 'ImageProbe actual core/transport not compiled freshly')
                    fresh.add(message['target']['name'])
            support.require(fresh == {'newt_core', 'image-probe'}, 'ImageProbe actual-core artifacts missing')
            return target / 'debug/image-probe'

        def reply(binary, case):
            raw = (reference[(case['input'], ())] if binary == cpp else
                   runner.execute([binary], root, case['input'])).encode('utf-8')
            return protocol.validate(case['input'], raw), raw

        target = output / 'baseline-target'
        rust = build(root, target)
        support.verify_source_inventory(root, canonical)
        runner.cargo(root, target, 'test', '-p', 'newt-core', '--test', 'image_probe')
        baseline = {}
        positives = []
        for case in cases:
            cpp_rows, cpp_raw = reply(cpp, case)
            rust_rows, rust_raw = reply(rust, case)
            for expected in case['expected_lines']:
                support.require(expected in cpp_rows and expected in rust_rows, f'ImageProbe independent expectation: {case["name"]}')
            support.require(cpp_rows == rust_rows, f'ImageProbe original-C++ mismatch: {case["name"]}')
            baseline[case['name']] = cpp_rows
            positives.append({'name': case['name'], 'cpp_sha256': hashlib.sha256(cpp_raw).hexdigest(),
                              'rust_sha256': hashlib.sha256(rust_raw).hexdigest(), 'response_rows': len(cpp_rows)})
        controls = json.loads((packet / 'mutations.json').read_text())
        support.require(len(controls) == 20 and len({c['name'] for c in controls}) == 20, 'ImageProbe controls incomplete')
        detected = []
        for index, control in enumerate([] if positive_only else controls):
            support.require(control['witness'] in lookup, 'ImageProbe mutation witness missing')
            workspace = output / f'mutant-{index:02d}'
            support.verify_source_inventory(root, canonical)
            shutil.copytree(root, workspace, symlinks=True)
            support.verify_source_inventory(workspace, canonical)
            source = workspace / 'core/src/image_probe.rs'
            pristine = source.read_text()
            support.require(pristine.count(control['needle']) == 1 and control['needle'] != control['replacement'], 'ImageProbe mutation site changed')
            mutated = pristine.replace(control['needle'], control['replacement'], 1)
            mutant_inventory = dict(canonical)
            support.require(str(source.relative_to(workspace)) in canonical, 'mutation path outside canonical inventory')
            mutant_inventory[str(source.relative_to(workspace))] = hashlib.sha256(mutated.encode()).hexdigest()
            with ExitStack() as mutant_seals:
                mutant_seals.callback(support.verify_source_inventory, workspace, mutant_inventory)
                source.write_bytes(mutated.encode())
                support.verify_source_inventory(workspace, mutant_inventory)
                mutant = build(workspace, output / f'mutant-target-{index:02d}', expected=mutant_inventory)
                actual, raw = reply(mutant, lookup[control['witness']])
                support.require(actual != baseline[control['witness']], f'ImageProbe semantic mutation survived: {control["name"]}')
                detected.append({'name': control['name'], 'witness': control['witness'],
                                 'fresh_core_and_transport': True, 'clean_exit_and_complete_protocol': True,
                                 'source_sha256': support.digest(source), 'binary_sha256': support.digest(mutant),
                                 'response_sha256': hashlib.sha256(raw).hexdigest()})
        # Separate native-host resource facts; never compare Rust and C sizes as equal.
        rejection_output = runner.run([sys.executable, '-B', packet / 'rejection_test.py', cpp, rust], packet)
        resources = {'cpp_native': reference[('', ('--resources',))],
                     'rust_native': runner.execute([rust, '--resources'], root)}
        support.require(support.verify_tools() == tools, 'ImageProbe approved tools changed')
        return {'complete': not positive_only, 'comparisons': len(positives), 'positive_cases': positives,
                'semantic_controls': detected, 'commands': runner.commands, 'provenance': provenance,
                'malformed_request_checks': rejection_output.strip(),
                'native_resources_separate_domains': resources, 'target_pod_compatibility': False,
                'expected_sha256': support.digest(cpp), 'rust_sha256': support.digest(rust),
                'logs_sha256': {p.name: support.digest(p) for p in sorted(output.iterdir()) if p.is_file()}}


def verify_target(gate, artifact):
    lock = json.loads((gate.ROOT / 'rust/tests/inflate/toolchains.json').read_text())
    entry = lock['executables']['nm'];nm = gate.ROOT / entry['path']
    if gate.digest(nm) != entry['sha256']:
        raise RuntimeError('ImageProbe target nm changed')
    output = gate.run([nm, '-S', '--defined-only', artifact], env=gate.environment()).stdout
    names = ['newt_image_probe_borrowed']
    for name in names:
        rows = [line.split() for line in output.splitlines() if line.split() and line.split()[-1] == name]
        if not any(len(f) == 4 and f[-2] == 'T' and int(f[1], 16) > 0 for f in rows):
            raise RuntimeError(f'ImageProbe nonempty borrowed target code missing: {name}')
    return names
