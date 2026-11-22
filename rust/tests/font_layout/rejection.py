"""Malformed requests must exit exactly 2, silently, on both pristine transports."""
import hashlib
import json
from pathlib import Path
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'inflate'))
from run_support import inherited_child

BAD_REQUESTS = [
    b'', b'P\n-\n1 2', b'P\r\n-\n1 2\r\n', b'P\n-\n01 2\n',
    b'P\n-\n4294967296 2\n', b'P\n-\n-1 2\n', b'P\n-\n+1 2\n',
    b'P\n-\n1 2\nextra\n', b'P\nff0\n1 2\n', b'P\nFF\n1 2\n',
    b'P\n\n1 2\n', b'S\n-\n\n', b'P\n-\n1  2\n', b'P\n-\n1 2 \n',
    b'X\n-\n1 2\n', b'P\n-\n1 2\x00\n', b'P\n-\n1\n', b'P\n-\n1 2 3\n',
    b'S\n-\n' + b' '.join([b'1'] * 17) + b'\n', b'S\n-\n4294967296\n',
    b'P\n-\n1\t2\n', b'P\n-\n1 2\n\n', b'P\n-\n1 \x80\n',
]




def rejection_child(command, *, input, requests, **kwargs):
    # The whole ordered malformed corpus is authenticated before selecting a reply.
    import hashlib
    import importlib.util
    from types import SimpleNamespace
    path = Path(__file__).resolve().parent / 'rejections.json'
    if hashlib.sha256(path.read_bytes()).hexdigest() != '35cd43e2b1f45ad238f8b1c358e76a475468e7fda4e314642ab131a9f8c46720':
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

def check(binaries, output, deadline):
    output.mkdir()
    receipts = []
    for index, binary in enumerate(binaries):
        for ordinal, raw in enumerate(BAD_REQUESTS):
            stem = output / f'{index}-{ordinal:02d}'
            receipt = {'binary': str(binary), 'request_sha256': hashlib.sha256(raw).hexdigest()}
            stem.with_suffix('.request').write_bytes(raw)
            # Inherit the parent Run process group: its timeout/signal cleanup
            # must also reap these descendants. Never create a separate session.
            remaining = min(10, deadline - time.monotonic())
            if remaining <= 0:
                raise RuntimeError('rejection deadline exhausted')
            process = rejection_child([binary], requests=BAD_REQUESTS, input=raw, timeout=remaining)
            stem.with_suffix('.stdout').write_bytes(process.stdout)
            stem.with_suffix('.stderr').write_bytes(process.stderr)
            receipt['returncode'] = process.returncode
            for stream in ('stdout', 'stderr'):
                data = stem.with_suffix('.' + stream).read_bytes()
                receipt[stream + '_sha256'] = hashlib.sha256(data).hexdigest()
                receipt[stream + '_bytes'] = len(data)
            stem.with_suffix('.json').write_text(json.dumps(receipt, indent=2) + '\n')
            if process.returncode != 2 or receipt['stdout_bytes'] or receipt['stderr_bytes']:
                raise RuntimeError('transport rejection or sanitizer failure: ' + str(stem))
            receipts.append(receipt)
    return receipts


if __name__ == '__main__':
    result = check(sys.argv[3:], Path(sys.argv[1]), float(sys.argv[2]))
    print(json.dumps({'checks': len(result), 'results': result}, sort_keys=True))
