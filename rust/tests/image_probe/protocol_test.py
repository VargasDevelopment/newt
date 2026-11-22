import json
from pathlib import Path
import unittest
from protocol import validate, require_cases
from corpus import corpus

class ProtocolTests(unittest.TestCase):
    def test_complete(self):
        request='P _ 0 0 0 4294967295 1 -1 -1 0 1024\n'
        raw=b'OP 0\nSTATUS 0\nINFO 0 0 0 0\nSCRATCH 1\nEND 0\n'
        validate(request,raw)
        for bad in [raw[:-1],raw+raw,raw.replace(b'\n',b'\r\n'),raw.replace(b'END 0',b'END 1'),raw.replace(b'INFO 0',b'INFO 3'),raw.replace(b'OP 0',b'OP 1'),raw.replace(b'SCRATCH 1\n',b''),raw.replace(b'INFO 0 0',b'INFO 00 0')]:
            with self.assertRaises(ValueError): validate(request,bad)
        self.assertNotEqual(validate(request,raw),validate(request,raw.replace(b'INFO 0 0 0 0',b'INFO 2 1 2 1')))
    def test_fault_trace(self):
        request='P 6162 0 2 2 4294967295 1 0 -7 0 1024\n'
        raw=b'OP 0\nSTATUS 0\nINFO 0 0 0 0\nSCRATCH 1\nREAD 0 2 -7\nEND 1\n'
        validate(request,raw)
        with self.assertRaises(ValueError): validate(request,raw.replace(b'2 -7',b'2 1'))
    def test_frozen(self):
        frozen=json.loads((Path(__file__).parent/'case_labels.json').read_text());cases=corpus();require_cases(cases,frozen)
        for key,value in [('name','replacement'),('input',cases[0]['input']+'\n'),('expected_lines',[])]:
            changed=[dict(c) for c in cases];changed[0][key]=value
            with self.assertRaises(ValueError): require_cases(changed,frozen)

if __name__=='__main__': unittest.main()
