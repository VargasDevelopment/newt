"""Root executes this through Run.execute; every rejected child is group-owned."""
import importlib.util,sys
from pathlib import Path
sys.dont_write_bytecode=True
root=Path(__file__).resolve().parents[3]
s=importlib.util.spec_from_file_location('support',root/'tests/inflate/run_support.py');support=importlib.util.module_from_spec(s);s.loader.exec_module(support)
support.install_signal_handlers()
inherited_child=support.inherited_child


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

if sys.flags.optimize: raise RuntimeError('Validation requires unoptimized Python')
valid='JP1 D 0 0 0 1 1048576 ffd8\n'
bad=['',valid.rstrip(),valid+'\n',valid.replace(' D ',' X '),valid.replace(' 0 ',' 00 ',1),valid.replace(' 1 ',' 0 '),valid.replace('ffd8','FFD8'),valid.replace('ffd8','f'),valid.replace('ffd8','ff d8'),valid.replace('D 0 0','D 4 0'),valid.replace('D 0 0','D 0 1'),valid.replace('D 0 0','R 0 1'),valid.replace(' 1 ',' +1 '),valid.replace(' 1 ',' 1\t'),valid.replace('ffd8','ff\x00d8')]
bad += [valid.replace('D 0 0',mode+' 0 1') for mode in ('P','A')]
for executable in sys.argv[1:]:
    for request in bad:
        child=observed_child([executable], requests=bad,input=request.encode(),timeout=10)
        assert child.returncode==2 and child.stdout==b'' and child.stderr==b'',(executable,repr(request),child.returncode,child.stdout,child.stderr)
print('canonical silent exit2 rejections admitted')
