"""Pinned whole-state app policy comparisons against the actual shared core."""
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
    packet = root / 'tests/app_policy'
    support = load(root / 'tests/inflate/run_support.py', 'app_shared_support')
    support.ROOT = gate.ROOT
    support.install_signal_handlers()
    tools = support.verify_tools()
    before = support.source_inventory(root)
    provenance = json.loads((packet / 'PROVENANCE.json').read_text())
    support.require(provenance['newt_reference'] == 'bc9ffda52b85f277e46c63718ac88d62fba5d368',
                    'application reference changed')
    captured = json.loads((packet / 'expected-provenance.json').read_text())
    support.require(support.digest(packet / 'expected.json.zlib') == captured['fixture_sha256'],
                    'captured app observations changed')
    support.require(captured['reference_revision'] == '9c627fe5cdc4d53bd2feca6a51ea5c5aa0cab6ca',
                    'app expected outputs must match frozen alpha.16')
    corpus = load(packet / 'cases.py', 'app_cases')
    protocol = load(packet / 'protocol.py', 'app_protocol')
    commands = corpus.corpus()
    independent = corpus.independent()
    output = Path(tempfile.mkdtemp(prefix='app-family-', dir=gate.SCRATCH))
    runner = support.Run(output, tools, time.monotonic() + 200)
    runner.env['RUSTFLAGS'] = '-Dwarnings -Ccodegen-units=1'
    with ExitStack() as seals:
        seals.callback(support.verify_source_inventory, root, before)
        def build(workspace, target, expected_source):
            support.verify_source_inventory(workspace, expected_source)
            raw = runner.cargo(workspace, target, 'build', '-p', 'newt-parity',
                               '--bin', 'app-host', '--message-format=json')
            support.verify_source_inventory(workspace, expected_source)
            fresh = set()
            binary = None
            for line in raw.splitlines():
                item = json.loads(line)
                if item.get('reason') == 'compiler-artifact':
                    name = item['target']['name']
                    if name in ('newt_core', 'app-host'):
                        support.require(item['fresh'] is False, 'app core and transport must compile fresh')
                        fresh.add(name)
                    if name == 'app-host':
                        binary = Path(item['executable'])
            support.require(fresh == {'newt_core', 'app-host'} and binary is not None,
                            'actual app core/host artifacts absent')
            return binary
        rust = build(root, output / 'baseline-target', before)
        def reply(binary, batch):
            text = runner.execute([binary], root, 'AP1\n' + '\n'.join(batch) + '\n')
            return protocol.reply(text, batch)
        wires = [wire for _, wire, _ in independent]
        expected = [result for _, _, result in independent]
        support.require(reply(rust, wires) == expected,
                        'independent full-state app expectations')
        expected_data = load(root / 'expected.py', 'app_expected')
        baseline = expected_data.load(packet / 'expected.json.zlib', commands)
        for offset in range(0, len(commands), 8192):
            batch = commands[offset:offset + 8192]
            native = baseline[offset:offset + len(batch)]
            support.require(reply(rust, batch) == native, 'original app policy differs')
        valid = 'AP1\nH 0\n'
        invalid_requests = ['', valid[:-1], valid + '\n', valid.replace('\n', '\r\n'),
                            valid + 'H 00\n', valid + 'BOGUS\n',
                            valid.replace('H 0', 'H 4294967296'),
                            valid.replace('H 0', 'D 8 0 0 0 0 0 0 0 0 0 0 0 0')]
        for binary in (rust,):
            for request in invalid_requests:
                rejected = support.inherited_child([binary], input=request.encode())
                support.require(rejected.returncode == 2 and rejected.stdout == b'' and rejected.stderr == b'',
                                'silent whole-request rejection required')
        for request, invalid in [(['H 0'], 'AP1\nH 3 Books 6\nEND\n'),
                                 (['H 0'], 'AP1\nH 0 Unknown 6\nEND\n'),
                                 (['G 0 0'], 'AP1\nG 2 0 0 0\nEND\n'),
                                 (['G 0 0'], 'AP1\nG 0 0 3 0\nEND\n')]:
            try:
                protocol.reply(invalid, request)
            except ValueError:
                pass
            else:
                raise RuntimeError('malformed app output admitted')
        controls = []
        if not positive_only:
            mutations = json.loads((packet / 'mutations.json').read_text())
            for index, (name, old, new) in enumerate(mutations):
                workspace = output / f'mutant-{index:02d}'
                shutil.copytree(root, workspace, symlinks=True)
                support.verify_source_inventory(workspace, before)
                path = workspace / 'core/src/app.rs'
                source = path.read_text()
                support.require(source.count(old) == 1, 'app mutation anchor not unique: ' + name)
                changed = source.replace(old, new, 1)
                path.write_text(changed)
                sealed = dict(before)
                sealed['core/src/app.rs'] = hashlib.sha256(changed.encode()).hexdigest()
                with ExitStack() as mutant_seal:
                    mutant_seal.callback(support.verify_source_inventory, workspace, sealed)
                    support.verify_source_inventory(workspace, sealed)
                    binary = build(workspace, output / f'mutant-target-{index:02d}', sealed)
                    witness = None
                    for offset in range(0, len(commands), 8192):
                        batch = commands[offset:offset + 8192]
                        actual = reply(binary, batch)
                        witness = next((offset + i for i, (a, b) in enumerate(
                            zip(actual, baseline[offset:])) if a != b), None)
                        if witness is not None:
                            break
                    support.require(witness is not None, 'application semantic mutant survived: ' + name)
                    controls.append({'name': name, 'witness': commands[witness],
                                     'clean_semantic_kill': True,
                                     'source_sha256': support.digest(path),
                                     'binary_sha256': support.digest(binary)})
        support.require(support.verify_tools() == tools, 'approved app tools changed')
        return {'comparisons': len(commands), 'independent_checks': len(independent),
                'semantic_controls': controls, 'provenance': provenance,
                'source_sha256': before, 'commands': runner.commands, 'output': str(output),
                'expected_sha256': support.digest(packet / 'expected.json.zlib'), 'rust_host_sha256': support.digest(rust),
                'complete_family': not positive_only,
                'firmware_or_power_acceptance': False}


def verify_target(gate, artifact):
    names = ['newt_app_dispatch_borrowed', 'newt_app_wake_borrowed', 'newt_app_graph_borrowed']
    text = gate.run([gate.ROOT / '.toolchain/platformio/packages/toolchain-riscv32-esp/bin/riscv32-esp-elf-nm', '-S', '--defined-only', artifact]).stdout
    for name in names:
        rows = [row.split() for row in text.splitlines() if row.split()[-1:] == [name]]
        if not any(len(row) == 4 and row[-2] == 'T' and int(row[1], 16) > 0 for row in rows):
            raise RuntimeError('app borrowed RV32 symbol absent: ' + name)
    return names
