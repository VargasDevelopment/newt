"""Bounded malformed wire rejection with fatal sanitizer settings inherited."""
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'inflate'))
from run_support import inherited_child

valid=b'P _ 0 0 0 4294967295 1 -1 -1 0 1024\n'
requests=[b'',b'\n',valid[:-1],valid.replace(b'\n',b'\r\n'),valid+b'\n',valid.replace(b'P ',b'Q '),
          valid.replace(b'P _',b'P -'),valid.replace(b'P _',b'P zz'),valid.replace(b'P _',b'P 0'),
          valid.replace(b'0 0 0 4294967295',b'65536 0 0 4294967295'),
          valid.replace(b'0 1024',b'2 1024'),valid.replace(b'0 1024',b'0 2049'),
          valid.replace(b'-1 -1',b'-2 -1'),valid.replace(b'-1 -1',b'-1 1'),
          valid.replace(b'-1 -1',b'-1 -2147483649'),valid.replace(b' 1 -1',b' x -1'),
          valid.replace(b'P _',b'P  _'),valid.replace(b'P _',b'P\t_'),valid.replace(b'1024',b'1024 extra')]
fields=valid[:-1].split(b' ')


def rejection_child(command, *, input, requests, **kwargs):
    # The whole ordered malformed corpus is authenticated before selecting a reply.
    import hashlib
    import importlib.util
    from types import SimpleNamespace
    path = Path(__file__).resolve().parent / 'rejections.json'
    if hashlib.sha256(path.read_bytes()).hexdigest() != '254a5ac6cd5befb7653c1cce1fb35840785e83ca3063a761b7966d4b5cc1432a':
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

for index in (2,3,4,5,6,9,10):
    changed=list(fields);changed[index]=b'0'+changed[index]
    requests.append(b' '.join(changed)+b'\n')
for index in (7,8):
    changed=list(fields);changed[index]=b'-0'
    requests.append(b' '.join(changed)+b'\n')
requests += [valid.replace(b'P _', b'P AA'),valid.replace(b'P _', b'P ')]
count=0
for binary in sys.argv[1:]:
    for request in requests:
        result=rejection_child([binary], requests=requests,input=request,timeout=5)
        if result.returncode!=2 or result.stderr:
            raise RuntimeError(f'rejection exit/sanitizer failure: {result.returncode} {result.stderr!r}')
        count+=1
print(f'PASS {count} malformed-request rejection checks')
