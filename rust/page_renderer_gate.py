"""Actual-core PageRenderer/FontChain parity with synthetic font/image backends."""
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
    packet = root / 'tests/page_renderer'
    support = load(root / 'tests/inflate/run_support.py', 'page_renderer_support')
    support.ROOT = gate.ROOT
    canonical = support.source_inventory(root)
    with ExitStack() as seals:
        seals.callback(support.verify_source_inventory, root, canonical)
        support.install_signal_handlers()
        tools = support.verify_tools()
        output = Path(tempfile.mkdtemp(prefix='page-renderer-', dir=gate.SCRATCH)).resolve()
        support.require(not output.resolve().is_relative_to(root.resolve()), 'build output inside sealed source')
        runner = support.Run(output, tools, time.monotonic() + 360)
        runner.env['RUSTFLAGS'] = '-Dwarnings -Ccodegen-units=1'
        provenance = json.loads((packet / 'PROVENANCE.json').read_text())
        support.require(provenance['commit'] == 'cd6f5b831c8fad899723b6d406586f00c575f021'
                        and provenance['reference'] == 'bc9ffda52b85f277e46c63718ac88d62fba5d368',
                        'renderer source pins changed')
        def build(workspace, target, expected=None):
            support.require(not target.resolve().is_relative_to(workspace.resolve()), 'target inside sealed source')
            support.verify_source_inventory(workspace, canonical if expected is None else expected)
            messages = runner.cargo(workspace, target, 'build', '-p', 'newt-parity',
                                    '--bin', 'page-renderer-host', '--message-format=json')
            fresh = set()
            for line in messages.splitlines():
                message = json.loads(line)
                if message.get('reason') == 'compiler-artifact' and message['target']['name'] in ('newt_core', 'page-renderer-host'):
                    support.require(message.get('fresh') is False, 'renderer stale core/transport')
                    fresh.add(message['target']['name'])
            support.require(fresh == {'newt_core', 'page-renderer-host'}, 'renderer fresh artifacts missing')
            return target / 'debug/page-renderer-host'

        rust = build(root, output / 'baseline-target')
        support.verify_source_inventory(root, canonical)
        runner.cargo(root, output / 'baseline-target', 'test', '-p', 'newt-core', '--test', 'page_renderer_safety')
        cases = load(packet / 'corpus.py', 'page_renderer_corpus').corpus()
        support.require(len({c['name'] for c in cases}) == len(cases), 'renderer duplicate case')
        replies = load(packet / 'replies.py', 'page_renderer_replies')
        malformed = replies.malformed_fixtures(cases)

        cpp = packet / 'expected.json'
        support.require(support.digest(cpp) == '105509e8eeaab00a2723d553f1b09a4b4d4f8270d35e7a28ce652b73b414a2d1',
                        'frozen expected data changed')
        expected_data = load(root / 'expected.py', 'page_renderer_expected')
        reference_requests = [dict(name=c['name'], input=c['input'], args=[]) for c in cases]
        literal_answers = [c['expected'] for c in cases]
        support.require(json.loads(cpp.read_bytes())['source_provenance'] == provenance,
                        'frozen expected provenance changed')
        expected_data.load_literals(cpp, reference_requests, literal_answers)

        def reply(binary, case):
            if binary == cpp:
                return case['expected']
            return replies.validate(runner.execute([binary], root, case['input']), case['input'])

        positives = []
        for case in cases:
            a, b = reply(cpp, case), reply(rust, case)
            support.require(a == b == case['expected'], 'renderer pixels/trace/status mismatch: ' + case['name'])
            positives.append(dict(name=case['name'], request_sha256=hashlib.sha256(case['input'].encode()).hexdigest(), reply=a))
        runner.run([sys.executable, '-B', packet / 'rejection.py', root / 'tests/inflate/run_support.py', cpp, rust], root)
        proposals = json.loads((packet / 'mutations.json').read_text())
        support.require(len(proposals) == 28 and len({m['name'] for m in proposals}) == 28, 'renderer controls changed')
        controls = []
        by_name = {c['name']: c for c in cases}
        for index, mutation in enumerate([] if positive_only else proposals):
            workspace = output / f'mutant-{index:02d}'
            support.verify_source_inventory(root, canonical)
            shutil.copytree(root, workspace, symlinks=True)
            support.verify_source_inventory(workspace, canonical)
            path = workspace / 'core/src/page_renderer.rs'
            source = path.read_text()
            support.require(source.count(mutation['needle']) == 1 and mutation['needle'] != mutation['replacement'],
                            'renderer mutation needs one fresh actual-core site')
            mutated = source.replace(mutation['needle'], mutation['replacement'], 1)
            mutant_inventory = dict(canonical)
            support.require(str(path.relative_to(workspace)) in canonical, 'mutation path outside canonical inventory')
            mutant_inventory[str(path.relative_to(workspace))] = hashlib.sha256(mutated.encode()).hexdigest()
            with ExitStack() as mutant_seals:
                mutant_seals.callback(support.verify_source_inventory, workspace, mutant_inventory)
                path.write_bytes(mutated.encode())
                support.verify_source_inventory(workspace, mutant_inventory)
                binary = build(workspace, output / f'mutant-target-{index:02d}', expected=mutant_inventory)
                witness = by_name[mutation['witness']]
                raw = reply(binary, witness)
                support.require(raw != witness['expected'], 'renderer semantic mutation survived: ' + mutation['name'])
                controls.append(dict(name=mutation['name'], witness=mutation['witness'], fresh_core_and_transport=True,
                                     complete_protocol_clean_exit_empty_stderr=True, source_sha256=support.digest(path),
                                     binary_sha256=support.digest(binary), reply=raw))
        support.require(support.verify_tools() == tools, 'renderer approved tools changed')
        return dict(complete=not positive_only, comparisons=len(positives), positive_cases=positives,
                    semantic_controls=controls, typed_safety_test_count=5, malformed_response_fixture_count=malformed,
                    provenance=provenance, recorded_sources=canonical, commands=runner.commands,
                    expected_sha256=support.digest(cpp), rust_sha256=support.digest(rust),
                    scope='PageRenderer/FontChain only; synthetic raster/decoded-row backends; no firmware/device claim',
                    logs_sha256={p.name: support.digest(p) for p in output.iterdir() if p.is_file()})


def verify_target(gate, artifact):
    lock = json.loads((gate.ROOT / 'rust/tests/inflate/toolchains.json').read_text())
    entry = lock['executables']['nm']
    nm = gate.ROOT / entry['path']
    if gate.digest(nm) != entry['sha256']:
        raise RuntimeError('renderer approved nm changed')
    rows = gate.run([nm, '-C', '-S', '--defined-only', artifact], env=gate.environment()).stdout
    names = ['page_text_runtime', 'page_images_runtime', 'page_combined_runtime',
             'page_chain_runtime', 'page_borrowed_caller_runtime']
    for name in names:
        if not any(row.split()[-1:] == [name] and re.search(r'\b[0-9a-fA-F]+\s+0*[1-9a-fA-F][0-9a-fA-F]*\s+[Tt]\b', row)
                   for row in rows.splitlines()):
            raise RuntimeError('renderer borrowed RV32 body missing: ' + name)
    return names
