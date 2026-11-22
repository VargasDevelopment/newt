"""Parent Run owns this entire process group, including transport children."""
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'inflate'))
from run_support import inherited_child
from corpus import INVALID




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

def requests():
    # Empty corpus entry denotes an empty command, rather than an empty stream.
    for text in INVALID:
        yield text + '\n'
    yield 'F - 1 -2 -1 0 1\n'
    yield 'F - 1 1048577 -1 0 1\n'
    yield 'F - 1 -1 -2147483649 0 1\n'
    yield 'F - 1 -1 1 0 1\n'
    yield 'D - 1048577\n'
    yield 'P 1 null - 1 extra\n'
    yield 'T\n'
    yield 'L - -\nX\n'


def main():
    count = 0
    for request in requests():
        for binary in sys.argv[1:]:
            result = observed_child([binary], requests=list(requests()), input=request.encode('ascii'))
            if result.returncode != 2:
                raise RuntimeError(f'malformed request returned {result.returncode}, expected exactly 2: {request!r}')
            if any(marker in result.stderr for marker in (b'AddressSanitizer', b'runtime error:', b'UndefinedBehaviorSanitizer')):
                raise RuntimeError('malformed request crashed or emitted sanitizer/panic diagnostic')
            count += 1
    print(f'PASS {count} exact-exit-2 malformed-request checks')


if __name__ == '__main__':
    main()
