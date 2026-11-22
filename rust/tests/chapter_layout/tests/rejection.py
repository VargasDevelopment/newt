"""Owned negative transports. Called as a Run child, never standalone spawning."""
import sys
from pathlib import Path
sys.path.insert(0, str(Path(sys.argv.pop(1)).resolve().parent))
from run_support import inherited_child
from cases import CASES
from protocol import parse_reply, malformed_controls as chapter_malformed_controls
from types import SimpleNamespace
from xml_protocol import malformed_controls

malformed_controls()
chapter_malformed_controls()
wire=CASES['single']
# Independent parser controls need no oracle execution.
from expectations import WITNESSES
for witness in WITNESSES:
    witness.validate()
    raw=witness.rust
    for bad in (raw[:-1],raw+'END\n',raw.replace('\n','\r\n'),raw.replace('END\n','junk\nEND\n')):
        try:parse_reply(bad,witness.wire)
        except ValueError:pass
        else:raise AssertionError('whole-response parser accepted malformed reply')
def admit_rejection(result):
    if result.returncode!=2 or result.stdout!=b'' or result.stderr!=b'':
        details = f"command={getattr(result, 'args', None)!r}, rc={result.returncode}, stdout={len(result.stdout)}, stderr={result.stderr[:160]!r}"
        raise AssertionError("malformed request must have designated silent rejection: " + details)
for code,out,err in [(0,b'',b''),(-11,b'',b''),(134,b'',b''),(2,b'',b'sanitizer'),(2,b'partial',b'')]:
    try:admit_rejection(SimpleNamespace(returncode=code,stdout=out,stderr=err))
    except AssertionError:pass
    else:raise AssertionError('invalid rejection admission fixture survived')
admit_rejection(SimpleNamespace(returncode=2,stdout=b'',stderr=b''))
for binary in sys.argv[1:]:
    valid=wire if 'xml' not in binary else 'Y 3c722f3e 2048 -1 0 0 1 -1 0 0 4 f\n'
    for bad in (valid[:-1],valid.replace('\n','\r\n'),valid+'garbage\n',valid.replace(' 2048 ', ' 0 ') if 'xml' in binary else valid.replace('L ', 'Unknown ',1)):
        result=inherited_child([binary],input=bad.encode())
        admit_rejection(result)
