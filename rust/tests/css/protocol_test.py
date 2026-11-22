import json
from pathlib import Path
import unittest
from protocol import commands, validate, require_cases
import corpus


class ProtocolTests(unittest.TestCase):
    def test_valid_and_well_formed_different_fields(self):
        request = 'B 512\nI _\nF\n'
        response = b'BEGIN 1\nDECL 0 0 -1 -1 0 -1 -1 -1 -1 -1 -1 -1\nSHEET 0 2166136261 0\n'
        first = validate(request, response)
        changed = validate(request, response.replace(b'DECL 0 0', b'DECL 100 0'))
        self.assertNotEqual(first, changed)

    def test_complete_raw_grammar_rejects_missing_or_extra_fields(self):
        request = 'I _\n'
        valid = b'DECL 0 0 -1 -1 0 -1 -1 -1 -1 -1 -1 -1\n'
        variants = [valid[:-1], valid + valid, valid.replace(b'\n', b'\r\n'),
                    valid.replace(b'\n', b'\v\n'), valid.replace(b'DECL', b'OTHER'),
                    b'DECL 0 0 -1 -1 0 -1 -1 -1 -1 -1 -1\n',
                    valid.replace(b'DECL 0 0', b'DECL 0 2')]
        for raw in variants:
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                validate(request, raw)

    def test_sheet_source_trace_must_match_the_declared_fault(self):
        request = 'S 6162 0 2 2 4294967295 1 0 -7 0 3\n'
        good = b'STATUS 1\nSCRATCH 1\nREAD 0 2 -7\n'
        validate(request, good)
        with self.assertRaises(ValueError):
            validate(request, good.replace(b'2 -7', b'2 1'))

    def test_exact_frozen_case_set_and_replacement_control(self):
        expected = set(json.loads((Path(__file__).parent / 'case_labels.json').read_text()))
        cases = corpus.corpus()
        self.assertEqual({c['name'] for c in cases}, expected)
        self.assertEqual(len(cases), len(expected))
        changed = [dict(c) for c in cases]
        changed[0]['name'] = 'replacement-empty'
        require_cases(cases, sorted(expected))
        with self.assertRaises(ValueError):
            require_cases(changed, sorted(expected))
        for case in cases:
            commands(case['input'])


if __name__ == '__main__':
    unittest.main()
