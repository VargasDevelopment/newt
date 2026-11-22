"""Real PNG decoding/scaling on the actual shared Rust core; native UB scope explicit."""
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import shutil
import sys
import tempfile
import time

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path)
    module=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def require(ok,why):
    if not ok: raise RuntimeError(why)


def admitted(run,exe,request,protocol,cwd):
    text=run.execute([exe],cwd,request)
    return text,protocol.reply(text,request)


def checks(run,exe,protocol,independent,vectors,cwd,modes=(0,1)):
    receipts=[]
    for name,req,pixels in independent.pixel_cases():
        for bits in modes:
            tokens=req.split(' ');tokens[2]=str(bits);request=' '.join(tokens)
            text,words=admitted(run,exe,request,protocol,cwd)
            try: independent.check_pixels(words,pixels)
            except AssertionError as error: raise AssertionError(f'{name}, bits={bits}, request={hashlib.sha256(request.encode()).hexdigest()}: {error}') from error
            receipts.append({'case':name,'bits':bits,'basis':'authored-prefilter-pixels','sha256':hashlib.sha256(text.encode()).hexdigest()})
    for name,req,expected in independent.literal_cases():
        for bits in modes:
            tokens=req.split(' ');tokens[2]=str(bits);request=' '.join(tokens)
            text,words=admitted(run,exe,request,protocol,cwd)
            try: independent.check_literal(name,words,text,expected)
            except AssertionError as error: raise AssertionError(f'{name}, bits={bits}, request={hashlib.sha256(request.encode()).hexdigest()}: {error}') from error
            receipts.append({'case':name,'bits':bits,'basis':'literal-source-trace'})
    return receipts


def rejection_checks(run, exe, packet, root):
    run.execute([sys.executable, '-B', packet / 'tests/rejection.py',
                 root / 'tests/inflate/run_support.py', exe], root)


def check(gate, snapshot, positive_only):
    root = snapshot / 'rust'
    packet = root / 'tests/png_decoder'
    support = load('png_shared_support', root / 'tests/inflate/run_support.py')
    support.ROOT = gate.ROOT
    support.install_signal_handlers()
    tools = support.verify_tools()
    output = Path(tempfile.mkdtemp(prefix='png-decoder-', dir=gate.SCRATCH)).resolve()
    run = support.Run(output, tools, time.monotonic() + 480)
    run.env['RUSTFLAGS'] = ('-Dwarnings -Ccodegen-units=1 -Clinker='
                            + str(tools['clang++']) + ' -Clink-arg=--no-default-config')
    provenance = json.loads((packet / 'PROVENANCE.json').read_text())
    require(provenance['sdk_commit'] == 'cd6f5b831c8fad899723b6d406586f00c575f021' and provenance['newt_reference'] == 'bc9ffda52b85f277e46c63718ac88d62fba5d368', 'PNG SDK pin changed')
    vectors = load('vectors', packet / 'tests/vectors.py')
    baseline_inventory = support.source_inventory(root)
    previous = sys.modules.get('vectors')
    sys.modules['vectors'] = vectors
    protocol = load('png_protocol', packet / 'tests/protocol.py')
    independent = load('png_independent', packet / 'tests/independent.py')

    def build(workspace, target):
        messages = run.cargo(workspace, target, 'build', '-p', 'newt-parity',
                             '--bin', 'png-host', '--message-format=json')
        fresh = set()
        for row in messages.splitlines():
            item = json.loads(row)
            if item.get('reason') == 'compiler-artifact' and item['target']['name'] in ('newt_core', 'png-host'):
                require(item['fresh'] is False, 'PNG actual core/transport reused stale objects')
                fresh.add(item['target']['name'])
        require(fresh == {'newt_core', 'png-host'}, 'PNG actual core/transport missing')
        return target / 'debug/png-host'

    native_inventory = None
    try:
        host = build(root, output / 'baseline-target')
        run.cargo(root, output / 'baseline-target', 'test', '-p', 'newt-core',
                  '--test', 'png_decoder_safety')
        rejection_checks(run, host, packet, root)
        authored = checks(run, host, protocol, independent, vectors, root)
        frozen_transport = load('png_expected', root / 'tests/inflate/expected_transport.py')
        native_requests = []
        for _, req, _ in list(independent.pixel_cases()) + list(independent.literal_cases()):
            for bits in (0, 1):
                tokens = req.split(' '); tokens[2] = str(bits)
                native_requests.append(dict(mode=0, input=' '.join(tokens)))
        native_requests += [dict(mode=0, input=wire) for _, wire in vectors.cases()]
        reference = frozen_transport.Frozen(packet / 'expected-v2.json.zlib', native_requests, provenance)
        oracle = packet / 'expected-v2.json.zlib'
        native_run = reference
        rejection_checks(run, oracle, packet, root)
        checks(native_run, oracle, protocol, independent, vectors, root)
        baseline = []
        for name, request in vectors.cases():
            native, _ = admitted(native_run, oracle, request, protocol, root)
            for bits in (0, 1):
                fields = request.split(' ')
                fields[2] = str(bits)
                wire = ' '.join(fields)
                actual, _ = admitted(run, host, wire, protocol, root)
                require(native == actual, 'PNG normalized differential mismatch: ' + name)
                baseline.append((name, wire, actual))
        proposals = json.loads((packet / 'mutations.json').read_text())
        require(len(proposals) == 13, 'PNG control count changed')
        unique, controls = set(), []
        for index, mutation in enumerate([] if positive_only else proposals):
            key = mutation['path'], mutation['old'], mutation['new']
            require(key not in unique, 'duplicate PNG control')
            unique.add(key)
            workspace = output / f'mutant-{index:02d}'
            shutil.copytree(root, workspace, symlinks=True)
            support.verify_source_inventory(workspace, baseline_inventory)
            path = Path(mutation['path'])
            require(str(path) in ('core/src/pngle.rs', 'core/src/image_renderer.rs'), 'PNG mutation domain')
            source = workspace / path
            text = source.read_text()
            require(text.count(mutation['old']) == 1, 'PNG mutation anchor not unique')
            intended = text.replace(mutation['old'], mutation['new'], 1).encode()
            source.write_bytes(intended)
            mutated_inventory = dict(baseline_inventory)
            mutated_inventory[str(path)] = hashlib.sha256(intended).hexdigest()
            support.verify_source_inventory(workspace, mutated_inventory)
            try:
                exe = build(workspace, output / f'mutant-target-{index:02d}')
                rejection_checks(run, exe, packet, root)
                witnesses = []
                for bits in (0, 1):
                    witness = None
                    try:
                        checks(run, exe, protocol, independent, vectors, workspace, modes=(bits,))
                    except AssertionError as error:
                        witness = dict(basis='independent-source-expectation', detail=str(error), bits=bits)
                    if witness is None:
                        for name, wire, expected in baseline:
                            if int(wire.split(' ')[2]) != bits:
                                continue
                            actual, _ = admitted(run, exe, wire, protocol, workspace)
                            if actual != expected:
                                witness = dict(basis='normalized-differential', case=name, bits=bits,
                                               request_sha256=hashlib.sha256(wire.encode()).hexdigest())
                                break
                    require(witness is not None, 'PNG mutation survived: ' + mutation['id'])
                    witnesses.append(witness)
                controls.append(dict(name=mutation['id'], source_sha256=sha(source),
                                     fresh_core_and_transport=True, clean_semantic_witnesses=witnesses))
            finally:
                support.verify_source_inventory(workspace, mutated_inventory)
        require(support.verify_tools() == tools, 'PNG approved tools changed')
        return dict(complete=not positive_only, comparisons=len(baseline),
                    independent_checks=authored, semantic_controls=controls,
                    normalized_differential=[dict(case=n, request_sha256=hashlib.sha256(w.encode()).hexdigest(),
                                                 reply_sha256=hashlib.sha256(r.encode()).hexdigest())
                                             for n, w, r in baseline],
                    native_normalization=json.loads(__import__('zlib').decompress(oracle.read_bytes()))['normalization'],
                    pristine_native_full_corpus=False, target_math_and_resources_accepted=False,
                    scope='PNG/RowScaler only, normalized native oracle; no linked firmware or hardware claim',
                    provenance=provenance, recorded_sources=baseline_inventory, commands=run.commands,
                    expected_sha256=sha(oracle), rust_sha256=sha(host),
                    logs_sha256={p.name: sha(p) for p in output.iterdir() if p.is_file()})
    finally:
        if native_inventory is not None:
            support.verify_source_inventory(native_root, native_inventory)
        support.verify_source_inventory(root, baseline_inventory)
        if previous is None:
            sys.modules.pop('vectors', None)
        else:
            sys.modules['vectors'] = previous


def verify_target(gate, artifact):
    lock = json.loads((gate.ROOT / 'rust/tests/inflate/toolchains.json').read_text())
    entry = lock['executables']['nm']
    nm = gate.ROOT / entry['path']
    require(gate.digest(nm) == entry['sha256'], 'PNG approved nm changed')
    rows = gate.run([nm, '-S', '--defined-only', artifact], env=gate.environment()).stdout
    names = ['png_feed_runtime', 'png_gray_memory_runtime']
    for name in names:
        matches = [row.split() for row in rows.splitlines() if row.split()[-1:] == [name]]
        require(any(len(row) == 4 and row[-2] == 'T' and int(row[1], 16) > 0 for row in matches),
                'PNG nonempty borrowed RV32 body missing: ' + name)
    return names
