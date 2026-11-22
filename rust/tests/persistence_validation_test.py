# SPDX-License-Identifier: MIT
# MIT; see LICENSE.
import unittest
from persistence_protocol_validation import validate
from persistence_cases import cases


class ProtocolValidationTest(unittest.TestCase):
    def test_independent_expectations_have_valid_transport_grammar(self):
        for case in cases():
            with self.subTest(case=case.name):
                validate(case.expected, case.request)

    def test_malformed_response_never_becomes_a_semantic_witness(self):
        case = cases()[0]
        for response in ('garbage||||', '00||||', '5||||', '0|0|||',
                         '0|gg|||', '0||unknown||', '0||read:slot9||',
                         '0||begin:name:2||', '0|||name:0,|',
                         '0|||name:00,;name:00,|', '0|||z:,;a:,|'):
            with self.subTest(response=response), self.assertRaises(AssertionError):
                validate(response, case.request)

    def test_corrupt_bytes_are_valid_protocol_not_prejudged_policy(self):
        validate('2|000000|begin:sb0:1;read:slot0;end|sb0:0000,ff|sb0:0000,ff', cases()[0].request)


if __name__ == '__main__':
    unittest.main()
