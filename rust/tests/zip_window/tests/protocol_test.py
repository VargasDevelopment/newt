"""Future runner tests; these fixtures are intentionally independent of product cases."""
import unittest
import sys
from pathlib import Path
import tempfile
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from check import read_output
from protocol import ProtocolError, validate

INPUT = '10 10 0 0 3 0 10 0 0 0 0 0 0 0 10\nR 0 2 1\nR 10 0 0\n'
GOOD = 'S 10\nA 0 0 0\nR 0 2 2\nO 1 abcd\nO 1 \nD 0 0 0 2\n'


class Grammar(unittest.TestCase):
    def test_runner_preserves_invalid_newlines(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'reply'
            for ending in ('\r\n','\r'):
                data=GOOD.replace('\n',ending)
                path.write_bytes(data.encode())
                self.assertEqual(read_output(path),data)
                with self.assertRaises(ProtocolError):validate(INPUT,read_output(path))

    def test_valid_semantic_changes_are_not_grammar_failures(self):
        for data in [GOOD, GOOD.replace('R 0 2 2', 'S 11\nP 0 -1\nY\nR 0 2 3'),
                     GOOD.replace('O 1 abcd', 'O 0 a5a5'),
                     GOOD.replace('A 0 0 0', 'A 4096 4096 0')]:
            validate(INPUT, data)

    def test_malformed_cannot_be_a_kill(self):
        bad = {
            'missing LF': GOOD[:-1], 'CRLF': GOOD.replace('\n', '\r\n'),
            'blank': GOOD + '\n', 'unknown': GOOD.replace('R 0 2 2', 'Q 0 2 2'),
            'extra field': GOOD.replace('A 0 0 0', 'A 0 0 0 0'),
            'short field': GOOD.replace('D 0 0 0 2', 'D 0 0 0'),
            'duplicate A': GOOD.replace('A 0 0 0', 'A 0 0 0\nA 0 0 0'),
            'missing A': GOOD.replace('A 0 0 0\n', ''),
            'missing D': GOOD.replace('D 0 0 0 2\n', ''),
            'after D': GOOD + 'Y\n', 'before A': GOOD.replace('S 10', 'Y\nS 10'),
            'early D': GOOD.replace('O 1 abcd', 'D 0 0 0 0\nO 1 abcd'),
            'extra output': GOOD.replace('O 1 abcd', 'O 1 abcd\nO 1 '),
            'missing output': GOOD.replace('O 1 \n', ''),
            'wrong command length': GOOD.replace('O 1 abcd', 'O 1 ab'),
            'nonempty zero output': GOOD.replace('O 1 \n', 'O 1 00\n'),
            'odd hex': GOOD.replace('abcd', 'abc'), 'uppercase': GOOD.replace('abcd', 'ABCD'),
            'nonhex': GOOD.replace('abcd', 'abxz'), 'flag': GOOD.replace('O 1 abcd', 'O 2 abcd'),
            'negative unsigned': GOOD.replace('S 10', 'S -1'),
            'unsigned overflow': GOOD.replace('S 10', 'S 18446744073709551616'),
            'signed overflow': GOOD.replace('R 0 2 2', 'R 0 2 2147483648'),
            'request overflow': GOOD.replace('R 0 2 2', 'R 0 4294967296 2'),
            'negative overflow': GOOD.replace('R 0 2 2', 'P 0 -2147483649'),
            'noncanonical': GOOD.replace('S 10', 'S 010'),
            'double spaces': GOOD.replace('S 10', 'S  10'),
            'zero physical read': GOOD.replace('R 0 2 2', 'R 0 0 0'),
            'event after final output': GOOD.replace('D 0 0 0 2', 'Y\nD 0 0 0 2'),
        }
        for name, data in bad.items():
            with self.subTest(name=name), self.assertRaises(ProtocolError):
                validate(INPUT, data)


if __name__ == '__main__':
    unittest.main()
