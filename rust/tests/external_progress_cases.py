# SPDX-License-Identifier: MIT
"""MIT. Independent wire/expectation corpus.
Run ONLY after authorization: python3 cases.py ORACLE RUST_HOST.
No imports from Rust port or oracle. Zero-length blobs remain PRESENT.
"""
import struct
import subprocess
import sys
import zlib
from dataclasses import dataclass

D = bytes(range(1, 33))
BOOK = int.from_bytes(D[:8], 'little')
NS = 'nb' + D.hex()[:13]


def progress(seq=1, book=BOOK, character=4, reserved=0):
    b = struct.pack('<4sIIQIIQIII', b'NWTR', 1, seq, book, 3, character, 70000, 8, 2, reserved)
    return b + struct.pack('<I', zlib.crc32(b))


def blob(b):
    return struct.pack('<H', len(b)) + b


@dataclass
class Case:
    name: str
    command: str
    result: int
    status: int
    ends: int
    puts: int
    output: bytes | None = None
    effects: bool = True
    entered: str | None = None
    durable_equal: bool | None = None
    protected_slot: bytes | None = None


def case(name, op='load', digest=D, records=None, initial=None, checkpoint=None,
         strict=False, cancel=False, open_result=0, fail_read=0, failures=0,
         durable=True, corrupt_read=0, result=0, status=2, ends=1, puts=0,
         output=None, effects=True, entered=None, durable_equal=None, protected_slot=None, readback_override=b''):
    records = {(NS, 'digest'): D} if records is None else records
    initial = progress(seq=33, character=99) if initial is None else initial
    payload = bytes([digest is not None]) + (digest or bytes(32)) + initial + struct.pack('<I', 456)
    payload += bytes([checkpoint is not None]) + struct.pack('<I', checkpoint or 0)
    payload += bytes([strict, cancel, open_result]) + struct.pack('<HBBH', fail_read, failures, durable, corrupt_read) + blob(readback_override) + struct.pack('<H', len(records))
    for (ns, key), value in sorted(records.items()):
        payload += blob(ns.encode()) + blob(key.encode()) + blob(value)
    return Case(name, 'xp.' + op + ' ' + payload.hex(), result, status, ends, puts,
                output, effects, entered, durable_equal, protected_slot)


def same_book_collision():
    """Solve only the unconstrained spine word; book identity stays unchanged."""
    first = progress(seq=10)
    body = bytearray(progress(seq=11)[:-4]); body[20:24] = bytes(4)
    base = zlib.crc32(body); basis = {}
    for bit in range(32):
        changed = bytearray(body); changed[20+bit//8] ^= 1 << (bit%8)
        column, combination = zlib.crc32(changed)^base, 1 << bit
        while column:
            pivot = column.bit_length()-1
            if pivot not in basis:
                basis[pivot] = (column, combination); break
            column ^= basis[pivot][0]; combination ^= basis[pivot][1]
    target, patch = int.from_bytes(first[-4:],'little')^base, 0
    while target:
        column, combination = basis[target.bit_length()-1]
        target ^= column; patch ^= combination
    body[20:24] = struct.pack('<I',patch)
    if zlib.crc32(body) != int.from_bytes(first[-4:],'little'):
        raise RuntimeError('invalid same-book collision fixture')
    return first, bytes(body)+first[-4:]


def corpus():
    valid = progress()
    known = {(NS, 'digest'): D, (NS, 'slot0'): valid}
    torn = valid[:-1]
    yield case('missing-namespace', records={}, result=1, status=1, ends=0)
    yield case('empty-digest-only', result=1, status=1)
    yield case('null-identity', digest=None, result=3, status=5, ends=0, effects=False)
    yield case('zero-book', digest=bytes(32), result=3, status=5, ends=0, effects=False)
    yield case('zero-first64-nonzero-tail', digest=bytes(8)+D[8:], result=3, status=5, ends=0, effects=False)
    yield case('valid', records=known, output=valid)
    yield case('valid-torn-inactive', records={**known,(NS,'slot1'):torn}, output=valid)
    yield case('corrupt-active-valid-peer', records={**known,(NS,'slot0'):torn,(NS,'slot1'):valid}, output=valid)
    yield case('both-corrupt',records={**known,(NS,'slot0'):torn},result=3,status=9)
    yield case('empty-present-is-not-missing',records={(NS,'slot0'):b''},result=3,status=9)
    yield case('missing-digest-valid-slot',records={(NS,'slot0'):valid},result=3,status=9)
    yield case('missing-digest-torn-slot',op='save',records={(NS,'slot0'):torn},result=0,status=9,entered='10')
    yield case('missing-digest-empty-slots-readonly',records={(NS,'other'):b'x'},result=1,status=1)
    for i in range(7,32):
        changed=bytearray(D);changed[i]^=128
        yield case(f'full-digest-collision-byte-{i}',records={**known,(NS,'digest'):bytes(changed)},result=3,status=7)
    # The unused low nibble of byte 6 is outside the 52-bit route too.
    changed=bytearray(D);changed[6]^=1
    yield case('route-nibble-collision',records={**known,(NS,'digest'):bytes(changed)},result=3,status=7)
    for n in (0,1,31,33,80):
        yield case(f'malformed-digest-{n}',records={**known,(NS,'digest'):bytes(n)},result=3,status=8)
    yield case('valid-wrong-book-inactive',records={**known,(NS,'slot1'):progress(book=BOOK+1)},result=3,status=6)
    yield case('missing-digest-wrong-book-precedence',records={(NS,'slot0'):progress(book=BOOK+1)},result=3,status=6)
    for op, result in [('load',2),('checkpoint',3),('peers',3),('save',0)]:
        yield case(f'ambiguous-{op}',op=op,records={**known,(NS,'slot1'):progress(character=9)},result=result,status=4)
    yield case('half-range',records={**known,(NS,'slot1'):progress(seq=0x80000001)},result=2,status=4)
    yield case('equal-identical',records={**known,(NS,'slot1'):valid},output=valid)
    yield case('wrap-order',records={**known,(NS,'slot0'):progress(seq=0xffffffff),(NS,'slot1'):progress(seq=0)},output=progress(seq=0))
    for open_result in (1,2,3):
        yield case(f'open-result-{open_result}',open_result=open_result,result=1 if open_result==1 else 3,status=1 if open_result==1 else 10,ends=0)
    for read in (1,2,3):
        yield case(f'read-failure-{read}',records=known,fail_read=read,result=3,status=10)
    yield case('cancel-before-effects',op='save',records={},cancel=True,result=0,status=11,ends=0,effects=False,entered='01')
    yield case('proposed-book-mismatch-before-admission',op='save',initial=progress(book=BOOK+1),cancel=True,result=0,status=6,ends=0,effects=False,entered='00')
    yield case('adopt-then-slot',op='save',records={},result=1,status=3,puts=2,entered='10',durable_equal=True)
    yield case('digest-short-ack-durable-no-slot',op='save',records={},failures=1,result=0,status=11,puts=1,durable_equal=True)
    yield case('digest-bounded-sync-retry',op='save',records={},failures=1,durable=False,result=1,status=3,puts=3,durable_equal=True)
    yield case('digest-two-failures',op='save',records={},failures=2,durable=False,result=0,status=11,puts=2,durable_equal=False)
    yield case('digest-verify-failure',op='save',records={},corrupt_read=4,result=0,status=12,puts=1)
    yield case('digest-readback-io-precedence',op='save',records={},fail_read=4,result=0,status=12,puts=1)
    yield case('slot-save',op='save',records=known,result=1,status=3,puts=1)
    yield case('slot-lost-ack-durable-reconcile',op='save',records=known,failures=1,result=1,status=3,ends=2,puts=1)
    yield case('slot-sync-retry',op='save',records=known,failures=1,durable=False,result=1,status=3,puts=2)
    yield case('slot-two-unconfirmed',op='save',records=known,failures=2,durable=False,result=0,status=11,puts=2,durable_equal=False)
    yield case('slot-readback-physical',op='save',records=known,corrupt_read=4,result=1,status=3,ends=2,puts=1)
    yield case('verify-then-reopen-storage-precedence',op='save',records=known,corrupt_read=4,fail_read=5,result=0,status=10,ends=2,puts=1)
    yield case('same-record-no-write',op='save',records=known,initial=valid,result=1,status=3,puts=0)
    yield case('same-record-unsynced-must-write',op='save',records=known,initial=valid,durable=False,result=1,status=3,puts=1)
    crc=int.from_bytes(valid[48:], 'little')
    yield case('checkpoint-found',op='checkpoint',records=known,checkpoint=crc,output=valid)
    yield case('checkpoint-absent-status',op='checkpoint',records=known,checkpoint=crc^1,result=1,status=9)
    yield case('checkpoint-empty-status',op='checkpoint',result=1,status=9)
    yield case('checkpoint-missing-namespace',op='checkpoint',records={},result=1,status=1,ends=0)
    yield case('protect-old-slot',op='save-peers',records={**known,(NS,'slot1'):progress(seq=2)},checkpoint=crc,result=1,status=3,puts=1,protected_slot=valid)
    yield case('missing-protected',op='save',records=known,checkpoint=crc^1,result=0,status=11)
    for torn_bytes,label in ((valid[:8],'known-torn'),(b'garbage!','unknown'),(valid+bytes(1),'oversize'),(progress(reserved=1),'reserved')):
        for strict in (False,True):
            accepted=not strict or label=='known-torn'
            yield case(f'{label}-strict-{strict}',op='save',records={**known,(NS,'slot1'):torn_bytes},strict=strict,result=int(accepted),status=3 if accepted else 11,puts=int(accepted))
    yield case('peers-loaded',op='peers',records=known)
    yield case('peers-empty-clears',op='peers',result=1,status=1)
    yield case('peers-missing-clears',op='peers',records={},result=1,status=1,ends=0)
    yield case('save-peers',op='save-peers',records=known,result=1,status=3,puts=1)
    a,b = same_book_collision(); collision_records={**known,(NS,'slot0'):a,(NS,'slot1'):b}
    checkpoint=int.from_bytes(a[-4:],'little')
    yield case('checkpoint-same-book-collision',op='checkpoint',records=collision_records,checkpoint=checkpoint,result=2,status=9,output=progress(seq=33,character=99))
    yield case('protected-same-book-collision',op='save-peers',records=collision_records,checkpoint=checkpoint,result=0,status=11,output=progress(seq=33,character=99))
    yield case('readback-valid-wrong-book',op='save-peers',records=known,durable=False,readback_override=progress(book=BOOK+1),result=0,status=12,puts=2,output=progress(seq=33,character=99))



def check(case, text):
    parts=text.split('|')
    assert len(parts)==9,(case.name,text)
    result,status,book,ns,out,admission,trace,visible,durable=parts
    assert (int(result),int(status))==(case.result,case.status),(case.name,result,status)
    calls=trace.split(',') if trace else []
    assert calls.count('end')==case.ends,(case.name,calls)
    assert sum(x.startswith('put:') for x in calls)==case.puts,(case.name,calls)
    if not case.effects: assert not calls,(case.name,calls)
    if case.entered is not None: assert admission==case.entered,(case.name,admission)
    if case.durable_equal is not None: assert (visible==durable)==case.durable_equal,(case.name,visible,durable)
    if case.output is not None: assert bytes.fromhex(out)[:52]==case.output,case.name
    if case.protected_slot is not None: assert f'{NS}/slot0={case.protected_slot.hex()}' in visible,case.name
    if case.name=='slot-readback-physical': assert calls[5]=='read:slot1',(case.name,calls)
    if case.name.startswith('peers-') and case.result==1: assert bytes.fromhex(out)[56:]==bytes(17),case.name
    if case.name in ('slot-save','same-record-no-write','save-peers','adopt-then-slot'):
        assert int.from_bytes(bytes.fromhex(out)[52:56],'little')==456,case.name
    if case.name=='save-peers': assert bytes.fromhex(out)[56]==3,case.name
    if case.name in ('checkpoint-same-book-collision','protected-same-book-collision','readback-valid-wrong-book'):
        assert bytes.fromhex(out)==progress(seq=33,character=99)+struct.pack('<I',456)+bytes([3])+struct.pack('<IIII',71,72,73,74),case.name
    if case.name=='readback-valid-wrong-book':
        assert sum(item.startswith('read:slot') for item in calls)==4,case.name
    if case.name not in ('null-identity','zero-book','zero-first64-nonzero-tail'):
        assert int(book)==BOOK and ns==NS,case.name
    if case.name=='null-identity': assert book=='0' and ns=='',case.name
    if case.name in ('zero-book','zero-first64-nonzero-tail'): assert book=='0' and ns=='nb'+'0'*13,case.name


def main():
    cases=list(corpus())
    payload='\n'.join(c.command for c in cases)+'\n'
    results=[]
    for executable in sys.argv[1:]:
        output=subprocess.run([executable],input=payload,text=True,capture_output=True,check=True).stdout.splitlines()
        assert len(output)==len(cases),(executable,len(output),len(cases))
        for c,text in zip(cases,output): check(c,text)
        results.append(output)
    assert len(results)==2,'supply pinned C++ oracle and Rust host'
    for c,a,b in zip(cases,*results): assert a==b,(c.name,a,b)
    print(f'{len(cases)} independent expectations and full trace/state parity checks passed')


if __name__=='__main__': main()


_fixture_corpus = corpus
def corpus():
    return [case.command for case in _fixture_corpus()]

def assert_expectations(inputs, replies):
    rows = list(_fixture_corpus())
    fixtures = {case.command: case for case in rows}
    if len(fixtures) != len(rows):
        raise RuntimeError("Duplicate external progress expectation command")
    for command, reply in zip(inputs, replies):
        if command in fixtures:
            check(fixtures[command], reply)
