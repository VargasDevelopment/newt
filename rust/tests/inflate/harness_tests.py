"""Focused checks of framing, semantic-kill classification and process cleanup."""
import os
import hashlib
import json
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
from run_support import owned_process, process_running, Run, source_inventory, verify_source_inventory
from cargo_context_test import ContextTests
from compare import parse_result
from expected_stream import StreamDigest
from stream_compare import Peer, ParityMismatch, compare_script, equal, stop_for_signal

HERE = Path(__file__).resolve().parent


class HarnessTests(unittest.TestCase):
    def test_stream_digest_verifies_calls_and_detects_single_byte_flip(self):
        header = b'TINFL1 32 16\n'
        request = b'CALL 4 0 3 6162\n'
        raw = b'RESULT 0 2 3 65535 616263\n'
        req_sha = hashlib.sha256(header + hashlib.sha256(request).digest()).hexdigest()
        resp_sha = hashlib.sha256(raw).hexdigest()
        expected = (1, req_sha, resp_sha)

        matched = StreamDigest(expected, header)
        matched.record(request, raw)
        self.assertTrue(matched.matches())

        # Flipping a single byte in Rust's wire response must fail digest comparison.
        flipped_resp = StreamDigest(expected, header)
        flipped_resp.record(request, raw.replace(b'616263', b'616264', 1))
        self.assertFalse(flipped_resp.matches())

        # Changing the request or call count or header must also fail.
        flipped_req = StreamDigest(expected, header)
        flipped_req.record(request.replace(b'4', b'5', 1), raw)
        self.assertFalse(flipped_req.matches())

        extra_call = StreamDigest(expected, header)
        extra_call.record(request, raw)
        extra_call.record(request, raw)
        self.assertFalse(extra_call.matches())

        wrong_header = StreamDigest(expected, b'TINFL1 64 16\n')
        wrong_header.record(request, raw)
        self.assertFalse(wrong_header.matches())

    def test_children_use_explicit_compiler_environment(self):
        poison = dict.fromkeys(('CPATH', 'CPLUS_INCLUDE_PATH', 'LIBRARY_PATH', 'SDKROOT',
                                'OPENSSL_CONF', 'PYTHONPATH'), '/unrecorded-input')
        poison.update(PATH='/unrecorded-tools', NEWT_HOST_JOBS='2', NEWT_HOST_GLOBAL_JOBS='2')
        with tempfile.TemporaryDirectory(dir=os.environ.get('TMPDIR')) as temp:
            with patch.dict(os.environ, poison):
                run = Run(Path(temp), {"rustc": Path(sys.executable), "rustdoc": Path(sys.executable)},
                          time.monotonic() + 10)
                raw = run.execute([sys.executable, '-B', '-c',
                                   'import json,os; print(json.dumps(dict(os.environ)))'], HERE)
            observed = json.loads(raw)
            self.assertTrue(set(poison).difference(('PATH', 'NEWT_HOST_JOBS',
                                                   'NEWT_HOST_GLOBAL_JOBS')).isdisjoint(observed))
            self.assertEqual(observed['PATH'], '/usr/bin:/bin:/usr/sbin:/sbin')
            self.assertEqual(observed['NEWT_HOST_JOBS'], '2')
            self.assertEqual(observed['NEWT_HOST_GLOBAL_JOBS'], '2')
            self.assertEqual(observed['CARGO_NET_OFFLINE'], 'true')
            self.assertEqual(observed['RUST_TEST_THREADS'], '2')
            self.assertEqual(observed['TMPDIR'], str(Path(temp)/'tmp'))

    def test_source_seal_rejects_unlisted_and_changed_compiler_inputs(self):
        with tempfile.TemporaryDirectory() as temp:
            source = Path(temp) / 'source'
            source.mkdir()
            manifest = source / 'Cargo.toml'
            manifest.write_text('[package]\nname="fixture"\n')
            original = manifest.read_bytes()
            expected = source_inventory(source)
            verify_source_inventory(source, expected)
            for name in ('build.rs', 'extra-module.rs', 'pngle.h'):
                added = source / name
                added.write_text('fn main() {}')
                with self.assertRaisesRegex(RuntimeError, 'sealed source inventory changed'):
                    verify_source_inventory(source, expected)
                added.unlink()
            manifest.write_bytes(original + b'# changed\n')
            with self.assertRaisesRegex(RuntimeError, 'sealed source inventory changed'):
                verify_source_inventory(source, expected)
            manifest.unlink()
            with self.assertRaisesRegex(RuntimeError, 'sealed source inventory changed'):
                verify_source_inventory(source, expected)

    def test_source_seal_rejects_file_and_directory_symlinks(self):
        with tempfile.TemporaryDirectory() as temp:
            parent = Path(temp)
            source = parent / 'source'
            source.mkdir()
            (source / 'lib.rs').write_text('pub fn value() {}')
            expected = source_inventory(source)
            outside = parent / 'outside'
            outside.mkdir()
            (outside / 'lib.rs').write_text('unsealed')
            for destination in (outside, outside / 'lib.rs'):
                link = source / 'extra'
                link.symlink_to(destination, target_is_directory=destination.is_dir())
                with self.assertRaisesRegex(RuntimeError, 'symlink in sealed source tree'):
                    verify_source_inventory(source, expected)
                link.unlink()

    def test_runtime_diagnostics_are_never_semantic_results(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            runner = Run(directory, {'rustc': Path(sys.executable), 'rustdoc': Path(sys.executable)}, time.monotonic()+5)
            for reply in ['RESULT 0 0 0 1 -\n', 'RESULT -1 0 0 1 -\n']:
                code = f'import sys; sys.stdout.write({reply!r})'
                self.assertEqual(runner.execute([sys.executable, '-c', code], directory), reply)
                with self.assertRaisesRegex(RuntimeError, 'runtime diagnostics, never a mutation kill'):
                    runner.execute([sys.executable, '-c', code + ';sys.stderr.write("diagnostic")'], directory)
            # Compiler/harness diagnostics remain separate from protocol results.
            self.assertEqual(runner.run([sys.executable, '-c', 'import sys;sys.stderr.write("compiler diagnostic")'], directory), '')

    def test_raw_result_grammar(self):
        call = [(4, 0, 8, b'abc')]
        valid = b'RESULT 0 3 3 1 616263\n'
        self.assertEqual(parse_result(valid, call), [(0, 3, 3, 1, b'abc')])
        invalid = [valid.replace(b'\n', ending) for ending in
                   [b'\r\n', b'\r', b'\v\n', b'\f\n', b'\x00\n', b'\xc2\x85\n']]
        invalid += [valid + valid, valid[:-1], b'garbage||||\n', b'RESULT 7 0 0 1 -\n',
                    b'RESULT 0 4 0 1 -\n', b'RESULT 0 0 9 1 00\n',
                    b'RESULT 0 0 0 4294967296 -\n', b'RESULT 00 0 0 1 -\n']
        for raw in invalid:
            with self.subTest(raw=raw), self.assertRaises((AssertionError, UnicodeError)):
                parse_result(raw, call)

    def fake(self, directory, name, response, exit_code=0):
        path = directory / name
        path.write_text(f'#!{sys.executable}\nimport sys\nsys.stdin.buffer.readline()\n'
                        f'for request in sys.stdin.buffer:\n'
                        f' sys.stdout.buffer.write({response!r});sys.stdout.buffer.flush()\n'
                        f' break\nsys.stdin.buffer.read()\nsys.exit({exit_code})\n')
        path.chmod(0o700)
        return path

    def test_valid_different_rows_are_semantic_mismatch(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            original = self.fake(directory, 'original', b'RESULT 0 0 0 1 -\n')
            mutant = self.fake(directory, 'mutant', b'RESULT -1 0 0 1 -\n')
            calls = [((4, 0, 8, b''), (0, 0, 0, 1, b''))]
            with self.assertRaises(ParityMismatch):
                compare_script(original, mutant, 32, 'control', 8, calls,
                               time.monotonic() + 5, directory)

    def test_crash_is_not_a_semantic_kill(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            original = self.fake(directory, 'original', b'RESULT 0 0 0 1 -\n')
            mutant = self.fake(directory, 'mutant', b'RESULT -1 0 0 1 -\n', 3)
            calls = [((4, 0, 8, b''), (0, 0, 0, 1, b''))]
            try:
                compare_script(original, mutant, 32, 'control', 8, calls,
                               time.monotonic() + 5, directory)
            except ParityMismatch:
                self.fail('crashing transport was classified as a semantic kill')
            except RuntimeError as error:
                self.assertIn('nonzero exit', str(error))
            else:
                self.fail('crash accepted')

    def test_peer_does_not_normalize_newlines(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            binary = self.fake(directory, 'bad-newline', b'RESULT 0 0 0 1 -\r\n')
            with self.assertRaises(AssertionError):
                with Peer(binary, b'TINFL1 32 8\n', time.monotonic() + 5,
                          directory / 'stderr') as peer:
                    peer.call(b'CALL 4 0 8 -\n', (4, 0, 8, b''))

    def test_timeout_is_not_a_semantic_kill(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            binary = directory / 'silent'
            binary.write_text(f'#!{sys.executable}\nimport sys\nsys.stdin.buffer.read()\n')
            binary.chmod(0o700)
            with self.assertRaisesRegex(RuntimeError, 'deadline'):
                with Peer(binary, b'TINFL1 32 8\n', time.monotonic() + .1,
                          directory / 'stderr') as peer:
                    peer.call(b'CALL 4 0 8 -\n', (4, 0, 8, b''))

    def test_outer_stop_reaps_command_and_stops_transport(self):
        # A transport must share its suite's process group. A separate session
        # would escape the root runner's termination of that suite and compiler.
        for stop in (signal.SIGINT, signal.SIGTERM):
            with self.subTest(stop=stop), tempfile.TemporaryDirectory(dir=os.environ.get('NEWT_INFLATE_FIXTURE_DIR')) as temp:
                directory = Path(temp)
                ready = directory / 'ready'
                fake = directory / 'waiting'
                fake.write_text(f'#!{sys.executable}\nimport os,sys\nfrom pathlib import Path\n'
                                f'p=Path({str(ready)!r});q=Path(str(p)+".partial");q.write_text(str(os.getpid()));q.replace(p)\n'
                                'sys.stdin.buffer.read()\n')
                fake.chmod(0o700)
                suite = directory / 'suite.py'
                suite.write_text(f'import sys,time\nfrom pathlib import Path\nsys.path.insert(0,{str(HERE)!r})\n'
                                 'from stream_compare import Peer\n'
                                 f'with Peer({str(fake)!r}, b"header\\n", time.monotonic()+15, '
                                 f'Path({str(directory / "stderr")!r})):\n time.sleep(15)\n')
                outer = directory / 'outer.py'
                outer.write_text(f'import sys,time\nfrom pathlib import Path\nsys.path.insert(0,{str(HERE)!r})\n'
                                 'import run_support as s\ns.install_signal_handlers()\n'
                                 f'out=Path({str(directory / "logs")!r});out.mkdir()\n'
                                 'run=s.Run(out,{"rustc":Path(sys.executable),"rustdoc":Path(sys.executable)},time.monotonic()+15)\n'
                                 f'run.run([sys.executable,{str(suite)!r}],Path({str(directory)!r}))\n')
                transport_pid = None
                with (directory / 'outer.stderr').open('wb') as errors:
                    def cleanup_fixture():
                        if transport_pid is not None:
                            try:
                                os.kill(transport_pid, signal.SIGKILL)
                            except ProcessLookupError:
                                pass
                    with owned_process([sys.executable, '-B', outer], stdout=subprocess.DEVNULL, stderr=errors, start_new_session=True, cleanup=cleanup_fixture) as process:
                        deadline = time.monotonic() + 5
                        while not ready.exists() and time.monotonic() < deadline:
                            if process.poll() is not None:
                                self.fail('outer setup failed')
                            time.sleep(0.01)
                        self.assertTrue(ready.exists(), 'transport setup timed out')
                        transport_pid = int(ready.read_text())
                        hold = os.environ.get('NEWT_INFLATE_STOP_FIXTURE')
                        if hold:
                            marker = Path(hold)

                            def remove_cleanup_handlers(_signal, _frame):
                                for number in (signal.SIGINT, signal.SIGTERM):
                                    signal.signal(number, signal.SIG_DFL)
                                Path(str(marker) + '.disabled').touch()
                            signal.signal(signal.SIGUSR1, remove_cleanup_handlers)
                            partial = Path(str(marker) + '.partial')
                            partial.write_text(json.dumps({'harness': os.getpid(), 'helper': process.pid, 'suite': os.getpgid(transport_pid), 'peer': transport_pid}))
                            partial.replace(marker)
                            while True:
                                signal.pause()
                        process.send_signal(stop)
                        self.assertNotEqual(process.wait(timeout=5), 0)
                        self.assertFalse(process_running(transport_pid), 'transport escaped the terminated suite process group')



    def test_enclosing_stop_cleans_nested_helper_groups(self):
        # Disable the nested harness handler only AFTER publishing every PID.
        # This reproduces the escape while retaining unconditional test cleanup.
        for stop in (signal.SIGINT, signal.SIGTERM):
            for broken in (False, True):
                with self.subTest(stop=stop, removed_handler=broken), tempfile.TemporaryDirectory() as temp:
                    directory = Path(temp)
                    marker = directory / 'pids.json'
                    enclosing = directory / 'enclosing.py'
                    enclosing.write_text(f'import sys,time\nfrom pathlib import Path\nsys.path.insert(0,{str(HERE)!r})\n'
                        'import run_support as s\ns.install_signal_handlers()\n'
                        f'out=Path({str(directory / "logs")!r});out.mkdir()\n'
                        'run=s.Run(out,{"rustc":Path(sys.executable),"rustdoc":Path(sys.executable)},time.monotonic()+15)\n'
                        f'run.env["NEWT_INFLATE_STOP_FIXTURE"]={str(marker)!r}\n'
                        f'run.env["NEWT_INFLATE_FIXTURE_DIR"]={str(directory)!r}\n'
                        f'run.run([sys.executable,"-B",{str(HERE / "harness_tests.py")!r},'
                        f'"HarnessTests.test_outer_stop_reaps_command_and_stops_transport"],Path({str(directory)!r}))\n')
                    pids = None
                    with (directory / 'enclosing.stderr').open('wb') as errors:
                        def cleanup_fixture():
                            if pids is not None:
                                for name in ('helper', 'suite'):
                                    try:
                                        os.killpg(pids[name], signal.SIGKILL)
                                    except ProcessLookupError:
                                        pass
                                try:
                                    os.kill(pids['peer'], signal.SIGKILL)
                                except ProcessLookupError:
                                    pass
                        with owned_process([sys.executable, '-B', enclosing], stdout=subprocess.DEVNULL, stderr=errors, start_new_session=True, cleanup=cleanup_fixture) as process:
                            deadline = time.monotonic() + 5
                            while not marker.exists() and time.monotonic() < deadline:
                                if process.poll() is not None:
                                    self.fail('nested cancellation fixture setup failed')
                                time.sleep(0.01)
                            self.assertTrue(marker.exists(), 'nested fixture readiness timeout')
                            pids = json.loads(marker.read_text())
                            if broken:
                                os.kill(pids['harness'], signal.SIGUSR1)
                                disabled = Path(str(marker) + '.disabled')
                                while not disabled.exists() and time.monotonic() < deadline:
                                    time.sleep(0.01)
                                self.assertTrue(disabled.exists(), 'negative handler removal failed')
                            process.send_signal(stop)
                            self.assertNotEqual(process.wait(timeout=5), 0)
                            live = []
                            for name in ('helper', 'suite', 'peer'):
                                if process_running(pids[name]):
                                    live.append(name)
                            if broken:
                                self.assertTrue(live, 'removed-handler negative control was not detected')
                            else:
                                self.assertEqual(live, [], 'nested descendants escaped enclosing cancellation')



if __name__ == '__main__':
    for signum in (signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, stop_for_signal)
    unittest.main()
