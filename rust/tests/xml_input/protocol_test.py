"""Runner controls: grammar, expectations, and exact corpus identity."""
import json
from pathlib import Path
import unittest
import corpus
from protocol import commands, validate, require_cases, independent
from xml_input_records import fixtures


class ProtocolTests(unittest.TestCase):
    def test_every_control_still_selects_exactly_one_current_rust_site(self):
        here = Path(__file__).parent
        rust = here.parents[1]
        controls = json.loads((here/'mutations.json').read_text())
        for control in controls:
            source = rust/control.get('rust_file', 'core/src/xml_input.rs')
            self.assertEqual(source.read_text().count(control['old']), 1, control['name'])
        moved = next(control for control in controls if 'rust_file' in control)
        self.assertNotIn(moved['old'], (rust/'core/src/xml_input.rs').read_text())

    def test_all_command_families_and_semantic_difference(self):
        request = ('F 61 512 -1 -1 0 1,0,r\nD 612f62 3\nP 8 null 61 1\n'
                   'L 783a61 61\nH 61 61\nA local 61\nE local 61\nT a61 t e\n')
        response = (b'F 1 61 1 1\nF 0 - 1 1\nRESET\nEND\nD 1 6100a5\nEND\n'
                    b'P 6100 null 2 2 0\nEND\nL 61 1\nEND\nH 1\nEND\nA null\nEND\nE 0\nEND\n'
                    b'T 1 1 null 6100\nT 1 1 61 6100\nT 1 0 null 6100\nEND\n')
        parsed = validate(request, response)
        self.assertEqual(parsed['payload'], b'a')
        changed = validate(request, response.replace(b'F 1 61', b'F 1 62'))
        self.assertNotEqual(parsed, changed)
        independent({'name': 'fixture', 'payload': b'a', 'lines': parsed['lines']}, parsed)
        with self.assertRaises(ValueError):
            independent({'name': 'fixture', 'payload': b'a'}, changed)

    def test_raw_lf_complete_command_aware_grammar(self):
        request = 'F 61 512 -1 -1 0 2,r\n'
        good = b'F 1 61a5 1 1\nRESET\nEND\n'
        validate(request, good)
        variants = [good[:-1], good + b'END\n', good.replace(b'\n', b'\r\n'),
                    good.replace(b'\n', b'\v\n'), good.replace(b'F 1', b'F 01'),
                    good.replace(b'F 1', b'F 3'), good.replace(b'61a5', b'61'),
                    good.replace(b'61a5', b'61A5'), good.replace(b'RESET\n', b''),
                    good.replace(b' 1 1\n', b' 1\n'), b'END\n' + good,
                    good.replace(b'F 1', b'F  1'), good.replace(b'RESET', b'OTHER')]
        for raw in variants:
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                validate(request, raw)
        for request, raw in [
            ('D - 2\n', b'D 1 -\nEND\n'),
            ('P 2 null 61 1\n', b'P 61 null 1 1 0\nEND\n'),
            ('L - -\n', b'L - 2\nEND\n'),
            ('H - -\n', b'H -1\nEND\n'),
            ('A local -\n', b'A ff00 extra\nEND\n'),
            ('E local -\n', b'E 3\nEND\n'),
            ('T a61\n', b'T 1 1 null 61\nEND\n'),
            ('T a61\n', b'T 256 1 null 00\nEND\n'),
            ('T a61\n', b'T 1 1 null 6100\nT 1 1 null 6100\nEND\n'),
        ]:
            with self.subTest(request=request), self.assertRaises(ValueError):
                validate(request, raw)

    def test_request_bounds(self):
        commands('F - 1048576 1048576 -2147483648 0 0,r\n')
        commands('F - 1 -1 0 0 1\n')
        for request in ['F - 1 -2 -1 0 1\n', 'F - 1 1048577 -1 0 1\n',
                        'F - 1 -1 1 0 1\n', 'P 1 null - 2\n', 'D - -1\n',
                        'A local 61 62\n', 'T a0\n', 'L - -\r\n', '\n']:
            with self.subTest(request=request), self.assertRaises(ValueError):
                commands(request)

    def test_frozen_id_set_and_replacement_controls(self):
        packet = Path(__file__).parent
        controls = json.loads((packet / 'mutations.json').read_text())
        cases = fixtures(corpus, controls)
        labels = json.loads((packet / 'case_labels.json').read_text())
        require_cases(cases, labels)
        self.assertTrue(all(c['witness'] in {case['input'] for case in cases} for c in controls))
        variants = [cases[:-1], cases + [cases[0]], [],
                    [dict(case, name='replacement') if i == 0 else case for i, case in enumerate(cases)],
                    [dict(case, input='L - -') if i == 0 else case for i, case in enumerate(cases)],
                    [dict(case, input='') if i == 0 else case for i, case in enumerate(cases)],
                    [dict(case, payload=b'') if i == 0 else case for i, case in enumerate(cases)]]
        for changed in variants:
            with self.assertRaises(ValueError):
                require_cases(changed, labels)


if __name__ == '__main__':
    unittest.main()
