"""ZIP family of the unified gate; same workspace/core, independent real transports."""
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
    packet = root / 'tests/zip_window'
    support = load(root / 'tests/inflate/run_support.py', 'zip_runner_support')
    # Frozen tools resolve at the candidate/adopted root, never at a temporary snapshot.
    support.ROOT = gate.ROOT
    canonical = support.source_inventory(root)
    with ExitStack() as seals:
        seals.callback(support.verify_source_inventory, root, canonical)
        support.install_signal_handlers()
        tools = support.verify_tools()
        zip_tools = json.loads((packet / 'toolchains.json').read_text())
        for name, entry in zip_tools['executables'].items():
            support.require(gate.ROOT / entry['path'] == tools[name]
                            and support.digest(tools[name]) == entry['sha256'], 'ZIP recorded tool identity changed')
        output = Path(tempfile.mkdtemp(prefix='zip-window-', dir=gate.SCRATCH))
        support.require(not output.resolve().is_relative_to(root.resolve()), 'build output inside sealed source')
        runner = support.Run(output, tools, time.monotonic() + 240)
        sys.path.insert(0, str(packet / 'tests'))
        protocol = load(packet / 'tests/protocol.py', 'zip_protocol')
        expectations = load(packet / 'tests/expectations.py', 'zip_expectations')
        cases = list(expectations.fixtures())
        lookup = {name: (stdin, expected) for name, stdin, expected in cases}
        support.require(len(cases) == 29 and len(lookup) == 29, 'ZIP independent corpus changed')
        provenance = json.loads((packet / 'provenance.json').read_text())
        for _, stdin, expected in cases:
            protocol.validate(stdin, '\n'.join(expected) + '\n')
        # Retain both original runner suites, including byte/newline and process cleanup controls.
        runner.run([sys.executable, '-B', packet / 'tests/protocol_test.py'], packet)
        runner.run([sys.executable, '-B', packet / 'tests/runtime_test.py'], packet)
        cpp = packet / 'expected.json'
        support.require(support.digest(cpp) == '8884efe51e377b36f50a0c394f048c3d324317108170c09c88d79a725c6a9974',
                        'frozen expected data changed')
        expected_data = load(root / 'expected.py', 'zip_window_expected')
        reference_requests = [dict(name=name, input=wire, args=[]) for name, wire, _ in cases]
        literal_answers = ['\n'.join(c[2]) + '\n' for c in cases]
        support.require(json.loads(cpp.read_bytes())['source_provenance'] == provenance,
                        'frozen expected provenance changed')
        expected_data.load_literals(cpp, reference_requests, literal_answers)

        core_artifacts = {}

        def build(workspace, target, expected=None):
            support.require(not target.resolve().is_relative_to(workspace.resolve()), 'target inside sealed source')
            support.verify_source_inventory(workspace, canonical if expected is None else expected)
            messages = runner.cargo(workspace, target, 'build', '-p', 'newt-parity',
                                    '--bin', 'zip-window', '--message-format=json')
            found = set()
            for line in messages.splitlines():
                message = json.loads(line)
                if message.get('reason') == 'compiler-artifact':
                    name = message['target']['name']
                    if name in ('newt_core', 'zip-window'):
                        support.require(message.get('fresh') is False,
                                        'ZIP control reused core/transport artifact')
                        found.add(name)
                        if name == 'newt_core':
                            libraries = [Path(p) for p in message['filenames'] if p.endswith('.rlib')]
                            support.require(len(libraries) == 1 and libraries[0].is_relative_to(target),
                                            'ZIP core artifact outside fresh target')
                            core_artifacts[target] = libraries[0]
            support.require(found == {'newt_core', 'zip-window'}, 'ZIP core/transport not compiled')
            return target / 'debug/zip-window'

        target = output / 'baseline-target'
        rust = build(root, target)
        # The original focused tests now import the actual same newt-core.
        support.verify_source_inventory(root, canonical)
        runner.cargo(root, target, 'test', '-p', 'newt-core', '--test', 'zip_window')
        library = core_artifacts[target]
        support.verify_source_inventory(root, canonical)
        runner.run([tools['rustc'], '--edition=2024', '--crate-type=rlib',
                    packet / 'borrowed_no_std.rs', '--extern', f'newt_core={library}',
                    '-L', f'dependency={target}/debug/deps', '-o', output / 'borrowed-host.rlib'], root)
        replies = {}
        for label, binary in [('cpp', cpp), ('rust', rust)]:
            replies[label] = [protocol.validate(stdin, ('\n'.join(expected) + '\n') if binary == cpp else runner.execute([binary], root, stdin))
                              for _, stdin, expected in cases]
        positives = []
        # All replies from both baselines pass the complete grammar before comparisons.
        for index, (name, stdin, expected) in enumerate(cases):
            for label in replies:
                support.require(replies[label][index] == expected,
                                f'ZIP {label} independent expectation failed: {name}')
            positives.append({'name': name, 'stdin': stdin, 'expected': expected,
                              'cpp': replies['cpp'][index], 'rust': replies['rust'][index]})
        controls = json.loads((packet / 'mutations.json').read_text())
        support.require(len(controls) == 14 and len({c['id'] for c in controls}) == 14,
                        'ZIP controls missing or duplicated')
        detected = []
        for index, control in enumerate([] if positive_only else controls):
            support.require(control['path'] in ('src/window.rs', 'src/arena.rs'),
                            'ZIP mutation outside allowed source')
            support.require(control['expectation'] in lookup, 'ZIP control missing named witness')
            workspace = output / f'mutant-{index:02d}'
            support.verify_source_inventory(root, canonical)
            shutil.copytree(root, workspace, symlinks=True)
            support.verify_source_inventory(workspace, canonical)
            source = workspace / 'core/src/zip_window' / Path(control['path']).name
            text = source.read_text()
            support.require(control['anchor_occurrences'] == 1 and text.count(control['anchor']) == 1,
                            f'ZIP mutation anchor changed: {control["id"]}')
            support.require(control['anchor'] != control['replacement'], 'ZIP no-op mutation')
            mutated = text.replace(control['anchor'], control['replacement'], 1)
            mutant_inventory = dict(canonical)
            support.require(str(source.relative_to(workspace)) in canonical, 'mutation path outside canonical inventory')
            mutant_inventory[str(source.relative_to(workspace))] = hashlib.sha256(mutated.encode()).hexdigest()
            with ExitStack() as mutant_seals:
                mutant_seals.callback(support.verify_source_inventory, workspace, mutant_inventory)
                source.write_bytes(mutated.encode())
                support.verify_source_inventory(workspace, mutant_inventory)
                binary = build(workspace, output / f'mutant-target-{index:02d}', expected=mutant_inventory)
                stdin, expected = lookup[control['expectation']]
                actual = protocol.validate(stdin, runner.execute([binary], workspace, stdin))
                support.require(actual != expected, f'ZIP semantic control survived: {control["id"]}')
                detected.append({'id': control['id'], 'witness': control['expectation'],
                                 'fresh_core_and_transport': True, 'valid_grammar': True,
                                 'source_sha256': support.digest(source), 'binary_sha256': support.digest(binary),
                                 'actual': actual, 'expected': expected})
        support.require(support.verify_tools() == tools, 'ZIP toolchain changed during run')
        return {'independent_cases': positives, 'semantic_controls': detected,
                'complete': not positive_only, 'provenance': provenance,
                'toolchains': json.loads((packet / 'toolchains.json').read_text()),
                'expected_sha256': support.digest(cpp), 'rust_transport_sha256': support.digest(rust),
                'commands': runner.commands,
                'logs_sha256': {p.name: support.digest(p) for p in sorted(output.iterdir()) if p.is_file()},
                'borrowed_host_sha256': support.digest(output / 'borrowed-host.rlib')}


def verify_target(gate, artifact):
    """Both borrowed probes live in the existing archive alongside every legacy symbol."""
    tools_lock = json.loads((gate.ROOT / 'rust/tests/zip_window/toolchains.json').read_text())
    entry = tools_lock['executables']['nm']
    nm = gate.ROOT / entry['path']
    if gate.digest(nm) != entry['sha256']:
        raise RuntimeError('ZIP pinned target nm changed')
    symbols = gate.run([nm, '--defined-only', '-S', artifact], env=gate.environment()).stdout
    names = ['newt_zip_window_probe', 'newt_zip_arena_probe']
    for name in names:
        matches = [line.split() for line in symbols.splitlines() if line.split() and line.split()[-1] == name]
        if not any(len(fields) == 4 and fields[-2] == 'T' and int(fields[1], 16) > 0 for fields in matches):
            raise RuntimeError(f'ZIP borrowed target probe has no defined nonempty code: {name}')
    return names
