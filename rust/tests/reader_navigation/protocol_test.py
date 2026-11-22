import unittest
import protocol
from corpus import base,BASE_EXPECTED
class ProtocolTest(unittest.TestCase):
    def test_positive_full_trace(self):
        protocol.response(BASE_EXPECTED,base())
    def test_output_state_and_numeric_corruption(self):
        edits=[
            ('B 1 4','B 01 4'),('P 10 30 100 1 3 1','P 10 30 0 1 3 1'),
            ('P 10 30 100 1 3 1','P 10 30 100 2 3 1'),('G 0 3','G 0 13'),
            ('M 1 4 3 0 4 1 1 1','M 1 4 4 0 4 1 1 1'),
            ('R 0 0 65535 1 0 0 1','R 1 0 65535 1 0 0 1'),
            ('R 0 0 65535 1 0 0 1','R 0 0 65535 1 0 0 0'),
            ('T 1 1 0 1 0 0 1 1','T 1 1 0 1 0 0 0 1'),
            ('50617274','5061727A'),('U 1 0\n',''),('END\n','END\nEND\n'),
        ]
        for old,new in edits:
            changed=BASE_EXPECTED.replace(old,new)
            self.assertNotEqual(changed,BASE_EXPECTED)
            with self.subTest(old=old),self.assertRaises(ValueError):protocol.response(changed,base())
        for changed in [BASE_EXPECTED[:-1],BASE_EXPECTED.replace('\n','\r\n'),BASE_EXPECTED+'\n']:
            with self.assertRaises(ValueError):protocol.response(changed,base())
    def test_request_bounds_and_batch_framing(self):
        good=protocol.wire(base());protocol.request(good)
        bad=[good[:-1],good+'\n',good.replace('\n','\r\n'),good.replace('N1 3','N1 129'),good.replace('N1 3','N1 03'),good.replace('S 66726f6e74','S 00'),good.replace('Z 66726f6e74','Z -')]
        for changed in bad:
            self.assertNotEqual(changed,good)
            with self.assertRaises(ValueError):protocol.request(changed)
if __name__=='__main__':unittest.main()
