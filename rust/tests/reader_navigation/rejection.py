"""Canonical process-group-owned rejection checks for both N1 transports."""
import importlib.util
from pathlib import Path
import sys
sys.dont_write_bytecode=True
here=Path(__file__).resolve().parent
root=Path(sys.argv[1])
spec=importlib.util.spec_from_file_location('nav_rejection_support',root/'rust/tests/inflate/run_support.py')
support=importlib.util.module_from_spec(spec);spec.loader.exec_module(support)
support.install_signal_handlers()
inherited_child=support.inherited_child
sys.path.insert(0,str(here))
from corpus import base
from protocol import wire
valid=wire(base()).encode()
bad=[b'',valid[:-1],valid+b'\n',valid.replace(b'\n',b'\r\n'),valid.replace(b'N1 3',b'N1 03'),valid.replace(b'N1 3',b'N1 129'),valid.replace(b'S 66726f6e74',b'S 00'),valid.replace(b'Z 66726f6e74',b'Z -'),valid+b'\xff']


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

for binary in sys.argv[2:]:
    for request in bad:
        child=observed_child([binary], requests=bad,input=request,timeout=5)
        support.require(child.returncode==2 and not child.stdout and not child.stderr,'not a clean protocol rejection')
print('N1_REJECTIONS '+str(len(bad)))
