import copy
import json
from pathlib import Path
import unittest
from corpus import cases
from protocol import validate, require_cases


class ProtocolTests(unittest.TestCase):
    def test_complete_grammar(self):
        commands = ['I null', 'R', 'B 2 616263646566']
        good = b'I 0 0\nR 0\nB 2 2 3\n'
        validate(commands, good)
        self.assertNotEqual(validate(commands, good), validate(commands, good.replace(b'2 3\n', b'2 4\n')))
        for bad in [good[:-1], good + b'R 0\n', good.replace(b'\n', b'\r\n'),
                    good.replace(b'R 0', b'R 2'), good.replace(b'B 2 2 3', b'B 2 2'),
                    good.replace(b'B 2 2 3', b'B 3 2 3 4'), good.replace(b'B 2 2 3', b'B 2 2 256'),
                    good.replace(b'R 0', b'R 00'), good.replace(b'R 0', b'R  0'),
                    good.replace(b'I 0 0', b'OTHER 0 0')]:
            with self.subTest(bad=bad), self.assertRaises(ValueError): validate(commands, bad)

    def test_frozen_corpus(self):
        original = cases()
        labels = json.loads((Path(__file__).parent/'case_labels.json').read_text())
        require_cases(original, labels)
        for changed in [original[:-1], original+[original[0]], []]:
            with self.assertRaises(ValueError): require_cases(changed, labels)
        for attribute, value in [('commands', ['R']), ('expected', []), ('native', False), ('name', 'replacement')]:
            changed = copy.deepcopy(original)
            setattr(changed[0], attribute, value)
            with self.assertRaises(ValueError): require_cases(changed, labels)


if __name__ == '__main__': unittest.main()
