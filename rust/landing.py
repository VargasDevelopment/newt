"""Bounded landing evidence, deliberately separate from the mutation/full gate."""
from contextlib import contextmanager
import importlib.util
import json
from pathlib import Path
import re
import shlex
import subprocess
import sys
import time

SDK_PIN = 'cd6f5b831c8fad899723b6d406586f00c575f021'
HOST = 'x86_64-unknown-linux-gnu'
# Exact count of the workspace's host tests, so a test that stops running fails landing.
PORTABLE_TESTS = 428
RV32 = 'riscv32imc-unknown-none-elf'
FEATURES = 'dev-usb,wifi-update,embedded-sample'


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def sdk_inputs(root):
    """Copy only authenticated canonical SDK inputs, never historical bulk trees."""
    support = load('landing_support_inputs', root / 'rust/tests/inflate/run_support.py')
    native = load('landing_native_inputs', root / 'rust/tests/phased_reader/native.py')
    sdk = root / 'vendor/freeink-sdk'
    native.verify_native_inputs(support, sdk)
    pins = json.loads((root / 'rust/tests/phased_reader/native-inputs.json').read_text())
    support.require(pins['sdk'] == SDK_PIN, 'landing SDK identity')
    sources = dict(pins['sha256'])
    # Expat's file-set guard also authenticates its licence/build metadata.
    provenance = json.loads((root / 'rust/tests/xml_session/PROVENANCE.json').read_text())
    for row in provenance['files']:
        name, expected = row['path'], row['sha256']
        if name.startswith('libs/book/FreeInkBook/third_party/expat/'):
            support.require(name not in sources or sources[name] == expected, 'consistent Expat pin')
            support.require(support.digest(sdk / name) == expected, 'native Expat source changed')
            sources[name] = expected
    for record in ('inflate/provenance.json', 'page_renderer/PROVENANCE.json'):
        captured = json.loads((root / 'rust/tests' / record).read_text())['sha256']
        for name, expected in captured.items():
            if any(word in Path(name).name for word in ('LICENSE', 'NOTICE', 'FTL.TXT')):
                support.require(name not in sources or sources[name] == expected, 'consistent licence pin')
                support.require(support.digest(sdk / name) == expected, 'native licence source changed')
                sources[name] = expected
    return {'vendor/freeink-sdk/' + name: sha for name, sha in sources.items()}


def require_tests(support, output, expected, *, filtered=False):
    rows = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;', output)
    support.require(bool(rows) and sum(int(row[0]) for row in rows) == expected
                    and all(row[1:4] == ('0', '0', '0') and (filtered or row[4] == '0') for row in rows),
                    f'landing requires exactly {expected} passing tests, none failed/ignored/measured')


@contextmanager
def foundation_commands(checker, runner):
    """Keep the shared foundation implementation on the same recorded Run."""
    original = checker.run

    def recorded(command, *, seals=(), **kwargs):
        env = runner.env
        try:
            runner.env = kwargs.pop('env', checker.environment())
            text = kwargs.pop('text', True)
            stdin = kwargs.pop('input', None)
            if isinstance(stdin, bytes):
                stdin = stdin.decode('ascii')
            cwd = kwargs.pop('cwd', checker.ROOT)
            if kwargs:
                raise RuntimeError('unsupported landing foundation command options')
            with checker.source_commands(seals):
                stdout = runner.run(command, cwd, stdin)
            log = runner.output / f'{len(runner.commands)-1:03d}.stderr'
            return subprocess.CompletedProcess(command, 0, stdout if text else stdout.encode('utf-8'), log.read_text())
        finally:
            runner.env = env
    checker.run = recorded
    try:
        yield
    finally:
        checker.run = original


def native_forwarding(checker, support, runner, snapshot):
    native = load('landing_native', snapshot / 'rust/tests/phased_reader/native.py')
    # assemble's source roots must both resolve inside the sealed snapshot.
    native.ROOT = snapshot
    native.root_ws = snapshot / 'rust'
    ws = native.assemble(runner.output, support)
    sdk = snapshot / 'vendor/freeink-sdk'
    native.verify_native_inputs(support, sdk)
    crypto = checker.ROOT / '.toolchain/openssl'
    sha = load('landing_sha', snapshot / 'rust/tests/native_crypto.py')
    sha.verify(snapshot)
    seals = (support.SourceSeal(ws), support.SourceSeal(crypto))
    runner.extra_seals = seals
    try:
        _, executable = native.native(runner, support, ws, snapshot, sdk)
        native.reader_layout(runner, ws, executable)
        obj = runner.output / 'native_sha.o'
        runner.run([runner.tools['clang++'], '--no-default-config', '-std=c++17',
                    '-O1', '-g', '-fsanitize=address,undefined', '-fno-sanitize-recover=all',
                    '-fno-omit-frame-pointer', '-I', snapshot / 'rust/tests/include',
                    '-I', crypto / 'include', '-c',
                    snapshot / 'rust/tests/producer_forwarding/native_sha.cpp', '-o', obj], snapshot)
        runner.env['RUSTFLAGS'] += ' ' + shlex.join([
            '-C', 'link-arg='+str(obj), '-C', 'link-arg='+str(crypto/'lib/libcrypto.a'),
            '-C', 'link-arg=-lstdc++', '-C', 'link-arg=-ldl', '-C', 'link-arg=-lpthread'])
        output = runner.cargo(ws, runner.output/'target', 'test', '-p', 'newt-phased-reader',
                              '--lib', '--features', 'native-host', '--target', HOST,
                              'producer_stream_tests::traces')
        require_tests(support, output, 16, filtered=True)
        recipe = load('landing_recipe', snapshot / 'rust/tests/producer_forwarding/recipe.py')
        support.require(len(recipe.MATRIX) == 55, 'complete forwarding matrix')
        return {'passed_tests': 16, 'forwarding_cases': 55,
                'fixture_sha256': checker.digest(ws/'tests/producer_forwarding/expected-outputs.bin.zlib'),
                'native_source_sha256': support.source_inventory(ws),
                'openssl_inputs': seals[1].hashes}
    finally:
        try:
            support.close_sources(seals)
        finally:
            runner.extra_seals = ()


def check(checker, before, snapshot, identity):
    output = checker.BUILD / 'landing.json'
    # An interrupted rerun cannot leave an old receipt looking like this run passed.
    output.unlink(missing_ok=True)
    support = load('landing_support', snapshot / 'rust/tests/inflate/run_support.py')
    support.ROOT = checker.ROOT
    tools = support.verify_tools()
    with checker.tempfile.TemporaryDirectory(prefix='landing-', dir=checker.SCRATCH) as directory:
        class SealedRun(support.Run):
            extra_seals = ()

            def run(self, *args, **kwargs):
                try:
                    with support.source_commands((*checker._command_seals, *self.extra_seals)):
                        result = super().run(*args, **kwargs)
                        self.commands[-1]['rustflags'] = self.env.get('RUSTFLAGS', '')
                        return result
                except RuntimeError:
                    # Temporary build outputs go away; preserve failing diagnostics in the gate log.
                    if self.commands and self.commands[-1].get('returncode') != 0:
                        for kind in ('stdout', 'stderr'):
                            path = self.output/(self.commands[-1]['log']+'.'+kind)
                            if path.is_file(): print(path.read_text(), file=sys.stderr, end='')
                    raise

        runner = SealedRun(Path(directory), tools, time.monotonic() + 600)
        runner.env.update(checker.environment())
        ws = snapshot / 'rust'
        # tests/landing_test.py (contract and source privacy) is its own gate
        # phase: with it, landing exceeded the per-phase CPU budget (#337).
        portable = runner.cargo(ws, checker.BUILD/'landing-host-target', 'test', '--workspace',
                                '--exclude', 'newt-app-image', '--exclude', 'newt-native-input-owner',
                                '--target', HOST)
        require_tests(support, portable, PORTABLE_TESTS)
        with foundation_commands(checker, runner):
            cases, expected, binary = checker.foundation(snapshot, checker.BUILD/'target', portable_done=True)
        support.require(len(cases) == 60368, 'complete foundation corpus')
        forwarding = native_forwarding(checker, support, runner, snapshot)
        runner.env = checker.environment()
        runner.env['RUSTFLAGS'] = ('-Dwarnings -Ctarget-feature=+zicsr,+zifencei '
                                  '-Cllvm-args=--force-dwarf-frame-section '
                                  f'--remap-path-prefix={snapshot}=.')
        revision = runner.run(['git', 'rev-parse', '--short=12', 'HEAD'], checker.ROOT).strip()
        dirty = bool(runner.run(['git', 'status', '--porcelain', '--untracked-files=no'], checker.ROOT))
        date = runner.run(['git', 'show', '-s', '--format=%cs', 'HEAD'], checker.ROOT).strip()
        support.require(re.fullmatch(r'[0-9a-f]{12}', revision) is not None
                        and re.fullmatch(r'\d{4}-\d{2}-\d{2}', date) is not None,
                        'product build metadata absent/ambiguous')
        build_metadata = {'NEWT_DEV_BUILD_ID': revision + ('-dirty' if dirty else ''),
                          'NEWT_DEV_BUILD_DATE': date}
        runner.env.update(build_metadata)
        target = checker.BUILD/'landing-rv32-target'
        archive_output = runner.cargo(ws, target, 'build', '-p', 'newt-app-image', '--release',
                     '--target', RV32, '--features', FEATURES, '--message-format=json')
        artifact = target/RV32/'release/libnewt_app_image.a'
        compiled = [json.loads(line) for line in archive_output.splitlines() if line]
        products = [message for message in compiled
                    if message.get('reason') == 'compiler-artifact'
                    and message.get('target', {}).get('name') == 'newt_app_image'
                    and str(artifact) in message.get('filenames', [])]
        support.require(len(products) == 1, 'product archive compiler receipt absent/ambiguous')
        features = products[0].get('features', [])
        support.require(isinstance(features, list) and all(isinstance(f, str) for f in features)
                        and len(features) == len(set(features))
                        and set(features) == {'default', 'rust-app', *FEATURES.split(',')},
                        'product features changed or fixture-only feature enabled')
        support.require(artifact.is_file() and artifact.stat().st_size > 0, 'product archive absent/empty')
        for command in runner.commands:
            for kind in ('stdout', 'stderr'):
                command[kind + '_sha256'] = checker.digest(runner.output / (command['log'] + '.' + kind))
        evidence = {
            'scope': 'bounded-rust-landing', 'complete_rust_gate': False,
            'omitted': ['mutation controls', '20-family gate', 'SDK image/link', 'hardware',
                        'landing contract test (own gate phase)'],
            'source_sha256': before, 'toolchain': identity, 'sdk_pin': SDK_PIN,
            'portable': {'passed_tests': PORTABLE_TESTS, 'excluded': ['newt-app-image', 'newt-native-input-owner']},
            'foundation': {'comparisons': len(cases), 'independent_checks': True,
                           'host_binary_sha256': checker.digest(binary)},
            'native_forwarding': forwarding,
            'archive': {'target': RV32, 'features': FEATURES, 'default_features': True,
                        'enabled_features': features, 'fixture_only_feature': False,
                        'build_metadata': build_metadata,
                        'bytes': artifact.stat().st_size, 'sha256': checker.digest(artifact)},
            'logs_directory': 'landing-logs',
            'commands': runner.commands, 'firmware_linked': False, 'hardware_checked': False,
        }
        # Preserve actual stdout/stderr and command files next to the receipt.
        logs = checker.BUILD/'landing-logs'
        logs.mkdir(exist_ok=True)
        for path in runner.output.iterdir():
            if path.is_file() and path.suffix in ('.stdout', '.stderr', '.json'):
                checker.shutil.copyfile(path, logs/path.name)
        expected_toolchain = json.loads((snapshot/'rust/toolchain.json').read_text())
        support.require(checker.toolchain.verify_installed(expected_toolchain) == identity,
                        'Installed Rust toolchain changed during landing')
        checker.verify_sources(before)
        support.verify_source_inventory(snapshot, before)
        support.close_sources(checker._command_seals)
        temporary = output.with_suffix('.json.partial')
        temporary.write_text(json.dumps(evidence, indent=2)+'\n')
        temporary.replace(output)
    print(f'PASS bounded Rust landing (not complete Rust gate): {output}', flush=True)
