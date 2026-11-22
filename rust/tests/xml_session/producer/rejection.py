"""Strict malformed request controls within the enclosing Run-owned group."""
from pathlib import Path
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'inflate'))
from run_support import inherited_child, install_signal_handlers



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

if __name__ == '__main__':
    install_signal_handlers()
    valid = 'Y 3c722f3e 2048 -1 0 0 1 -1 0 0 4 f\n'
    invalid = [valid[:-1], valid.replace('\n', '\r\n'), valid + '\n',
               valid.replace('3c', '3C'), valid.replace(' f\n', ' f,\n'),
               valid.replace(' 2048 ', ' 0 '), valid.replace(' -1 ', ' -2 ', 1),
               valid.replace(' f\n', ' f extra\n')]
    for executable in sys.argv[1:]:
        for wire in invalid:
            result = observed_child([executable], requests=invalid, input=wire.encode())
            assert result.returncode == 2 and not result.stderr
