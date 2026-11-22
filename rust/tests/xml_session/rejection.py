"""Malformed request controls, always inside the enclosing Run-owned group."""
from pathlib import Path
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'inflate'))
from run_support import inherited_child, install_signal_handlers
from vectors import request




def observed_child(command, *, input, requests, **kwargs):
    import importlib.util
    from pathlib import Path
    here = Path(__file__).resolve()
    rust = next(parent for parent in here.parents if (parent / 'expected.py').is_file())
    spec = importlib.util.spec_from_file_location('malformed_expected_transport', rust / 'tests/inflate/expected_transport.py')
    frozen = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(frozen)
    return frozen.rejection_child(lambda argv, **options: inherited_child(argv, **options), command, input=input, requests=requests,
                                  fixture=here.parent / 'rejections.json', **kwargs)

def main():
    install_signal_handlers()
    valid = request(b'<r/>')
    invalid = [valid[:-1], valid.replace('\n', '\r\n'), '\n', valid+'\n',
               valid.replace('X ', 'X  '), valid.replace('3c', '3C', 1),
               valid.replace('f,f,f,c,f', 'f,'), valid.replace('X ', 'Q ', 1),
               valid.replace(' 2048 ', ' 0 '), valid.replace(' -1 ', ' -2 '),
               valid.replace(' 0 0 ', ' 2 0 '), valid.replace(' 0 0 ', ' 0 3 ')]
    for executable in sys.argv[1:]:
        for wire in invalid:
            result = observed_child([executable], requests=invalid, input=wire.encode())
            assert result.returncode == 2 and not result.stderr, (wire, result.returncode, result.stderr)


if __name__ == '__main__':
    main()
