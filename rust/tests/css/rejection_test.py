"""Malformed requests must return the declared transport error, never crash."""
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'inflate'))
from run_support import inherited_child
from corpus import rejection_corpus

count = 0


def rejection_child(command, *, input, requests, **kwargs):
    # The whole ordered malformed corpus is authenticated before selecting a reply.
    import hashlib
    import importlib.util
    from types import SimpleNamespace
    path = Path(__file__).resolve().parent / 'rejections.json'
    if hashlib.sha256(path.read_bytes()).hexdigest() != '645811ccda16b1e7ca713043ab7143d4ceb66a77df34655707cc15be345a8cc1':
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

requests = list(rejection_corpus())
for request in requests:
    for binary in sys.argv[1:]:
        result = rejection_child([binary], requests=requests, input=request.encode('ascii'), timeout=5)
        if result.returncode != 2:
            raise RuntimeError(f'malformed request returned {result.returncode}, expected2')
        count += 1
print(f'PASS {count} malformed-request rejection checks')
