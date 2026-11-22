"""Run only within canonical Run.execute's owned group; no standalone gate."""
import importlib.util,sys
sys.dont_write_bytecode=True
spec=importlib.util.spec_from_file_location('ft_reject_support',sys.argv[1]);s=importlib.util.module_from_spec(spec);spec.loader.exec_module(s)
inherited_child=s.inherited_child
valid=b'FTFONT1\n00\nSTATUS\n'
invalid=[b'',valid[:-1],valid+b'\n',valid.replace(b'\n',b'\r\n'),b' '+valid,valid.replace(b'STATUS',b'UNKNOWN'),valid.replace(b'STATUS',b'LINE 01'),valid.replace(b'STATUS',b'LINE +1'),valid.replace(b'STATUS',b'LINE -0'),valid.replace(b'STATUS',b'LINE 4294967296'),valid.replace(b'STATUS',b'INIT 2 10 400 0 0'),valid.replace(b'00',b'0'),valid.replace(b'00',b'ffzz'),valid.replace(b'00',b'FF'),valid.replace(b'STATUS',b'OPTIONS 5 40 0 0 0 0'),valid.replace(b'STATUS',b'LINE  1'),valid.replace(b'STATUS',b'INSPECT 0 4097'),valid.replace(b'STATUS',b'STATUS\nBOGUS'),valid+b'\xff']


def observed_child(command, *, input, requests, **kwargs):
    import importlib.util
    from pathlib import Path
    here = Path(__file__).resolve()
    rust = next(parent for parent in here.parents if (parent / 'expected.py').is_file())
    spec = importlib.util.spec_from_file_location('malformed_expected_transport', rust / 'tests/inflate/expected_transport.py')
    frozen = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(frozen)
    return frozen.rejection_child(lambda argv, **options: inherited_child(argv, **options), command, input=input, requests=requests,
                                  fixture=here.parent / ('rejections-' + __import__('os').environ['NEWT_FT_REFERENCE_MODE'] + '.json'), **kwargs)

for binary in sys.argv[2:]:
 for raw in invalid:
  r=observed_child([binary], requests=invalid,input=raw,timeout=5)
  s.require(r.returncode==2 and r.stdout==b'' and r.stderr==b'', 'invalid request did not reject cleanly; crash/diagnostic is not rejection')
print('REJECTIONS '+str(len(invalid)))
