"""Unexecuted JPEG actual-core recipe; clean source-sealed semantic controls."""
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
import time


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def bound(root, relative):
    path = Path(relative)
    require(not path.is_absolute() and '..' not in path.parts, 'JPEG binding escapes')
    target = root / path
    for entry in (target, *target.parents):
        if entry == root:
            break
        require(not entry.is_symlink(), 'JPEG symlink in binding')
    return target


def verify_target(gate, archive):
    """Read-only retention check on the actual target-probe archive supplied by root."""
    lock = json.loads((gate.ROOT / 'rust/tests/inflate/toolchains.json').read_text())
    entry = lock['executables']['nm']
    nm = gate.ROOT / entry['path']
    require(gate.digest(nm) == entry['sha256'], 'JPEG approved nm changed')
    result = gate.run([nm, '-S', '--defined-only', archive], env=gate.environment())
    require(result.returncode == 0 and not result.stderr, 'JPEG nm diagnostics')
    names = ['jpeg_gray_memory_runtime', 'jpeg_progressive_memory_runtime',
             'jpeg_info_runtime', 'newt_jpeg_baseline', 'newt_jpeg_progressive']
    for name in names:
        require(any(len(fields) == 4 and fields[-1] == name and fields[-2] == 'T'
                    and int(fields[1], 16) > 0
                    for fields in map(str.split, result.stdout.splitlines())),
                'JPEG nonempty borrowed RV32 body missing: ' + name)
    return names


def check(gate, snapshot, positive_only):
    require(not sys.flags.optimize, 'JPEG validation requires unoptimized Python')
    root = snapshot / 'rust'
    packet = root / 'tests/jpeg_decoder'
    support = load('jpeg_canonical_support', root / 'tests/inflate/run_support.py')
    support.ROOT = gate.ROOT
    support.install_signal_handlers()
    tools = support.verify_tools()
    shared_tools = dict(tools)
    for name, entry in json.loads((packet / 'tool_bindings.json').read_text()).items():
        path = gate.ROOT / entry['path']
        require(sha(path) == entry['sha256'], 'JPEG approved tool changed: ' + name)
        if name in tools:
            require(tools[name] == path, 'JPEG shared tool differs: ' + name)
        tools[name] = path
    # Shared Run and cargo_context are the sole command/configuration owners.
    out = Path(tempfile.mkdtemp(prefix='jpeg-decoder-', dir=gate.SCRATCH)).resolve()
    run = support.Run(out, tools, time.monotonic() + 600)
    run.run([tools['python'], '-B', packet / 'tests/tool_bindings_test.py'], root)
    run.env['RUSTFLAGS'] = '-Dwarnings -Ccodegen-units=1'
    canonical_inventory = support.source_inventory(root)
    mutants, native_expected, native_root = [], None, out / 'native'
    old_modules = {name: sys.modules.get(name) for name in ('vectors', 'progressive_vectors', 'jpeg_protocol')}
    controls, receipts = [], []
    failure = None

    def canonical_seal():
        support.verify_source_inventory(root, canonical_inventory)

    def seals():
        # Run all seals even when an earlier seal fails.
        errors = []
        checks = [('canonical', canonical_seal)]
        if native_expected is not None:
            checks.append(('native', lambda: support.verify_source_inventory(native_root, native_expected)))
        checks.extend((str(tree), lambda tree=tree, inventory=inventory:
                       support.verify_source_inventory(tree, inventory)) for tree, inventory in mutants)
        for name, verify in checks:
            try:
                verify()
            except BaseException as error:
                errors.append((name, repr(error)))
        require(not errors, 'JPEG source seals failed: ' + repr(errors))

    def build(workspace, target):
        seals()
        output = run.cargo(workspace, target, 'build', '-p', 'newt-parity',
                           '--bin', 'jpeg-host', '--message-format=json')
        fresh = set()
        for line in output.splitlines():
            item = json.loads(line)
            if item.get('reason') == 'compiler-artifact' and item['target']['name'] in ('newt_core', 'jpeg-host'):
                require(item['fresh'] is False, 'JPEG reused actual core/host objects')
                fresh.add(item['target']['name'])
        require(fresh == {'newt_core', 'jpeg-host'}, 'JPEG actual core/host missing')
        return target / 'debug/jpeg-host'

    def admitted(exe, request, cwd):
        # Run.execute admits zero exit and empty stderr; grammar precedes mismatch.
        answer = reference.text(request) if exe == oracle else run.execute([exe], cwd, request)
        protocol.reply(answer, request)
        return answer

    def rejection(exe):
        run.execute([tools['python'], '-B', packet / 'tests/rejection.py', exe], root)

    try:
        canonical_seal()
        provenance = json.loads((packet / 'PROVENANCE.json').read_text())
        require(provenance['sdk_commit'] == 'cd6f5b831c8fad899723b6d406586f00c575f021'
                and provenance['newt_reference'] == 'bc9ffda52b85f277e46c63718ac88d62fba5d368', 'JPEG pins changed')
        mapping = json.loads((packet / 'native-sources.json').read_text())
        require({name: entry['sha256'] for name, entry in mapping.items()} == provenance['sha256'],
                'JPEG native closure differs from pinned provenance')
        host = build(root, out / 'canonical-target')
        vectors = load('vectors', packet / 'tests/vectors.py')
        progressive = load('progressive_vectors', packet / 'tests/progressive_vectors.py')
        protocol = load('jpeg_protocol', packet / 'tests/protocol.py')
        cases = list(vectors.cases()) + list(progressive.cases())
        lookup = dict(cases)
        require(len(cases) == len(lookup) == 189, 'JPEG authentic corpus count changed')
        require('row-band-overwrite-stop' in lookup, 'JPEG independent band-stop witness missing')
        frozen_transport = load('jpeg_expected', root / 'tests/inflate/expected_transport.py')
        reference = frozen_transport.Frozen(packet / 'expected-v2.json.zlib',
            [dict(mode=0, input=wire) for _, wire in cases], provenance)
        oracle = packet / 'expected-v2.json.zlib'
        canonical = {}
        rejection(oracle)
        rejection(host)
        for name, request in cases:
            native = admitted(oracle, request, root)
            actual = admitted(host, request, root)
            for independent in (vectors.independent, progressive.independent):
                independent(native, request)
                independent(actual, request)
            require(actual == native, 'JPEG differential mismatch: ' + name)
            canonical[name] = actual
            receipts.append(dict(case=name, request_sha256=hashlib.sha256(request.encode()).hexdigest(),
                                 reply_sha256=hashlib.sha256(actual.encode()).hexdigest()))
        proposals = json.loads((packet / 'mutations.json').read_text())
        require(len(proposals) == 12 and len({m['id'] for m in proposals}) == 12, 'JPEG control set changed')
        unique = set()
        for index, mutation in enumerate([] if positive_only else proposals):
            key = mutation['path'], mutation['old'], mutation['new']
            require(key not in unique, 'JPEG duplicate mutation')
            unique.add(key)
            relative = mutation['path']
            require(relative in ('core/src/jpeg/mod.rs', 'core/src/jpeg/renderer.rs',
                                 'core/src/jpeg/progressive.rs'), 'JPEG mutation outside algorithm')
            original = bound(root, relative).read_text()
            require(original.count(mutation['old']) == 1, 'JPEG mutation anchor not unique')
            intended = original.replace(mutation['old'], mutation['new'], 1).encode()
            expected = dict(canonical_inventory)
            expected[relative] = hashlib.sha256(intended).hexdigest()
            tree = out / f'mutant-{index:02d}'
            seals()
            shutil.copytree(root, tree)
            support.verify_source_inventory(tree, canonical_inventory)
            (tree / relative).write_bytes(intended)
            mutants.append((tree, expected))
            try:
                exe = build(tree, out / f'mutant-target-{index:02d}')
                rejection(exe)
                witness = mutation['witness']
                request = lookup[witness]
                answer = admitted(exe, request, tree)
                require(answer != canonical[witness], 'JPEG surviving clean semantic control: ' + mutation['id'])
                independent_kill = False
                for independent in (vectors.independent, progressive.independent):
                    try:
                        independent(answer, request)
                    except AssertionError:
                        independent_kill = True
                if witness == 'row-band-overwrite-stop':
                    require(independent_kill, 'JPEG band control needs authored vertical DC witness')
                controls.append(dict(id=mutation['id'], witness=witness, source_sha256=expected[relative],
                                     fresh_core_and_transport=True, independent_kill=independent_kill,
                                     request_sha256=hashlib.sha256(request.encode()).hexdigest(),
                                     reply_sha256=hashlib.sha256(answer.encode()).hexdigest()))
            finally:
                seals()
        return dict(complete=not positive_only, comparisons=len(receipts), cases=receipts,
                    semantic_controls=controls, pristine_native=True, provenance=provenance, recorded_sources=canonical_inventory,
                    expected_sha256=sha(oracle), rust_sha256=sha(host), commands=run.commands,
                    pending=['NativePool accounting', 'native target backends', 'resources', 'whole firmware', 'device'],
                    scope='JPEG-only decoder, DC preview, RowScaler and JPEG probe/reopen transport')
    except BaseException as error:
        failure = repr(error)
        raise
    finally:
        final_errors = []
        checks = [('sources', seals), ('shared-tools', lambda: require(
            support.verify_tools() == shared_tools, 'JPEG shared tools changed'))]
        for name, entry in json.loads((packet / 'tool_bindings.json').read_text()).items():
            checks.append((name, lambda entry=entry: require(
                sha(gate.ROOT / entry['path']) == entry['sha256'], 'JPEG extra tool changed')))
        for name, verify in checks:
            try:
                verify()
            except BaseException as error:
                final_errors.append(dict(seal=name, error=repr(error)))
        for name, previous in old_modules.items():
            if previous is None:
                sys.modules.pop(name, None)
            else:
                sys.modules[name] = previous
        (out / 'jpeg-receipt.json').write_text(json.dumps(dict(
            complete=failure is None and not final_errors and not positive_only,
            failure=failure, seal_failures=final_errors, cases=receipts,
            semantic_controls=controls, commands=run.commands), indent=2) + '\n')
        require(not final_errors, 'JPEG final seals failed: ' + repr(final_errors))


def retain_target(gate, snapshot, archive):
    """Optional freestanding C caller; root supplies the actual target-probe archive.

    This links already-built code and checks section-GC retention; it does not
    establish NativePool, native backend, resource, firmware or device acceptance.
    """
    require(not sys.flags.optimize, 'JPEG retention requires unoptimized Python')
    root = snapshot / 'rust'
    support = load('jpeg_retention_support', root / 'tests/inflate/run_support.py')
    support.ROOT = gate.ROOT
    support.install_signal_handlers()
    tools = support.verify_tools()
    extra = json.loads((root / 'tests/jpeg_decoder/tool_bindings.json').read_text())
    for name, entry in extra.items():
        require(sha(gate.ROOT / entry['path']) == entry['sha256'], 'JPEG retention tool changed')
        tools[name] = gate.ROOT / entry['path']
    out = Path(tempfile.mkdtemp(prefix='jpeg-retained-', dir=gate.SCRATCH)).resolve()
    run = support.Run(out, tools, time.monotonic() + 120)
    inventory = support.source_inventory(root)
    archive_sha = sha(archive)

    def seal():
        support.verify_source_inventory(root, inventory)
        require(sha(archive) == archive_sha, 'JPEG target archive changed')

    try:
        obj, elf = out / 'retain.o', out / 'jpeg-retained.elf'
        seal()
        run.run([tools['clang++'], '--no-default-config', '--target=riscv32', '-march=rv32imc',
                 '-mabi=ilp32', '-x', 'c', '-std=c99', '-Os', '-ffreestanding', '-fno-builtin',
                 '-c', root / 'tests/jpeg_decoder/tests/retain.c', '-o', obj], root)
        seal()
        run.run([tools['rust-lld'], '-flavor', 'gnu', '-m', 'elf32lriscv', '--gc-sections',
                 '-e', 'jpeg_retained_probe', obj, archive, '-o', elf], root)
        symbols = run.execute([tools['nm'], '-S', '--defined-only', elf], root)
        for name in ('newt_jpeg_baseline', 'newt_jpeg_progressive', 'jpeg_retained_probe'):
            require(any(len(words) == 4 and words[-1] == name and words[-2] == 'T'
                        and int(words[1], 16) > 0 for words in map(str.split, symbols.splitlines())),
                    'JPEG C retention missing body: ' + name)
        undefined = run.execute([tools['nm'], '--undefined-only', elf], root)
        require(not undefined.strip(), 'JPEG unresolved retained target imports')
        return dict(elf_sha256=sha(elf), archive_sha256=archive_sha, commands=run.commands,
                    scope='freestanding borrowed-runtime C retention only')
    finally:
        seal()
        for name, entry in extra.items():
            require(sha(gate.ROOT / entry['path']) == entry['sha256'], 'JPEG retention tool changed: ' + name)
