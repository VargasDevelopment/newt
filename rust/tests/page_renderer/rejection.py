"""Runs only inside root's enclosing owned Run; no separate process groups."""
import importlib.util
from pathlib import Path
import sys
sys.dont_write_bytecode=True
spec=importlib.util.spec_from_file_location('support',sys.argv[1]);support=importlib.util.module_from_spec(spec);spec.loader.exec_module(support)
inherited_child=support.inherited_child
# Tiny valid blank request; rejection fixtures mutate only transport descriptors.
valid=b'PR1 0 8 4 2 1 0 255 128 0 0 0 0 0\n'
invalid=[b'',valid[:-1],valid+b'\n',valid.replace(b'\n',b'\r\n'),b' '+valid,valid.replace(b'PR1 ',b'PR1  '),valid+b'x',valid.replace(b' 8 ',b' 08 ',1),valid.replace(b' 8 ',b' +8 ',1),valid.replace(b' 8 ',b' -0 ',1),valid.replace(b' 8 ',b' 65 ',1),valid.replace(b' 2 ',b' 0 ',1),valid.replace(b'PR1',b'PR2'),valid.replace(b'PR1',b'PR\xff'),b'x'*65537+b'\n',valid.replace(b' 0\n',b' 1\n')]


def rejection_child(command, *, input, requests, **kwargs):
    # The whole ordered malformed corpus is authenticated before selecting a reply.
    import hashlib
    import importlib.util
    from types import SimpleNamespace
    path = Path(__file__).resolve().parent / 'rejections.json'
    if hashlib.sha256(path.read_bytes()).hexdigest() != '510a09b855e4515fd9c7dbdc7ccae6debe019dae4b47f808a60da71655f0241b':
        raise RuntimeError('frozen malformed-request observations changed')
    spec = importlib.util.spec_from_file_location('rejection_expected', Path(__file__).resolve().parents[2] / 'expected.py')
    expected = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(expected)
    wires = [raw.encode() if isinstance(raw, str) else raw for raw in requests]
    outputs = expected.load(path, [dict(input_hex=raw.hex()) for raw in wires])
    observation = outputs[wires.index(input)]
    if Path(command[0]).name in ('expected.json', 'expected.json.zlib'):
        return SimpleNamespace(returncode=observation['returncode'],
                               stdout=bytes.fromhex(observation['stdout']),
                               stderr=bytes.fromhex(observation['stderr']))
    return inherited_child(command, input=input, **kwargs)

for binary in sys.argv[2:]:
    for raw in invalid:
        result=rejection_child([binary], requests=invalid,input=raw,timeout=4)
        support.require(result.returncode==2 and result.stdout==b'' and result.stderr==b'', 'rejection failure or sanitizer failure')
print('rejections '+str(len(invalid))+' per transport')
