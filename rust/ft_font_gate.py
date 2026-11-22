"""Actual-core FtFont policy / native FreeType boundary. Prepared, unexecuted.
Caller owns coordinated gate lock and verified toolchain lease. No acquisition.
"""
from contextlib import ExitStack
import hashlib
import importlib.util
import json
from pathlib import Path
import re
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
    packet = root / 'tests/ft_font'
    support = load(root / 'tests/inflate/run_support.py', 'ft_font_support')
    support.ROOT = gate.ROOT
    canonical = support.source_inventory(root)
    tools = support.verify_tools()
    tool_hashes = {name: support.digest(path) for name, path in tools.items()}
    output = Path(tempfile.mkdtemp(prefix='ft-font-', dir=gate.SCRATCH)).resolve()
    support.require(output.is_relative_to((gate.ROOT / '.build/rust').resolve()), 'font output outside .build/rust')
    support.require(not output.is_relative_to(root.resolve()), 'font output inside source')
    support.install_signal_handlers()
    runner = support.Run(output, tools, time.monotonic() + 480)
    inventories = {'canonical': canonical}
    trees = [(root, canonical)]

    def seal_all():
        for tree, expected in trees:
            support.verify_source_inventory(tree, expected)

    def compiler(command, cwd=root):
        seal_all()
        return runner.run(command, cwd)

    def cargo(workspace, target, *args):
        support.require(not target.resolve().is_relative_to(workspace.resolve()), 'font target inside source')
        seal_all()
        return runner.cargo(workspace, target, *args)

    def final_tools():
        support.require(support.verify_tools() == tools, 'font tool paths changed')
        support.require({k: support.digest(v) for k, v in tools.items()} == tool_hashes,
                        'font tool contents changed')

    with ExitStack() as seals:
        seals.callback(final_tools)
        seals.callback(seal_all)
        provenance = json.loads((root / 'tests/page_renderer/PROVENANCE.json').read_text())
        support.require(provenance['commit'] == 'cd6f5b831c8fad899723b6d406586f00c575f021'
                        and provenance['reference'] == 'bc9ffda52b85f277e46c63718ac88d62fba5d368',
                        'font source pins changed')
        frozen = gate.ROOT / 'vendor/freeink-sdk'
        fixture = load(packet / 'fixture.py', 'ft_font_fixture')
        protocol = load(packet / 'protocol.py', 'ft_font_protocol')
        protocol.controls()  # Includes all 13 status-dependent malformed outputs.
        cases = list(fixture.cases(snapshot))  # Reads the frozen snapshot, never live root.
        support.require(len(cases) == 9 and len({c[0] for c in cases}) == 9, 'font cases changed')
        proposals = json.loads((packet / 'mutations.json').read_text())
        support.require(len(proposals) == 7 and len({m['name'] for m in proposals}) == 7, 'font controls changed')
        by_name = {c[0]: c for c in cases}

        # Expected derived closure is computed from canonical bytes BEFORE copying.
        native = output / 'native-source'
        product_prefixes = ('libs/font/FreeInkFont/src/freetype/',
                            'libs/font/FreeInkFont/third_party/freetype/')
        product_sources = {name: sha for name, sha in provenance['sha256'].items()
                           if name.startswith(product_prefixes)}
        support.require(bool(product_sources), 'native FreeType product closure absent')
        for name, sha in product_sources.items():
            support.require(support.digest(frozen / name) == sha, 'native FreeType source: ' + name)
        native_expected = {'sdk/' + name: sha for name, sha in product_sources.items()}
        for name, relative in [('native.c', 'firmware/fonts/native/native.c'),
                               ('native.h', 'firmware/fonts/native/native.h')]:
            native_expected[name] = canonical[relative]
        trees.append((native, native_expected))
        inventories['native'] = native_expected
        native.mkdir()
        for name in product_sources:
            destination = native / 'sdk' / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(frozen / name, destination)
        for name, relative in [('native.c', 'firmware/fonts/native/native.c'),
                               ('native.h', 'firmware/fonts/native/native.h')]:
            shutil.copyfile(root / relative, native / name)
        seal_all()
        font = native / 'sdk/libs/font/FreeInkFont'
        positives, controls, artifacts = [], [], []
        frozen_transport = load(root / 'tests/inflate/expected_transport.py', 'frozen_transport')
        reference_provenance = dict(sdk=provenance, source_bindings=json.loads((packet / 'source_bindings.json').read_text()),
                                    notice=json.loads((packet / 'UPSTREAM-NOTICE.json').read_text()))
        reference = frozen_transport.Frozen(packet / 'expected-v2.json.zlib', [dict(mode=mode, input=wire) for mode in (0, 1) for _, wire, _ in cases], reference_provenance)
        cpp = packet / 'expected-v2.json.zlib'


        def admit(binary, request):
            return protocol.admit(reference.text(request, mode) if binary == cpp else runner.execute([binary], root, request), request)

        def rejection(binary):
            text = runner.execute([sys.executable, '-B', packet / 'rejection.py',
                                   root / 'tests/inflate/run_support.py', binary], root)
            support.require(text == 'REJECTIONS 19\n', 'font rejection harness result changed')

        def build(workspace, target):
            messages = cargo(workspace, target, 'build', '-p', 'newt-parity', '--features', 'ft-native',
                             '--bin', 'ft-font-host', '--message-format=json')
            fresh = set()
            for line in messages.splitlines():
                item = json.loads(line)
                if item.get('reason') == 'compiler-artifact' and item['target']['name'] in ('newt_core', 'ft-font-host'):
                    support.require(item.get('fresh') is False, 'font stale actual core/transport')
                    fresh.add(item['target']['name'])
            support.require(fresh == {'newt_core', 'ft-font-host'}, 'font fresh artifacts missing')
            return target / 'debug/ft-font-host'

        def typed_tests(target, package, expected):
            args = ['test', '-p', package, '--no-run', '--message-format=json']
            if package == 'newt-core':
                args += ['--lib']
            else:
                args += ['--features', 'ft-native', '--bin', 'ft-font-host']
            messages = cargo(root, target, *args)
            executables = [item['executable'] for line in messages.splitlines()
                           if (item := json.loads(line)).get('reason') == 'compiler-artifact'
                           and item.get('executable') and item.get('profile', {}).get('test')]
            support.require(len(executables) == 1, 'font test artifact cardinality')
            command = [executables[0], '--test-threads=2']
            if package == 'newt-core':
                command += ['ft_font::tests::']
            text = runner.execute(command, root)
            support.require(f'test result: ok. {expected} passed; 0 failed;' in text,
                            'font typed tests missing or failed')

        for mode in (0, 1):
            runner.env['NEWT_FT_REFERENCE_MODE'] = str(mode)
            compiled = output / f'native-{mode}'
            compiled.mkdir()
            flags = [f'-DFREEINK_FONT_ENABLE_{name}={mode}' for name in ('AUTOHINT', 'NATIVE_HINTING', 'MONOCHROME')]
            includes = ['-I', font / 'include', '-I', font / 'third_party/freetype/include']
            cflags = ['--no-default-config', '-x', 'c', '-std=c11', '-O1', '-g', '-fsanitize=address,undefined',
                      '-fno-sanitize-recover=all', '-fno-omit-frame-pointer', *includes, *flags]
            objects = []
            for path in sorted((font / 'src/freetype').glob('*.c')):
                obj = compiled / (path.stem + '.o')
                compiler([tools['clang++'], *cflags, '-c', path, '-o', obj])
                objects.append(obj)
            support.require(bool(objects), 'font FreeType C closure missing')
            adapter = compiled / 'native.o'
            compiler([tools['clang++'], *cflags, '-c', native / 'native.c', '-o', adapter])
            # No FtFont/Gpos/Gsub C++ object enters a Rust link.
            link = ['-Dwarnings', '-Ccodegen-units=1', '-Clinker=' + str(tools['clang++']),
                    '-Clink-arg=--no-default-config', '-Clink-arg=-fsanitize=address,undefined',
                    '-Clink-arg=-fno-sanitize-recover=all']
            link += ['-Clink-arg=' + str(p) for p in (adapter, *objects)]
            link += ['-Clink-arg=-lm']
            runner.env['RUSTFLAGS'] = ' '.join(link)
            target = output / f'target-{mode}'
            rust = build(root, target)
            for name, request, expected in cases:
                a, b = admit(cpp, request), admit(rust, request)
                support.require(a == b, 'font differential mismatch: ' + name)
                if expected is not None:
                    support.require(b == expected, 'font independent expected mismatch: ' + name)
                positives.append(dict(mode=mode, name=name, request_sha256=hashlib.sha256(request.encode()).hexdigest(),
                                      reply=b, independent=expected is not None))
            rejection(cpp)
            rejection(rust)
            artifacts.append(dict(mode=mode, expected_sha256=support.digest(cpp), rust_sha256=support.digest(rust),
                                  native_objects={p.name: support.digest(p) for p in (adapter, *objects)}))
            if mode == 0:
                typed_tests(target, 'newt-core', 5)
                typed_tests(target, 'newt-parity', 3)
                for index, mutation in enumerate([] if positive_only else proposals):
                    workspace = output / f'mutant-source-{index}'
                    relative = 'core/src/ft_font.rs'
                    text = (root / relative).read_text()
                    support.require(text.count(mutation['needle']) == 1
                                    and mutation['needle'] != mutation['replacement'], 'font ambiguous mutation')
                    intended = text.replace(mutation['needle'], mutation['replacement'], 1).encode()
                    expected_inventory = dict(canonical)
                    expected_inventory[relative] = hashlib.sha256(intended).hexdigest()
                    seal_all()
                    shutil.copytree(root, workspace)
                    support.verify_source_inventory(workspace, canonical)
                    trees.append((workspace, expected_inventory))
                    inventories[f'mutant-{index}'] = expected_inventory
                    (workspace / relative).write_bytes(intended)
                    seal_all()
                    binary = build(workspace, output / f'mutant-target-{index}')
                    rejection(binary)
                    _, wire, independent = by_name[mutation['witness']]
                    baseline = admit(rust, wire)
                    actual = admit(binary, wire)
                    support.require(actual != baseline, 'font semantic mutation survived: ' + mutation['name'])
                    controls.append(dict(name=mutation['name'], witness=mutation['witness'],
                                         source_sha256=expected_inventory[relative], binary_sha256=support.digest(binary),
                                         fresh_core_and_transport=True, clean_complete_protocol=True, reply=actual))
        seal_all()
        final_tools()
        (output / 'source-identities.json').write_text(json.dumps(inventories, indent=2) + '\n')
        return dict(complete=not positive_only, comparisons=len(positives), positive_cases=positives,
                    semantic_controls=controls, typed_safety_test_count=8, malformed_response_fixture_count=13,
                    commands=runner.commands, artifacts=artifacts, source_inventory_sha256=support.digest(output / 'source-identities.json'),
                    tool_sha256=tool_hashes, provenance=provenance, recorded_sources=canonical,
                    scope='Actual-core FtFont policy; host native FreeType C only; target backend/resources/firmware/hardware unaccepted',
                    target_native_backend_accepted=False,
                    logs_sha256={p.name: support.digest(p) for p in output.iterdir() if p.is_file()})


def verify_target(gate, artifact):
    lock = json.loads((gate.ROOT / 'rust/tests/inflate/toolchains.json').read_text())
    entry = lock['executables']['nm']
    nm = gate.ROOT / entry['path']
    if gate.digest(nm) != entry['sha256']:
        raise RuntimeError('font approved nm changed')
    rows = gate.run([nm, '-C', '-S', '--defined-only', artifact], env=gate.environment()).stdout
    names = ['ft_borrowed_raster_runtime', 'ft_policy_runtime']
    for name in names:
        if not any(row.split()[-1:] == [name] and re.search(r'\b[0-9a-fA-F]+\s+0*[1-9a-fA-F][0-9a-fA-F]*\s+[Tt]\b', row)
                   for row in rows.splitlines()):
            raise RuntimeError('font borrowed RV32 policy body missing: ' + name)
    return names
