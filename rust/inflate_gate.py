"""Real streaming decoder family of the shared newt-core validation gate."""
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
    packet = root / 'tests/inflate'
    support = load(packet / 'run_support.py', 'inflate_support')
    support.ROOT = gate.ROOT
    canonical = support.source_inventory(root)
    with ExitStack() as seals:
        seals.callback(support.verify_source_inventory, root, canonical)
        support.install_signal_handlers()
        tools = support.verify_tools()
        output = Path(tempfile.mkdtemp(prefix='inflate-', dir=gate.SCRATCH))
        support.require(not output.resolve().is_relative_to(root.resolve()), 'build output inside sealed source')
        runner = support.Run(output, tools, time.monotonic() + 300)
        runner.env['RUSTFLAGS'] = '-Dwarnings -Ccodegen-units=1'
        provenance = json.loads((packet / 'provenance.json').read_text())
        support.require(provenance['sdk_commit'] == 'cd6f5b831c8fad899723b6d406586f00c575f021',
                        'inflater SDK pin changed')
        runner.run([sys.executable, '-B', packet / 'harness_tests.py'], packet)
        sanitizer = output / 'sanitizer-control'
        support.verify_source_inventory(root, canonical)
        runner.run([tools['clang++'], '--no-default-config', '-x', 'c', '-std=c99', '-O1',
                    '-fsanitize=address,undefined', '-fno-sanitize-recover=all',
                    packet / 'sanitizer_control.c', '-o', sanitizer], root)
        runner.run([sys.executable, '-B', packet / 'sanitizer_test.py', sanitizer], packet)
        expected_path = packet / 'expected-streams.json.zlib'

        def build(workspace, target, expected=None):
            support.require(not target.resolve().is_relative_to(workspace.resolve()), 'target inside sealed source')
            support.verify_source_inventory(workspace, canonical if expected is None else expected)
            messages = runner.cargo(workspace, target, 'build', '-p', 'newt-parity',
                                    '--bin', 'inflate', '--message-format=json')
            fresh = set()
            for line in messages.splitlines():
                message = json.loads(line)
                if message.get('reason') == 'compiler-artifact':
                    name = message['target']['name']
                    if name in ('newt_core', 'inflate'):
                        support.require(message.get('fresh') is False,
                                        'inflater core and real transport must compile freshly')
                        fresh.add(name)
            support.require(fresh == {'newt_core', 'inflate'}, 'missing fresh actual-core compiler artifacts')
            return target / 'debug/inflate'

        binary = build(root, output / 'baseline-target')
        request = {'rust': str(binary), 'expected': str(expected_path), 'deadline': runner.deadline,
                   'quick': False, 'output': str(output), 'source': str(root / 'core/src/inflate.rs')}
        request_path = output / 'request.json'
        request_path.write_text(json.dumps(request))
        runner.run([sys.executable, '-B', packet / 'suite.py', request_path], packet)
        positives = json.loads((output / 'positive.json').read_text())
        controls = json.loads((packet / 'mutations.json').read_text())
        support.require(len(controls) == 13 and len({c['id'] for c in controls}) == 13,
                        'inflater controls missing or duplicated')
        detected = []
        request['mutants'] = {}
        for index, control in enumerate([] if positive_only else controls):
            workspace = output / f'mutant-{index:02d}'
            support.verify_source_inventory(root, canonical)
            shutil.copytree(root, workspace, symlinks=True)
            support.verify_source_inventory(workspace, canonical)
            source = workspace / 'core/src/inflate.rs'
            pristine = source.read_text()
            support.require(control['expected_occurrences'] == 1 and pristine.count(control['before']) == 1,
                            f'inflater mutation site changed: {control["id"]}')
            support.require(control['before'] != control['after'], 'inflater no-op mutation')
            mutated = pristine.replace(control['before'], control['after'], 1)
            mutant_inventory = dict(canonical)
            support.require(str(source.relative_to(workspace)) in canonical, 'mutation path outside canonical inventory')
            mutant_inventory[str(source.relative_to(workspace))] = hashlib.sha256(mutated.encode()).hexdigest()
            # Controls execute all compiled mutants later; retain each seal until that run exits.
            seals.callback(support.verify_source_inventory, workspace, mutant_inventory)
            source.write_bytes(mutated.encode())
            support.verify_source_inventory(workspace, mutant_inventory)
            mutant = build(workspace, output / f'mutant-target-{index:02d}', expected=mutant_inventory)
            request['mutants'][control['id']] = str(mutant)
        if not positive_only:
            request_path.write_text(json.dumps(request))
            runner.run([sys.executable, '-B', packet / 'controls.py', request_path], packet)
            detected = json.loads((output / 'controls.json').read_text())
            support.require(len(detected) == len(controls), 'missing inflater semantic controls')
        support.require(support.verify_tools() == tools, 'inflater approved tools changed')
        return {'complete': not positive_only, 'comparison_unit': 'per-stream rolling SHA-256 over every complete per-call request and RESULT tuple',
                'comparisons': positives['calls'], 'positives': positives, 'semantic_controls': detected,
                'provenance': provenance, 'toolchains': json.loads((packet / 'toolchains.json').read_text()),
                'rust_transport_sha256': support.digest(binary),
                'expected_sha256': support.digest(expected_path),
                'commands': runner.commands,
                'logs_sha256': {p.name: support.digest(p) for p in sorted(output.iterdir()) if p.is_file()}}


def verify_target(gate, artifact):
    lock = json.loads((gate.ROOT / 'rust/tests/inflate/toolchains.json').read_text())
    entry = lock['executables']['nm']
    nm = gate.ROOT / entry['path']
    if gate.digest(nm) != entry['sha256']:
        raise RuntimeError('inflater approved target nm changed')
    rows = gate.run([nm, '-S', '--defined-only', artifact], env=gate.environment()).stdout
    name = 'newt_rust_inflate_probe'
    matches = [row.split() for row in rows.splitlines() if row.split() and row.split()[-1] == name]
    if not any(len(fields) == 4 and fields[-2] == 'T' and int(fields[1], 16) > 0 for fields in matches):
        raise RuntimeError('actual-core borrowed inflater probe has no defined nonempty RV32 code')
    return name
