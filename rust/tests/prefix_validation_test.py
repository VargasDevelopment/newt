"""Exercise transport rejection separately from pixel semantics."""
import unittest
from frame_cases import FRAME_BYTES
from prefix_validation import validate, batch_lines
class PrefixGrammar(unittest.TestCase):
    def test_raw_bytes_keep_newline_errors_visible(self):
        self.assertEqual(batch_lines(b'1 ab\n', 1), ['1 ab'])
        for raw in (b'1 ab\r\n', b'1 ab', b'1 ab\nextra\n', b'1 \xff\n'):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                batch_lines(raw, 1)

    def test_arbitrary_pixels_are_protocol_valid(self):
        for flag in ('0','1'):
            validate(flag+' '+'ac'*FRAME_BYTES)
    def test_bad_result_extent_and_hex_fail_setup(self):
        values=['garbage||||','2 '+'00'*FRAME_BYTES,'1 00','1 '+'GG'*FRAME_BYTES,'1 '+'AA'*FRAME_BYTES,'1  '+'00'*FRAME_BYTES]
        for value in values:
            with self.subTest(prefix=value[:12]):
                with self.assertRaises(ValueError):validate(value)
if __name__=='__main__':unittest.main()
