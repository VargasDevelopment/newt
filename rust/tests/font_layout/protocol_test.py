"""Focused harness controls, independent of compilers and oracle execution."""
import json
from pathlib import Path
import unittest
import corpus
from protocol import commands, require_cases, validate

HERE = Path(__file__).resolve().parent


class ProtocolTests(unittest.TestCase):
    def test_command_bounds(self):
        for value in (-2147483648, -1, 0, 2147483647):
            self.assertEqual(validate('P\n-\n1 2\n', f'R {value}\n'.encode()), value)
        for value in (0, 65535, 4294967295):
            self.assertEqual(validate('S\n-\n1 2\n', f'R {value}\n'.encode()), value)
        for mode, values in [('P', [-2147483649, 2147483648]), ('S', [-1, 4294967296])]:
            for value in values:
                with self.subTest(mode=mode, value=value), self.assertRaises(ValueError):
                    validate(f'{mode}\n-\n1 2\n', f'R {value}\n'.encode())

    def test_raw_response_grammar(self):
        for raw in (b'', b'R 0', b'R 0\r\n', b'R 0\nR 0\n', b'R 0\n\n',
                    b'R 00\n', b'R -0\n', b'R +1\n', b'R  1\n', b'R 1 \n',
                    b'R\t1\n', b'R 1\v\n', b'R 1\x00\n', b'X 1\n', b'R \xff\n'):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                validate('P\n-\n1 2\n', raw)

    def test_request_grammar(self):
        from rejection import BAD_REQUESTS
        for raw in BAD_REQUESTS:
            with self.subTest(raw=raw[:80]), self.assertRaises(ValueError):
                commands(raw.decode('ascii'))
        commands('P\n00ff\n0 4294967295\n')
        commands('S\n-\n1\n')
        commands('S\n-\n' + ' '.join(['1'] * 16) + '\n')

    def test_manifest_and_all_expected_replies(self):
        cases = corpus.corpus()
        manifest = json.loads((HERE / 'case_labels.json').read_text())
        require_cases(cases, manifest)
        for changed in ([], cases[1:], cases + [cases[0]]):
            with self.assertRaises(ValueError):
                require_cases(changed, manifest)
        with self.assertRaises(ValueError):
            require_cases(cases, [])
        for field, replacement in [('name', 'replacement'), ('input', 'P\n-\n1 2\n'),
                                   ('expected', 'R 1\n')]:
            changed = [dict(case) for case in cases]
            changed[0][field] = replacement
            with self.subTest(field=field), self.assertRaises(ValueError):
                require_cases(changed, manifest)
        for erased in ([dict(case, expected='R 0\n') for case in cases],
                       [dict(case, input='P\n-\n1 2\n', expected='R 0\n') for case in cases]):
            with self.assertRaises(ValueError):
                require_cases(erased, manifest)


if __name__ == '__main__':
    unittest.main()
