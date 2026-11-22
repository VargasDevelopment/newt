"""Actual shared-core font-layout parity gate; host evidence is not hardware evidence."""
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
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def check(gate, snapshot, positive_only):
    root = snapshot / 'rust'
    packet = root / 'tests/font_layout'
    support = load(root / 'tests/inflate/run_support.py', 'font_layout_support')
    support.ROOT = gate.ROOT
    canonical = support.source_inventory(root)
    with ExitStack() as seals:
        seals.callback(support.verify_source_inventory, root, canonical)
        support.install_signal_handlers()
        tools = support.verify_tools()
        output = Path(tempfile.mkdtemp(prefix='font-layout-', dir=gate.SCRATCH))
        support.require(not output.resolve().is_relative_to(root.resolve()), 'build output inside sealed source')
        deadline = time.monotonic() + 300
        runner = support.Run(output, tools, deadline)
        runner.env['RUSTFLAGS'] = '-Dwarnings -Ccodegen-units=1'
        provenance = json.loads((packet / 'provenance.json').read_text())
        support.require(provenance['commit'] == 'cd6f5b831c8fad899723b6d406586f00c575f021',
                        'Font-layout original source pin changed')
        corpus = load(packet / 'corpus.py', 'font_layout_corpus')
        protocol = load(packet / 'protocol.py', 'font_layout_protocol')
        cases = corpus.corpus()
        protocol.require_cases(cases, json.loads((packet / 'case_labels.json').read_text()))
        lookup = {case['name']: case for case in cases}
        runner.run([sys.executable, '-B', packet / 'protocol_test.py'], packet)
        cpp = packet / 'expected.json'
        support.require(support.digest(cpp) == 'b72fff18d765305cd0c19bc5dcde91326ab9260c0582a95dadf473838cf7df7d',
                        'frozen expected data changed')
        expected_data = load(root / 'expected.py', 'font_layout_expected')
        reference_requests = [dict(name=c['name'], input=c['input'], args=[]) for c in cases]
        literal_answers = [c['expected'] for c in cases]
        support.require(json.loads(cpp.read_bytes())['source_provenance'] == provenance,
                        'frozen expected provenance changed')
        expected_data.load_literals(cpp, reference_requests, literal_answers)

        def build(workspace, target, expected=None):
            support.require(not target.resolve().is_relative_to(workspace.resolve()), 'target inside sealed source')
            support.verify_source_inventory(workspace, canonical if expected is None else expected)
            messages = runner.cargo(workspace, target, 'build', '-p', 'newt-parity',
                                    '--bin', 'font-layout-host', '--message-format=json')
            fresh = set()
            for line in messages.splitlines():
                message = json.loads(line)
                if message.get('reason') == 'compiler-artifact' and message['target']['name'] in ('newt_core', 'font-layout-host'):
                    support.require(message.get('fresh') is False,
                                    'Font-layout actual core/transport compilation was reused')
                    fresh.add(message['target']['name'])
            support.require(fresh == {'newt_core', 'font-layout-host'},
                            'Font-layout fresh actual-core artifacts missing')
            return target / 'debug/font-layout-host'

        def reply(binary, case):
            # Run reads stdout as bytes before decoding, so CRLF cannot become LF.
            raw = (case['expected'] if binary == cpp else
                   runner.execute([binary], root, case['input'])).encode('utf-8')
            return protocol.validate(case['input'], raw), raw

        target = output / 'baseline-target'
        rust = build(root, target)
        support.verify_source_inventory(root, canonical)
        runner.cargo(root, target, 'test', '-p', 'newt-core', '--test', 'font_layout')
        baseline = {}
        positives = []
        for case in cases:
            cpp_value, cpp_raw = reply(cpp, case)
            rust_value, rust_raw = reply(rust, case)
            expected = case['expected'].encode('ascii')
            support.require(cpp_raw == expected, 'Font-layout independent C++ expectation: ' + case['name'])
            support.require(rust_raw == expected, 'Font-layout independent Rust expectation: ' + case['name'])
            support.require(cpp_value == rust_value and cpp_raw == rust_raw,
                            'Font-layout original-C++ mismatch: ' + case['name'])
            baseline[case['name']] = cpp_raw
            positives.append({**protocol.fingerprint(case),
                              'cpp_sha256': hashlib.sha256(cpp_raw).hexdigest(),
                              'rust_sha256': hashlib.sha256(rust_raw).hexdigest(),
                              'response': cpp_raw.decode('ascii')})
        # Rejections are checked against pristine binaries only; no malformed request
        # or transport failure can qualify as detection of a semantic mutant.
        rejection = runner.run([sys.executable, '-B', packet / 'rejection.py', output / 'rejections',
                                str(deadline), cpp, rust], packet)
        controls = json.loads((packet / 'mutations.json').read_text())
        support.require(len(controls) == 17 and len({c['name'] for c in controls}) == 17,
                        'Font-layout controls incomplete')
        detected = []
        for index, control in enumerate([] if positive_only else controls):
            support.require(control['witness'] in lookup, 'Font-layout original witness missing')
            workspace = output / f'mutant-{index:02d}'
            # Copy the actual workspace, including its real lib, exports and host bin.
            # Only this copied module changes; never generate an isolated substitute.
            support.verify_source_inventory(root, canonical)
            shutil.copytree(root, workspace, symlinks=True)
            support.verify_source_inventory(workspace, canonical)
            source = workspace / 'core/src/font_layout.rs'
            pristine = source.read_text()
            support.require(pristine.count(control['needle']) == 1 and control['needle'] != control['replacement'],
                            'Font-layout mutation site changed: ' + control['name'])
            mutated = pristine.replace(control['needle'], control['replacement'], 1)
            mutant_inventory = dict(canonical)
            support.require(str(source.relative_to(workspace)) in canonical, 'mutation path outside canonical inventory')
            mutant_inventory[str(source.relative_to(workspace))] = hashlib.sha256(mutated.encode()).hexdigest()
            with ExitStack() as mutant_seals:
                mutant_seals.callback(support.verify_source_inventory, workspace, mutant_inventory)
                source.write_bytes(mutated.encode())
                support.verify_source_inventory(workspace, mutant_inventory)
                mutant = build(workspace, output / f'mutant-target-{index:02d}', expected=mutant_inventory)
                _, raw = reply(mutant, lookup[control['witness']])
                original = baseline[control['witness']]
                support.require(raw != original, 'Font-layout semantic mutation survived: ' + control['name'])
                detected.append({'name': control['name'], 'witness': control['witness'],
                                 'fresh_core_and_transport': True, 'clean_exit_and_complete_protocol': True,
                                 'source_sha256': support.digest(source), 'binary_sha256': support.digest(mutant),
                                 'original_response_sha256': hashlib.sha256(original).hexdigest(),
                                 'response_sha256': hashlib.sha256(raw).hexdigest(),
                                 'original_response': original.decode('ascii'), 'response': raw.decode('ascii')})
        support.require(support.verify_tools() == tools, 'Font-layout approved tools changed')
        sources = ['core/src/font_layout.rs', 'core/src/lib.rs', 'core/tests/font_layout.rs',
                   'host/src/bin/font-layout-host.rs', 'font_layout_gate.py',
                   *['tests/font_layout/' + name for name in ('protocol.py', 'protocol_test.py', 'case_labels.json',
                                                             'rejection.py', 'corpus.py', 'mutations.json',
                                                             'expected.json', 'provenance.json')]]
        logs = list(output.glob('*.command.json')) + list(output.glob('*.stdout')) + list(output.glob('*.stderr'))
        logs += [path for path in (output / 'rejections').iterdir() if path.is_file()]
        return {'complete': not positive_only, 'comparisons': len(positives), 'positive_cases': positives,
                'semantic_controls': detected, 'commands': runner.commands, 'provenance': provenance,
                'malformed_request_checks': json.loads(rejection),
                'source_sha256': {name: support.digest(root / name) for name in sources},
                'expected_sha256': support.digest(cpp), 'rust_sha256': support.digest(rust),
                'logs_sha256': {str(path.relative_to(output)): support.digest(path) for path in sorted(logs)},
                'evidence_scope': 'native host raw-table parity only; no hardware, font-engine integration, resource or performance claim'}


def verify_target(gate, artifact):
    """Require exact, nonempty defined text symbols; this does not execute RV32."""
    lock = json.loads((gate.ROOT / 'rust/tests/inflate/toolchains.json').read_text())
    entry = lock['executables']['nm']
    nm = gate.ROOT / entry['path']
    if gate.digest(nm) != entry['sha256']:
        raise RuntimeError('Font-layout target nm changed')
    output = gate.run([nm, '-S', '--defined-only', artifact], env=gate.environment()).stdout
    names = ['newt_font_pair_borrowed', 'newt_font_ligature_borrowed']
    for name in names:
        rows = [line.split() for line in output.splitlines() if line.split() and line.split()[-1] == name]
        if not (len(rows) == 1 and len(rows[0]) == 4 and rows[0][-2] == 'T' and int(rows[0][1], 16) > 0):
            raise RuntimeError('Font-layout exact nonempty borrowed target code missing: ' + name)
    return names
