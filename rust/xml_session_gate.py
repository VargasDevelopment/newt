"""Actual-core XML session gate; temporary host Expat, never a target backend."""
from contextlib import ExitStack
import hashlib
import importlib.util
import json
from pathlib import Path
import shlex
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
    packet = root / 'tests/xml_session'
    load(packet / 'native_inputs.py', 'session_expat_inputs').verify(packet, gate.ROOT / 'vendor/freeink-sdk')
    support = load(root / 'tests/inflate/run_support.py', 'xml_session_support')
    support.ROOT = gate.ROOT
    canonical = support.source_inventory(root)
    with ExitStack() as seals:
        seals.callback(support.verify_source_inventory, root, canonical)
        support.install_signal_handlers()
        tools = support.verify_tools()
        output = Path(tempfile.mkdtemp(prefix='xml-session-', dir=gate.SCRATCH)).resolve()
        support.require(not output.resolve().is_relative_to(root.resolve()), 'build output inside sealed source')
        runner = support.Run(output, tools, time.monotonic() + 600)
        runner.env['RUSTFLAGS'] = '-Dwarnings -Ccodegen-units=1'
        provenance = json.loads((packet / 'PROVENANCE.json').read_text())
        support.require(provenance['sdk_commit'] == 'cd6f5b831c8fad899723b6d406586f00c575f021'
                        and provenance['newt_commit'] == 'bc9ffda52b85f277e46c63718ac88d62fba5d368',
                        'XML source pins changed')
        runner.run([sys.executable, '-B', packet / 'protocol_controls.py'], packet)
        runner.run([sys.executable, '-B', packet / 'field_controls.py'], packet)
        runner.run([sys.executable, '-B', root / 'tests/inflate/owned_children_test.py'], root)
        # This is the ordinary, native-free lifecycle path too; mocks do not prove XML or native OOM.
        support.verify_source_inventory(root, canonical)
        runner.cargo(root, output / 'mock-target', 'test', '-p', 'newt-core', '--test', 'xml_session_lifecycle')
        vectors = load(packet / 'vectors.py', 'xml_session_vectors').VECTORS
        protocol = load(packet / 'protocol.py', 'xml_session_protocol')
        cases = json.loads((packet / 'cases.json').read_text())
        mandatory = {'attributes', 'namespace', 'utf8', 'xml-entity', 'mismatch', 'empty', 'io-failure',
                     'short-reads', 'initial-stop', 'same-chunk-stop-error', 'html-entity', 'stepped-stop'}
        support.require(len(cases) == len(vectors) == len(mandatory) and {c['id'] for c in cases} == mandatory,
                        'XML mandatory case IDs missing/duplicated')
        for case, (wire, expected) in zip(cases, vectors):
            support.require(case['request_sha256'] == hashlib.sha256(wire.encode()).hexdigest()
                            and case['trace_sha256'] == hashlib.sha256(expected.encode()).hexdigest(),
                            'XML case fingerprint changed')
            protocol.parse(expected, wire)
        book = gate.ROOT / 'vendor/freeink-sdk/libs/book/FreeInkBook'
        include = book / 'include'
        expat = book / 'third_party/expat'
        flags = ['-O1', '-g', '-fsanitize=address,undefined', '-fno-sanitize-recover=all', '-fno-omit-frame-pointer']
        objects = {}
        for name, source in [('parse', book / 'src/vendor/expat_xmlparse.c'),
                             ('tok', book / 'src/vendor/expat_xmltok.c'),
                             ('role', book / 'src/vendor/expat_xmlrole.c'),
                             ('bridge', root / 'target-expat/native/expat_bridge.c')]:
            obj = output / (name + '.o')
            support.verify_source_inventory(root, canonical)
            runner.run([tools['clang++'], '--no-default-config', '-x', 'c', '-std=c99', *flags,
                        '-I', include, '-I', expat, '-c', source, '-o', obj], root)
            objects[name] = obj
        frozen_transport = load(root / 'tests/inflate/expected_transport.py', 'frozen_transport')
        reference = frozen_transport.Frozen(packet / 'expected.json', [dict(name=c['id'], input=v[0], args=[]) for c, v in zip(cases, vectors)], provenance, literal_answers=[answer for _, answer in vectors])
        cpp = packet / 'expected.json'
        # Objects, sanitizer runtime selection and approved linker are scoped to this
        # Run only. No miniz/C++ session/reader/filter object enters the Rust runtime.
        native = ['-Dwarnings', '-Ccodegen-units=1', '-C', 'linker=' + str(tools['clang++']),
                  '-C', 'link-arg=--no-default-config', '-C', 'link-arg=-fsanitize=address,undefined',
                  '-C', 'link-arg=-fno-sanitize-recover=all']
        for name in ('parse', 'tok', 'role', 'bridge'):
            native += ['-C', 'link-arg=' + str(objects[name])]
        runner.env['RUSTFLAGS'] = shlex.join(native)

        def build(workspace, target, execute_tests=False, expected=None):
            support.require(not target.resolve().is_relative_to(workspace.resolve()), 'target inside sealed source')
            support.verify_source_inventory(workspace, canonical if expected is None else expected)
            messages = runner.cargo(workspace, target, 'test', '--no-run', '-p', 'newt-parity',
                                    '--bin', 'xml-session-host', '--features', 'xml-native', '--message-format=json')
            tests = []
            for line in messages.splitlines():
                message = json.loads(line)
                if message.get('reason') == 'compiler-artifact' and message['target']['name'] == 'xml-session-host':
                    support.require(message.get('fresh') is False, 'XML native tests must compile freshly')
                    tests.append(message['executable'])
            support.require(len(tests) == 1 and tests[0], 'XML native test artifact missing')
            if execute_tests:
                runner.run([tests[0], '--test-threads=2'], workspace)
            # Separate fresh target directory ensures a fresh core AND real host binary,
            # even after compiling the test harness above.
            support.verify_source_inventory(workspace, canonical if expected is None else expected)
            messages = runner.cargo(workspace, target / 'transport', 'build', '-p', 'newt-parity',
                                    '--bin', 'xml-session-host', '--features', 'xml-native', '--message-format=json')
            fresh = set()
            for line in messages.splitlines():
                message = json.loads(line)
                if message.get('reason') == 'compiler-artifact' and message['target']['name'] in ('newt_core', 'xml-session-host'):
                    support.require(message.get('fresh') is False, 'XML actual core/host must compile freshly')
                    fresh.add(message['target']['name'])
            support.require(fresh == {'newt_core', 'xml-session-host'}, 'XML fresh actual artifacts missing')
            return target / 'transport/debug/xml-session-host'

        def reply(binary, wire):
            raw = reference.text(wire) if binary == cpp else runner.execute([binary], root, wire)
            return protocol.parse(raw, wire), raw

        rust = build(root, output / 'baseline-target', True)
        baseline = {}
        positives = []
        for case, (wire, expected) in zip(cases, vectors):
            original, original_raw = reply(cpp, wire)
            actual, actual_raw = reply(rust, wire)
            support.require(original_raw == expected == actual_raw, 'XML independent/original trace mismatch: ' + case['id'])
            baseline[case['id']] = original
            positives.append({**case, 'cpp_sha256': hashlib.sha256(original_raw.encode()).hexdigest(),
                              'rust_sha256': hashlib.sha256(actual_raw.encode()).hexdigest()})
        runner.run([sys.executable, '-B', packet / 'rejection.py', cpp, rust], packet)
        producer = load(packet / 'producer/gate.py', 'producer_gate').check(
            root, packet, output, runner, tools, objects, flags, build, rust,
            support, load, positive_only, canonical)
        controls = json.loads((packet / 'mutations.json').read_text())
        support.require(len(controls) == 7 and len({c['id'] for c in controls}) == 7, 'XML semantic controls incomplete')
        lookup = {case['id']: vector[0] for case, vector in zip(cases, vectors)}
        detected = []
        for index, control in enumerate([] if positive_only else controls):
            workspace = output / f'mutant-{index:02d}'
            support.verify_source_inventory(root, canonical)
            shutil.copytree(root, workspace, symlinks=True)
            support.verify_source_inventory(workspace, canonical)
            source = workspace / 'core/src/xml_session.rs'
            pristine = source.read_text()
            support.require(pristine.count(control['before']) == 1 and control['before'] != control['after'], 'XML mutation site changed: ' + control['id'])
            mutated = pristine.replace(control['before'], control['after'], 1)
            mutant_inventory = dict(canonical)
            support.require(str(source.relative_to(workspace)) in canonical, 'mutation path outside canonical inventory')
            mutant_inventory[str(source.relative_to(workspace))] = hashlib.sha256(mutated.encode()).hexdigest()
            with ExitStack() as mutant_seals:
                mutant_seals.callback(support.verify_source_inventory, workspace, mutant_inventory)
                source.write_bytes(mutated.encode())
                support.verify_source_inventory(workspace, mutant_inventory)
                mutant = build(workspace, output / f'mutant-target-{index:02d}', expected=mutant_inventory)
                actual, raw = reply(mutant, lookup[control['witness']])
                support.require(actual != baseline[control['witness']], 'XML semantic mutation survived: ' + control['id'])
                detected.append({'id': control['id'], 'witness': control['witness'],
                                 'fresh_core_host_and_tests': True, 'clean_exit_and_complete_protocol': True,
                                 'source_sha256': support.digest(source), 'binary_sha256': support.digest(mutant),
                                 'response_sha256': hashlib.sha256(raw.encode()).hexdigest()})
        support.require(support.verify_tools() == tools, 'XML approved tools changed')
        return {'complete': not positive_only, 'comparisons': len(positives) + len(producer['positives']),
                'positive_cases': positives,
                'semantic_controls': detected + producer['controls'], 'commands': runner.commands, 'provenance': provenance,
                'recorded_sources': canonical, 'expected_sha256': support.digest(cpp), 'rust_sha256': support.digest(rust),
                'scope': 'ordinary and producer; raw/stored/deflated/local headers; host-native Expat',
                'producer': producer,
                'pending': ['pure Rust XML', 'target native backend', 'hardware evidence'],
                'logs_sha256': {p.name: support.digest(p) for p in sorted(output.iterdir()) if p.is_file()}}


def verify_target(gate, artifact):
    lock = json.loads((gate.ROOT / 'rust/tests/inflate/toolchains.json').read_text())
    entry = lock['executables']['nm']
    nm = gate.ROOT / entry['path']
    if gate.digest(nm) != entry['sha256']:
        raise RuntimeError('XML approved target nm changed')
    rows = gate.run([nm, '-S', '--defined-only', artifact], env=gate.environment()).stdout
    name = 'newt_rust_xml_session_probe'
    matches = [row.split() for row in rows.splitlines() if row.split() and row.split()[-1] == name]
    if not any(len(f) == 4 and f[-2] == 'T' and int(f[1], 16) > 0 for f in matches):
        raise RuntimeError('XML runtime-borrowed target probe missing nonempty code')
    producer = 'newt_rust_xml_producer_probe'
    matches = [row.split() for row in rows.splitlines() if row.split() and row.split()[-1] == producer]
    if not any(len(f) == 4 and f[-2] == 'T' and int(f[1], 16) > 0 for f in matches):
        raise RuntimeError('XML runtime-borrowed producer probe missing nonempty code')
    return [name, producer]
