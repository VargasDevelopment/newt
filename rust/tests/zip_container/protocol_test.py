import unittest
from protocol import request, semantic_digest, validate


class ProtocolTests(unittest.TestCase):
    line = '616263 2 -1 -1 0 0 0 1 3'
    wire = b'RAW 3\nOPEN 0 0\nOUT 0 1 1 1 61\nSIZE 3\nREAD 0 1 1\nEND\n'

    def test_complete_valid_response_and_semantic_difference(self):
        first = validate(self.line, self.wire, 'cpp')
        changed = validate(self.line, self.wire.replace(b'1 61\n', b'1 62\n'), 'rust')
        self.assertNotEqual(first['semantic'], changed['semantic'])
        self.assertEqual(semantic_digest(first['semantic']), semantic_digest(first['semantic']))
        self.assertNotEqual(semantic_digest(first['semantic']), semantic_digest(changed['semantic']))

    def test_semantic_digest_detects_single_byte_output_flip(self):
        baseline = validate(self.line, self.wire, 'rust')
        expected = semantic_digest(baseline['semantic'])
        # Flipping a single output byte in Rust wire output must fail digest comparison.
        flipped = validate(self.line, self.wire.replace(b' 61\n', b' 60\n'), 'rust')
        self.assertNotEqual(semantic_digest(flipped['semantic']), expected)

    def test_invalid_complete_response_is_rejected(self):
        malformed = [self.wire.replace(b'\n', b'\r\n'), self.wire[:-1], self.wire + b'END\n',
            self.wire.replace(b'OUT 0', b'OUT 1'), self.wire.replace(b'1 61\n', b'1 6g\n'),
            self.wire.replace(b'READ 0 1 1', b'READ 0 1 2'),
            self.wire.replace(b'OPEN 0 0', b'OPEN 0 11'), b'garbage||||\n']
        for raw in malformed:
            with self.subTest(raw=raw), self.assertRaises((ValueError, IndexError)):
                validate(self.line, raw, 'cpp')

    def test_all_required_case_identities_are_enforced(self):
        import corpus
        import expectations
        from types import SimpleNamespace
        original = list(corpus.cases())
        changed = [dict(case) for case in original]
        target = next(i for i, case in enumerate(changed) if case['label'] == 'io-call-0-0')
        changed[target] = {'label': 'replacement-empty', 'input': original[0]['input']}
        with self.assertRaises(RuntimeError):
            expectations.fixtures(SimpleNamespace(cases=lambda: changed))
        self.assertEqual({c['label'] for c in expectations.fixtures(corpus)}, expectations.MANDATORY_CASES)

    def test_request_delimiters_and_widths(self):
        invalid = [self.line.replace('0 1 3', '0 1, 3'), self.line + ' extra',
                   self.line.replace('616263', '61626'), self.line.replace(' 2 ', ' 02 '),
                   self.line.replace('-7', '7') + '\r', self.line.replace(' 3', ' 4294967296')]
        for line in invalid:
            with self.subTest(line=line), self.assertRaises(ValueError):
                request(line)


if __name__ == '__main__':
    unittest.main()
