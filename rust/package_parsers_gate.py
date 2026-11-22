"""Actual-core package parser gate; temporary host Expat, never a target backend."""
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
    packet = root / 'tests/package_parsers'
    load(root / 'tests/xml_session/native_inputs.py', 'package_expat_inputs').verify(packet, gate.ROOT / 'vendor/freeink-sdk')
    support = load(root / 'tests/inflate/run_support.py', 'package_parsers_support')
    support.ROOT = gate.ROOT
    canonical = support.source_inventory(root)
    with ExitStack() as seals:
        seals.callback(support.verify_source_inventory, root, canonical)
        support.install_signal_handlers()
        tools = support.verify_tools()
        output = Path(tempfile.mkdtemp(prefix='package-parsers-', dir=gate.SCRATCH)).resolve()
        support.require(not output.resolve().is_relative_to(root.resolve()), 'build output inside sealed source')
        runner = support.Run(output, tools, time.monotonic() + 360)
        runner.env['RUSTFLAGS'] = '-Dwarnings -Ccodegen-units=1'
        provenance = json.loads((packet / 'PROVENANCE.json').read_text())
        support.require(provenance['sdk_commit'] == 'cd6f5b831c8fad899723b6d406586f00c575f021'
                        and provenance['newt_commit'] == 'bc9ffda52b85f277e46c63718ac88d62fba5d368',
                        'package source pins changed')
        runner.run([sys.executable, '-B', packet / 'protocol_controls.py'], packet)
        runner.run([sys.executable, '-B', root / 'tests/inflate/owned_children_test.py'], root)
        # This is the ordinary, native-free lifecycle path too; mocks do not prove package or native OOM.
        support.verify_source_inventory(root, canonical)
        runner.cargo(root, output / 'mock-target', 'test', '-p', 'newt-core', 'package_parsers::tests')
        vectors = load(packet / 'vectors.py', 'package_parsers_vectors').VECTORS
        protocol = load(packet / 'protocol.py', 'package_parsers_protocol')
        support.require(len(vectors) >= 23 and len({v['id'] for v in vectors}) == len(vectors),
                        'package independent cases missing/duplicated')
        for vector in vectors:
            protocol.parse(vector['expected'], vector['wire'])
            protocol.parse(vector.get('rust_expected', vector['expected']), vector['wire'])
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
        # Objects, sanitizer runtime selection and approved linker are scoped to this
        # Run only. No miniz/C++ session/reader/filter object enters the Rust runtime.
        native = ['-Dwarnings', '-Ccodegen-units=1', '-C', 'linker=' + str(tools['clang++']),
                  '-C', 'link-arg=--no-default-config', '-C', 'link-arg=-fsanitize=address,undefined',
                  '-C', 'link-arg=-fno-sanitize-recover=all']
        for name in ('parse', 'tok', 'role', 'bridge'):
            native += ['-C', 'link-arg=' + str(objects[name])]
        runner.env['RUSTFLAGS'] = shlex.join(native)
        cpp = packet / 'expected.json'
        support.require(support.digest(cpp) == 'b7320acd55986bfc769751bc7b3f5e3928eec0386fb3bf3185b04f426e6626ba',
                        'frozen expected data changed')
        expected_data = load(root / 'expected.py', 'package_parsers_expected')
        reference_requests = [dict(name=c['id'], input=c['wire'], args=[]) for c in vectors]
        literal_answers = [c['expected'] for c in vectors]
        support.require(json.loads(cpp.read_bytes())['source_provenance'] == provenance,
                        'frozen expected provenance changed')
        expected_data.load_literals(cpp, reference_requests, literal_answers)

        def build(workspace, target, execute_tests=False, expected=None):
            support.require(not target.resolve().is_relative_to(workspace.resolve()), 'target inside sealed source')
            support.verify_source_inventory(workspace, canonical if expected is None else expected)
            messages = runner.cargo(workspace, target, 'test', '--no-run', '-p', 'newt-parity',
                                    '--bin', 'package-parsers-host', '--features', 'xml-native', '--message-format=json')
            tests = []
            for line in messages.splitlines():
                message = json.loads(line)
                if message.get('reason') == 'compiler-artifact' and message['target']['name'] == 'package-parsers-host':
                    support.require(message.get('fresh') is False, 'package native tests must compile freshly')
                    tests.append(message['executable'])
            support.require(len(tests) == 1 and tests[0], 'package native test artifact missing')
            if execute_tests:
                runner.run([tests[0], '--test-threads=2'], workspace)
            # Separate fresh target directory ensures a fresh core AND real host binary,
            # even after compiling the test harness above.
            support.verify_source_inventory(workspace, canonical if expected is None else expected)
            messages = runner.cargo(workspace, target / 'transport', 'build', '-p', 'newt-parity',
                                    '--bin', 'package-parsers-host', '--features', 'xml-native', '--message-format=json')
            fresh = set()
            for line in messages.splitlines():
                message = json.loads(line)
                if message.get('reason') == 'compiler-artifact' and message['target']['name'] in ('newt_core', 'package-parsers-host'):
                    support.require(message.get('fresh') is False, 'package actual core/host must compile freshly')
                    fresh.add(message['target']['name'])
            support.require(fresh == {'newt_core', 'package-parsers-host'}, 'package fresh actual artifacts missing')
            return target / 'transport/debug/package-parsers-host'

        def reply(binary, wire):
            raw = next(c['expected'] for c in vectors if c['wire'] == wire) if binary == cpp else runner.execute([binary], root, wire)
            return protocol.parse(raw, wire), raw

        rust = build(root, output / 'baseline-target', True)
        baseline = {}
        positives = []
        for case in vectors:
            wire, expected = case['wire'], case['expected']
            original, original_raw = reply(cpp, wire)
            actual, actual_raw = reply(rust, wire)
            support.require(original_raw == expected and actual_raw == case.get('rust_expected', expected),
                            'package independent/original trace mismatch: ' + case['id'])
            baseline[case['id']] = actual
            positives.append({**case, 'cpp_sha256': hashlib.sha256(original_raw.encode()).hexdigest(),
                              'rust_sha256': hashlib.sha256(actual_raw.encode()).hexdigest()})
        runner.run([sys.executable, '-B', packet / 'rejection.py', cpp, rust], packet)
        controls = json.loads((packet / 'mutations.json').read_text())
        support.require(len(controls) == 9 and len({c['id'] for c in controls}) == 9, 'package semantic controls incomplete')
        lookup = {case['id']: case['wire'] for case in vectors}
        detected = []
        for index, control in enumerate([] if positive_only else controls):
            workspace = output / f'mutant-{index:02d}'
            support.verify_source_inventory(root, canonical)
            shutil.copytree(root, workspace, symlinks=True)
            support.verify_source_inventory(workspace, canonical)
            relative = control.get('path', 'core/src/package_parsers.rs')
            support.require(relative in ('core/src/package_parsers.rs', 'core/src/zip_window/arena.rs'), 'unexpected package mutation path')
            source = workspace / relative
            pristine = source.read_text()
            support.require(pristine.count(control['old']) == 1 and control['old'] != control['new'], 'package mutation site changed')
            mutated = pristine.replace(control['old'], control['new'], 1)
            mutant_inventory = dict(canonical)
            support.require(str(source.relative_to(workspace)) in canonical, 'mutation path outside canonical inventory')
            mutant_inventory[str(source.relative_to(workspace))] = hashlib.sha256(mutated.encode()).hexdigest()
            with ExitStack() as mutant_seals:
                mutant_seals.callback(support.verify_source_inventory, workspace, mutant_inventory)
                source.write_bytes(mutated.encode())
                support.verify_source_inventory(workspace, mutant_inventory)
                mutant = build(workspace, output / f'mutant-target-{index:02d}', expected=mutant_inventory)
                actual, raw = reply(mutant, lookup[control['witness']])
                support.require(actual != baseline[control['witness']], 'package semantic mutation survived: ' + control['id'])
                compressed_witness = control['witness'] + '-local-fixed'
                support.require(compressed_witness in lookup, 'package compressed control witness missing')
                compressed_actual, compressed_raw = reply(mutant, lookup[compressed_witness])
                support.require(compressed_actual != baseline[compressed_witness],
                                'package compressed semantic mutation survived: ' + control['id'])
                detected.append({'id': control['id'], 'witness': control['witness'],
                                 'fresh_core_host_and_tests': True, 'clean_exit_and_complete_protocol': True,
                                 'source_sha256': support.digest(source), 'binary_sha256': support.digest(mutant),
                                 'response_sha256': hashlib.sha256(raw.encode()).hexdigest(),
                                 'compressed_witness': compressed_witness,
                                 'compressed_response_sha256': hashlib.sha256(compressed_raw.encode()).hexdigest()})
        support.require(support.verify_tools() == tools, 'package approved tools changed')
        return {'complete': not positive_only, 'comparisons': sum(case['cpp_sha256'] == case['rust_sha256'] for case in positives),
                'resource_domain_cases': sum(case['cpp_sha256'] != case['rust_sha256'] for case in positives),
                'positive_cases': positives,
                'semantic_controls': detected, 'commands': runner.commands, 'provenance': provenance, 'recorded_sources': canonical,
                'expected_sha256': support.digest(cpp), 'rust_sha256': support.digest(rust),
                'scope': 'raw/local stored and raw-DEFLATE XML; host-native Expat; typed caller resource domains are separate',
                'pending': ['producer policy reconciliation', 'pure Rust XML backend', 'target native backend',
                            'broader compressed/source-failure matrix', 'native allocator failure injection',
                            'target resource acceptance'],
                'logs_sha256': {p.name: support.digest(p) for p in sorted(output.iterdir()) if p.is_file()}}


def verify_target(gate, artifact):
    lock = json.loads((gate.ROOT / 'rust/tests/inflate/toolchains.json').read_text())
    entry = lock['executables']['nm']
    nm = gate.ROOT / entry['path']
    if gate.digest(nm) != entry['sha256']:
        raise RuntimeError('package approved target nm changed')
    rows = gate.run([nm, '-S', '--defined-only', artifact], env=gate.environment()).stdout
    names = ['newt_rust_container_probe', 'newt_rust_package_probe', 'newt_rust_toc_probe']
    for name in names:
        matches = [row.split() for row in rows.splitlines() if row.split() and row.split()[-1] == name]
        if not any(len(f) == 4 and f[-2] == 'T' and int(f[1], 16) > 0 for f in matches):
            raise RuntimeError('package runtime-borrowed target probe missing nonempty code: ' + name)
    return names
