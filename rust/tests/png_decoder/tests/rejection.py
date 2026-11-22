"""Root-run transport rejection checks; children stay in the owning gate group."""
import importlib.util
from pathlib import Path
import sys
sys.dont_write_bytecode = True
HERE=Path(__file__).resolve().parent



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

def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path)
    m=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def main():
    global inherited_child
    sys.path.insert(0,str(Path(sys.argv[1]).parent))
    from run_support import inherited_child, require
    vectors=load('reject_vectors',HERE/'vectors.py')
    protocol=load('reject_protocol',HERE/'protocol.py')
    request=vectors.request(vectors.png())
    malformed=[b'',request.rstrip('\n').encode(),request.replace('PN1','PN2').encode(),
               request.replace('PN1 0','PN1 00').encode(),request.replace('PN1 0','PN1 -1').encode(),
               request.replace('PN1 0','PN1 2').encode(),request.replace('PN1 ','PN1  ').encode(),
               request.replace('\n',' \n').encode(),request.replace('\n','\r\n').encode(),
               request.replace('4096','0').encode(),request.replace('\n','\nextra\n').encode()]
    for executable in sys.argv[2:]:
        for payload in malformed:
            result=observed_child([executable], requests=malformed,input=payload)
            require(result.returncode==2,'malformed request must reject, never crash')
            require(result.stdout==b'' and result.stderr==b'','rejection must be silent')
    # These are validation fixtures, never substitute decoder output.
    size=len(request.rstrip('\n').split(' ')[-1])//2
    valid=f'PN1 0 1 0 000000 F:0:{size}:0\n'
    protocol.reply(valid,request)
    for broken in (valid.rstrip('\n'),valid+'\n',valid.replace(' 1 0 ',' 2 0 '),
                   valid.replace('PN1 0','PN1 9'),valid.replace('F:0:', 'F:1:'),
                   valid.replace('000000','ABCDEF'),valid.replace('F:','R:'),valid.replace(f':{size}:0',f':{size}:{size+1}')):
        try:
            protocol.reply(broken,request)
        except ValueError:
            pass
        else:
            raise RuntimeError('reply validator admitted malformed reply')
    print('request/reply rejection checks complete')


if __name__=='__main__':
    main()
