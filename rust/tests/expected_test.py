"""Frozen outputs bind all ordered requests, without a regeneration fallback."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import zlib

SPEC = importlib.util.spec_from_file_location('expected_outputs', Path(__file__).parents[1] / 'expected.py')
EXPECTED = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EXPECTED)


class ExpectedOutputs(unittest.TestCase):
    def test_plain_and_compressed_outputs(self):
        requests = ['one', {'two': [0, 1]}]
        document = dict(schema=1, input_sha256=EXPECTED.input_digest(requests), outputs=['A', ['B', 2]])
        with tempfile.TemporaryDirectory() as directory:
            for suffix in ('.json', '.json.zlib'):
                path = Path(directory) / ('expected' + suffix)
                raw = json.dumps(document).encode()
                path.write_bytes(zlib.compress(raw) if suffix.endswith('zlib') else raw)
                self.assertEqual(EXPECTED.load(path, requests), ['A', ['B', 2]])
                for changed in (requests[::-1], requests[:1], ['different', requests[1]]):
                    with self.assertRaises(ValueError):
                        EXPECTED.load(path, changed)
                self.assertEqual(path.read_bytes(), zlib.compress(raw) if suffix.endswith('zlib') else raw)

    def test_incomplete_outputs_and_missing_fixture_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'expected.json'
            with self.assertRaises(FileNotFoundError):
                EXPECTED.load(path, ['one'])
            path.write_text(json.dumps(dict(schema=1, input_sha256=EXPECTED.input_digest(['one']), outputs=[])))
            with self.assertRaises(ValueError):
                EXPECTED.load(path, ['one'])

            path.write_text(json.dumps(dict(schema=True, input_sha256=EXPECTED.input_digest(['one']), outputs=['A'])))
            with self.assertRaises(ValueError):
                EXPECTED.load(path, ['one'])

    def test_literal_answers_match_every_captured_digest(self):
        import hashlib
        requests = [dict(input='font'), dict(input='zip')]
        answers = ['advance 12\n', 'entry Ω\n']
        bound = [dict(request, expected_sha256=EXPECTED.input_digest([answer]))
                 for request, answer in zip(requests, answers)]
        document = dict(schema=1, input_sha256=EXPECTED.input_digest(bound),
                        outputs=[hashlib.sha256(answer.encode()).hexdigest() for answer in answers])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'expected.json'
            path.write_text(json.dumps(document))
            self.assertEqual(EXPECTED.load_literals(path, requests, answers), answers)
            for index in range(len(answers)):
                changed = dict(document, outputs=list(document['outputs']))
                changed['outputs'][index] = '00' * 32
                path.write_text(json.dumps(changed))
                with self.assertRaisesRegex(ValueError, 'differs from captured native output'):
                    EXPECTED.load_literals(path, requests, answers)


    def test_cache_owner_consumes_complete_order_before_controls(self):
        spec = importlib.util.spec_from_file_location('recorded_cache', Path(__file__).parent / 'cache_owner/recorded.py')
        recorded = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(recorded)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            books = [root / name for name in ('one.epub', 'two.epub')]
            for i, book in enumerate(books):
                book.write_bytes(bytes([i]))
            calls = [[book, 'normal', 0, 1] for book in books]
            requests = [recorded.request(call) for call in calls]
            for name in ('base', 'show'):
                path = root / (name + '.json.zlib')
                path.write_bytes(zlib.compress(json.dumps(dict(schema=1,
                    input_sha256=EXPECTED.input_digest(requests), requests=requests,
                    outputs=['one', 'two'])).encode()))
                corpus = recorded.Corpus(path)
                with self.assertRaises(ValueError):
                    corpus.witness(calls[0])
                for indices in ((1, 0), (0, 0), (0,)):
                    changed = recorded.Corpus(path)
                    with self.assertRaises(ValueError):
                        for index in indices:
                            recorded.observe([changed, *calls[index]])
                        changed.finish()
                self.assertEqual([recorded.observe([corpus, *call]) for call in calls], ['one', 'two'])
                corpus.finish()
                self.assertEqual(recorded.witness([corpus, *calls[0]]), 'one')
                with self.assertRaises(ValueError):
                    recorded.observe([corpus, *calls[0]])

    def test_native_sha_header_authentication(self):
        import shutil
        root = Path(__file__).resolve().parents[2]
        spec = importlib.util.spec_from_file_location('native_crypto', Path(__file__).parent / 'native_crypto.py')
        native = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(native)
        native.verify(root)
        with tempfile.TemporaryDirectory() as directory:
            copy = Path(directory)
            header = copy / 'rust/tests/include/newt/host_sha256.h'
            header.parent.mkdir(parents=True)
            shutil.copyfile(root / 'rust/tests/include/newt/host_sha256.h', header)
            native.verify(copy)
            header.write_bytes(header.read_bytes() + b'changed')
            with self.assertRaises(ValueError):
                native.verify(copy)
            # Both standalone and package recipes use this exact native builder.
            import unittest.mock
            helper = copy / 'rust/tests/native_crypto.py'
            helper.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(Path(__file__).parent / 'native_crypto.py', helper)
            spec = importlib.util.spec_from_file_location('provider_check', Path(__file__).parent / 'reader_providers/check.py')
            provider = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(provider)
            provider.ROOT = copy
            run = unittest.mock.Mock()
            with self.assertRaisesRegex(ValueError, 'Native SHA'):
                provider.native(run, unittest.mock.Mock(), {}, [])
            run.run.assert_not_called()
            header.unlink()
            with self.assertRaises(FileNotFoundError):
                native.verify(copy)

if __name__ == '__main__':
    unittest.main()
