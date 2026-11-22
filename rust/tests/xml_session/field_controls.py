"""Every XML response path must reject out-of-domain numeric mutation results."""
import importlib.util
from pathlib import Path
from protocol import parse as ordinary
from protocol_fields import validate_numeric_fields

HERE = Path(__file__).resolve().parent


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


producer = load(HERE / 'producer/protocol.py', 'producer_field_controls')
chapter = load(HERE.parent / 'chapter_layout/tests/xml_protocol.py', 'chapter_field_controls')
wire = 'Y 3c722f3e 2048 -1 0 0 1 -1 0 0 4 f\n'
valid = 'O Ok 0 1\nR 0 2048 4\nS 72\nE 72\nF Ok 4 0 1\nEND\n'
for parser in [lambda text: ordinary(text),
               lambda text: producer.parse(text, wire), lambda text: chapter.parse(text, wire)]:
    parser(valid)
    for old, new in [('R 0 2048 4', 'R 0 2048 2049'),
                     ('R 0 2048 4', 'R 18446744073709551616 2048 4'),
                     ('R 0 2048 4', 'R 0 4294967296 4'),
                     ('R 0 2048 4', 'R 0 4294967295 2147483648'),
                     ('R 0 2048 4', 'R 0 2048 -2'),
                     ('F Ok 4 0 1', 'F Ok 18446744073709551616 0 1'),
                     ('O Ok 0 1', 'O Ok 18446744073709551616 1')]:
        try:
            parser(valid.replace(old, new))
        except ValueError:
            pass
        else:
            raise RuntimeError('out-of-domain XML response admitted')
validate_numeric_fields(['R 18446744073709551615 4294967295 2147483647',
                         'F Ok 18446744073709551615 0 1', 'R 0 0 -1'])
producer.malformed_controls()
chapter.malformed_controls()
print('PASS all three XML numeric admission paths and existing malformed fixtures')
