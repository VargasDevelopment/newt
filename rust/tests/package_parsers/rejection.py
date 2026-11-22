"""Run only as a child of the shared Run process group."""
from pathlib import Path
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'inflate'))
from run_support import inherited_child, install_signal_handlers
from vectors import request



def rejection_child(command, *, input, requests, **kwargs):
    # The whole ordered malformed corpus is authenticated before selecting a reply.
    import hashlib
    import importlib.util
    from types import SimpleNamespace
    path = Path(__file__).resolve().parent / 'rejections.json'
    if hashlib.sha256(path.read_bytes()).hexdigest() != 'fdca2f553ea1a2cbd8157d3c59f846d170479609c2d11b09db865489db390ed9':
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

def main():
    install_signal_handlers()
    good=request('C','<r/>')
    invalid=[good[:-1],good.replace('\n','\r\n'),'\n',good+'\n',
             good.replace('P C','P  C'),good.replace('3c','3C',1),
             good.replace('P C','P Z'),good.replace(' 1048576 -1',' 0 -1'),
             good.replace(' -1 ',' -2 '),good.replace(' 1\n',' 2\n'),
             good.replace(' 1048576 -1',' 01048576 -1')]
    from vectors import VECTORS
    z = next(v['wire'] for v in VECTORS if v['wire'].startswith('Z '))
    fields = z.rstrip('\n').split(' ')
    invalid += [z[:-1], z.replace('\n','\r\n'), z.replace('Z ', 'Z  ', 1)]
    for at, value in [(8,'65536'),(8,'-1'),(9,'010'),(10,'2'),(10,'-1')]:
        changed=fields.copy();changed[at]=value;invalid.append(' '.join(changed)+'\n')
    for executable in sys.argv[1:]:
        for wire in invalid:
            result=rejection_child([executable], requests=invalid,input=wire.encode())
            if result.returncode!=2 or result.stderr:
                raise RuntimeError('request rejection mismatch '+repr(wire))
if __name__=='__main__':
    main()
