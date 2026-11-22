"""Inherited suite process group owns all transport children."""
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'inflate'))
from run_support import inherited_child

bad = ['\n', 'X\n', 'I\n', 'I null extra\n', 'I 0\n', 'I zz\n',
       'B 256 -\n', 'B -1 -\n', 'B +1 -\n', 'B 1 0\n', 'B 1\n', 'R extra\n']
count = 0


def rejection_child(command, *, input, requests, **kwargs):
    # The whole ordered malformed corpus is authenticated before selecting a reply.
    import hashlib
    import importlib.util
    from types import SimpleNamespace
    path = Path(__file__).resolve().parent / 'rejections.json'
    if hashlib.sha256(path.read_bytes()).hexdigest() != '20fdfe76c98049e42bfe3b79d625ed337f55123cdbd119c872d8777e3d52c50a':
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

for request in bad:
    for binary in sys.argv[1:]:
        result = rejection_child([binary], requests=bad, input=request.encode())
        if result.returncode != 2:
            raise RuntimeError(f'malformed request returned {result.returncode}: {request!r}')
        if any(marker in result.stderr for marker in (b'AddressSanitizer', b'runtime error:', b'UndefinedBehaviorSanitizer', b'panicked at')):
            raise RuntimeError('malformed request caused runtime diagnostic')
        count += 1
print(f'PASS {count} exact-exit-2 malformed-request checks')
