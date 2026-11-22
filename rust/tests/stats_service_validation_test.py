import unittest
from stats_service_validation import ProtocolError, validate, validate_wire
RUN='SS1 run fold '+'11'*32+' 0 0 0 9800 12 0 65535 0'
GOOD='R fold done 0 0 0 1\nV ns slot0 00\nD ns slot0 -\nT begin:ns:0|put:slot0:00|end\n'
COMMAND='stats_service '+(RUN+'\nSS1 dump\n').encode().hex()
class Grammar(unittest.TestCase):
    def test_corrupt_record_bytes_are_valid_transport(self):
        self.assertEqual(validate_wire(GOOD.encode().hex(),COMMAND),GOOD)
    def test_no_newline_laundering_inside_hex_wrapper(self):
        for text in [GOOD.replace('\n','\r\n'),GOOD.replace('\n','\r'),GOOD[:-1],*[GOOD.replace('\n',separator,1) for separator in ('\v','\f','\x85','\u2028','\u2029')]]:
            with self.assertRaises(ProtocolError):validate_wire(text.encode().hex(),COMMAND)
    def test_invalid_cannot_be_behavioral_kill(self):
        for text in [GOOD.replace(' 0 0 0 1\n',' 0 0 0\n'),GOOD+'T \n',GOOD.replace('T begin:ns:0|put:slot0:00|end','T garbage'),GOOD.replace('slot0 00','slot0 z0'),GOOD.replace('R fold','R seed')]:
            with self.assertRaises(ProtocolError):validate_wire(text.encode().hex(),COMMAND)
        for reply in ['00x','',GOOD.encode().hex().upper()]:
            with self.assertRaises((ProtocolError,UnicodeError)):validate_wire(reply,COMMAND)
    def test_all_summary_sleep_fields_and_callback_receipt(self):
        summary=RUN.replace('run fold','run summary');sleep=RUN.replace('run fold','run sleep')
        validate('S 0 0 0 0 0 0 0 0 0 0\nR fold done 0 0 0 0\n',[summary])
        with self.assertRaises(ProtocolError):validate('S 0 0 0 0 0 0 0\nR fold done 0 0 0 0\n',[summary])
        validate('E 1 4 1 255 65535\nU 0 - -\nR fold done 0 0 0 0\n',[sleep])
        for value in ['U 0 4 0','U 1 - 0','U 1 4 -']:
            with self.assertRaises(ProtocolError):validate('E 1 4 1 0 0\n'+value+'\nR fold done 0 0 0 0\n',[sleep])
if __name__=='__main__':unittest.main()
