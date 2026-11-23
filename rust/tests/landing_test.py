"""Landing command/receipt contract without invoking a compiler or gate."""
import ast
from contextlib import nullcontext
import json
import hashlib
import io
import re
from pathlib import Path
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[2]
sys.dont_write_bytecode = True
sys.path.insert(0, str(ROOT/'rust'))
import check as checker
import landing
import run_support as support


def privacy_hits(root, scanner):
    """Apply the exporter's text rules; report locations/rules, never private values."""
    owner = re.search(rb'Copyright[^\n]*?\d{4}\s+([^\n]+)',
                      (root/'LICENSE').read_bytes()).group(1).strip()
    rules = list(scanner.PATTERNS.items())
    owner_rule = [('owner-name', re.compile(re.escape(owner), re.I))]
    emails = {address.lower().encode() for _, address in scanner.list_rows(
        (root/scanner.EMAIL_LIST).read_text(), 2, scanner.EMAIL_LIST)}
    allowed = {(name, rule, digest) for _, name, rule, digest in scanner.list_rows(
        (root/scanner.ALLOW_LIST).read_text(), 4, scanner.ALLOW_LIST)}
    hits = []
    for path in sorted((root/'rust').rglob('*')):
        if not path.is_file():
            continue
        name, data = str(path.relative_to(root)), path.read_bytes()
        findings = set()
        for _, kind, payload in scanner.subjects(0, name, data):
            if kind == scanner.UNREAD:
                hits.append((name, 0, 'unread'))
            else:
                findings |= scanner.scan_text(payload, [*rules, *owner_rule], emails,
                                              name=kind == scanner.NAME)
        hits.extend((name, line, rule) for line, rule, digest in findings
                    if (name, rule, digest) not in allowed)
    return sorted(hits)


def passed(count, filtered=0):
    return f'test result: ok. {count} passed; 0 failed; 0 ignored; 0 measured; {filtered} filtered out;\n'


class LandingTest(unittest.TestCase):
    def test_real_loop_inventory_resolves_all_optional_sources(self):
        idle = landing.load('idle_inventory', ROOT/'rust/tests/reader_idle_quiet/check.py')
        for mode in (None, 'pictures', 'cold_geometry', 'home_import', 'library'):
            args = SimpleNamespace(**{name: mode == name
                for name in ('pictures', 'cold_geometry', 'home_import', 'library')})
            sources = idle.source_files(args)
            for name in sources:
                self.assertTrue((ROOT/'rust/firmware'/name).is_file(), name)
            self.assertEqual('reader_image_hash.rs' in sources, mode == 'home_import')

    def test_privacy_scans_compressed_contents_and_refuses_unread_bytes(self):
        import clean_export as scanner
        import zlib
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'rust').mkdir(); (root/'tools').mkdir()
            (root/'LICENSE').write_bytes((ROOT/'LICENSE').read_bytes())
            for name in (scanner.EMAIL_LIST, scanner.ALLOW_LIST):
                (root/name).write_bytes((ROOT/name).read_bytes())
            path = root/'rust/expected.json.zlib'
            path.write_bytes(zlib.compress(b'{}'))
            self.assertEqual(privacy_hits(root, scanner), [])
            path.write_bytes(zlib.compress(('/'+'home'+'/fixture/private').encode()))
            self.assertEqual(privacy_hits(root, scanner), [('rust/expected.json.zlib', 1, 'home-path')])
            path.write_bytes(zlib.compress(b'x'*32))
            with patch.object(scanner, 'MAX_INFLATED', 16):
                self.assertIn(('rust/expected.json.zlib', 0, 'unread'), privacy_hits(root, scanner))

    def test_compiler_admission_uses_recorded_paths_and_refuses_empty_selection(self):
        preferences = landing.load('receipt_preferences', ROOT/'rust/tests/reader_preferences/check.py')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch.object(support, 'ROOT', root):
                tools = {name: root/'.toolchain'/name for name in ('rustc', 'cargo', 'clang++')}
                observed = []
                for name in ('first', 'second'):
                    output = root/'.build'/name
                    command = [tools['rustc'], output/'source.rs', '-o', output/'program']
                    run = SimpleNamespace(tools=tools, output=output,
                        commands=[{'command': list(map(support.receipt_path, command))}])
                    observed.append(preferences.compiler_commands(run, support))
                self.assertEqual(observed[0], observed[1])
                self.assertEqual(observed[0], [['.toolchain/rustc', '@OUTPUT@/source.rs',
                                                '-o', '@OUTPUT@/program']])
                run.commands = [{'command': ['runtime']}]
                with self.assertRaisesRegex(RuntimeError, 'requires recorded compiler commands'):
                    preferences.compiler_commands(run, support)

    def test_native_adapter_admission_rejects_baseline_source_and_noncompiler(self):
        walk = landing.load('receipt_walk', ROOT/'rust/tests/native_page_walk/recipe.py')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch.object(support, 'ROOT', root):
                source = root/'.build/mutant'
                command = ['.toolchain/rustc', '.build/mutant/salamander/native_io/lib.rs']
                run = SimpleNamespace(tools={'rustc': root/'.toolchain/rustc'},
                                      commands=[{'command': command}])
                walk.admit_adapter_command(run, support, 0, source)
                for wrong in (['.toolchain/rustc', 'rust/salamander/native_io/lib.rs'],
                              ['runtime', command[1]]):
                    run.commands[0]['command'] = wrong
                    with self.assertRaisesRegex(RuntimeError, 'must read this mutant'):
                        walk.admit_adapter_command(run, support, 0, source)

    def test_nvs_owner_licence_is_guarded_at_repository_root(self):
        owner = landing.load('receipt_nvs_owner', ROOT/'rust/tests/nvs_guard/owner/recipe.py')
        provenance = json.loads((owner.HERE/'provenance.json').read_text())
        self.assertNotIn('../../../../LICENSE', provenance['packet_sha256'])
        self.assertEqual(provenance['retained_inputs_sha256']['LICENSE'],
                         hashlib.sha256((ROOT/'LICENSE').read_bytes()).hexdigest())
        owner.verify_inputs(ROOT)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); packet = root/'copied/tests/nvs_guard/owner'; packet.mkdir(parents=True)
            licence = root/'LICENSE'; licence.write_bytes((ROOT/'LICENSE').read_bytes())
            manifest = {'packet_sha256': {},
                        'retained_inputs_sha256': {'LICENSE': provenance['retained_inputs_sha256']['LICENSE']}}
            (packet/'provenance.json').write_text(json.dumps(manifest))
            with patch.object(owner, 'HERE', packet):
                owner.verify_inputs(root)
                licence.write_bytes(b'changed')
                with self.assertRaisesRegex(RuntimeError, 'retained input changed: LICENSE'):
                    owner.verify_inputs(root)

    def test_source_privacy_uses_portable_canonical_rules_and_exact_exceptions(self):
        sys.path.insert(0, str(ROOT/'tools'))
        import clean_export as scanner
        self.assertEqual(privacy_hits(ROOT, scanner), [])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'rust').mkdir(); (root/'tools').mkdir()
            (root/'LICENSE').write_bytes((ROOT/'LICENSE').read_bytes())
            for name in (scanner.EMAIL_LIST, scanner.ALLOW_LIST):
                (root/name).write_bytes((ROOT/name).read_bytes())
            source = root/'rust/example.rs'
            source.write_text('// SPDX-License-Identifier: MIT\n')
            self.assertEqual(privacy_hits(root, scanner), [])
            owner = re.search(rb'Copyright[^\n]*?\d{4}\s+([^\n]+)',
                              (root/'LICENSE').read_bytes()).group(1).strip()
            for rule, value in (
                    ('home-path', ('/'+'home'+'/fixture/checkout').encode()),
                    ('email', ('private'+'@'+'mail.org').encode()),
                    ('local-host', ('fixture'+'.local').encode()),
                    ('local-host', ('fixture'+'.tailnet'+'.ts.net').encode()),
                    ('owner-name', owner.lower())):
                with self.subTest(rule=rule):
                    source.write_bytes(value)
                    self.assertEqual(privacy_hits(root, scanner), [('rust/example.rs', 1, rule)])
            source.write_text('// SPDX-License-Identifier: MIT\n')
            (root/'rust/LICENSE').write_bytes((root/'LICENSE').read_bytes())
            self.assertEqual(privacy_hits(root, scanner), [('rust/LICENSE', 3, 'owner-name')])
            (root/'rust/LICENSE').unlink()
            source.write_bytes(b'\x00\xff'+owner)
            self.assertEqual(privacy_hits(root, scanner), [('rust/example.rs', 1, 'owner-name')])
            source.write_bytes(b'\xff'+('/'+'home'+'/fixture/private').encode())
            self.assertEqual(privacy_hits(root, scanner), [('rust/example.rs', 1, 'home-path')])
            source.write_bytes(('self'+'.resume'+'.local').encode())
            self.assertEqual(privacy_hits(root, scanner), [('rust/example.rs', 1, 'local-host')])
            source.unlink()
            exception = root/'rust/firmware/cover_session.rs'
            exception.parent.mkdir(parents=True)
            exception.write_bytes(('self'+'.resume'+'.local').encode())
            self.assertEqual(privacy_hits(root, scanner), [])
            exception.write_bytes(('another'+'.local').encode())
            self.assertEqual(privacy_hits(root, scanner),
                             [('rust/firmware/cover_session.rs', 1, 'local-host')])

    def test_command_receipts_are_relative_without_changing_execution(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root/'output'; output.mkdir()
            runner = object.__new__(support.Run)
            runner.output, runner.tools, runner.commands, runner.env = output, {}, [], {}
            runner.deadline = time.monotonic()+30
            command = [root/'.toolchain/compiler', root/'rust/source.rs',
                       '--extern=core='+str(root/'lib/core.rlib'), '/usr/bin/tool',
                       str(root)+'-other/source.rs']
            process = SimpleNamespace(communicate=Mock(), returncode=0)
            with patch.object(support, 'ROOT', root), \
                 patch.object(support, 'owned_process', return_value=nullcontext(process)) as dispatch:
                runner.run(command, root)
            self.assertEqual(dispatch.call_args.args[0], list(map(str, command)))
            self.assertEqual(dispatch.call_args.kwargs['cwd'], root)
            self.assertEqual(runner.commands[0]['command'], ['.toolchain/compiler', 'rust/source.rs',
                             '--extern=core=lib/core.rlib', '/usr/bin/tool', str(root)+'-other/source.rs'])
            self.assertEqual(runner.commands[0]['cwd'], '.')
            self.assertEqual(json.loads((output/'000.command.json').read_text()), runner.commands[0])
        for name in ('sd_reading_state', 'nvs_guard', 'nvs_backend'):
            document = json.loads((ROOT/'rust/tests'/name/'expected-PROVENANCE.json').read_text())
            for command in document['commands']:
                self.assertFalse(Path(command['cwd']).is_absolute())
                self.assertFalse(any(Path(arg).is_absolute() for arg in command['command']))

    def test_tool_payload_is_portable_but_every_installed_file_stays_bound(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            # Follow the sealed gate inputs instead of duplicating tool ownership.
            for name in checker.source_hashes(ROOT):
                if name.startswith('tools/'):
                    target = root/name
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes((ROOT/name).read_bytes())
            prefix = root/'.toolchain/rust/toolchain'
            component = {'name': 'rustc', 'target': landing.HOST}
            files = {}
            def put(name, value):
                path = prefix/name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(value)
                files[name] = {'sha256': hashlib.sha256(value).hexdigest(),
                               'bytes': len(value), 'executable': False}
            put('bin/rustc', b'approved compiler')
            put('lib/core.rlib', b'approved library')
            put('lib/rustlib/manifest-rustc', b'file:/first/prefix/bin/rustc\n')
            put('lib/rustlib/install.log', b'first installation log')
            receipt = {'lock': {'components': [component]}, 'files': files}
            payload = support.rust_payload_identity(receipt)
            lock = {'executables': {'rustc': {'path': '.toolchain/rust/toolchain/bin/rustc',
                    'sha256': files['bin/rustc']['sha256']}}, 'rust_lock': receipt['lock'],
                    'rust_payload_sha256': payload}
            (root/'toolchains.json').write_text(json.dumps(lock))
            record = root/'.toolchain/rust/installed.json'
            def save(): record.write_text(json.dumps(receipt))
            save()
            with patch.object(support, 'ROOT', root), patch.object(support, 'HERE', root):
                support.verify_tools()
                put('lib/rustlib/manifest-rustc', b'file:/second/prefix/bin/rustc\n')
                put('lib/rustlib/install.log', b'fresh installation log')
                save()
                self.assertEqual(support.rust_payload_identity(receipt), payload)
                support.verify_tools()
                (prefix/'lib/rustlib/install.log').write_bytes(b'unrecorded log change')
                with self.assertRaisesRegex(RuntimeError, 'Rust component changed'):
                    support.verify_tools()
                put('lib/rustlib/install.log', b'recorded metadata')
                save()
                put('lib/core.rlib', b'changed library even with a rewritten receipt')
                save()
                with self.assertRaisesRegex(RuntimeError, 'Rust compiler payload changed'):
                    support.verify_tools()
                put('lib/core.rlib', b'approved library')
                save()
                (prefix/'extra').write_bytes(b'unrecorded installed file')
                with self.assertRaisesRegex(RuntimeError, 'Rust installation file set changed'):
                    support.verify_tools()

    def test_licence_citations_resolve_in_tree_or_pinned_sdk(self):
        citation = re.compile(r'(?<![\w./-])(?:[\w.-]+/)*(?:[\w.-]*(?:LICENSE|LICENCE|COPYING|NOTICE)[\w.-]*|FTL\.TXT|THIRD_PARTY\.md)')
        def missing(root):
            documents = [*root.glob('rust/**/*.rs'), *root.glob('rust/**/NOTICE'),
                         root / 'rust/THIRD_PARTY.md']
            return [(str(document.relative_to(root)), name)
                    for document in documents if document.is_file()
                    for match in citation.finditer(document.read_text())
                    if (name := match[0].rstrip('.'))
                    and not any(path.resolve().is_relative_to(root.resolve()) and path.is_file()
                                for path in (document.parent / name, root / name))]
        self.assertEqual(missing(ROOT), [])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'rust').mkdir()
            source = root / 'rust/example.rs'
            source.write_text('// MIT: vendor/freeink-sdk/LICENSE.\n')
            licence = root / 'vendor/freeink-sdk/LICENSE'
            licence.parent.mkdir(parents=True); licence.write_text('fixture licence')
            self.assertEqual(missing(root), [])
            licence.unlink()
            self.assertEqual(missing(root), [('rust/example.rs', 'vendor/freeink-sdk/LICENSE')])
            source.write_text('// MIT: frozen/LICENSE.\n')
            self.assertEqual(missing(root), [('rust/example.rs', 'frozen/LICENSE')])

    def test_selector_is_bounded_and_exclusive(self):
        args = checker.parse_args(['--landing'])
        self.assertEqual(args.tier, 'landing')
        self.assertEqual(args.control_families, ())
        self.assertFalse(checker.selection_evidence(args)['complete_rust_gate'])
        for options in (['--tier', 'full'], ['--tier', 'fast'], ['--positive-only'],
                        ['--control-family', 'none'], ['--control-family', 'foundation']):
            with self.subTest(options=options), patch('sys.stderr'), self.assertRaises(SystemExit):
                checker.parse_args(['--landing', *options])

    def test_counts_reject_partial_or_ignored_results(self):
        landing.require_tests(support, passed(300)+passed(105), 405)
        landing.require_tests(support, passed(13, 51), 13, filtered=True)
        for output in ('', passed(404), passed(406), passed(405, 1),
                       passed(405).replace('0 ignored', '1 ignored'),
                       passed(405).replace('0 failed', '1 failed')):
            with self.subTest(output=output), self.assertRaises(RuntimeError):
                landing.require_tests(support, output, 405)

    def test_foundation_adapter_records_bytes_and_restores_dispatch(self):
        original = checker.run
        with tempfile.TemporaryDirectory() as directory:
            runner = SimpleNamespace(env={}, output=Path(directory), commands=[])
            def run(command, cwd, stdin):
                self.assertEqual(stdin, 'frame_prefix 0\n')
                runner.commands.append({})
                (runner.output/'000.stderr').write_text('')
                return 'bad\r\n'
            runner.run = run
            with landing.foundation_commands(checker, runner):
                result = checker.run(['fixture'], input=b'frame_prefix 0\n', text=False)
                self.assertEqual(result.stdout, b'bad\r\n')
            self.assertIs(checker.run, original)
            with self.assertRaisesRegex(RuntimeError, 'unsupported'):
                with landing.foundation_commands(checker, runner):
                    checker.run(['fixture'], unexpected=True)
            self.assertIs(checker.run, original)

    def run_mocked(self, root, *, fail=None):
        """Exercise production orchestration, replacing only expensive commands/inputs."""
        build = root/'build'; build.mkdir()
        snapshot = root/'snapshot'; (snapshot/'rust').mkdir(parents=True)
        (snapshot/'rust/toolchain.json').write_text('{}')
        (snapshot/'rust/firmware/native').mkdir(parents=True)
        (snapshot/'rust/firmware/native/firmware_version.h').write_text('#define NEWT_FIRMWARE_VERSION \"0.1.0-alpha.16\"\n')
        receipt = build/'landing.json'; receipt.write_text('stale')
        calls = []
        class FakeRun:
            def __init__(self, output, tools, deadline):
                self.output, self.tools, self.env, self.commands = output, tools, {}, []
            def run(self, command, cwd, stdin=None):
                calls.append((list(map(str,command)), str(cwd), self.env.copy()))
                log = f'{len(self.commands):03d}'
                self.commands.append({'command': list(map(str, command)), 'log': log, 'returncode': 0})
                (self.output/(log+'.stdout')).write_text('mock output')
                (self.output/(log+'.stderr')).write_text('')
                if fail == 'compile':
                    self.commands[-1]['returncode'] = 1
                    (self.output/(log+'.stderr')).write_text('controlled compiler diagnostic\n')
                    raise RuntimeError('mock compiler failed')
                if command[:2] == ['git', 'rev-parse']: return 'a'*12+'\n'
                if command[:2] == ['git', 'status']: return ''
                if command[:2] == ['git', 'show']: return '2026-11-16\n'
                return passed(418 if fail == 'portable' else 419)
            def cargo(self, ws, target, *args):
                result = self.run(['cargo', *args], ws)
                if 'build' in args:
                    archive = target/landing.RV32/'release/libnewt_app_image.a'
                    archive.parent.mkdir(parents=True)
                    archive.write_bytes(b'archive')
                    features = ['default', 'rust-app', *landing.FEATURES.split(',')]
                    if fail == 'fixture': features.append('checkpoint-fixture')
                    return json.dumps({'reason': 'compiler-artifact',
                        'target': {'name': 'newt_app_image'}, 'filenames': [str(archive)],
                        'features': features}) + '\n'
                return result
        fake_support = SimpleNamespace(Run=FakeRun, verify_tools=lambda: {}, require=support.require,
            source_commands=lambda seals: nullcontext(), verify_source_inventory=Mock(), close_sources=Mock())
        binary = root/'parity'; binary.write_bytes(b'parity')
        foundation = Mock(return_value=([None]*(60367 if fail == 'foundation' else 60368), [], binary))
        forwarding = Mock(return_value={'passed_tests': 15, 'forwarding_cases': 55})
        if fail == 'native': forwarding.side_effect = RuntimeError('native failure')
        if fail == 'seal': fake_support.close_sources.side_effect = RuntimeError('seal failure')
        with patch.object(landing, 'load', return_value=fake_support), \
             patch.object(landing, 'native_forwarding', forwarding), \
             patch.object(checker, 'BUILD', build), patch.object(checker, 'foundation', foundation), \
             patch.object(checker, 'SCRATCH', build), \
             patch.object(checker, 'verify_sources'), \
             patch.object(checker.toolchain, 'verify_installed', return_value={'verified': True}):
            if fail:
                with self.assertRaises(RuntimeError):
                    landing.check(checker, {}, snapshot, {'verified': True})
                self.assertFalse(receipt.exists())
            else:
                landing.check(checker, {}, snapshot, {'verified': True})
                evidence = json.loads(receipt.read_text())
                self.assertFalse(evidence['complete_rust_gate'])
                self.assertEqual(evidence['portable']['passed_tests'], 419)
                self.assertEqual(evidence['foundation']['comparisons'], 60368)
                self.assertEqual(evidence['archive']['features'], landing.FEATURES)
                self.assertTrue(evidence['archive']['default_features'])
                self.assertFalse(evidence['archive']['fixture_only_feature'])
                self.assertNotIn('checkpoint-fixture', evidence['archive']['enabled_features'])
                # This test is its own gate phase (#337); landing must not repeat it.
                self.assertFalse(any('landing_test.py' in arg for command, _, _ in calls for arg in command))
                self.assertIn('--workspace', calls[0][0])
                self.assertEqual(calls[0][0].count('--exclude'), 2)
                archive_command, cwd, env = calls[-1]
                self.assertEqual(cwd, str(snapshot/'rust'))
                self.assertIn('newt-app-image', archive_command)
                self.assertIn(landing.FEATURES, archive_command)
                self.assertNotIn('--no-default-features', archive_command)
                self.assertEqual(env['NEWT_DEV_BUILD_ID'], 'a'*12)
                self.assertEqual(env['NEWT_DEV_BUILD_DATE'], '2026-11-16')
                self.assertIn('+zicsr,+zifencei', env['RUSTFLAGS'])
                self.assertIn('--force-dwarf-frame-section', env['RUSTFLAGS'])
                self.assertIn(f'--remap-path-prefix={snapshot}=.', env['RUSTFLAGS'])
                foundation.assert_called_once_with(snapshot, build/'target', portable_done=True)

    def test_orchestration_publishes_only_complete_bounded_result(self):
        with tempfile.TemporaryDirectory() as directory:
            self.run_mocked(Path(directory))

    def test_failed_child_diagnostics_survive_temporary_output_cleanup(self):
        captured = io.StringIO()
        with tempfile.TemporaryDirectory() as directory, patch('sys.stderr', captured):
            self.run_mocked(Path(directory), fail='compile')
        self.assertIn('controlled compiler diagnostic', captured.getvalue())

    def test_failures_never_publish_partial_receipt(self):
        for failure in ('compile', 'portable', 'foundation', 'native', 'seal', 'fixture'):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as directory:
                self.run_mocked(Path(directory), fail=failure)

    def test_sdk_inputs_authenticate_canonical_pin_without_bulk_tree(self):
        with patch.object(landing, 'load', side_effect=[support, Mock()]):
            inputs = landing.sdk_inputs(ROOT)
        self.assertTrue(inputs)
        self.assertTrue(all(name.startswith('vendor/freeink-sdk/') for name in inputs))
        self.assertFalse(any('/frozen/' in name for name in inputs))

    def test_sdk_input_mismatch_fails_before_compilation(self):
        native = landing.load('test_landing_native', ROOT/'rust/tests/phased_reader/native.py')
        with patch.object(landing, 'load', side_effect=[support, native]), \
             patch.object(support, 'digest', return_value='changed'), \
             self.assertRaisesRegex(RuntimeError, 'native product source'):
            landing.sdk_inputs(ROOT)

    def test_source_admission_failure_invalidates_previous_landing_receipt(self):
        with tempfile.TemporaryDirectory() as directory:
            build = Path(directory)
            receipt = build/'landing.json'; receipt.write_text('old pass')
            with patch.object(checker, 'BUILD', build), \
                 patch.object(checker, 'source_hashes', side_effect=RuntimeError('source changed')), \
                 self.assertRaisesRegex(RuntimeError, 'source changed'):
                checker.check(checker.parse_args(['--landing']))
            self.assertFalse(receipt.exists())

    def test_native_recipe_uses_sealed_product_inputs_and_exact_trace_filter(self):
        for count in (15, 14):
            with self.subTest(count=count), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); ws = root/'source'; ws.mkdir()
                fixture = ws/'tests/producer_forwarding/expected-outputs.bin.zlib'
                fixture.parent.mkdir(parents=True); fixture.write_bytes(b'fixture')
                native = SimpleNamespace(assemble=Mock(return_value=ws),
                    verify_native_inputs=Mock(), native=Mock(return_value=(None, root/"phased-reader-host")),
                    book_language=Mock())
                sha = SimpleNamespace(verify=Mock())
                recipe = SimpleNamespace(MATRIX=list(range(55)))
                seals = [SimpleNamespace(hashes={'input': 'sha'}), SimpleNamespace(hashes={'crypto': 'sha'})]
                mocked_support = SimpleNamespace(SourceSeal=Mock(side_effect=seals),
                    close_sources=Mock(), source_inventory=Mock(return_value={'source': 'sha'}),
                    require=support.require)
                runner = SimpleNamespace(output=root, env={'RUSTFLAGS': '-Dwarnings native-objects'},
                    tools={'clang++': 'mock-clang'}, run=Mock(), cargo=Mock(return_value=passed(count, 17)))
                with patch.object(landing, 'load', side_effect=[native, sha, recipe]):
                    if count == 15:
                        result = landing.native_forwarding(checker, mocked_support, runner, root)
                        self.assertEqual(result['forwarding_cases'], 55)
                    else:
                        with self.assertRaises(RuntimeError):
                            landing.native_forwarding(checker, mocked_support, runner, root)
                native.assemble.assert_called_once_with(root, mocked_support)
                self.assertEqual(native.ROOT, root)
                self.assertEqual(native.root_ws, root/'rust')
                native.verify_native_inputs.assert_called_once_with(mocked_support, root/'vendor/freeink-sdk')
                native.native.assert_called_once_with(runner, mocked_support, ws, root, root/'vendor/freeink-sdk')
                native.book_language.assert_called_once_with(runner, ws, root/"phased-reader-host")
                command = runner.run.call_args.args[0]
                self.assertIn(root/'rust/tests/include', command)
                self.assertIn(root/'rust/tests/producer_forwarding/native_sha.cpp', command)
                self.assertIn('libcrypto.a', runner.env['RUSTFLAGS'])
                self.assertIn('-lstdc++', runner.env['RUSTFLAGS'])
                self.assertEqual(runner.cargo.call_args.args[2:], ('test', '-p', 'newt-phased-reader',
                    '--lib', '--features', 'native-host', '--target', landing.HOST,
                    'producer_stream_tests::traces'))
                self.assertNotIn('--', runner.cargo.call_args.args)
                mocked_support.close_sources.assert_called_once_with(tuple(seals))
                self.assertEqual(runner.extra_seals, ())

    def test_legacy_foundation_controls_still_use_shared_body(self):
        tree = ast.parse((ROOT/'rust/check.py').read_text())
        full = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == '_check_snapshot')
        called = [node.func.id for node in ast.walk(full) if isinstance(node, ast.Call) and isinstance(node.func, ast.Name)]
        self.assertEqual(called.count('foundation'), 1)
        self.assertEqual(called.count('controls'), 1)
        self.assertFalse(any(isinstance(node, ast.Name) and node.id == 'load_expected' for node in ast.walk(full)))


if __name__ == '__main__':
    unittest.main()
